pub mod episodes;
pub mod menu;
pub mod notices;
pub mod source;
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

use self::episodes::EpisodeDrawer;
use self::notices::SkipKind;
use self::source::{resume_after_switch, Source, SourceQueue};

/// Gömülü oynatıcının uygulamaya bağlanan callback'leri.
pub struct PlayerHost {
    pub client: Arc<Client>,
    pub ops: OperationStack,
    pub discord: DiscordRpc,
    pub progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    pub on_back: Rc<dyn Fn()>,
    pub on_play_episode: Rc<dyn Fn(Episode)>,
    pub on_select_fansub: Rc<dyn Fn(api::FansubInfo)>,
    pub on_neighbor: Rc<dyn Fn(i8)>,
    pub on_download: Rc<dyn Fn(Episode)>,
    pub on_ready: Rc<dyn Fn()>,
    pub on_terminal: Rc<dyn Fn(crate::app::PlayTerminal, String)>,
}

pub struct EmbedRequest {
    pub title: Title,
    pub ep: Episode,
    pub candidates: Vec<String>,
    pub fast_embeds: Vec<String>,
    pub fallback_embeds: Vec<String>,
    pub source_labels: Vec<String>,
    pub fallback_labels: Vec<String>,
    pub fansubs: Vec<api::FansubInfo>,
    pub fansub_id: i64,
    pub quality_map: HashMap<String, String>,
    pub plan: Option<SkipPlan>,
    pub saved_pos: Option<f64>,
    pub upscale: String,
    pub auto_fullscreen: bool,
    pub show_skip_button: bool,
    pub show_song_card: bool,
    pub show_music_hint: bool,
    pub cache_forward_mb: u64,
    pub cache_back_mb: u64,
    pub patience_secs: u64,
    pub auto_next: bool,
    pub auto_skip_outro: bool,
    pub episodes: Vec<Episode>,
}

struct PlayerState {
    guard: PlaybackGuard,
    alive: Arc<AtomicBool>,
    player: Option<MpvEmbed>,
    render_ctx: Option<RenderContext>,
    host: PlayerHost,
    title: Title,
    ep: Episode,
    queue: SourceQueue,
    media_loaded: bool,
    ready_sent: bool,
    terminal_sent: bool,
    source_loaded: bool,
    resolving: bool,
    pending_resolve: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    load_started: Instant,
    patience_secs: u64,
    resume: Option<f64>,
    watched_marked: bool,
    host_saved: bool,
    auto_next_enabled: bool,
    auto_skip_outro: bool,
    auto_next_fired: bool,
    plan: Option<SkipPlan>,
    plan_rx: Option<std::sync::mpsc::Receiver<Option<SkipPlan>>>,
    plan_requested: bool,
    fast_embeds: Vec<String>,
    fallback_embeds: Vec<String>,
    op_done: bool,
    ed_done: bool,
    cards_kind: Option<SkipKind>,
    cards_deadline: Option<Instant>,
    cards_hidden: bool,
    show_skip_button: bool,
    show_song_card: bool,
    show_music_hint: bool,
    song_card: ui::SongCard,
    skip_pill_rev: gtk::Revealer,
    skip_pill: gtk::Button,
    drawer: EpisodeDrawer,
    episodes: Vec<Episode>,
    episode_rx: Option<std::sync::mpsc::Receiver<Result<Vec<Episode>, String>>>,
    fansubs: Vec<api::FansubInfo>,
    fansub_id: i64,
    window: adw::ApplicationWindow,
    gl_area: gtk::GLArea,
    topbar: gtk::Revealer,
    shade: gtk::Revealer,
    controls: gtk::Revealer,
    center: gtk::Revealer,
    play_btn: gtk::Button,
    seek: gtk::Scale,
    spinner: ui::BusySpinner,
    cur_lbl: gtk::Label,
    dur_lbl: gtk::Label,
    vol_btn: gtk::Button,
    vol_scale: gtk::Scale,
    fs_btn: gtk::Button,
    status: gtk::Label,
    speed_pop: gtk::Popover,
    quality_btn: gtk::MenuButton,
    quality_box: gtk::Box,
    quality_pop: gtk::Popover,
    ctx_pop: gtk::Popover,
    last_motion: Instant,
    last_mx: f64,
    last_my: f64,
    chrome_hidden: bool,
    cursor_hidden: bool,
    we_fullscreened: bool,
    click_pending: Rc<Cell<bool>>,
    seeking: Rc<Cell<bool>>,
    seek_spinner: Option<(Instant, f64)>,
    vol_changing: Rc<Cell<bool>>,
    progress_key: String,
    progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
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

    /// Klavye odağını video alanına verir; oynatıcı açılır açılmaz
    /// Space/F/S/E gibi tuşlar ilk tıklama olmadan çalışsın.
    pub fn focus(&self) {
        self.state.borrow().gl_area.grab_focus();
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
        s.host.discord.clear();
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
        ("cache-pause-wait".into(), "1".into()),
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
    let queue = SourceQueue::new(
        &req.candidates,
        &req.fast_embeds,
        &req.fallback_embeds,
        &req.quality_map,
        &req.source_labels,
        &req.fallback_labels,
    );
    let is_movie = req.title.title_type.as_deref() == Some("movie");
    let discord_state =
        crate::discord_rpc::episode_state(req.ep.season, req.ep.episode, &req.ep.name, is_movie);
    host.discord
        .set_activity(crate::discord_rpc::Activity::from_playback(
            &req.title.name,
            &discord_state,
            req.saved_pos.unwrap_or(0.0),
            0.0,
            false,
            req.title.poster.as_deref(),
        ));

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
    let skip_card = ui::build_song_card(&stage);
    let (skip_pill_rev, skip_pill) = ui::build_skip_pill(&stage);
    // Kartlar kontrol barının ALTINDA kalsın: bar tıklamaları öncelikli.
    let controls = ui::build_control_bar(&stage, start_volume, start_muted, &media_title);
    let topbar = ui::build_topbar(&stage);
    let drawer = episodes::build(&stage);
    let initial_episodes = req.episodes.clone();
    let on_play_episode = host.on_play_episode.clone();
    let episode_rx = if initial_episodes.len() <= 1 {
        let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<Episode>, String>>();
        let client = host.client.clone();
        let title = req.title.clone();
        std::thread::spawn(move || {
            let result = client
                .episodes(&client.enrich_title(&title))
                .map(|mut eps| {
                    eps.sort_by_key(|e| (e.season, e.episode));
                    eps
                });
            let _ = tx.send(result);
        });
        Some(rx)
    } else {
        None
    };

    let alive = Arc::new(AtomicBool::new(true));
    let click_pending = Rc::new(Cell::new(false));
    let seeking = Rc::new(Cell::new(false));
    let vol_changing = Rc::new(Cell::new(false));
    let progress_key = format!("{}:{}:{}", req.title.id, req.ep.season, req.ep.episode);
    let host_progress = host.progress.clone();
    let ctx_pop = gtk::Popover::new();
    ctx_pop.set_parent(&gl_area);
    ctx_pop.add_css_class("menu");
    let state = Rc::new(RefCell::new(PlayerState {
        guard: PlaybackGuard::new(),
        alive: alive.clone(),
        player: Some(player),
        render_ctx: None,
        host,
        title: req.title.clone(),
        ep: req.ep.clone(),
        queue,
        media_loaded: false,
        ready_sent: false,
        terminal_sent: false,
        source_loaded: false,
        resolving: false,
        pending_resolve: None,
        load_started: Instant::now(),
        patience_secs: req.patience_secs.max(10),
        resume: req.saved_pos,
        watched_marked: false,
        host_saved: false,
        auto_next_enabled: req.auto_next,
        auto_skip_outro: req.auto_skip_outro,
        auto_next_fired: false,
        plan: req.plan.clone(),
        plan_rx: None,
        plan_requested: false,
        fast_embeds: req.fast_embeds.clone(),
        fallback_embeds: req.fallback_embeds.clone(),
        op_done: false,
        ed_done: false,
        cards_kind: None,
        cards_deadline: None,
        cards_hidden: false,
        show_skip_button: req.show_skip_button,
        show_song_card: req.show_song_card,
        show_music_hint: req.show_music_hint,
        song_card: skip_card.clone(),
        skip_pill_rev: skip_pill_rev.clone(),
        skip_pill: skip_pill.clone(),
        drawer: drawer.clone(),
        episodes: initial_episodes.clone(),
        episode_rx,
        fansubs: req.fansubs.clone(),
        fansub_id: req.fansub_id,
        window: parent.clone(),
        gl_area: gl_area.clone(),
        topbar: topbar.revealer.clone(),
        shade: shade.clone(),
        controls: controls.controls_revealer.clone(),
        center: controls.center_rev.clone(),
        play_btn: controls.play_btn.clone(),
        seek: controls.seek.clone(),
        spinner: controls.spinner.clone(),
        cur_lbl: controls.cur_lbl.clone(),
        dur_lbl: controls.dur_lbl.clone(),
        vol_btn: controls.vol_btn.clone(),
        vol_scale: controls.vol_scale.clone(),
        fs_btn: topbar.fullscreen.clone(),
        status: controls.status.clone(),
        speed_pop: controls.speed_pop.clone(),
        quality_btn: controls.quality_btn.clone(),
        quality_box: controls.quality_box.clone(),
        quality_pop: controls.quality_pop.clone(),
        ctx_pop: ctx_pop.clone(),
        last_motion: Instant::now(),
        last_mx: -1.0,
        last_my: -1.0,
        chrome_hidden: false,
        cursor_hidden: false,
        we_fullscreened: false,
        click_pending: click_pending.clone(),
        seeking: seeking.clone(),
        seek_spinner: None,
        vol_changing: vol_changing.clone(),
        progress_key,
        progress: host_progress,
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
                    let resume = s.resume;
                    drop(s);
                    if should_load {
                        let st_load = st.clone();
                        // Kritik yükleme idle önceliğine bırakılmaz: yoğun
                        // çizim/animasyon kaynakları idle'ı açlığa uğratabiliyor.
                        glib::timeout_add_local_once(Duration::from_millis(1), move || {
                            start_source(&st_load, resume);
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
                if let Ok(mut s) = st.try_borrow_mut() {
                    let target = {
                        let Some(p) = s.player.as_ref() else { return };
                        let target = (p.pos() - 10.0).max(0.0);
                        p.seek_rel(-10.0);
                        target
                    };
                    mark_seek_feedback(&mut s, target);
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(&state);
        controls.fwd10_btn.connect_clicked(move |_| {
            if let Some(st) = weak.upgrade() {
                if let Ok(mut s) = st.try_borrow_mut() {
                    let target = {
                        let Some(p) = s.player.as_ref() else { return };
                        let target = p.pos() + 10.0;
                        p.seek_rel(10.0);
                        target
                    };
                    mark_seek_feedback(&mut s, target);
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
                if let Ok(mut s) = st.try_borrow_mut() {
                    let target = s
                        .player
                        .as_ref()
                        .map(|p| p.dur())
                        .filter(|d| *d > 0.0)
                        .map(|d| d * v / 1000.0);
                    if let Some(target) = target {
                        if let Some(p) = s.player.as_ref() {
                            p.seek_abs(target);
                        }
                        mark_seek_feedback(&mut s, target);
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
        ("0.5x", 0.5),
        ("0.75x", 0.75),
        ("1.0x", 1.0),
        ("1.25x", 1.25),
        ("1.5x", 1.5),
        ("1.75x", 1.75),
        ("2.0x", 2.0),
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
    // Kaynak listesi.
    {
        let weak = Rc::downgrade(&state);
        topbar.back.set_visible(true);
        controls.quality_pop.connect_show(move |_| {
            if let Some(st) = weak.upgrade() {
                rebuild_quality_menu(&st);
            }
        });
        rebuild_quality_menu(&state);
    }
    {
        let weak = Rc::downgrade(&state);
        skip_card.song.connect_clicked(move |_| {
            if let Some(st) = weak.upgrade() {
                open_song(&st);
            }
        });
    }
    {
        let weak = Rc::downgrade(&state);
        skip_pill.connect_clicked(move |_| {
            if let Some(st) = weak.upgrade() {
                do_skip(&st);
            }
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
        let pop = ctx_pop.clone();
        let weak = Rc::downgrade(&state);
        let pop_c = pop.clone();
        let rc = gtk::GestureClick::new();
        rc.set_button(3);
        rc.connect_pressed(move |_, _, x, y| {
            if let Some(st) = weak.upgrade() {
                rebuild_ctx_menu(&pop_c, &st);
                pop_c.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                pop_c.popup();
            }
        });
        gl_area.add_controller(rc);
        let weak = Rc::downgrade(&state);
        pop.connect_closed(move |_| {
            if let Some(st) = weak.upgrade() {
                if let Ok(mut s) = st.try_borrow_mut() {
                    s.last_motion = Instant::now();
                    s.chrome_hidden = false;
                    s.cursor_hidden = false;
                    s.gl_area.set_cursor_from_name(None);
                    show_chrome_locked(&mut s);
                }
            }
        });
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
        key.connect_key_pressed(move |_, keyval, _, mods| {
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
                    if let Ok(mut s) = st.try_borrow_mut() {
                        let target = {
                            let Some(p) = s.player.as_ref() else {
                                return glib::Propagation::Stop;
                            };
                            let target = p.pos() + 10.0;
                            p.seek_rel(10.0);
                            target
                        };
                        mark_seek_feedback(&mut s, target);
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left => {
                    if let Ok(mut s) = st.try_borrow_mut() {
                        let target = {
                            let Some(p) = s.player.as_ref() else {
                                return glib::Propagation::Stop;
                            };
                            let target = (p.pos() - 10.0).max(0.0);
                            p.seek_rel(-10.0);
                            target
                        };
                        mark_seek_feedback(&mut s, target);
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
                    if mods.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                        open_song(&st);
                    } else {
                        let s = st.borrow();
                        if let Some(p) = s.player.as_ref() {
                            p.set_mute(!p.muted());
                        }
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
                gtk::gdk::Key::s | gtk::gdk::Key::S => {
                    skip_to(&st, SkipKind::Intro);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::e | gtk::gdk::Key::E => {
                    skip_to(&st, SkipKind::Outro);
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
    {
        // Sayfa stack'e eklenip görünür olunca odağı klavye kontrolüne ver.
        let area = gl_area.clone();
        gl_area.connect_map(move |_| {
            area.grab_focus();
        });
    }

    // İlk kaynak, GLArea realize olup render context kurulduktan sonra
    // connect_realize içinde yüklenir (libmpv render API sözleşmesi).
    episodes::render(
        &drawer,
        &initial_episodes,
        req.ep.season,
        req.ep.episode,
        req.title.runtime,
        &on_play_episode,
    );

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

fn start_source(state: &Rc<RefCell<PlayerState>>, resume: Option<f64>) {
    let src = {
        let mut s = match state.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => return,
        };
        if s.resolving {
            return;
        }
        let Some(src) = s.queue.current().cloned() else {
            drop(s);
            fail_source(state, "Oynatılacak kaynak yok");
            return;
        };
        s.source_loaded = true;
        s.media_loaded = false;
        s.seek_spinner = None;
        s.load_started = Instant::now();
        s.resume = resume;
        set_spinner(&mut s, true);
        src
    };

    match src {
        Source::Direct(url) => {
            let result = {
                let s = state.borrow();
                let Some(p) = s.player.as_ref() else { return };
                if let Some(referer) = sibnet_referer(&url) {
                    p.set_http_headers(&referer);
                }
                p.load_url(&url)
            };
            match result {
                Ok(()) => {
                    let s = state.borrow();
                    s.status.set_text("yükleniyor…");
                    s.status.set_visible(true);
                }
                Err(e) => {
                    eprintln!("[EMBED] kaynak yüklenemedi: {e}");
                    advance_source(state);
                }
            }
        }
        Source::Embed(url) => {
            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            let client = match state.try_borrow() {
                Ok(s) => s.host.client.clone(),
                Err(_) => return,
            };
            std::thread::spawn(move || {
                let _ = tx.send(client.resolve_single(&url));
            });
            let mut s = match state.try_borrow_mut() {
                Ok(s) => s,
                Err(_) => return,
            };
            s.resolving = true;
            s.pending_resolve = Some(rx);
            s.status.set_text("yedek kaynak çözümleniyor…");
            s.status.set_visible(true);
        }
    }
}

fn advance_source(state: &Rc<RefCell<PlayerState>>) {
    let resume = match state.try_borrow() {
        Ok(s) => s.player.as_ref().and_then(|p| {
            let pos = p.pos();
            let dur = p.dur();
            resume_after_switch(pos, dur)
        }),
        Err(_) => None,
    };
    let next = match state.try_borrow_mut() {
        Ok(mut s) => match s.queue.next_after_failure() {
            Some(n) => {
                s.queue.index = n;
                Some(n)
            }
            None => None,
        },
        Err(_) => None,
    };
    if next.is_some() {
        eprintln!("[EMBED] sonraki kaynağa geçiliyor");
        start_source(state, resume);
    } else {
        fail_source(state, "Bölüm hiçbir kaynakta açılamadı");
    }
}

fn fail_source(state: &Rc<RefCell<PlayerState>>, msg: &str) {
    let (status, terminal, callback) = match state.try_borrow() {
        Ok(s) => (
            s.status.clone(),
            s.terminal_sent,
            s.host.on_terminal.clone(),
        ),
        Err(_) => return,
    };
    status.set_text(msg);
    status.set_visible(true);
    if !terminal {
        if let Ok(mut s) = state.try_borrow_mut() {
            s.terminal_sent = true;
        }
        callback(crate::app::PlayTerminal::Failed, msg.to_string());
    }
}

fn rebuild_quality_menu(state: &Rc<RefCell<PlayerState>>) {
    let (sources, quality, labels, index, fansubs, fansub_id, current_height) =
        match state.try_borrow() {
            Ok(s) => (
                s.queue.sources.clone(),
                s.queue.quality.clone(),
                s.queue.labels.clone(),
                s.queue.index,
                s.fansubs.clone(),
                s.fansub_id,
                s.player.as_ref().and_then(|p| p.video_height()),
            ),
            Err(_) => return,
        };
    let Ok(s) = state.try_borrow() else { return };
    let box_ = s.quality_box.clone();
    let btn = s.quality_btn.clone();
    let pop = s.quality_pop.clone();
    box_.set_size_request(300, -1);
    let mut cur = box_.first_child();
    while let Some(child) = cur {
        let next = child.next_sibling();
        box_.remove(&child);
        cur = next;
    }

    section(&box_, "ÇEVİRİ", 8);
    if fansubs.is_empty() {
        let l = gtk::Label::new(Some("Çeviri bilgisi yok"));
        l.add_css_class("dim-label");
        l.set_xalign(0.0);
        l.set_margin_start(10);
        box_.append(&l);
    } else {
        for fs in &fansubs {
            let row = gtk::Button::new();
            row.add_css_class("flat");
            row.add_css_class("fan-row");
            row.set_halign(gtk::Align::Fill);
            let h = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            h.set_margin_top(6);
            h.set_margin_bottom(6);
            h.set_margin_start(10);
            h.set_margin_end(10);
            let name = gtk::Label::new(Some(&fs.name));
            name.add_css_class("fan-name");
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            h.append(&name);
            let rating = gtk::Label::new(Some(&format!("★ {:.1}", fs.rating)));
            rating.add_css_class("fan-rating");
            h.append(&rating);
            let check = gtk::Label::new(Some(if fs.template_id == fansub_id {
                "✓"
            } else {
                " "
            }));
            check.add_css_class("fan-check");
            check.set_width_chars(2);
            h.append(&check);
            row.set_child(Some(&h));
            let weak = Rc::downgrade(state);
            let chosen = fs.clone();
            let pop_c = pop.clone();
            row.connect_clicked(move |_| {
                let Some(st) = weak.upgrade() else { return };
                pop_c.popdown();
                if chosen.template_id == st.borrow().fansub_id {
                    return;
                }
                let cb = st.borrow().host.on_select_fansub.clone();
                cb(chosen.clone());
            });
            box_.append(&row);
        }
    }

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.set_margin_top(8);
    sep.set_margin_bottom(6);
    box_.append(&sep);
    section(&box_, "KAYNAK", 10);
    if sources.is_empty() {
        let l = gtk::Label::new(Some("Kaynak yok"));
        l.add_css_class("dim-label");
        l.set_xalign(0.0);
        l.set_margin_start(10);
        box_.append(&l);
    } else {
        for (i, src) in sources.iter().enumerate() {
            let host = crate::api::Client::source_host_hint(src.hint_url());
            let host = if host.is_empty() { "Kaynak" } else { host };
            let q = if i == index {
                current_height.map(|h| format!("{h}p"))
            } else {
                quality.get(i).and_then(|q| q.clone())
            };
            let row = gtk::Button::new();
            row.add_css_class("flat");
            row.add_css_class("source-row");
            row.set_halign(gtk::Align::Fill);
            let h = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            h.set_margin_top(4);
            h.set_margin_bottom(4);
            h.set_margin_start(16);
            h.set_margin_end(10);
            let name = gtk::Label::new(Some(host));
            name.add_css_class("source-name");
            name.set_xalign(0.0);
            name.set_hexpand(true);
            h.append(&name);
            if let Some(q) = q {
                let ql = gtk::Label::new(Some(&q));
                ql.add_css_class("source-quality");
                h.append(&ql);
            }
            let check = gtk::Label::new(Some(if i == index { "✓" } else { " " }));
            check.add_css_class("fan-check");
            check.set_width_chars(2);
            h.append(&check);
            row.set_child(Some(&h));
            let weak = Rc::downgrade(state);
            row.connect_clicked(move |_| {
                if let Some(st) = weak.upgrade() {
                    pick_source(&st, i);
                }
            });
            box_.append(&row);
        }
    }

    let cur = sources
        .get(index)
        .map(|_| {
            labels
                .get(index)
                .map(String::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    sources
                        .get(index)
                        .map(|s| crate::api::Client::source_host_hint(s.hint_url()).to_string())
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Kaynak".to_string())
        })
        .unwrap_or_else(|| "Kaynak".to_string());
    btn.set_label(&cur);
}

fn section(box_: &gtk::Box, text: &str, top_margin: i32) {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("section-label");
    l.set_xalign(0.0);
    l.set_margin_start(10);
    l.set_margin_top(top_margin);
    l.set_margin_bottom(4);
    box_.append(&l);
}

fn pick_source(state: &Rc<RefCell<PlayerState>>, index: usize) {
    let resume = match state.try_borrow() {
        Ok(s) => s
            .player
            .as_ref()
            .and_then(|p| resume_after_switch(p.pos(), p.dur())),
        Err(_) => None,
    };
    {
        let Ok(mut s) = state.try_borrow_mut() else {
            return;
        };
        if index >= s.queue.len() {
            return;
        }
        s.queue.index = index;
        s.pending_resolve = None;
        s.resolving = false;
        s.status.set_text("kaynak değiştiriliyor…");
        s.status.set_visible(true);
    }
    if let Ok(s) = state.try_borrow() {
        s.quality_pop.popdown();
    }
    start_source(state, resume);
}

fn mark_skip_done(s: &mut PlayerState, kind: SkipKind) {
    match kind {
        SkipKind::Intro => {
            s.op_done = true;
        }
        SkipKind::Outro => {
            s.ed_done = true;
        }
    }
    s.song_card.hide();
    s.skip_pill_rev.set_reveal_child(false);
}

fn do_skip(state: &Rc<RefCell<PlayerState>>) {
    let target = {
        let Ok(s) = state.try_borrow() else { return };
        let Some(plan) = s.plan.as_ref() else { return };
        let Some(p) = s.player.as_ref() else { return };
        notices::skip_target(&plan.times, p.pos(), s.op_done, s.ed_done)
    };
    if let Some((target, kind)) = target {
        let mut s = state.borrow_mut();
        mark_skip_done(&mut s, kind);
        if let Some(p) = s.player.as_ref() {
            p.seek_abs(target);
        }
        s.host.ops.info(
            &format!("{} atlandı", kind.short()),
            "",
            std::time::Duration::from_secs(2),
        );
    }
}

fn skip_to(state: &Rc<RefCell<PlayerState>>, kind: SkipKind) {
    let target = {
        let Ok(s) = state.try_borrow() else { return };
        let Some(plan) = s.plan.as_ref() else {
            return;
        };
        match kind {
            SkipKind::Intro => plan.times.op_end,
            SkipKind::Outro => plan.times.ed_end,
        }
    };
    match target {
        Some(target) => {
            let mut s = state.borrow_mut();
            mark_skip_done(&mut s, kind);
            if let Some(p) = s.player.as_ref() {
                p.seek_abs(target);
            }
            s.host.ops.info(
                &format!("{} atlandı", kind.short()),
                "",
                std::time::Duration::from_secs(2),
            );
        }
        None => {
            let s = state.borrow();
            s.host.ops.info(
                &format!("{} zamanı yok", kind.short()),
                "",
                std::time::Duration::from_secs(2),
            );
        }
    }
}

fn open_song(state: &Rc<RefCell<PlayerState>>) {
    let (url, hint) = {
        let Ok(s) = state.try_borrow() else { return };
        (
            s.plan.as_ref().and_then(|p| p.song_url.clone()),
            s.show_music_hint,
        )
    };
    if let Some(url) = url {
        if let Ok(s) = state.try_borrow() {
            s.window.minimize();
        }
        if !crate::music::open_song(&url) {
            let s = state.borrow();
            s.host.ops.error(
                "Şarkı açılamadı",
                "Tarayıcı başlatılamadı.",
                std::time::Duration::from_secs(3),
            );
        }
    } else if hint {
        let s = state.borrow();
        s.host.ops.info(
            "Şarkı bilgisi yok",
            "Bu bölüm için şarkı bağlantısı bulunamadı.",
            std::time::Duration::from_secs(2),
        );
    }
}

fn ctx_row(
    pop: &gtk::Popover,
    parent: &gtk::Box,
    label: &str,
    icon: &str,
    sensitive: bool,
    hint: Option<&str>,
    f: impl Fn() + 'static,
) {
    let b = gtk::Button::new();
    b.add_css_class("flat");
    b.set_sensitive(sensitive);
    b.set_hexpand(true);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_hexpand(true);
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(8);
    row.set_margin_end(8);
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(16);
    img.set_valign(gtk::Align::Center);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&img);
    row.append(&lbl);
    if let Some(h) = hint.filter(|h| !h.is_empty()) {
        let hl = gtk::Label::new(Some(h));
        hl.add_css_class("dim-label");
        hl.add_css_class("caption");
        row.append(&hl);
    }
    b.set_child(Some(&row));
    let pop_c = pop.clone();
    b.connect_clicked(move |_| {
        pop_c.popdown();
        f();
    });
    parent.append(&b);
}

fn ctx_sep(parent: &gtk::Box) {
    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    parent.append(&sep);
}

fn rebuild_ctx_menu(pop: &gtk::Popover, state: &Rc<RefCell<PlayerState>>) {
    let (paused, fullscreen, skip, fill, title, season, episode, ep, can_download) = {
        let Ok(s) = state.try_borrow() else { return };
        let Some(p) = s.player.as_ref() else { return };
        (
            p.paused(),
            s.window.is_fullscreen(),
            notices::skip_target(
                &s.plan.as_ref().map(|p| p.times.clone()).unwrap_or_default(),
                p.pos(),
                s.op_done,
                s.ed_done,
            ),
            p.panscan() >= 0.5,
            s.title.name.clone(),
            s.ep.season,
            s.ep.episode,
            s.ep.clone(),
            s.host.on_download.clone(),
        )
    };
    let v = gtk::Box::new(gtk::Orientation::Vertical, 2);
    v.set_margin_top(6);
    v.set_margin_bottom(6);
    v.set_margin_start(6);
    v.set_margin_end(6);

    {
        let weak = Rc::downgrade(state);
        ctx_row(
            pop,
            &v,
            if paused { "Oynat" } else { "Duraklat" },
            if paused {
                "media-playback-start-symbolic"
            } else {
                "media-playback-pause-symbolic"
            },
            true,
            Some("Space"),
            move || {
                if let Some(st) = weak.upgrade() {
                    if let Ok(s) = st.try_borrow() {
                        if let Some(p) = s.player.as_ref() {
                            p.toggle_pause();
                        }
                    }
                }
            },
        );
    }
    {
        let weak = Rc::downgrade(state);
        ctx_row(
            pop,
            &v,
            if fullscreen {
                "Pencereli moda dön"
            } else {
                "Tam ekran"
            },
            if fullscreen {
                "view-restore-symbolic"
            } else {
                "view-fullscreen-symbolic"
            },
            true,
            Some("F"),
            move || {
                if let Some(st) = weak.upgrade() {
                    set_fullscreen(&st, !fullscreen);
                }
            },
        );
    }
    ctx_sep(&v);
    {
        let kind = skip.map(|(_, k)| k);
        let weak = Rc::downgrade(state);
        let label = kind.map(SkipKind::action).unwrap_or("Atlanacak yer yok");
        ctx_row(
            pop,
            &v,
            label,
            "media-skip-forward-symbolic",
            kind.is_some(),
            kind.map(|k| if k == SkipKind::Intro { "S" } else { "E" }),
            move || {
                if let (Some(st), Some(kind)) = (weak.upgrade(), kind) {
                    skip_to(&st, kind);
                }
            },
        );
    }
    {
        let weak = Rc::downgrade(state);
        ctx_row(
            pop,
            &v,
            "Önceki bölüm",
            "go-previous-symbolic",
            true,
            None,
            move || {
                if let Some(st) = weak.upgrade() {
                    let cb = st.borrow().host.on_neighbor.clone();
                    cb(-1);
                }
            },
        );
    }
    {
        let weak = Rc::downgrade(state);
        ctx_row(
            pop,
            &v,
            "Sonraki bölüm",
            "go-next-symbolic",
            true,
            None,
            move || {
                if let Some(st) = weak.upgrade() {
                    let cb = st.borrow().host.on_neighbor.clone();
                    cb(1);
                }
            },
        );
    }
    ctx_sep(&v);
    {
        let weak = Rc::downgrade(state);
        ctx_row(
            pop,
            &v,
            if fill {
                "Orijinal oran"
            } else {
                "Ekranı doldur"
            },
            "view-fullscreen-symbolic",
            true,
            Some("A"),
            move || {
                if let Some(st) = weak.upgrade() {
                    if let Ok(s) = st.try_borrow() {
                        if let Some(p) = s.player.as_ref() {
                            p.set_panscan(if fill { 0.0 } else { 1.0 });
                        }
                        s.host.ops.info(
                            if fill {
                                "Orijinal oran"
                            } else {
                                "Ekranı doldur"
                            },
                            "",
                            std::time::Duration::from_secs(2),
                        );
                    }
                }
            },
        );
    }
    {
        let weak = Rc::downgrade(state);
        let win = state.borrow().window.clone();
        let title_c = title.clone();
        ctx_row(
            pop,
            &v,
            "Ekran görüntüsü al",
            "camera-photo-symbolic",
            true,
            None,
            move || {
                if let Some(st) = weak.upgrade() {
                    take_screenshot(&st, &win, &title_c, season, episode);
                }
            },
        );
    }
    {
        let weak = Rc::downgrade(state);
        let win = state.borrow().window.clone();
        ctx_row(
            pop,
            &v,
            "Video bilgisi",
            "dialog-information-symbolic",
            true,
            None,
            move || {
                if let Some(st) = weak.upgrade() {
                    show_video_info(&st, &win);
                }
            },
        );
    }
    {
        let cb = can_download;
        ctx_row(
            pop,
            &v,
            "Bölümü indir",
            "folder-download-symbolic",
            true,
            None,
            move || {
                cb(ep.clone());
            },
        );
    }
    pop.set_child(Some(&v));
}

fn take_screenshot(
    state: &Rc<RefCell<PlayerState>>,
    window: &adw::ApplicationWindow,
    title: &str,
    season: u64,
    episode: u64,
) {
    let Some(pictures) = glib::user_special_dir(glib::UserDirectory::Pictures) else {
        let s = state.borrow();
        s.host.ops.error(
            "Ekran görüntüsü",
            "Resim klasörü bulunamadı.",
            std::time::Duration::from_secs(3),
        );
        return;
    };
    let dir = pictures.join("AnimeciX");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        let s = state.borrow();
        s.host.ops.error(
            "Ekran görüntüsü",
            &format!("Klasör oluşturulamadı: {e}"),
            std::time::Duration::from_secs(3),
        );
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(menu::screenshot_filename(title, season, episode, ts));
    let result = {
        let Ok(s) = state.try_borrow() else { return };
        let Some(p) = s.player.as_ref() else { return };
        p.screenshot_to_file(&path.to_string_lossy())
    };
    match result {
        Ok(()) => {
            let file = gtk::gio::File::for_path(&path);
            if let Ok(tex) = gtk::gdk::Texture::from_file(&file) {
                gtk::prelude::WidgetExt::display(window)
                    .clipboard()
                    .set_texture(&tex);
            }
            let s = state.borrow();
            s.host.ops.info(
                "Ekran görüntüsü",
                &path.to_string_lossy(),
                std::time::Duration::from_secs(4),
            );
        }
        Err(e) => {
            let s = state.borrow();
            s.host.ops.error(
                "Ekran görüntüsü",
                &format!("Alınamadı: {e}"),
                std::time::Duration::from_secs(3),
            );
        }
    }
}

fn show_video_info(state: &Rc<RefCell<PlayerState>>, window: &adw::ApplicationWindow) {
    let info = {
        let Ok(s) = state.try_borrow() else { return };
        let Some(p) = s.player.as_ref() else { return };
        p.video_info()
    };
    let heading = {
        let s = state.borrow();
        format!("{} — S{:02}E{:02}", s.title.name, s.ep.season, s.ep.episode)
    };
    let dlg = adw::MessageDialog::builder()
        .heading(&heading)
        .body(menu::info_text(&info))
        .close_response("ok")
        .default_response("ok")
        .build();
    dlg.add_response("ok", "Tamam");
    dlg.set_transient_for(Some(window));
    dlg.present();
}

fn load_resolved_source(state: &Rc<RefCell<PlayerState>>, url: &str) {
    let result = {
        let mut s = match state.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => return,
        };
        s.load_started = Instant::now();
        s.media_loaded = false;
        s.status.set_text("yükleniyor…");
        s.status.set_visible(true);
        let Some(p) = s.player.as_ref() else { return };
        if let Some(referer) = sibnet_referer(url) {
            p.set_http_headers(&referer);
        }
        p.load_url(url)
    };
    if let Err(e) = result {
        eprintln!("[EMBED] çözülen kaynak yüklenemedi: {e}");
        advance_source(state);
    }
}

/// Önbellek sonunun süreye oranı (0..1). `demuxer-cache-time` oynatma
/// konumunun gerisinde kalırsa çubuk geriye sarmasın diye `pos` taban alınır.
fn buffer_percent(pos: f64, cache_time: f64, dur: f64) -> f64 {
    if !(dur > 0.0) || !cache_time.is_finite() {
        return 0.0;
    }
    (cache_time.max(pos).max(0.0) / dur).clamp(0.0, 1.0)
}

/// Oynatma butonundaki dönengeci aç/kapat. İkon dönerken gizlenir.
fn set_spinner(s: &mut PlayerState, on: bool) {
    s.spinner.set_active(on);
    if on {
        s.play_btn.add_css_class("player-loading");
        if s.chrome_hidden {
            show_chrome_locked(s);
        }
    } else {
        s.play_btn.remove_css_class("player-loading");
    }
}

/// Kullanıcı seek'i sonrası oynatma butonunda kısa süre dönengeç göster.
fn mark_seek_feedback(s: &mut PlayerState, target: f64) {
    s.seek_spinner = Some((Instant::now(), target.max(0.0)));
    set_spinner(s, true);
}

/// Seek hedefine ulaşıldı mı? En az 900 ms görünür kalır ki gözden kaçmasın.
fn seek_feedback_pending(s: &mut PlayerState, pos: f64) -> bool {
    let Some((at, target)) = s.seek_spinner else {
        return false;
    };
    let elapsed = at.elapsed();
    let reached = (pos - target).abs() <= 1.5;
    if (reached || elapsed >= Duration::from_secs(4)) && elapsed >= Duration::from_millis(900) {
        s.seek_spinner = None;
        false
    } else {
        true
    }
}

fn tick(st: &Rc<RefCell<PlayerState>>, _start_volume: f64) {
    // Skip planı video başladıktan sonra arka planda geldi mi?
    {
        let mut s = match st.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => return,
        };
        if s.plan_rx.is_some() {
            let recv = s.plan_rx.as_ref().map(|rx| rx.try_recv());
            match recv {
                Some(Ok(plan)) => {
                    s.plan = plan;
                    s.plan_rx = None;
                    if s.plan.is_some() {
                        eprintln!("[EMBED] skip planı hazır");
                    } else {
                        eprintln!("[EMBED] skip planı bulunamadı");
                    }
                }
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {}
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    s.plan_rx = None;
                }
                None => {}
            }
        }
    }

    // Bölüm listesi arka plandan geldi mi?
    let episodes_updated = {
        let mut s = match st.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut updated = false;
        if let Some(rx) = s.episode_rx.as_ref() {
            match rx.try_recv() {
                Ok(Ok(eps)) => {
                    s.episode_rx = None;
                    if !eps.is_empty() {
                        s.episodes = eps;
                        updated = true;
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("[EMBED] bölüm listesi alınamadı: {e}");
                    s.episode_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    s.episode_rx = None;
                }
            }
        }
        updated
    };
    if episodes_updated {
        let (drawer, eps, season, episode, runtime, cb) = {
            let s = match st.try_borrow() {
                Ok(s) => s,
                Err(_) => return,
            };
            (
                s.drawer.clone(),
                s.episodes.clone(),
                s.ep.season,
                s.ep.episode,
                s.title.runtime,
                s.host.on_play_episode.clone(),
            )
        };
        episodes::render(&drawer, &eps, season, episode, runtime, &cb);
    }

    // Embed çözümleme sonucu geldi mi?
    let resolved = {
        let mut s = match st.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut out = None;
        if s.resolving {
            match s.pending_resolve.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(res)) => out = Some(res),
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {}
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    out = Some(Err("embed çözümleyici kapandı".to_string()));
                }
                None => out = Some(Err("embed çözümleyici yok".to_string())),
            }
            if out.is_some() {
                s.pending_resolve = None;
                s.resolving = false;
            }
        }
        out
    };
    if let Some(Ok(url)) = resolved {
        load_resolved_source(st, &url);
        return;
    }
    if let Some(Err(e)) = resolved {
        eprintln!("[EMBED] embed çözülemedi: {e}");
        let status = st.borrow().status.clone();
        status.set_text("Yedek kaynak çözülemedi, diğerine geçiliyor…");
        status.set_visible(true);
        advance_source(st);
        return;
    }

    // Kaynak açılış zaman aşımı (embed çözülürken sayılmaz).
    let timed_out = match st.try_borrow() {
        Ok(s) => {
            // İlk açılışta hızlı failover; normal oynatmada kullanıcı ayarı.
            let limit = if s.ready_sent {
                s.patience_secs
            } else {
                s.patience_secs.min(8)
            };
            s.source_loaded
                && !s.media_loaded
                && !s.resolving
                && s.load_started.elapsed().as_secs() >= limit
        }
        Err(_) => false,
    };
    if timed_out {
        eprintln!("[EMBED] kaynak zaman aşımı, sonrakine geçiliyor");
        advance_source(st);
        return;
    }

    let (pos, dur, paused, idle, eof, volume, muted, first_load, paused_for_cache, cache_time);
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
        paused_for_cache = p.paused_for_cache();
        cache_time = p.cache_time();
    }

    let mut advance_now = false;
    let (ready_now, finished_now, auto_next_now) = {
        let mut s = match st.try_borrow_mut() {
            Ok(s) => s,
            Err(_) => return,
        };
        if first_load {
            s.media_loaded = true;
            s.terminal_sent = false;
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
            if s.plan.is_none()
                && !s.plan_requested
                && (!s.fast_embeds.is_empty() || !s.fallback_embeds.is_empty())
            {
                let (tx, rx) = std::sync::mpsc::channel::<Option<SkipPlan>>();
                let client = s.host.client.clone();
                let (tid, season, episode) = (s.title.id, s.ep.season, s.ep.episode);
                let fast = s.fast_embeds.clone();
                let fallback = s.fallback_embeds.clone();
                let others: Vec<api::FansubInfo> = s.fansubs.iter().skip(1).cloned().collect();
                std::thread::spawn(move || {
                    let manual = client.load_settings().official_skip_secret;
                    let plan = crate::skip::fetch_plan_for_embeds_with_sources(
                        &client, tid, season, episode, &fast, &fallback, &others, &manual,
                    );
                    let _ = tx.send(plan);
                });
                s.plan_rx = Some(rx);
                s.plan_requested = true;
            }
            eprintln!("[EMBED] video hazır");
        }

        // Önbellek doluluk katmanı: oynatılan beyaz kısmın ötesinde,
        // mpv'nin indirdiği bölüm yarı saydam görünür.
        s.seek
            .set_fill_level(buffer_percent(pos, cache_time, dur) * 1000.0);
        // İlk açılış, ağ kaynaklı takılmalar ve kullanıcı seek'i sırasında
        // dönengeç oynatma butonunun üstünde görünür.
        let seek_pending = seek_feedback_pending(&mut s, pos);
        let spinner_on = (!s.media_loaded && !s.terminal_sent)
            || (s.media_loaded && paused_for_cache)
            || seek_pending;
        set_spinner(&mut s, spinner_on);

        let ready_now = s.media_loaded && !s.ready_sent;
        if ready_now {
            s.ready_sent = true;
        }

        if s.media_loaded && pos >= 1.0 {
            s.progress
                .borrow_mut()
                .insert(s.progress_key.clone(), (pos, dur));
            s.host
                .client
                .save_progress(s.title.id, s.ep.season, s.ep.episode, pos, dur);

            if !s.watched_marked && api::Client::played_enough(pos, dur) {
                s.watched_marked = true;
                let w = api::Watched {
                    title_id: s.title.id,
                    episode: s.ep.episode,
                    season: s.ep.season,
                    title_name: Some(s.title.name.clone()),
                    title_type: s.title.title_type.clone(),
                    ..Default::default()
                };
                s.host.client.save_watched(&w, &s.title.name);
                if !s.host_saved {
                    if let Some(src) = s.queue.current() {
                        let hint = api::Client::source_host_hint(src.hint_url());
                        if !hint.is_empty() {
                            s.host.client.set_preferred_host(s.title.id, hint);
                            s.host_saved = true;
                        }
                    }
                }
            }
        }

        let mut finished_now = false;
        let mut auto_next_now = false;
        if eof && dur > 0.0 && !s.terminal_sent {
            if pos < dur * 0.9 && s.load_started.elapsed().as_secs() < 60 {
                advance_now = true;
            } else {
                s.terminal_sent = true;
                finished_now = true;
                s.host.discord.clear();
                if s.auto_next_enabled
                    && !s.auto_next_fired
                    && s.title.title_type.as_deref() != Some("movie")
                {
                    s.auto_next_fired = true;
                    auto_next_now = true;
                }
            }
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

        // Sağ-alt atla pill'i + sol şarkı kartı. 10 sn sonra yanlara kaçar;
        // fare hareketi tekrar gösterir.
        let now = Instant::now();
        let mut skip_outro_now = false;
        let (active, song) = if let Some(plan) = s.plan.as_ref() {
            let op_active = !s.op_done
                && plan
                    .times
                    .op_start
                    .zip(plan.times.op_end)
                    .is_some_and(|(start, end)| pos >= start - 1.5 && pos < end);
            let ed_active = !s.ed_done
                && plan
                    .times
                    .ed_start
                    .zip(plan.times.ed_end)
                    .is_some_and(|(start, end)| pos >= start - 1.5 && pos < end);
            let active = if op_active {
                Some(SkipKind::Intro)
            } else if ed_active {
                Some(SkipKind::Outro)
            } else {
                None
            };
            if ed_active
                && s.auto_next_enabled
                && s.auto_skip_outro
                && !s.auto_next_fired
                && s.title.title_type.as_deref() != Some("movie")
            {
                skip_outro_now = true;
            }
            let song = if s.show_song_card {
                if op_active {
                    plan.music_op.clone()
                } else if ed_active {
                    plan.music_ed.clone()
                } else {
                    None
                }
            } else {
                None
            };
            (active, song)
        } else {
            (None, None)
        };
        if skip_outro_now {
            s.auto_next_fired = true;
            s.terminal_sent = true;
            s.ed_done = true;
            finished_now = true;
            auto_next_now = true;
            s.host.discord.clear();
        }
        if active != s.cards_kind {
            s.cards_kind = active;
            s.cards_hidden = false;
            s.cards_deadline = active.map(|_| now + Duration::from_secs(10));
        } else if let Some(deadline) = s.cards_deadline {
            if now >= deadline {
                s.cards_hidden = true;
                s.cards_deadline = None;
            }
        }
        let cards_visible = active.is_some() && !s.cards_hidden;
        if cards_visible && s.show_skip_button {
            if let Some(kind) = active {
                s.skip_pill.set_label(kind.action());
                s.skip_pill_rev.set_reveal_child(true);
            }
        } else {
            s.skip_pill_rev.set_reveal_child(false);
        }
        if cards_visible && s.show_song_card {
            if let Some(music) = song {
                let song_label = if s.show_music_hint {
                    "Tarayıcıda aç (Shift+M)"
                } else {
                    "Tarayıcıda aç"
                };
                s.song_card.show(
                    &music,
                    s.plan.as_ref().is_some_and(|p| p.song_url.is_some()),
                    song_label,
                );
            } else {
                s.song_card.hide();
            }
        } else {
            s.song_card.hide();
        }

        let quality = s
            .player
            .as_ref()
            .and_then(|p| p.video_height())
            .map(|h| format!("{h}p"))
            .or_else(|| s.queue.current_quality().map(str::to_string));
        let label = s
            .queue
            .current_label()
            .map(str::to_string)
            .or_else(|| {
                s.queue
                    .current()
                    .map(|src| api::Client::source_host_hint(src.hint_url()).to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "Kaynak".to_string());
        let quality = quality.map(|q| format!(" · {q}")).unwrap_or_default();
        s.quality_btn.set_label(&format!("{label}{quality}"));

        if eof {
            s.status.set_text("bitti");
            s.status.set_visible(true);
        }

        // Krom oto-gizleme: duraklatılmışken/ipucu açıkken gizlenme.
        let smoke_hold = std::env::var_os("ANIMECIX_SMOKE_HOLD").is_some();
        let pop_open =
            s.speed_pop.is_visible() || s.quality_pop.is_visible() || s.ctx_pop.is_visible();
        let idle_time = s.last_motion.elapsed();
        let keep_chrome = paused || s.status.is_visible() || s.spinner.is_active();
        if !smoke_hold && !keep_chrome && !s.cursor_hidden && idle_time.as_secs() >= 1 && !pop_open
        {
            s.cursor_hidden = true;
            s.gl_area.set_cursor_from_name(Some("none"));
        }
        if !smoke_hold && !keep_chrome && !s.chrome_hidden && idle_time.as_secs() >= 2 && !pop_open
        {
            hide_chrome_locked(&mut s);
        }
        if !idle || s.media_loaded {
            s.gl_area.queue_render();
        }
        (ready_now, finished_now, auto_next_now)
    };

    if advance_now {
        advance_source(st);
        return;
    }
    if ready_now {
        let cb = st.borrow().host.on_ready.clone();
        cb();
    }
    if finished_now {
        let cb = st.borrow().host.on_terminal.clone();
        cb(crate::app::PlayTerminal::Done, String::new());
    }
    if auto_next_now {
        let cb = st.borrow().host.on_neighbor.clone();
        cb(1);
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
    s.cards_hidden = false;
    s.cards_deadline = Some(Instant::now() + Duration::from_secs(10));
    let need_show = s.chrome_hidden || s.cursor_hidden;
    if need_show {
        show_chrome_locked(&mut s);
    }
}

fn show_chrome(state: &Rc<RefCell<PlayerState>>) {
    if let Ok(mut s) = state.try_borrow_mut() {
        s.last_motion = Instant::now();
        s.cards_hidden = false;
        s.cards_deadline = Some(Instant::now() + Duration::from_secs(10));
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
    fn fmt_time_handles_hours_and_invalid() {
        assert_eq!(fmt_time(65.0), "1:05");
        assert_eq!(fmt_time(3661.0), "1:01:01");
        assert_eq!(fmt_time(-1.0), "--:--");
    }

    #[test]
    fn buffer_percent_is_clamped_and_never_behind_playhead() {
        assert!((buffer_percent(50.0, 75.0, 100.0) - 0.75).abs() < 1e-9);
        assert!((buffer_percent(50.0, 40.0, 100.0) - 0.5).abs() < 1e-9);
        assert_eq!(buffer_percent(0.0, 0.0, 0.0), 0.0);
        assert_eq!(buffer_percent(0.0, f64::NAN, 100.0), 0.0);
        assert_eq!(buffer_percent(0.0, 500.0, 100.0), 1.0);
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
