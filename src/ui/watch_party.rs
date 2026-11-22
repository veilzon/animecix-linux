use std::rc::Rc;

use adw::prelude::*;

use crate::watch_party::Invite;

pub enum WatchPartyAction {
    Create { name: String },
    Join { invite: Invite, name: String },
}

fn default_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        "Ben".to_string()
    } else {
        name.to_string()
    }
}

/// Oda kur / odaya katıl penceresi. Ağ işi yapmaz; seçimi callback'e verir.
pub fn present<F>(parent: &impl IsA<gtk::Window>, name_hint: &str, on_action: F)
where
    F: Fn(WatchPartyAction) + 'static,
{
    let win = adw::Window::builder()
        .title("Birlikte İzle")
        .modal(true)
        .transient_for(parent)
        .default_width(460)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let intro = gtk::Label::new(Some(
        "Aynı bölümü birlikte izleyin. Host oynatmayı yönetir; ses bas-konuş ile gider.",
    ));
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    intro.add_css_class("dim-label");
    root.append(&intro);

    let name_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let name_label = gtk::Label::new(Some("Görünen ad"));
    name_label.set_xalign(0.0);
    name_label.set_hexpand(true);
    let name_entry = gtk::Entry::new();
    name_entry.set_text(&default_name(name_hint));
    name_entry.set_placeholder_text(Some("Adın"));
    name_entry.set_width_chars(18);
    name_row.append(&name_label);
    name_row.append(&name_entry);
    root.append(&name_row);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(140);

    // Oda kur.
    let host_page = gtk::Box::new(gtk::Orientation::Vertical, 10);
    host_page.set_margin_top(6);
    let host_hint = gtk::Label::new(Some(
        "Oda kodu oluşturulur; kodu arkadaşlarına gönderirsin. En fazla 4 kişi.",
    ));
    host_hint.set_wrap(true);
    host_hint.set_xalign(0.0);
    host_hint.add_css_class("dim-label");
    host_page.append(&host_hint);
    let create_btn = gtk::Button::with_label("Oda Oluştur");
    create_btn.add_css_class("suggested-action");
    create_btn.add_css_class("pill");
    host_page.append(&create_btn);
    stack.add_titled(&host_page, Some("host"), "Oda Kur");

    // Odaya katıl.
    let join_page = gtk::Box::new(gtk::Orientation::Vertical, 10);
    join_page.set_margin_top(6);
    let invite_entry = gtk::Entry::new();
    invite_entry.set_placeholder_text(Some("Davet kodunu yapıştır"));
    invite_entry.set_width_chars(42);
    join_page.append(&invite_entry);
    let join_btn = gtk::Button::with_label("Katıl");
    join_btn.add_css_class("suggested-action");
    join_btn.add_css_class("pill");
    join_page.append(&join_btn);
    let join_error = gtk::Label::new(None);
    join_error.set_xalign(0.0);
    join_error.set_wrap(true);
    join_error.add_css_class("error");
    join_error.set_visible(false);
    join_page.append(&join_error);
    stack.add_titled(&join_page, Some("join"), "Katıl");

    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_halign(gtk::Align::Center);
    root.append(&switcher);
    root.append(&stack);

    let cancel = gtk::Button::with_label("İptal");
    cancel.add_css_class("flat");
    cancel.set_halign(gtk::Align::End);
    root.append(&cancel);
    win.set_content(Some(&root));

    let action_cb: Rc<dyn Fn(WatchPartyAction)> = Rc::new(on_action);
    {
        let win_c = win.clone();
        let name_c = name_entry.clone();
        let action = action_cb.clone();
        create_btn.connect_clicked(move |_| {
            let name = default_name(&name_c.text());
            win_c.close();
            action(WatchPartyAction::Create { name });
        });
    }
    {
        let win_c = win.clone();
        let name_c = name_entry.clone();
        let invite_c = invite_entry.clone();
        let error_c = join_error.clone();
        let action = action_cb.clone();
        join_btn.connect_clicked(move |_| match Invite::decode(&invite_c.text()) {
            Ok(invite) => {
                let name = default_name(&name_c.text());
                win_c.close();
                action(WatchPartyAction::Join { invite, name });
            }
            Err(e) => {
                error_c.set_text(&e);
                error_c.set_visible(true);
            }
        });
    }
    {
        let win_c = win.clone();
        cancel.connect_clicked(move |_| win_c.close());
    }
    win.present();
}

/// Host için davet kodu ve kopyalama penceresi.
pub fn present_invite(parent: &impl IsA<gtk::Window>, invite: &Invite) {
    let win = adw::Window::builder()
        .title("Oda Hazır")
        .modal(true)
        .transient_for(parent)
        .default_width(480)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let title = gtk::Label::new(Some("Davet kodunu paylaş"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    root.append(&title);

    let code = gtk::Entry::new();
    code.set_text(&invite.encode());
    code.set_editable(false);
    code.set_width_chars(44);
    root.append(&code);

    let note = gtk::Label::new(Some(
        "Kod 10 dakika geçerlidir. En fazla 4 kişi katılabilir. Kodu yalnızca davet ettiğin kişilere gönder.",
    ));
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.add_css_class("dim-label");
    root.append(&note);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_halign(gtk::Align::End);
    let copy = gtk::Button::with_label("Kopyala");
    copy.add_css_class("pill");
    let close = gtk::Button::with_label("Tamam");
    close.add_css_class("suggested-action");
    close.add_css_class("pill");
    row.append(&copy);
    row.append(&close);
    root.append(&row);
    win.set_content(Some(&root));

    let code_text = code.text().to_string();
    copy.connect_clicked(move |btn| {
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&code_text);
        }
        btn.set_label("Kopyalandı");
    });
    {
        let win_c = win.clone();
        close.connect_clicked(move |_| win_c.close());
    }
    win.present();
}
