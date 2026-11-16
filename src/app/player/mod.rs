pub mod ui;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use adw::prelude::*;

use crate::api::{self, Client, Episode, Title};
use crate::discord_rpc::DiscordRpc;
use crate::embed_mpv::player::{options_from_cli, MpvApi, MpvEmbed};
use crate::embed_mpv::render::RenderContext;
use crate::skip::SkipPlan;
use crate::ui::operation_stack::OperationStack;

/// Gömülü oynatıcının uygulamaya bağlanan callback'leri.
pub struct PlayerHost {
    pub client: Arc<Client>,
    pub ops: OperationStack,
    pub discord: DiscordRpc,
    pub progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    pub on_back: Rc<dyn Fn()>,
    pub on_play_episode: Rc<dyn Fn(Episode)>,
    pub on_neighbor: Rc<dyn Fn(i8)>,
    pub on_download: Rc<dyn Fn(Episode)>,
    pub on_ready: Rc<dyn Fn()>,
    pub on_terminal: Rc<dyn Fn(crate::app::PlayTerminal, String)>,
    pub on_external: Rc<dyn Fn()>,
}

pub struct EmbedRequest {
    pub title: Title,
    pub ep: Episode,
    pub candidates: Vec<String>,
    pub fast_embeds: Vec<String>,
    pub fallback_embeds: Vec<String>,
    pub quality_map: HashMap<String, String>,
    pub plan: Option<SkipPlan>,
    pub saved_pos: Option<f64>,
    pub upscale: String,
    pub auto_fullscreen: bool,
    pub show_intro_hint: bool,
    pub show_music_hint: bool,
    pub cache_forward_mb: u64,
    pub cache_back_mb: u64,
    pub patience_secs: u64,
    pub auto_next: bool,
    pub episodes: Vec<Episode>,
    pub prev_ep: Option<Episode>,
    pub next_ep: Option<Episode>,
}

struct PlayerState {
    guard: PlaybackGuard,
    alive: Arc<AtomicBool>,
    player: Option<MpvEmbed>,
    render_ctx: Option<RenderContext>,
    host: PlayerHost,
    title: Title,
    ep: Episode,
    candidates: Vec<String>,
    index: usize,
    media_loaded: bool,
    ready_sent: bool,
    terminal_sent: bool,
    source_loaded: bool,
    resume: Option<f64>,
    window: adw::ApplicationWindow,
    gl_area: gtk::GLArea,
    root: gtk::Box,
    topbar: gtk::Revealer,
    shade: gtk::Revealer,
    controls: gtk::Revealer,
    center: gtk::Revealer,
    play_btn: gtk::Button,
    back10: gtk::Button,
    fwd10: gtk::Button,
    seek: gtk::Scale,
    cur_lbl: gtk::Label,
    dur_lbl: gtk::Label,
    vol_btn: gtk::Button,
    vol_scale: gtk::Scale,
    fs_btn: gtk::Button,
    status: gtk::Label,
    speed_btn: gtk::MenuButton,
    speed_pop: gtk::Popover,
    bot_title: gtk::Label,
    last_motion: Instant,
    last_mx: f64,
    last_my: f64,
    chrome_hidden: bool,
    cursor_hidden: bool,
    we_fullscreened: bool,
    click_pending: Rc<Cell<bool>>,
    seeking: Rc<Cell<bool>>,
    vol_changing: Rc<Cell<bool>>,
    progress_key: String,
    progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    last_report: Instant,
}

pub struct EmbeddedPlayer {
    state: Rc<RefCell<PlayerState>>,
    widget: gtk::Box,
}

impl EmbeddedPlayer {
    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn position(&self) -> f64 {
        self.state
            .borrow()
            .player
            .as_ref()
            .map(|p| p.pos())
            .unwrap_or(0.0)
    }

    pub fn duration(&self) -> f64 {
        self.state
            .borrow()
            .player
            .as_ref()
            .map(|p| p.dur())
            .unwrap_or(0.0)
    }

    pub fn is_alive(&self) -> bool {
        self.state.borrow().guard.is_alive()
    }

    /// Oynatmayı durdurur, GL bağlamını mpv'den önce bırakır.
    /// İkinci çağrı no-op.
    pub fn shutdown(&self) {
        let mut s = self.state.borrow_mut();
        if !s.guard.mark_dead() {
            return;
        }
        s.alive.store(false, Ordering::Relaxed);
        if let Some(p) = s.player.as_ref() {
            p.set_pause(true);
            let pos = p.pos();
            let dur = p.dur();
            if pos >= 1.0 {
                s.progress
                    .borrow_mut()
                    .insert(s.progress_key.clone(), (pos, dur));
                s.host
                    .client
                    .save_progress(s.title.id, s.ep.season, s.ep.episode, pos, dur);
            }
            p.stop();
        }
        // Render bağlamı HER ZAMAN player handle'ından önce düşmeli.
        let _ = s.gl_area.make_current();
        s.render_ctx = None;
        s.player = None;
        s.we_fullscreened = false;
        if s.window.is_fullscreen() {
            s.window.unfullscreen();
        }
        s.gl_area.set_cursor_from_name(None);
        s.topbar.set_reveal_child(true);
        s.controls.set_reveal_child(true);
        s.center.set_reveal_child(true);
        s.shade.set_reveal_child(true);
        eprintln!("[EMBED] oynatıcı kapatıldı");
    }
}

impl Drop for EmbeddedPlayer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn build(
    parent: &adw::ApplicationWindow,
    host: PlayerHost,
    req: EmbedRequest,
    api: Rc<MpvApi>,
) -> Option<EmbeddedPlayer> {
    let media_title = format!(
        "{} — {}",
        req.title.name,
        crate::ui::components::episode_label_compact(req.ep.season, req.ep.episode, &req.ep.name)
    );
    eprintln!(
        "[EMBED] açılıyor: {media_title} (kaynak={})",
        req.candidates.len()
    );

    let mut opts: Vec<(String, String)> = vec![
        ("force-media-title".into(), media_title.clone()),
        (
            "demuxer-max-bytes".into(),
            format!("{}MiB", req.cache_forward_mb.max(16)),
        ),
        (
            "demuxer-max-back-bytes".into(),
            format!("{}MiB", req.cache_back_mb.max(4)),
        ),
        ("demuxer-readahead-secs".into(), "120".into()),
        ("cache-secs".into(), "120".into()),
        ("cache-pause".into(), "yes".into()),
        ("cache-pause-wait".into(), "3".into()),
        (
            "stream-lavf-o".into(),
            "reconnect=1,reconnect_streamed=1,reconnect_delay_max=5".into(),
        ),
    ];
    let shader = match req.upscale.as_str() {
        "hafif" => crate::app::resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
        "ultra" => crate::app::resolve_upscale_shader("Anime4K_Upscale_CNN_x2_UL.glsl"),
        "hafif_keskin" => crate::app::resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
        _ => None,
    };
    let upscale_args = api::upscale_mpv_args(&req.upscale, shader.as_deref(), None);
    opts.extend(options_from_cli(&upscale_args));
    let opt_refs: Vec<(&str, &str)> = opts.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let player = match MpvEmbed::new(api, &opt_refs, req.saved_pos) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[EMBED] mpv init başarısız: {e}");
            return None;
        }
    };
    let start_volume = player.volume();
    let start_muted = player.muted();

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("player-root");
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.set_focusable(true);
    ui::install_css(&root);

    let stage = gtk::Overlay::new();
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    root.append(&stage);

    let gl_area = gtk::GLArea::builder().hexpand(true).vexpand(true).build();
    gl_area.set_required_version(3, 0);
    gl_area.set_focusable(true);
    stage.set_child(Some(&gl_area));

    let shade_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shade_box.add_css_class("player-shade");
    shade_box.set_hexpand(true);
    shade_box.set_vexpand(true);
    shade_box.set_can_target(false);
    let shade = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(220)
        .child(&shade_box)
        .build();
    shade.set_reveal_child(true);
    shade.set_can_target(false);

    let shade_enabled = std::env::var_os("ANIMECIX_PLAYER_NO_SHADE").is_none();
    if shade_enabled {
        stage.add_overlay(&shade);
    }
    let controls = ui::build_control_bar(&stage, start_volume, start_muted, &media_title);
    let topbar = ui::build_topbar(&stage);

    let alive = Arc::new(AtomicBool::new(true));
    let click_pending = Rc::new(Cell::new(false));
    let seeking = Rc::new(Cell::new(false));
    let vol_changing = Rc::new(Cell::new(false));
    let progress_key = format!("{}:{}:{}", req.title.id, req.ep.season, req.ep.episode);
    let host_progress = host.progress.clone();
    let state = Rc::new(RefCell::new(PlayerState {
        guard: PlaybackGuard::new(),
        alive: alive.clone(),
        player: Some(player),
        render_ctx: None,
        host,
        title: req.title.clone(),
        ep: req.ep.clone(),
        candidates: req.candidates.clone(),
        index: 0,
        media_loaded: false,
        ready_sent: false,
        terminal_sent: false,
        source_loaded: false,
        resume: req.saved_pos,
        window: parent.clone(),
        gl_area: gl_area.clone(),
        root: root.clone(),
        topbar: topbar.revealer.clone(),
        shade: shade.clone(),
        controls: controls.controls_revealer.clone(),
        center: controls.center_rev.clone(),
        play_btn: controls.play_btn.clone(),
        back10: controls.back10_btn.clone(),
        fwd10: controls.fwd10_btn.clone(),
        seek: controls.seek.clone(),
        cur_lbl: controls.cur_lbl.clone(),
        dur_lbl: controls.dur_lbl.clone(),
        vol_btn: controls.vol_btn.clone(),
        vol_scale: controls.vol_scale.clone(),
        fs_btn: topbar.fullscreen.clone(),
        status: controls.status.clone(),
        speed_btn: controls.speed_btn.clone(),
        speed_pop: controls.speed_pop.clone(),
        bot_title: controls.bot_title.clone(),
        last_motion: Instant::now(),
        last_mx: -1.0,
        last_my: -1.0,
        chrome_hidden: false,
        cursor_hidden: false,
        we_fullscreened: false,
        click_pending: click_pending.clone(),
        seeking: seeking.clone(),
        vol_changing: vol_changing.clone(),
        progress_key,
        progress: host_progress,
        last_report: Instant::now(),
    }));

    // ---- GLArea render döngüsü ----
    let needs_render = Arc::new(AtomicBool::new(false));
    let render_count = Arc::new(AtomicU32::new(0));
    {
        let weak = Rc::downgrade(&state);
        let flag = needs_render.clone();
        let status = controls.status.clone();
        gl_area.connect_realize(move |area| {
            area.make_current();
            if let Some(err) = area.error() {
                status.set_text(&format!("GL hatası: {err}"));
                status.set_visible(true);
                return;
            }
            let Some(st) = weak.upgrade() else { return };
            let mut s = st.borrow_mut();
            if !s.guard.is_alive() || s.render_ctx.is_some() {
                return;
            }
            let Some(player) = s.player.as_ref() else {
                return;
            };
            match player.create_render_context() {
                Ok(mut ctx) => {
                    let alive_cb = s.alive.clone();
                    let flag_cb = flag.clone();
                    ctx.set_update_callback(move || {
                        if alive_cb.load(Ordering::Relaxed) {
                            flag_cb.store(true, Ordering::Relaxed);
                        }
                    });
                    s.render_ctx = Some(ctx);
                    let should_load = !s.source_loaded;
                    drop(s);
                    if should_load {
                        let st_load = st.clone();
                        glib::idle_add_local_once(move || {
                            load_initial_source(&st_load);
                        });
                    }
                    eprintln!("[EMBED] render ctx hazır");
                }
                Err(e) => {
                    status.set_text(&format!("Render hatası: {e}"));
                    status.set_visible(true);
                    eprintln!("[EMBED] render ctx hatası: {e}");
                }
            }
        });
    }
    {
        let flag = needs_render.clone();
        let weak = Rc::downgrade(&state);
        let area = gl_area.clone();
        glib::timeout_add_local(Duration::from_millis(8), move || {
            let Some(st) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !st.borrow().guard.is_alive() {
                return glib::ControlFlow::Break;
            }
            if flag.swap(false, Ordering::Relaxed) {
                area.queue_render();
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let weak = Rc::downgrade(&state);
        let alive_c = alive.clone();
        let render_count_c = render_count.clone();
        gl_area.connect_render(move |area, _ctx| {
            if !alive_c.load(Ordering::Relaxed) {
                return glib::Propagation::Stop;
            }
            let Some(st) = weak.upgrade() else {
                return glib::Propagation::Stop;
            };
            let Ok(s) = st.try_borrow() else {
                eprintln!("[EMBED] render: state borrow meşgul");
                return glib::Propagation::Stop;
            };
            if let Some(ctx) = s.render_ctx.as_ref() {
                let fbo = ctx.current_fbo();
                let scale = area.scale_factor();
                let w = area.width() * scale;
                let h = area.height() * scale;
                let n = render_count_c.fetch_add(1, Ordering::Relaxed);
                if n < 5 {
                    eprintln!(
                        "[EMBED] render #{n} fbo={fbo} {w}x{h} backend={:?}",
                        ctx.backend()
                    );
                }
                if let Err(e) = ctx.render(fbo, w, h, true) {
                    eprintln!("[EMBED] render hatası: {e}");
                }
                ctx.report_swap();
            } else if render_count_c.fetch_add(1, Ordering::Relaxed) < 3 {
                eprintln!("[EMBED] render: ctx yok");
            }
            glib::Propagation::Stop
        });
    }
    {
        let weak = Rc::downgrade(&state);
        gl_area.connect_unrealize(move |area| {
            area.make_current();
            if let Some(st) = weak.upgrade() {
                if let Ok(mut s) = st.try_borrow_mut() {
                    s.render_ctx = None;
                }
            }
        });
    }

    // ---- üst bar / kontroller ----
    {
        let weak = Rc::downgrade(&state);
        topbar.back.connect_clicked(move |_| {
            let Some(st) = weak.upgrade() else { return };
            let cb = st.borrow().host.on_back.clone();
            cb();
        });
    }
    {
        let weak = Rc::downgrade(&state);
        topbar.fullscreen.connect_clicked(move |_| {
            let Some(st) = weak.upgrade() else { return };
            let on = !st.borrow().window.is_fullscreen();
            set_fullscreen(&st, on);
        });
    }
    {
        let weak = Rc::downgrade(&state);
        controls.play_btn.connect_clicked(move |_| {
            let Some(st) = weak.upgrade() else { return };
            if let Ok(s) = st.try_borrow() {
                if let Some(p) = s.player.as_ref() {
                    p.toggle_pause();
                }
            };
        });
    }
    {
        let weak = Rc::downgrade(&state);
        controls.back10_btn.connect_clicked(move |_| {
            if let Some(st) = weak.upgrade() {
                let s = st.borrow();
                if let Some(p) = s.player.as_ref() {
                    p.seek_rel(-10.0);
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(&state);
        controls.fwd10_btn.connect_clicked(move |_| {
            if let Some(st) = weak.upgrade() {
                let s = st.borrow();
                if let Some(p) = s.player.as_ref() {
                    p.seek_rel(10.0);
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(&state);
        let seeking = seeking.clone();
        controls.seek.connect_change_value(move |_, _, v| {
            seeking.set(true);
            if let Some(st) = weak.upgrade() {
                let s = st.borrow();
                if let Some(p) = s.player.as_ref() {
                    let d = p.dur();
                    if d > 0.0 {
                        p.seek_abs(d * v / 1000.0);
                    }
                }
            }
            seeking.set(false);
            glib::Propagation::Proceed
        });
    }
    {
        let weak = Rc::downgrade(&state);
        let changing = vol_changing.clone();
        controls.vol_scale.connect_change_value(move |_, _, v| {
            changing.set(true);
            if let Some(st) = weak.upgrade() {
                let s = st.borrow();
                if let Some(p) = s.player.as_ref() {
                    p.set_volume(v);
                    if v > 0.0 && p.muted() {
                        p.set_mute(false);
                    }
                }
            }
            changing.set(false);
            glib::Propagation::Proceed
        });
    }
    {
        let weak = Rc::downgrade(&state);
        controls.vol_btn.connect_clicked(move |_| {
            if let Some(st) = weak.upgrade() {
                let s = st.borrow();
                if let Some(p) = s.player.as_ref() {
                    p.set_mute(!p.muted());
                }
            }
        });
    }
    // Hız menüsü (Lowell'in 0.5-2.0 listesi).
    for (label, val) in [
        ("0.5x ▾", 0.5),
        ("0.75x ▾", 0.75),
        ("1.0x ▾", 1.0),
        ("1.25x ▾", 1.25),
        ("1.5x ▾", 1.5),
        ("1.75x ▾", 1.75),
        ("2.0x ▾", 2.0),
    ] {
        let b = gtk::Button::with_label(label);
        b.add_css_class("flat");
        let weak = Rc::downgrade(&state);
        let btn = controls.speed_btn.clone();
        let pop = controls.speed_pop.clone();
        b.connect_clicked(move |_| {
            if let Some(st) = weak.upgrade() {
                let s = st.borrow();
                if let Some(p) = s.player.as_ref() {
                    p.set_speed(val);
                }
            }
            btn.set_label(label);
            pop.popdown();
        });
        controls.speed_box.append(&b);
    }
    // Kaynak listesi (Task 4'te kalite çözümlemesiyle genişler).
    {
        let weak = Rc::downgrade(&state);
        topbar.back.set_visible(true);
        controls.quality_btn.set_label("Kaynak ▾");
        let label = gtk::Label::new(Some("Kaynak seçimi hazırlanıyor…"));
        label.add_css_class("dim-label");
        controls.quality_box.append(&label);
        controls.quality_pop.connect_show(move |_| {
            let _ = weak.upgrade();
        });
    }

    // ---- tıklama / oto gizleme / klavye ----
    {
        let weak = Rc::downgrade(&state);
        let gesture = gtk::GestureClick::new();
        gesture.set_button(1);
        gesture.connect_pressed(move |_, n_press, _, _| {
            let Some(st) = weak.upgrade() else { return };
            if n_press == 2 {
                if let Ok(s) = st.try_borrow() {
                    s.click_pending.set(false);
                }
                let on = !st.borrow().window.is_fullscreen();
                set_fullscreen(&st, on);
            } else if n_press == 1 {
                if let Ok(s) = st.try_borrow() {
                    s.click_pending.set(true);
                    s.gl_area.grab_focus();
                }
                let st2 = st.clone();
                glib::timeout_add_local_once(Duration::from_millis(260), move || {
                    let go = st2
                        .try_borrow()
                        .map(|s| {
                            let g = s.click_pending.get();
                            if g {
                                s.click_pending.set(false);
                            }
                            g
                        })
                        .unwrap_or(false);
                    if go {
                        if let Some(p) = st2.borrow().player.as_ref() {
                            p.toggle_pause();
                        }
                    }
                });
            }
        });
        gl_area.add_controller(gesture);
    }
    {
        let weak = Rc::downgrade(&state);
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| {
            if let Some(st) = weak.upgrade() {
                poke_activity(&st, x, y);
            }
        });
        root.add_controller(motion);
    }
    {
        let weak = Rc::downgrade(&state);
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| {
            if let Some(st) = weak.upgrade() {
                poke_activity(&st, x, y);
            }
        });
        controls.controls.add_controller(motion);
    }
    {
        let weak = Rc::downgrade(&state);
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, _| {
            let Some(st) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let is_pop_open = {
                let s = st.borrow();
                s.speed_pop.is_visible()
            };
            if !is_pop_open {
                show_chrome(&st);
            }
            match keyval {
                gtk::gdk::Key::space => {
                    let s = st.borrow();
                    if let Some(p) = s.player.as_ref() {
                        p.toggle_pause();
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Right => {
                    let s = st.borrow();
                    if let Some(p) = s.player.as_ref() {
                        p.seek_rel(10.0);
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left => {
                    let s = st.borrow();
                    if let Some(p) = s.player.as_ref() {
                        p.seek_rel(-10.0);
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Up => {
                    let s = st.borrow();
                    if let Some(p) = s.player.as_ref() {
                        p.set_volume(p.volume() + 5.0);
                        p.set_mute(false);
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Down => {
                    let s = st.borrow();
                    if let Some(p) = s.player.as_ref() {
                        p.set_volume(p.volume() - 5.0);
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::m | gtk::gdk::Key::M => {
                    let s = st.borrow();
                    if let Some(p) = s.player.as_ref() {
                        p.set_mute(!p.muted());
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::f | gtk::gdk::Key::F => {
                    let on = !st.borrow().window.is_fullscreen();
                    set_fullscreen(&st, on);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::a | gtk::gdk::Key::A => {
                    let s = st.borrow();
                    if let Some(p) = s.player.as_ref() {
                        let fill = p.panscan() < 0.5;
                        p.set_panscan(if fill { 1.0 } else { 0.0 });
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::n | gtk::gdk::Key::N => {
                    let cb = st.borrow().host.on_neighbor.clone();
                    cb(1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::p | gtk::gdk::Key::P => {
                    let cb = st.borrow().host.on_neighbor.clone();
                    cb(-1);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        root.add_controller(key);
    }

    // İlk kaynak, GLArea realize olup render context kurulduktan sonra
    // connect_realize içinde yüklenir (libmpv render API sözleşmesi).

    // 500 ms süpervizör.
    {
        let weak = Rc::downgrade(&state);
        let start_volume = start_volume;
        glib::timeout_add_local(Duration::from_millis(500), move || {
            let Some(st) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !st.borrow().guard.is_alive() {
                return glib::ControlFlow::Break;
            }
            tick(&st, start_volume);
            glib::ControlFlow::Continue
        });
    }

    if req.auto_fullscreen {
        let weak = Rc::downgrade(&state);
        glib::timeout_add_local_once(Duration::from_millis(450), move || {
            if let Some(st) = weak.upgrade() {
                set_fullscreen(&st, true);
            }
        });
    }

    // İlk açılışta kısa tuş ipucu.
    controls
        .status
        .set_text("Boşluk: oynat/duraklat • F: tam ekran • ←/→: 10 sn");
    controls.status.set_visible(true);

    Some(EmbeddedPlayer {
        state,
        widget: root,
    })
}

fn load_initial_source(state: &Rc<RefCell<PlayerState>>) {
    let (url, index) = {
        let mut s = state.borrow_mut();
        if s.source_loaded {
            return;
        }
        s.source_loaded = true;
        (s.candidates.get(s.index).cloned(), s.index)
    };
    let Some(url) = url else {
        let s = state.borrow();
        s.status.set_text("Oynatılacak kaynak yok");
        s.status.set_visible(true);
        let on_terminal = s.host.on_terminal.clone();
        drop(s);
        on_terminal(
            crate::app::PlayTerminal::Failed,
            "Oynatılacak kaynak yok".into(),
        );
        return;
    };
    let result = {
        let s = state.borrow();
        let Some(p) = s.player.as_ref() else {
            return;
        };
        if let Some(referer) = sibnet_referer(&url) {
            p.set_http_headers(&referer);
        }
        p.load_url(&url)
    };
    if let Err(e) = result {
        let s = state.borrow();
        s.status.set_text(&format!("Yükleme hatası: {e}"));
        s.status.set_visible(true);
        let on_terminal = s.host.on_terminal.clone();
        drop(s);
        on_terminal(
            crate::app::PlayTerminal::Failed,
            format!("Kaynak yüklenemedi: {e}"),
        );
        return;
    }
    let s = state.borrow();
    s.status.set_text("yükleniyor…");
    s.status.set_visible(true);
    let _ = index;
}

fn tick(st: &Rc<RefCell<PlayerState>>, _start_volume: f64) {
    let (pos, dur, paused, idle, eof, volume, muted, first_load);
    {
        let s = match st.try_borrow() {
            Ok(s) => s,
            Err(_) => return,
        };
        let Some(p) = s.player.as_ref() else { return };
        pos = p.pos();
        dur = p.dur();
        paused = p.paused();
        idle = p.idle();
        eof = p.eof();
        volume = p.volume();
        muted = p.muted();
        first_load = dur > 0.0 && !s.media_loaded;
    }

    let (ready_now, finished_now) = {
        let mut s = match st.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => return,
        };
        if first_load {
            s.media_loaded = true;
            s.status.set_visible(false);
            if let Some(p) = s.player.as_ref() {
                p.set_pause(false);
            }
            if let Some(resume) = s.resume.take() {
                if resume >= 1.0 {
                    if let Some(p) = s.player.as_ref() {
                        p.seek_abs(resume);
                    }
                }
            }
            eprintln!("[EMBED] video hazır");
        }
        let ready_now = s.media_loaded && !s.ready_sent;
        if ready_now {
            s.ready_sent = true;
        }
        let finished_now = eof && dur > 0.0 && !s.terminal_sent;
        if finished_now {
            s.terminal_sent = true;
        }
        if !s.seeking.get() && dur > 0.0 && pos >= 1.0 {
            s.seek.set_value((pos / dur * 1000.0).clamp(0.0, 1000.0));
        }
        s.cur_lbl.set_text(&fmt_time(pos));
        s.dur_lbl.set_text(&fmt_time(dur));
        if paused {
            s.play_btn.set_icon_name("media-playback-start-symbolic");
        } else {
            s.play_btn.set_icon_name("media-playback-pause-symbolic");
        }
        if !s.vol_changing.get() {
            if (s.vol_scale.value() - volume).abs() > 0.5 {
                s.vol_scale.set_value(volume);
            }
            s.vol_btn.set_icon_name(vol_icon(volume, muted));
        }
        if s.media_loaded && pos >= 1.0 {
            s.progress
                .borrow_mut()
                .insert(s.progress_key.clone(), (pos, dur));
            s.host
                .client
                .save_progress(s.title.id, s.ep.season, s.ep.episode, pos, dur);
        }
        if eof {
            s.status.set_text("bitti");
            s.status.set_visible(true);
        }
        // krom oto-gizleme
        let smoke_hold = std::env::var_os("ANIMECIX_SMOKE_HOLD").is_some();
        let pop_open = s.speed_pop.is_visible();
        let idle_time = s.last_motion.elapsed();
        if !smoke_hold && !s.cursor_hidden && idle_time.as_secs() >= 1 && !pop_open {
            s.cursor_hidden = true;
            s.gl_area.set_cursor_from_name(Some("none"));
        }
        if !smoke_hold && !s.chrome_hidden && idle_time.as_secs() >= 2 && !pop_open {
            hide_chrome_locked(&mut s);
        }
        if !idle {
            s.gl_area.queue_render();
        }
        (ready_now, finished_now)
    };

    if ready_now {
        let cb = st.borrow().host.on_ready.clone();
        cb();
    }
    if finished_now {
        let cb = st.borrow().host.on_terminal.clone();
        cb(crate::app::PlayTerminal::Done, String::new());
    }
}

fn set_fullscreen(state: &Rc<RefCell<PlayerState>>, on: bool) {
    let mut s = state.borrow_mut();
    if on {
        s.window.fullscreen();
        s.we_fullscreened = true;
    } else {
        s.window.unfullscreen();
        s.we_fullscreened = false;
    }
    s.fs_btn.set_icon_name(if on {
        "view-restore-symbolic"
    } else {
        "view-fullscreen-symbolic"
    });
    s.last_motion = Instant::now();
    s.chrome_hidden = false;
    s.cursor_hidden = false;
    s.gl_area.set_cursor_from_name(None);
    show_chrome_locked(&mut s);
}

fn poke_activity(state: &Rc<RefCell<PlayerState>>, x: f64, y: f64) {
    let mut s = state.borrow_mut();
    if (x - s.last_mx).abs() < 0.5 && (y - s.last_my).abs() < 0.5 {
        return;
    }
    s.last_mx = x;
    s.last_my = y;
    s.last_motion = Instant::now();
    let need_show = s.chrome_hidden || s.cursor_hidden;
    if need_show {
        show_chrome_locked(&mut s);
    }
}

fn show_chrome(state: &Rc<RefCell<PlayerState>>) {
    if let Ok(mut s) = state.try_borrow_mut() {
        s.last_motion = Instant::now();
        show_chrome_locked(&mut s);
    }
}

fn show_chrome_locked(s: &mut PlayerState) {
    s.chrome_hidden = false;
    s.cursor_hidden = false;
    s.gl_area.set_cursor_from_name(None);
    s.topbar.set_reveal_child(true);
    s.controls.set_reveal_child(true);
    s.center.set_reveal_child(true);
    s.shade.set_reveal_child(true);
}

fn hide_chrome_locked(s: &mut PlayerState) {
    if s.chrome_hidden {
        return;
    }
    s.chrome_hidden = true;
    s.topbar.set_reveal_child(false);
    s.controls.set_reveal_child(false);
    s.center.set_reveal_child(false);
    s.shade.set_reveal_child(false);
    if !s.cursor_hidden {
        s.cursor_hidden = true;
        s.gl_area.set_cursor_from_name(Some("none"));
    }
}

fn vol_icon(volume: f64, muted: bool) -> &'static str {
    if muted || volume <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if volume < 35.0 {
        "audio-volume-low-symbolic"
    } else if volume < 70.0 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

fn sibnet_referer(url: &str) -> Option<String> {
    if !url.contains("video.sibnet.ru/v/") {
        return None;
    }
    let vid = url
        .split("/v/")
        .nth(1)
        .and_then(|s| s.split('/').nth(1))
        .map(|s| s.trim_end_matches(".mp4"))
        .unwrap_or("");
    Some(if vid.is_empty() {
        "https://video.sibnet.ru/".to_string()
    } else {
        format!("https://video.sibnet.ru/shell.php?videoid={vid}")
    })
}

pub fn player_page_name(title_id: u64, season: u64, episode: u64) -> String {
    format!("player-{title_id}-{season}-{episode}")
}

pub fn fmt_time(seconds: f64) -> String {
    crate::embed_mpv::fmt_time(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_embedded_player() {
        let s: crate::api::Settings = serde_json::from_str("{}").expect("defaults");
        assert!(s.embedded_player);
        assert!(crate::api::Settings::default().embedded_player);
    }

    #[test]
    fn player_page_name_is_stable() {
        assert_eq!(player_page_name(42, 1, 3), "player-42-1-3");
    }

    #[test]
    fn fmt_time_handles_hours_and_invalid() {
        assert_eq!(fmt_time(65.0), "1:05");
        assert_eq!(fmt_time(3661.0), "1:01:01");
        assert_eq!(fmt_time(-1.0), "--:--");
    }

    #[test]
    fn shutdown_guard_is_idempotent() {
        let g = PlaybackGuard::new();
        assert!(g.is_alive());
        assert!(g.mark_dead());
        assert!(!g.mark_dead());
        assert!(!g.is_alive());
    }
}

/// Oynatıcı yaşam döngüsü. `shutdown` iki kez çağrılsa bile yalnız ilki
/// kaynakları serbest bırakır; ikincisi no-op olmalıdır.
pub struct PlaybackGuard {
    alive: AtomicBool,
}

impl PlaybackGuard {
    pub fn new() -> Self {
        Self {
            alive: AtomicBool::new(true),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// İlk çağrıda `true`, sonrakilerde `false`.
    pub fn mark_dead(&self) -> bool {
        self.alive.swap(false, Ordering::Relaxed)
    }
}

impl Default for PlaybackGuard {
    fn default() -> Self {
        Self::new()
    }
}
