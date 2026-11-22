use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::glib;

use crate::ui::operation_stack::OperationStack;
use crate::watch_party::audio::AudioEngine;
use crate::watch_party::{Event, Invite, PlaybackSnapshot, SessionHandle, SyncAction};

use super::PlayerState;

/// Gömülü oynatıcı ile watch-party çekirdeği arasındaki tek köprü.
/// mpv yalnızca UI thread'inde okunur/yazılır; ağ thread'i buraya dokunmaz.
pub struct WatchPartyPlayer {
    handle: Rc<SessionHandle>,
    weak: Weak<RefCell<PlayerState>>,
    root: gtk::Box,
    chat_revealer: gtk::Revealer,
    chat_buffer: gtk::TextBuffer,
    status: gtk::Label,
    audio: Rc<RefCell<Option<AudioEngine>>>,
    key_ctrl: Option<gtk::EventControllerKey>,
    speed_reset_at: Rc<Cell<Option<Instant>>>,
    invite: Rc<RefCell<Option<Invite>>>,
    invite_shown: Rc<Cell<bool>>,
    closed: Rc<Cell<bool>>,
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
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("watch-party-badge");
        root.set_halign(gtk::Align::End);
        root.set_valign(gtk::Align::Start);
        root.set_margin_top(64);
        root.set_margin_end(16);
        let status = gtk::Label::new(Some("Birlikte izleme bağlanıyor…"));
        status.add_css_class("player-text");
        status.set_xalign(1.0);
        let mic = gtk::Label::new(Some("🎙 Bas-konuş: V"));
        mic.add_css_class("player-text");
        mic.set_xalign(1.0);
        let chat_btn = gtk::Button::with_label("Sohbet");
        chat_btn.add_css_class("flat");
        chat_btn.add_css_class("pill");
        let invite_btn = gtk::Button::with_label("Davet");
        invite_btn.add_css_class("flat");
        invite_btn.add_css_class("pill");
        invite_btn.set_visible(!guest);
        let leave_btn = gtk::Button::with_label("Ayrıl");
        leave_btn.add_css_class("flat");
        leave_btn.add_css_class("pill");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.append(&status);
        head.append(&invite_btn);
        head.append(&chat_btn);
        head.append(&leave_btn);
        root.append(&head);
        root.append(&mic);
        if guest {
            let host_hint = gtk::Label::new(Some("Oynatmayı host yönetiyor"));
            host_hint.add_css_class("player-text");
            host_hint.add_css_class("dim-label");
            host_hint.set_xalign(1.0);
            root.append(&host_hint);
        }
        stage.add_overlay(&root);

        let chat_revealer = gtk::Revealer::new();
        chat_revealer.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
        chat_revealer.set_transition_duration(160);
        chat_revealer.set_halign(gtk::Align::End);
        chat_revealer.set_valign(gtk::Align::End);
        chat_revealer.set_margin_end(16);
        chat_revealer.set_margin_bottom(110);
        let chat_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        chat_box.add_css_class("watch-party-chat");
        chat_box.set_size_request(300, 240);
        let title = gtk::Label::new(Some("Oda sohbeti"));
        title.set_xalign(0.0);
        title.add_css_class("title-4");
        chat_box.append(&title);
        let chat_view = gtk::TextView::new();
        chat_view.set_editable(false);
        chat_view.set_cursor_visible(false);
        chat_view.set_wrap_mode(gtk::WrapMode::WordChar);
        let chat_buffer = chat_view.buffer();
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&chat_view));
        chat_box.append(&scroll);
        let chat_entry = gtk::Entry::new();
        chat_entry.set_placeholder_text(Some("Mesaj yaz…"));
        chat_box.append(&chat_entry);
        chat_revealer.set_child(Some(&chat_box));
        stage.add_overlay(&chat_revealer);

        let ptt = Rc::new(Cell::new(false));
        let speed_reset_at = Rc::new(Cell::new(None));
        let invite = Rc::new(RefCell::new(None));
        let invite_shown = Rc::new(Cell::new(false));
        let closed = Rc::new(Cell::new(false));
        let audio = Rc::new(RefCell::new(
            AudioEngine::start(handle.command_sender())
                .map_err(|e| eprintln!("[WATCHPARTY] ses: {e}"))
                .ok(),
        ));

        let key_ctrl = {
            let ctrl = gtk::EventControllerKey::new();
            ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
            let handle_p = handle.clone();
            let ptt_p = ptt.clone();
            let mic_p = mic.clone();
            let audio_p = audio.clone();
            ctrl.connect_key_pressed(move |_, key, _, mods| {
                if key == gtk::gdk::Key::v && mods.is_empty() {
                    if !ptt_p.replace(true) {
                        handle_p.ptt(true);
                        if let Some(a) = audio_p.borrow().as_ref() {
                            a.set_ptt(true);
                        }
                        mic_p.set_text("🎙 Konuşuyorsun…");
                    }
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            let handle_r = handle.clone();
            let ptt_r = ptt.clone();
            let mic_r = mic.clone();
            let audio_r = audio.clone();
            ctrl.connect_key_released(move |_, key, _, _| {
                if key == gtk::gdk::Key::v && ptt_r.replace(false) {
                    handle_r.ptt(false);
                    if let Some(a) = audio_r.borrow().as_ref() {
                        a.set_ptt(false);
                    }
                    mic_r.set_text("🎙 Bas-konuş: V");
                }
            });
            state.borrow().gl_area.add_controller(ctrl.clone());
            Some(ctrl)
        };

        {
            let rev = chat_revealer.clone();
            let open = Rc::new(Cell::new(false));
            chat_btn.connect_clicked(move |_| {
                let next = !open.get();
                open.set(next);
                rev.set_reveal_child(next);
            });
        }
        {
            let inv = invite.clone();
            let window = window.clone();
            invite_btn.connect_clicked(move |_| {
                if let Some(invite) = inv.borrow().as_ref() {
                    crate::ui::watch_party::present_invite(&window, invite);
                }
            });
        }
        {
            let handle_leave = handle.clone();
            leave_btn.connect_clicked(move |_| handle_leave.leave());
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

        let this = Self {
            handle,
            weak: Rc::downgrade(&state),
            root,
            chat_revealer,
            chat_buffer,
            status,
            audio,
            key_ctrl,
            speed_reset_at,
            invite,
            invite_shown,
            closed,
            ops,
        };
        this.start_tick(window.clone());
        this
    }

    fn start_tick(&self, window: adw::ApplicationWindow) {
        let weak = self.weak.clone();
        let handle = self.handle.clone();
        let root = self.root.clone();
        let chat_revealer = self.chat_revealer.clone();
        let status = self.status.clone();
        let chat_buffer = self.chat_buffer.clone();
        let speed_reset_at = self.speed_reset_at.clone();
        let invite = self.invite.clone();
        let invite_shown = self.invite_shown.clone();
        let closed = self.closed.clone();
        let audio = self.audio.clone();
        let ops = self.ops.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if closed.get() {
                return glib::ControlFlow::Break;
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
                    Event::Invite(new_invite) => {
                        *invite.borrow_mut() = Some(new_invite.clone());
                        if !invite_shown.replace(true) {
                            crate::ui::watch_party::present_invite(&window, &new_invite);
                            status.set_text("Oda hazır · davet kodunu paylaş");
                        }
                    }
                    Event::Joined { room, host, peers } => {
                        status.set_text(&format!(
                            "Odadasın · {} · {} kişi",
                            host.name,
                            peers.len() + 1
                        ));
                        let _ = room;
                    }
                    Event::Peers(peers) => {
                        status.set_text(&format!("Odada {} kişi", peers.len() + 1));
                    }
                    Event::Playback(state) => {
                        let _ = state.position;
                    }
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
                        let mut text = chat_buffer
                            .text(&chat_buffer.start_iter(), &chat_buffer.end_iter(), true)
                            .to_string();
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&format!("{}: {}", msg.name, msg.text));
                        chat_buffer.set_text(&text);
                    }
                    Event::Voice(v) => {
                        if let Some(a) = audio.borrow().as_ref() {
                            a.play(&v.samples);
                        }
                    }
                    Event::Error(e) => {
                        status.set_text("Birlikte izleme hatası");
                        ops.error("Birlikte izleme", &e, Duration::from_secs(6));
                    }
                    Event::Closed => {
                        status.set_text("Oda kapandı");
                        root.set_visible(false);
                        chat_revealer.set_reveal_child(false);
                        if let Some(audio) = audio.borrow_mut().take() {
                            audio.set_ptt(false);
                        }
                        if let Ok(s) = st.try_borrow() {
                            super::set_player_guest_lock(&s, false);
                        }
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
        self.closed.set(true);
        self.handle.leave();
        if let Some(st) = self.weak.upgrade() {
            if let Ok(s) = st.try_borrow() {
                super::set_player_guest_lock(&s, false);
            }
        }
        if let Some(audio) = self.audio.borrow_mut().take() {
            audio.set_ptt(false);
        }
        if let Some(ctrl) = self.key_ctrl.take() {
            if let Some(st) = self.weak.upgrade() {
                st.borrow().gl_area.remove_controller(&ctrl);
            }
        }
        self.root.set_visible(false);
        self.chat_revealer.set_visible(false);
    }
}

impl Drop for WatchPartyPlayer {
    fn drop(&mut self) {
        self.shutdown();
    }
}
