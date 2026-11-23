//! Uygulama seviyesindeki sağ oda paneli: lobi + oda + sohbet.

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

use crate::watch_party::chat::ChatMessage;
use crate::watch_party::types::{ContentKind, PeerInfo, Role, RoomContent, MAX_PEERS};

#[derive(Clone)]
pub struct RoomPanel {
    pub revealer: gtk::Revealer,
    pub panel_open: Rc<Cell<bool>>,
    pub stack: gtk::Stack,
    pub create_btn: gtk::Button,
    pub close_btn: gtk::Button,
    pub status: gtk::Label,
    pub count: gtk::Label,
    pub avatars: gtk::Box,
    pub content: gtk::Label,
    pub join_btn: gtk::Button,
    pub invite_btn: gtk::Button,
    pub leave_btn: gtk::Button,
    pub chat_list: gtk::Box,
    pub chat_scroll: gtk::ScrolledWindow,
    pub chat_empty: gtk::Label,
    pub chat_entry: gtk::Entry,
    pub new_msg_btn: gtk::Button,
    pub ptt_hint: gtk::Label,
    pub ptt_pill: gtk::Label,
    chat_rows: Rc<Cell<usize>>,
    self_info: Rc<std::cell::RefCell<Option<PeerInfo>>>,
    peers: Rc<std::cell::RefCell<Vec<PeerInfo>>>,
    guest: Rc<Cell<bool>>,
}

impl RoomPanel {
    pub fn build(root: &gtk::Overlay) -> Self {
        let panel_body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        panel_body.add_css_class("wp-panel");
        panel_body.set_size_request(330, -1);
        panel_body.set_margin_top(12);
        panel_body.set_margin_bottom(12);
        panel_body.set_vexpand(true);

        // ---- Lobi ----
        let lobby = gtk::Box::new(gtk::Orientation::Vertical, 12);
        lobby.set_margin_top(8);
        let lobby_head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let lobby_title = gtk::Label::new(Some("Birlikte İzle"));
        lobby_title.add_css_class("wp-title");
        lobby_title.set_xalign(0.0);
        lobby_title.set_hexpand(true);
        let close_lobby = gtk::Button::from_icon_name("window-close-symbolic");
        close_lobby.add_css_class("flat");
        close_lobby.set_tooltip_text(Some("Paneli kapat"));
        lobby_head.append(&lobby_title);
        lobby_head.append(&close_lobby);
        lobby.append(&lobby_head);
        let lobby_icon = gtk::Image::from_icon_name("system-users-symbolic");
        lobby_icon.set_pixel_size(42);
        lobby_icon.add_css_class("wp-lobby-icon");
        lobby_icon.set_halign(gtk::Align::Center);
        lobby.append(&lobby_icon);
        let lobby_text = gtk::Label::new(Some(
            "Aynı bölümü arkadaşlarınla senkron izle.\nEn fazla 4 kişi, hesap gerekmez.",
        ));
        lobby_text.add_css_class("wp-empty");
        lobby_text.set_wrap(true);
        lobby_text.set_justify(gtk::Justification::Center);
        lobby_text.set_halign(gtk::Align::Center);
        lobby.append(&lobby_text);
        let create_btn = gtk::Button::with_label("Oda Kur / Katıl");
        create_btn.add_css_class("wp-action");
        create_btn.add_css_class("wp-action-accent");
        create_btn.set_halign(gtk::Align::Center);
        lobby.append(&create_btn);
        let lobby_note = gtk::Label::new(Some(
            "Bölüm açmak zorunlu değil. Oda kurup içerik seçebilirsin.",
        ));
        lobby_note.add_css_class("wp-host-hint");
        lobby_note.set_wrap(true);
        lobby_note.set_justify(gtk::Justification::Center);
        lobby_note.set_halign(gtk::Align::Center);
        lobby.append(&lobby_note);
        let lobby_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        lobby_spacer.set_vexpand(true);
        lobby.append(&lobby_spacer);

        // ---- Oda ----
        let room = gtk::Box::new(gtk::Orientation::Vertical, 10);
        room.set_margin_top(0);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::new(Some("Birlikte İzle"));
        title.add_css_class("wp-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let live = gtk::Label::new(Some("● Canlı"));
        live.add_css_class("wp-live");
        let count = gtk::Label::new(Some("0/4"));
        count.add_css_class("wp-count");
        let close_btn = gtk::Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("flat");
        close_btn.set_tooltip_text(Some("Paneli kapat"));
        header.append(&title);
        header.append(&live);
        header.append(&count);
        header.append(&close_btn);
        room.append(&header);

        let avatars = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        avatars.set_halign(gtk::Align::Start);
        avatars.set_margin_top(2);
        avatars.set_margin_bottom(2);
        room.append(&avatars);

        let status = gtk::Label::new(Some("Bağlanılıyor…"));
        status.add_css_class("wp-host-hint");
        status.set_xalign(0.0);
        status.set_wrap(true);
        room.append(&status);

        let content_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let content = gtk::Label::new(None);
        content.add_css_class("wp-content");
        content.set_xalign(0.0);
        content.set_hexpand(true);
        content.set_wrap(true);
        let join_btn = gtk::Button::with_label("Katıl");
        join_btn.add_css_class("wp-action");
        join_btn.add_css_class("wp-action-accent");
        join_btn.set_visible(false);
        content_row.append(&content);
        content_row.append(&join_btn);
        room.append(&content_row);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_margin_top(2);
        actions.set_margin_bottom(2);
        let invite_btn = action_button("edit-copy-symbolic", "Davet", "wp-action-accent");
        let leave_btn = action_button("system-log-out-symbolic", "Ayrıl", "wp-action-danger");
        invite_btn.set_visible(false);
        actions.append(&invite_btn);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        actions.append(&spacer);
        actions.append(&leave_btn);
        room.append(&actions);

        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep.add_css_class("wp-divider");
        room.append(&sep);

        let chat_head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let chat_title = gtk::Label::new(Some("SOHBET"));
        chat_title.add_css_class("wp-chat-title");
        chat_title.set_xalign(0.0);
        chat_title.set_hexpand(true);
        chat_head.append(&chat_title);
        room.append(&chat_head);

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
        room.append(&chat_overlay);

        let chat_entry = gtk::Entry::new();
        chat_entry.set_placeholder_text(Some("Mesaj yaz…"));
        chat_entry.add_css_class("wp-entry");
        room.append(&chat_entry);

        let ptt_hint = gtk::Label::new(Some("🎙 Bas-konuş: V"));
        ptt_hint.add_css_class("wp-host-hint");
        ptt_hint.set_xalign(0.0);
        room.append(&ptt_hint);

        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(140);
        stack.add_named(&lobby, Some("lobby"));
        stack.add_named(&room, Some("room"));
        stack.set_visible_child_name("lobby");
        panel_body.append(&stack);

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .transition_duration(200)
            .child(&panel_body)
            .build();
        revealer.set_reveal_child(false);
        revealer.set_halign(gtk::Align::End);
        revealer.set_valign(gtk::Align::Fill);
        revealer.set_vexpand(true);
        root.add_overlay(&revealer);

        let ptt_pill = gtk::Label::new(Some("🎙 Konuşuyorsun…"));
        ptt_pill.add_css_class("wp-ptt-pill");
        ptt_pill.set_halign(gtk::Align::Center);
        ptt_pill.set_valign(gtk::Align::End);
        ptt_pill.set_margin_bottom(112);
        ptt_pill.set_visible(false);
        ptt_pill.set_can_target(false);
        root.add_overlay(&ptt_pill);

        let panel = Self {
            revealer,
            panel_open: Rc::new(Cell::new(false)),
            stack,
            create_btn,
            close_btn,
            status,
            count,
            avatars,
            content,
            join_btn,
            invite_btn,
            leave_btn,
            chat_list,
            chat_scroll,
            chat_empty,
            chat_entry,
            new_msg_btn,
            ptt_hint,
            ptt_pill,
            chat_rows: Rc::new(Cell::new(0)),
            self_info: Rc::new(std::cell::RefCell::new(None)),
            peers: Rc::new(std::cell::RefCell::new(Vec::new())),
            guest: Rc::new(Cell::new(false)),
        };
        {
            let scroll = panel.chat_scroll.clone();
            let btn = panel.new_msg_btn.clone();
            panel.new_msg_btn.connect_clicked(move |_| {
                scroll_to_bottom(&scroll);
                btn.set_visible(false);
            });
        }
        {
            let panel_open = panel.panel_open.clone();
            let revealer = panel.revealer.clone();
            close_lobby.connect_clicked(move |_| {
                panel_open.set(false);
                revealer.set_reveal_child(false);
            });
        }
        {
            let panel_open = panel.panel_open.clone();
            let revealer = panel.revealer.clone();
            panel.close_btn.connect_clicked(move |_| {
                panel_open.set(false);
                revealer.set_reveal_child(false);
            });
        }
        panel
    }

    pub fn is_open(&self) -> bool {
        self.panel_open.get()
    }

    pub fn open(&self) {
        self.panel_open.set(true);
        self.revealer.set_reveal_child(true);
        self.new_msg_btn.set_visible(false);
        scroll_to_bottom(&self.chat_scroll);
    }

    pub fn close(&self) {
        self.panel_open.set(false);
        self.revealer.set_reveal_child(false);
    }

    pub fn toggle(&self) {
        if self.is_open() {
            self.close();
        } else {
            self.open();
        }
    }

    pub fn set_active(&self, active: bool, guest: bool) {
        self.guest.set(guest);
        self.stack
            .set_visible_child_name(if active { "room" } else { "lobby" });
        self.invite_btn.set_visible(active && !guest);
        if !active {
            self.ptt_pill.set_visible(false);
            self.clear_chat();
        }
    }

    pub fn set_people(&self, me: Option<&PeerInfo>, peers: &[PeerInfo]) {
        *self.self_info.borrow_mut() = me.cloned();
        *self.peers.borrow_mut() = peers.to_vec();
        self.render_avatars();
    }

    pub fn set_status(&self, text: &str) {
        self.status.set_text(text);
    }

    pub fn set_invite_visible(&self, visible: bool) {
        self.invite_btn.set_visible(visible && !self.guest.get());
    }

    pub fn set_content(&self, content: &RoomContent, can_join: bool) {
        let label = content.label();
        self.content.set_text(if label.is_empty() {
            "İçerik seçilmedi"
        } else {
            &label
        });
        self.join_btn
            .set_visible(can_join && content.kind == ContentKind::Watching);
    }

    pub fn set_ptt(&self, active: bool, mic_ok: bool) {
        self.ptt_pill.set_visible(active);
        self.ptt_hint.set_text(if !mic_ok {
            "🎙 Mikrofon yok"
        } else if active {
            "🎙 Konuşuyorsun…"
        } else {
            "🎙 Bas-konuş: V"
        });
    }

    pub fn append_chat(&self, msg: &ChatMessage) {
        let at_bottom = scroll_at_bottom(&self.chat_scroll);
        if self.chat_empty.parent().is_some() {
            self.chat_empty.set_visible(false);
            self.chat_list.remove(&self.chat_empty);
        }
        let self_id = self.self_info.borrow().as_ref().map(|p| p.id);
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
        self.chat_list.append(&outer);
        self.chat_rows.set(self.chat_rows.get() + 1);
        while self.chat_rows.get() > 200 {
            if let Some(first) = self.chat_list.first_child() {
                self.chat_list.remove(&first);
                self.chat_rows.set(self.chat_rows.get() - 1);
            } else {
                break;
            }
        }
        if self.is_open() {
            if at_bottom || own {
                scroll_to_bottom(&self.chat_scroll);
                self.new_msg_btn.set_visible(false);
            } else {
                self.new_msg_btn.set_visible(true);
            }
        }
    }

    pub fn clear_chat(&self) {
        clear_box(&self.chat_list);
        self.chat_list.append(&self.chat_empty);
        self.chat_empty.set_visible(true);
        self.chat_rows.set(0);
    }

    fn render_avatars(&self) {
        clear_box(&self.avatars);
        let self_info = self.self_info.borrow().clone();
        let mut all = Vec::new();
        if let Some(me) = self_info.clone() {
            all.push(me);
        }
        all.extend(self.peers.borrow().iter().cloned());
        all.sort_by_key(|p| p.id);
        all.dedup_by_key(|p| p.id);
        all.sort_by_key(|p| (p.role != Role::Host, p.id));
        let count = all.len().min(MAX_PEERS);
        self.count.set_text(&format!("{count}/{MAX_PEERS}"));
        if all.is_empty() {
            let placeholder = gtk::Label::new(Some("…"));
            placeholder.add_css_class("wp-avatar");
            placeholder.add_css_class("wp-avatar-1");
            placeholder.set_size_request(34, 34);
            self.avatars.append(&placeholder);
            return;
        }
        for peer in all.iter().take(MAX_PEERS) {
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
            if self_info.as_ref().is_some_and(|me| me.id == peer.id) {
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
                if self_info.as_ref().is_some_and(|me| me.id == peer.id) {
                    " · sen"
                } else {
                    ""
                }
            )));
            self.avatars.append(&avatar);
        }
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

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
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

/// Oda paneli CSS'i. Tema yüklendikten sonra global provider'a eklenir.
pub fn room_panel_css() -> String {
    r#"
    .watch-dot {
        color: #3ddc97; font-size: 9px;
        text-shadow: 0 0 6px rgba(61,220,151,0.65);
    }
    .watch-dot.unread {
        color: #ffb454;
        text-shadow: 0 0 6px rgba(255,180,84,0.65);
    }
    .top-nav-btn.watch-party-active { background: alpha(@accent_color, 0.78); }
    .top-nav-btn.watch-party-active:hover { background: alpha(@accent_color, 0.95); }
    .wp-panel {
        background-color: rgba(12,12,18,0.92);
        background-image: linear-gradient(180deg, alpha(@accent_color, 0.10), transparent 140px);
        border-left: 1px solid rgba(255,255,255,0.12);
        border-radius: 18px 0 0 18px;
        padding: 14px;
        box-shadow: -16px 0 40px rgba(0,0,0,0.45);
    }
    .wp-title { font-weight: 800; font-size: 1.05em; color: #FFFFFF; }
    .wp-live { color: #3ddc97; font-size: 0.72em; font-weight: 700; }
    .wp-count { color: rgba(255,255,255,0.55); font-size: 0.78em; }
    .wp-host-hint { color: rgba(255,255,255,0.55); font-size: 0.78em; }
    .wp-content { color: rgba(255,255,255,0.82); font-size: 0.84em; font-weight: 600; }
    .wp-lobby-icon { color: alpha(@accent_color, 0.95); margin-top: 12px; }
    .wp-avatar {
        min-width: 34px; min-height: 34px;
        border-radius: 999px; padding: 0;
        color: #FFFFFF; font-weight: 800; font-size: 0.9em;
        border: 2px solid rgba(255,255,255,0.12);
    }
    .wp-avatar-self { border-color: alpha(@accent_color, 0.95); }
    .wp-avatar-host { border-color: #E5C07B; }
    .wp-avatar-0 { background: #E06C75; }
    .wp-avatar-1 { background: #61AFEF; }
    .wp-avatar-2 { background: #98C379; }
    .wp-avatar-3 { background: #E5C07B; }
    .wp-avatar-4 { background: #C678DD; }
    .wp-avatar-5 { background: #56B6C2; }
    .wp-action {
        background: rgba(255,255,255,0.08); background-image: none;
        border: 1px solid rgba(255,255,255,0.12); border-radius: 999px;
        box-shadow: none; padding: 6px 14px;
        color: #FFFFFF; font-weight: 600;
    }
    .wp-action:hover { background: rgba(255,255,255,0.14); }
    .wp-action-accent {
        background: alpha(@accent_color, 0.88);
        border-color: alpha(@accent_color, 0.95);
    }
    .wp-action-accent:hover { background: alpha(@accent_color, 1.0); }
    .wp-action-danger { color: #FF7B72; background: transparent; border-color: transparent; }
    .wp-action-danger:hover { background: rgba(255,123,114,0.12); }
    .wp-divider { background: rgba(255,255,255,0.08); min-height: 1px; }
    .wp-chat-title {
        color: rgba(255,255,255,0.70); font-size: 0.72em;
        font-weight: 800; letter-spacing: 0.08em;
    }
    .wp-bubble {
        background: rgba(255,255,255,0.07);
        border: 1px solid rgba(255,255,255,0.08);
        border-radius: 12px; padding: 7px 10px;
    }
    .wp-bubble-self {
        background: alpha(@accent_color, 0.20);
        border-color: alpha(@accent_color, 0.35);
    }
    .wp-msg-name { color: rgba(255,255,255,0.58); font-size: 0.72em; font-weight: 700; }
    .wp-msg-time { color: rgba(255,255,255,0.36); font-size: 0.68em; font-variant-numeric: tabular-nums; }
    .wp-msg-text { color: #FFFFFF; font-size: 0.92em; }
    .wp-empty { color: rgba(255,255,255,0.42); font-size: 0.86em; }
    .wp-entry {
        background: rgba(255,255,255,0.07); background-image: none;
        border: 1px solid rgba(255,255,255,0.10); border-radius: 10px;
        box-shadow: none; padding: 6px 10px; color: #FFFFFF;
    }
    .wp-entry:focus { border-color: alpha(@accent_color, 0.75); }
    .wp-new-msg {
        background: alpha(@accent_color, 0.92); background-image: none;
        border: none; border-radius: 999px; box-shadow: 0 4px 14px rgba(0,0,0,0.35);
        color: #FFFFFF; font-size: 0.78em; font-weight: 700; padding: 4px 12px;
    }
    .wp-ptt-pill {
        background: rgba(10,10,14,0.86);
        border: 1px solid alpha(@accent_color, 0.55); border-radius: 999px;
        box-shadow: 0 6px 24px rgba(0,0,0,0.45);
        color: #FFFFFF; font-weight: 600; padding: 6px 14px;
    }
    .side-row.room-active { background: alpha(@accent_color, 0.16); }
    .room-badge { color: #3ddc97; font-size: 9px; }
    .room-badge.unread { color: #ffb454; }
    "#
    .to_string()
}

pub fn install(root: &impl IsA<gtk::Widget>) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&room_panel_css());
    gtk::style_context_add_provider_for_display(
        &root.display(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
