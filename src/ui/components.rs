use gtk::prelude::*;
use adw::prelude::*;
use crate::api::{Client, Title};

pub fn bookmark_button(client: &Client, t: &Title) -> gtk::Button {
    let saved = client.is_saved(t.id);
    let b = gtk::Button::from_icon_name(if saved {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    });
    b.add_css_class("flat");
    b.add_css_class("circular");
    b.add_css_class("lg-icon");
    b.add_css_class("bookmark-btn");
    // Satır yüksekliğine göre dikey esneyip ovalleşmesin: hep yuvarlak kalsın.
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some(if saved { "Favorilerden Çıkar" } else { "Favorilere Ekle" }));
    b
}

pub fn marathon_button(client: &Client, t: &Title) -> gtk::Button {
    let in_marathon = client.is_in_marathon(t.id);
    let b = gtk::Button::from_icon_name(if in_marathon {
        "media-playlist-repeat-symbolic"
    } else {
        "flag-symbolic"
    });
    b.add_css_class("flat");
    b.add_css_class("circular");
    b.add_css_class("lg-icon");
    b.add_css_class("bookmark-btn");
    // Satır yüksekliğine göre dikey esneyip ovalleşmesin: hep yuvarlak kalsın.
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some(if in_marathon { "Maratondan Çıkar" } else { "İzleme Maratonuna Ekle" }));
    b
}

pub fn create_status_page(title: &str, description: &str, icon_name: &str) -> adw::StatusPage {
    let sp = adw::StatusPage::new();
    sp.set_title(title);
    sp.set_description(Some(description));
    sp.set_icon_name(Some(icon_name));
    sp.set_vexpand(true);
    sp
}

/// Adw 1.2 uyumlu açma/kapama satırı (adw::SwitchRow 1.4 ister).
/// Döner: (satır, anahtar). Satıra tıklamak anahtarı çevirir.
pub fn switch_row(title: &str, subtitle: &str, active: bool) -> (adw::ActionRow, gtk::Switch) {
    let row = adw::ActionRow::new();
    row.set_title(title);
    row.set_subtitle(subtitle);
    let sw = gtk::Switch::new();
    sw.set_valign(gtk::Align::Center);
    sw.set_active(active);
    row.add_suffix(&sw);
    row.set_activatable_widget(Some(&sw));
    (row, sw)
}
