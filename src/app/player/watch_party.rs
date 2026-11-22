use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::glib;
use gtk::pango;

use crate::ui::operation_stack::OperationStack;
use crate::watch_party::audio::AudioEngine;
use crate::watch_party::chat::ChatMessage;
use crate::watch_party::types::{PeerId, PeerInfo, Role, MAX_PEERS};
use crate::watch_party::{Event, Invite, PlaybackSnapshot, SessionHandle, SyncAction};

use super::PlayerState;

/// Gömülü oynatıcı ile watch-party çekirdeği arasındaki tek köprü.
/// mpv yalnızca UI thread'inde okunur/yazılır; ağ thread'i buraya dokunmaz.
pub struct WatchPartyPlayer {
    handle: Rc<SessionHandle>,
    weak: Weak<RefCell<PlayerState>>,
    window: adw::ApplicationWindow,
    stage: gtk::Overlay,
    gl_area: gtk::GLArea,
    drawer_revealer: gtk::Revealer,
    panel: gtk::Revealer,
    panel_open: Rc<Cell<bool>>,
    ptt_pill: gtk::Label,
    status: gtk::Label,
    count: gtk::Label,
    avatars: gtk::Box,
    chat_list: gtk::Box,
    chat_scroll: gtk::ScrolledWindow,
    chat_empty: gtk::Label,
    new_msg_btn: gtk::Button,
    chat_rows: Rc<Cell<usize>>,
    self_info: Rc<RefCell<Option<PeerInfo>>>,
    host_info: Rc<RefCell<Option<PeerInfo>>>,
    peers: Rc<RefCell<Vec<PeerInfo>>>,
    peer_count: Rc<Cell<usize>>,
    unread: Rc<Cell<bool>>,
    watch_btn: gtk::Button,
    watch_dot: gtk::Label,
    ptt_hint: gtk::Label,
    audio: Rc<RefCell<Option<AudioEngine>>>,
    ptt: Rc<Cell<bool>>,
    key_ctrl: Option<gtk::EventControllerKey>,
    focus_handler: Option<glib::SignalHandlerId>,
    speed_reset_at: Rc<Cell<Option<Instant>>>,
    invite: Rc<RefCell<Option<Invite>>>,
    invite_shown: Rc<Cell<bool>>,
    leaving: Rc<Cell<bool>>,
    closed: Rc<Cell<bool>>,
    shutdown_done: Cell<bool>,
    guest: bool,
    ops: OperationStack,
}

impl WatchPartyPlayer {
    pub(super) fn attach(
        state: Rc<RefCell<PlayerState>>,
        handle: Rc<SessionHandle>,
        stage: &gtk::Overlay,
        window: &adw::ApplicationWindow,
        ops: OperationStack,
        guest: bool,
    ) -> Self {
        let (gl_area, drawer_revealer, watch_btn, watch_dot) = {
            let s = state.borrow();
            (
                s.gl_area.clone(),
                s.drawer.revealer.clone(),
                s.watch_btn.clone(),
                s.watch_dot.clone(),
            )
        };

        let panel_body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        panel_body.add_css_class("wp-panel");
        panel_body.set_size_request(320, -1);
        panel_body.set_margin_top(12);
        panel_body.set_margin_bottom(12);
        panel_body.set_vexpand(true);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::new(Some("Birlikte İzle"));
        title.add_css_class("wp-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let live = gtk::Label::new(Some("● Canlı"));
        live.add_css_class("wp-live");
        let count = gtk::Label::new(Some("1/4"));
        count.add_css_class("wp-count");
        let close_btn = gtk::Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("flat");
        close_btn.set_tooltip_text(Some("Oda panelini kapat"));
        header.append(&title);
        header.append(&live);
        header.append(&count);
        header.append(&close_btn);
        panel_body.append(&header);

        let avatars = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        avatars.set_halign(gtk::Align::Start);
        avatars.set_margin_top(2);
        avatars.set_margin_bottom(2);
        panel_body.append(&avatars);

        let status = gtk::Label::new(Some("Bağlanılıyor…"));
        status.add_css_class("wp-host-hint");
        status.set_xalign(0.0);
        status.set_wrap(true);
        panel_body.append(&status);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_margin_top(4);
        actions.set_margin_bottom(4);
        let invite_btn = action_button("edit-copy-symbolic", "Davet", "wp-action-accent");
        invite_btn.set_visible(!guest);
        let leave_btn = action_button("system-log-out-symbolic", "Ayrıl", "wp-action-danger");
        actions.append(&invite_btn);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        actions.append(&spacer);
        actions.append(&leave_btn);
        panel_body.append(&actions);

        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep.add_css_class("wp-divider");
        sep.set_margin_top(2);
        sep.set_margin_bottom(2);
        panel_body.append(&sep);

        let chat_head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let chat_title = gtk::Label::new(Some("SOHBET"));
        chat_title.add_css_class("wp-chat-title");
        chat_title.set_xalign(0.0);
        chat_title.set_hexpand(true);
        chat_head.append(&chat_title);
        panel_body.append(&chat_head);

        let chat_list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        chat_list.set_valign(gtk::Align::Start);
        chat_list.set_margin_start(2);
        chat_list.set_margin_end(2);
        chat_list.set_margin_top(2);
        let chat_empty = gtk::Label::new(Some("Henüz mesaj yok.\nİlk mesajı sen yaz."));
        chat_empty.add_css_class("wp-empty");
        chat_empty.set_xalign(0.0);
        chat_empty.set_justify(gtk::Justification::Left);
        chat_list.append(&chat_empty);

        let chat_scroll = gtk::ScrolledWindow::new();
        chat_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        chat_scroll.set_vexpand(true);
        chat_scroll.set_hexpand(true);
        chat_scroll.set_child(Some(&chat_list));

        let new_msg_btn = gtk::Button::with_label("↓ Yeni mesaj");
        new_msg_btn.add_css_class("wp-new-msg");
        new_msg_btn.set_halign(gtk::Align::Center);
        new_msg_btn.set_valign(gtk::Align::End);
        new_msg_btn.set_margin_bottom(8);
        new_msg_btn.set_visible(false);

        let chat_overlay = gtk::Overlay::new();
        chat_overlay.set_child(Some(&chat_scroll));
        chat_overlay.set_vexpand(true);
        chat_overlay.set_hexpand(true);
        chat_overlay.add_overlay(&new_msg_btn);
        panel_body.append(&chat_overlay);

        let chat_entry = gtk::Entry::new();
        chat_entry.set_placeholder_text(Some("Mesaj yaz…"));
        chat_entry.add_css_class("wp-entry");
        panel_body.append(&chat_entry);

        let ptt_hint = gtk::Label::new(Some("🎙 Bas-konuş: V"));
        ptt_hint.add_css_class("wp-host-hint");
        ptt_hint.set_xalign(0.0);
        panel_body.append(&ptt_hint);

        let panel = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .transition_duration(200)
            .child(&panel_body)
            .build();
        panel.set_reveal_child(false);
        panel.set_halign(gtk::Align::End);
        panel.set_valign(gtk::Align::Fill);
        panel.set_vexpand(true);
        stage.add_overlay(&panel);

        let ptt_pill = gtk::Label::new(Some("🎙 Konuşuyorsun…"));
        ptt_pill.add_css_class("wp-ptt-pill");
        ptt_pill.set_halign(gtk::Align::Center);
        ptt_pill.set_valign(gtk::Align::End);
        ptt_pill.set_margin_bottom(112);
        ptt_pill.set_visible(false);
        ptt_pill.set_can_target(false);
        stage.add_overlay(&ptt_pill);

        let panel_open = Rc::new(Cell::new(false));
        let speed_reset_at = Rc::new(Cell::new(None));
        let invite = Rc::new(RefCell::new(None));
        let invite_shown = Rc::new(Cell::new(false));
        let leaving = Rc::new(Cell::new(false));
        let closed = Rc::new(Cell::new(false));
        let ptt = Rc::new(Cell::new(false));
        let self_info = Rc::new(RefCell::new(None));
        let host_info = Rc::new(RefCell::new(None));
        let peers = Rc::new(RefCell::new(Vec::new()));
        let peer_count = Rc::new(Cell::new(1));
        let unread = Rc::new(Cell::new(false));
        let chat_rows = Rc::new(Cell::new(0));

        let audio: Rc<RefCell<Option<AudioEngine>>> = Rc::new(RefCell::new(None));
        match AudioEngine::start(handle.command_sender()) {
            Ok(engine) => *audio.borrow_mut() = Some(engine),
            Err(e) => {
                eprintln!("[WATCHPARTY] ses motoru açılamadı: {e}");
                ptt_hint.set_text("🎙 Mikrofon yok");
                ops.error(
                    "Birlikte İzle",
                    &format!("Mikrofon/ses açılamadı: {e}"),
                    Duration::from_secs(6),
                );
            }
        }

        let start_ptt: Rc<dyn Fn()> = {
            let handle = handle.clone();
            let audio = audio.clone();
            let ptt = ptt.clone();
            let ptt_hint = ptt_hint.clone();
            let ptt_pill = ptt_pill.clone();
            let ops = ops.clone();
            let closed = closed.clone();
            let mic_error = Rc::new(Cell::new(false));
            Rc::new(move || {
                if closed.get() || ptt.get() {
                    return;
                }
                if audio.borrow().is_none() {
                    if !mic_error.replace(true) {
                        ops.error(
                            "Birlikte İzle",
                            "Mikrofon açılamadı; ses gönderilemiyor.",
                            Duration::from_secs(5),
                        );
                    }
                    ptt_hint.set_text("🎙 Mikrofon yok");
                    return;
                }
                ptt.set(true);
                eprintln!("[WATCHPARTY] PTT basıldı");
                handle.ptt(true);
                if let Some(a) = audio.borrow().as_ref() {
                    a.set_ptt(true);
                }
                ptt_hint.set_text("🎙 Konuşuyorsun…");
                ptt_pill.set_visible(true);
            })
        };

        let stop_ptt: Rc<dyn Fn()> = {
            let handle = handle.clone();
            let audio = audio.clone();
            let ptt = ptt.clone();
            let ptt_hint = ptt_hint.clone();
            let ptt_pill = ptt_pill.clone();
            Rc::new(move || {
                if !ptt.replace(false) {
                    return;
                }
                eprintln!("[WATCHPARTY] PTT bırakıldı");
                handle.ptt(false);
                if let Some(a) = audio.borrow().as_ref() {
                    a.set_ptt(false);
                }
                ptt_hint.set_text(if audio.borrow().is_some() {
                    "🎙 Bas-konuş: V"
                } else {
                    "🎙 Mikrofon yok"
                });
                ptt_pill.set_visible(false);
            })
        };

        let key_ctrl = {
            let ctrl = gtk::EventControllerKey::new();
            ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
            let start = start_ptt.clone();
            let stop = stop_ptt.clone();
            let panel_open_c = panel_open.clone();
            let window_c = window.clone();
            let closed_c = closed.clone();
            ctrl.connect_key_pressed(move |_, key, _, mods| {
                let blocked = gtk::gdk::ModifierType::CONTROL_MASK
                    | gtk::gdk::ModifierType::ALT_MASK
                    | gtk::gdk::ModifierType::SUPER_MASK
                    | gtk::gdk::ModifierType::HYPER_MASK
                    | gtk::gdk::ModifierType::META_MASK;
                if key == gtk::gdk::Key::v && !mods.intersects(blocked) {
                    if closed_c.get() {
                        return gtk::glib::Propagation::Proceed;
                    }
                    let text_focus = gtk::prelude::RootExt::focus(&window_c)
                        .map(|f| f.is::<gtk::Text>() || f.is::<gtk::Entry>())
                        .unwrap_or(false);
                    if text_focus && panel_open_c.get() {
                        return gtk::glib::Propagation::Proceed;
                    }
                    start();
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            ctrl.connect_key_released(move |_, key, _, _| {
                if key == gtk::gdk::Key::v {
                    stop();
                }
            });
            window.add_controller(ctrl.clone());
            Some(ctrl)
        };

        let focus_handler = {
            let stop = stop_ptt.clone();
            Some(
                window.connect_notify_local(Some("is-active"), move |win, _| {
                    if !win.is_active() {
                        stop();
                    }
                }),
            )
        };

        {
            let inv = invite.clone();
            let window_c = window.clone();
            invite_btn.connect_clicked(move |_| {
                if let Some(invite) = inv.borrow().as_ref() {
                    crate::ui::watch_party::present_invite(&window_c, invite);
                }
            });
        }
        {
            let leaving_c = leaving.clone();
            let handle_c = handle.clone();
            leave_btn.connect_clicked(move |_| {
                leaving_c.set(true);
                handle_c.leave();
            });
        }
        {
            let panel_c = panel.clone();
            let open_c = panel_open.clone();
            let gl_area_c = gl_area.clone();
            close_btn.connect_clicked(move |_| {
                open_c.set(false);
                panel_c.set_reveal_child(false);
                gl_area_c.grab_focus();
            });
        }
        {
            let handle_c = handle.clone();
            let entry_c = chat_entry.clone();
            entry_c.connect_activate(move |e| {
                let text = e.text().trim().to_string();
                if !text.is_empty() {
                    handle_c.chat(text);
                    e.set_text("");
                }
            });
        }
        {
            let scroll_c = chat_scroll.clone();
            let btn_c = new_msg_btn.clone();
            new_msg_btn.connect_clicked(move |_| {
                scroll_to_bottom(&scroll_c);
                btn_c.set_visible(false);
            });
        }

        let this = Self {
            handle,
            weak: Rc::downgrade(&state),
            window: window.clone(),
            stage: stage.clone(),
            gl_area,
            drawer_revealer,
            panel,
            panel_open,
            ptt_pill,
            status,
            count,
            avatars,
            chat_list,
            chat_scroll,
            chat_empty,
            new_msg_btn,
            chat_rows,
            self_info,
            host_info,
            peers,
            peer_count,
            unread,
            watch_btn: watch_btn.clone(),
            watch_dot: watch_dot.clone(),
            ptt_hint,
            audio,
            ptt,
            key_ctrl,
            focus_handler,
            speed_reset_at,
            invite,
            invite_shown,
            leaving,
            closed,
            shutdown_done: Cell::new(false),
            guest,
            ops,
        };
        this.watch_dot.set_visible(true);
        this.watch_btn.add_css_class("watch-party-active");
        this.refresh_indicator(1);
        this.start_tick(window.clone());
        this
    }

    pub fn is_active(&self) -> bool {
        !self.closed.get()
    }

    /// Üst bardaki kişiler ikonu: oda panelini açar/kapatır.
    pub fn toggle_panel(&self) {
        if self.panel_open.get() {
            self.close_panel();
        } else {
            self.open_panel();
        }
    }

    fn open_panel(&self) {
        self.drawer_revealer.set_reveal_child(false);
        self.panel_open.set(true);
        self.panel.set_reveal_child(true);
        self.unread.set(false);
        self.new_msg_btn.set_visible(false);
        scroll_to_bottom(&self.chat_scroll);
        self.refresh_indicator(self.peer_count.get());
    }

    fn close_panel(&self) {
        self.panel_open.set(false);
        self.panel.set_reveal_child(false);
        self.gl_area.grab_focus();
    }

    fn refresh_indicator(&self, count: usize) {
        refresh_indicator(
            count,
            &self.watch_btn,
            &self.watch_dot,
            &self.count,
            self.unread.get(),
        );
    }

    fn start_tick(&self, window: adw::ApplicationWindow) {
        let weak = self.weak.clone();
        let handle = self.handle.clone();
        let panel = self.panel.clone();
        let panel_open = self.panel_open.clone();
        let drawer_revealer = self.drawer_revealer.clone();
        let gl_area = self.gl_area.clone();
        let ptt_pill = self.ptt_pill.clone();
        let status = self.status.clone();
        let count_label = self.count.clone();
        let avatars = self.avatars.clone();
        let chat_list = self.chat_list.clone();
        let chat_scroll = self.chat_scroll.clone();
        let chat_empty = self.chat_empty.clone();
        let new_msg_btn = self.new_msg_btn.clone();
        let chat_rows = self.chat_rows.clone();
        let self_info = self.self_info.clone();
        let host_info = self.host_info.clone();
        let peers = self.peers.clone();
        let peer_count = self.peer_count.clone();
        let unread = self.unread.clone();
        let watch_btn = self.watch_btn.clone();
        let watch_dot = self.watch_dot.clone();
        let ptt_hint = self.ptt_hint.clone();
        let speed_reset_at = self.speed_reset_at.clone();
        let invite = self.invite.clone();
        let invite_shown = self.invite_shown.clone();
        let leaving = self.leaving.clone();
        let closed = self.closed.clone();
        let audio = self.audio.clone();
        let ptt = self.ptt.clone();
        let ops = self.ops.clone();
        let guest = self.guest;
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if closed.get() {
                return glib::ControlFlow::Break;
            }
            if drawer_revealer.reveals_child() && panel_open.get() {
                panel_open.set(false);
                panel.set_reveal_child(false);
            }
            let Some(st) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };

            // mpv snapshot -> çekirdek.
            let snapshot = {
                let s = match st.try_borrow() {
                    Ok(s) => s,
                    Err(_) => return glib::ControlFlow::Continue,
                };
                let Some(p) = s.player.as_ref() else {
                    return glib::ControlFlow::Continue;
                };
                PlaybackSnapshot {
                    position: p.pos(),
                    duration: p.dur(),
                    paused: p.paused(),
                    buffering: p.paused_for_cache(),
                    speed: 1.0,
                    episode: format!("{}:{}:{}", s.title.id, s.ep.season, s.ep.episode),
                }
            };
            handle.snapshot(snapshot);

            while let Some(event) = handle.try_recv() {
                match event {
                    Event::Me(me) => {
                        let host = me.role == Role::Host;
                        *self_info.borrow_mut() = Some(me.clone());
                        if host {
                            *host_info.borrow_mut() = Some(me);
                        }
                        status.set_text(&host_status(guest, &host_info));
                        refresh_people(
                            &avatars,
                            &self_info,
                            &peers,
                            &peer_count,
                            &watch_btn,
                            &watch_dot,
                            &count_label,
                            &unread,
                        );
                    }
                    Event::Invite(new_invite) => {
                        *invite.borrow_mut() = Some(new_invite.clone());
                        if !invite_shown.replace(true) {
                            crate::ui::watch_party::present_invite(&window, &new_invite);
                        }
                    }
                    Event::Joined {
                        room,
                        host,
                        peers: joined_peers,
                    } => {
                        *host_info.borrow_mut() = Some(host);
                        *peers.borrow_mut() = joined_peers;
                        status.set_text(&host_status(guest, &host_info));
                        refresh_people(
                            &avatars,
                            &self_info,
                            &peers,
                            &peer_count,
                            &watch_btn,
                            &watch_dot,
                            &count_label,
                            &unread,
                        );
                        let _ = room;
                    }
                    Event::Peers(new_peers) => {
                        *peers.borrow_mut() = new_peers;
                        status.set_text(&host_status(guest, &host_info));
                        refresh_people(
                            &avatars,
                            &self_info,
                            &peers,
                            &peer_count,
                            &watch_btn,
                            &watch_dot,
                            &count_label,
                            &unread,
                        );
                    }
                    Event::Playback(_) => {}
                    Event::Sync(action) => {
                        if let Ok(s) = st.try_borrow() {
                            if let Some(p) = s.player.as_ref() {
                                match action {
                                    SyncAction::Pause => p.set_pause(true),
                                    SyncAction::Play => p.set_pause(false),
                                    SyncAction::Seek(pos) => p.seek_abs(pos.max(0.0)),
                                    SyncAction::SetSpeed(rate) => {
                                        p.set_speed(rate);
                                        speed_reset_at
                                            .set(Some(Instant::now() + Duration::from_secs(2)));
                                    }
                                    SyncAction::None => {}
                                }
                            }
                        }
                    }
                    Event::Chat(msg) => {
                        let self_id = self_info.borrow().as_ref().map(|p| p.id);
                        append_chat(
                            &chat_list,
                            &chat_scroll,
                            &new_msg_btn,
                            &chat_empty,
                            &chat_rows,
                            &msg,
                            self_id,
                            panel_open.get(),
                            &unread,
                        );
                        refresh_indicator(
                            peer_count.get(),
                            &watch_btn,
                            &watch_dot,
                            &count_label,
                            unread.get(),
                        );
                    }
                    Event::Voice(v) => {
                        if let Some(a) = audio.borrow().as_ref() {
                            a.play(&v.samples);
                        }
                    }
                    Event::Error(e) => {
                        status.set_text("Bağlanılamadı");
                        ops.error("Birlikte İzle", &e, Duration::from_secs(6));
                        panel_open.set(false);
                        panel.set_reveal_child(false);
                        ptt_pill.set_visible(false);
                        watch_dot.set_visible(false);
                        watch_btn.remove_css_class("watch-party-active");
                        watch_btn.set_tooltip_text(Some("Birlikte İzle"));
                        ptt.set(false);
                        handle.ptt(false);
                        if let Some(a) = audio.borrow_mut().take() {
                            a.set_ptt(false);
                        }
                        gl_area.grab_focus();
                        closed.set(true);
                        return glib::ControlFlow::Break;
                    }
                    Event::Closed => {
                        status.set_text("Oda kapandı");
                        panel_open.set(false);
                        panel.set_reveal_child(false);
                        ptt_pill.set_visible(false);
                        ptt_hint.set_text("🎙 Bas-konuş: V");
                        watch_dot.set_visible(false);
                        watch_btn.remove_css_class("watch-party-active");
                        watch_btn.set_tooltip_text(Some("Birlikte İzle"));
                        ptt.set(false);
                        handle.ptt(false);
                        if let Some(a) = audio.borrow_mut().take() {
                            a.set_ptt(false);
                        }
                        gl_area.grab_focus();
                        ops.info(
                            "Birlikte İzle",
                            if leaving.get() {
                                "Odadan ayrıldın"
                            } else {
                                "Oda kapandı"
                            },
                            Duration::from_secs(3),
                        );
                        closed.set(true);
                        return glib::ControlFlow::Break;
                    }
                }
            }

            if let Some(at) = speed_reset_at.get() {
                if Instant::now() >= at {
                    if let Ok(s) = st.try_borrow() {
                        if let Some(p) = s.player.as_ref() {
                            p.set_speed(1.0);
                        }
                    }
                    speed_reset_at.set(None);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    pub fn shutdown(&mut self) {
        if self.shutdown_done.replace(true) {
            return;
        }
        self.closed.set(true);
        self.handle.leave();
        self.ptt.set(false);
        if let Some(a) = self.audio.borrow_mut().take() {
            a.set_ptt(false);
        }
        if let Some(ctrl) = self.key_ctrl.take() {
            self.window.remove_controller(&ctrl);
        }
        if let Some(id) = self.focus_handler.take() {
            self.window.disconnect(id);
        }
        self.panel.set_reveal_child(false);
        self.panel_open.set(false);
        self.ptt_pill.set_visible(false);
        self.watch_dot.set_visible(false);
        self.watch_btn.remove_css_class("watch-party-active");
        self.watch_btn.set_tooltip_text(Some("Birlikte İzle"));
        if self.panel.parent().is_some() {
            self.stage.remove_overlay(&self.panel);
        }
        if self.ptt_pill.parent().is_some() {
            self.stage.remove_overlay(&self.ptt_pill);
        }
    }
}

impl Drop for WatchPartyPlayer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn action_button(icon: &str, label: &str, css: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("wp-action");
    if !css.is_empty() {
        button.add_css_class(css);
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.append(&gtk::Image::from_icon_name(icon));
    row.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&row));
    button
}

fn host_status(guest: bool, host_info: &Rc<RefCell<Option<PeerInfo>>>) -> String {
    if guest {
        host_info
            .borrow()
            .as_ref()
            .map(|h| format!("Oynatmayı {} yönetiyor", h.name))
            .unwrap_or_else(|| "Oynatmayı host yönetiyor".to_string())
    } else {
        "Oynatmayı sen yönetiyorsun".to_string()
    }
}

fn participants(
    self_info: &Rc<RefCell<Option<PeerInfo>>>,
    peers: &Rc<RefCell<Vec<PeerInfo>>>,
) -> Vec<PeerInfo> {
    let mut all = Vec::new();
    if let Some(me) = self_info.borrow().clone() {
        all.push(me);
    }
    all.extend(peers.borrow().iter().cloned());
    all.sort_by_key(|p| p.id);
    all.dedup_by_key(|p| p.id);
    all.sort_by_key(|p| (p.role != Role::Host, p.id));
    all
}

fn refresh_people(
    avatars: &gtk::Box,
    self_info: &Rc<RefCell<Option<PeerInfo>>>,
    peers: &Rc<RefCell<Vec<PeerInfo>>>,
    peer_count: &Cell<usize>,
    watch_btn: &gtk::Button,
    watch_dot: &gtk::Label,
    count_label: &gtk::Label,
    unread: &Cell<bool>,
) {
    let all = participants(self_info, peers);
    let self_id = self_info.borrow().as_ref().map(|p| p.id);
    render_avatars(avatars, &all, self_id);
    let n = all.len().max(1);
    peer_count.set(n);
    refresh_indicator(n, watch_btn, watch_dot, count_label, unread.get());
}

fn render_avatars(avatars: &gtk::Box, all: &[PeerInfo], self_id: Option<PeerId>) {
    clear_box(avatars);
    if all.is_empty() {
        let placeholder = gtk::Label::new(Some("…"));
        placeholder.add_css_class("wp-avatar");
        placeholder.add_css_class("wp-avatar-1");
        placeholder.set_size_request(34, 34);
        avatars.append(&placeholder);
        return;
    }
    for peer in all {
        let initial = peer
            .name
            .trim()
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
        let avatar = gtk::Label::new(Some(&initial));
        avatar.add_css_class("wp-avatar");
        avatar.add_css_class(&format!("wp-avatar-{}", peer.id % 6));
        if peer.role == Role::Host {
            avatar.add_css_class("wp-avatar-host");
        }
        if self_id == Some(peer.id) {
            avatar.add_css_class("wp-avatar-self");
        }
        avatar.set_size_request(34, 34);
        avatar.set_tooltip_text(Some(&format!(
            "{}{}{}",
            peer.name,
            if peer.role == Role::Host {
                " · host"
            } else {
                ""
            },
            if self_id == Some(peer.id) {
                " · sen"
            } else {
                ""
            }
        )));
        avatars.append(&avatar);
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn refresh_indicator(
    count: usize,
    watch_btn: &gtk::Button,
    watch_dot: &gtk::Label,
    count_label: &gtk::Label,
    unread: bool,
) {
    let count = count.max(1);
    watch_dot.set_visible(true);
    if unread {
        watch_dot.add_css_class("unread");
    } else {
        watch_dot.remove_css_class("unread");
    }
    watch_btn.add_css_class("watch-party-active");
    watch_btn.set_tooltip_text(Some(&format!("Birlikte İzle · {count} kişi")));
    count_label.set_text(&format!("{count}/{MAX_PEERS}"));
}

fn append_chat(
    list: &gtk::Box,
    scroll: &gtk::ScrolledWindow,
    new_btn: &gtk::Button,
    empty: &gtk::Label,
    rows: &Cell<usize>,
    msg: &ChatMessage,
    self_id: Option<PeerId>,
    panel_open: bool,
    unread: &Cell<bool>,
) {
    let at_bottom = scroll_at_bottom(scroll);
    if empty.parent().is_some() {
        empty.set_visible(false);
        list.remove(empty);
    }
    let own = self_id == Some(msg.id);
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    outer.set_halign(if own {
        gtk::Align::End
    } else {
        gtk::Align::Start
    });
    outer.set_margin_top(2);

    let bubble = gtk::Box::new(gtk::Orientation::Vertical, 3);
    bubble.add_css_class("wp-bubble");
    if own {
        bubble.add_css_class("wp-bubble-self");
    }

    let meta = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let name = gtk::Label::new(Some(if own { "Sen" } else { &msg.name }));
    name.add_css_class("wp-msg-name");
    let time = gtk::Label::new(Some(&format_time(msg.ts)));
    time.add_css_class("wp-msg-time");
    meta.append(&name);
    meta.append(&time);
    bubble.append(&meta);

    let text = gtk::Label::new(Some(&msg.text));
    text.add_css_class("wp-msg-text");
    text.set_wrap(true);
    text.set_wrap_mode(pango::WrapMode::WordChar);
    text.set_max_width_chars(36);
    text.set_xalign(0.0);
    text.set_selectable(true);
    bubble.append(&text);

    outer.append(&bubble);
    list.append(&outer);
    rows.set(rows.get() + 1);
    while rows.get() > 200 {
        if let Some(first) = list.first_child() {
            list.remove(&first);
            rows.set(rows.get() - 1);
        } else {
            break;
        }
    }

    if panel_open {
        if at_bottom || own {
            scroll_to_bottom(scroll);
            new_btn.set_visible(false);
        } else {
            new_btn.set_visible(true);
        }
    } else {
        unread.set(true);
    }
}

fn format_time(ts: u64) -> String {
    glib::DateTime::from_unix_local((ts / 1000) as i64)
        .ok()
        .and_then(|dt| dt.format("%H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn scroll_at_bottom(scroll: &gtk::ScrolledWindow) -> bool {
    let adj = scroll.vadjustment();
    adj.upper() <= adj.page_size() + 1.0 || adj.value() + adj.page_size() >= adj.upper() - 24.0
}

fn scroll_to_bottom(scroll: &gtk::ScrolledWindow) {
    let scroll = scroll.clone();
    glib::idle_add_local_once(move || {
        let adj = scroll.vadjustment();
        adj.set_value((adj.upper() - adj.page_size()).max(0.0));
    });
}
