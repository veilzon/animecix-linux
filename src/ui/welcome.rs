//! Karşılama ekranı: hero + özellikler + kurulum + tercihler.

use gtk::prelude::*;
use adw::prelude::*;
use std::rc::Rc;

use crate::api::Settings;
use crate::{check_all_dependencies, check_desktop_entry_installed, install_desktop_entry};

pub struct WelcomeView;

struct Feature {
    icon: &'static str,
    title: &'static str,
    desc: &'static str,
}

const FEATURES: [Feature; 3] = [
    Feature {
        icon: "⏩",
        title: "Akıllı Atlama",
        desc: "'s' / 'e' ile intro/outro sonuna atla, şarkıyı gör ('Shift+M' ile aç)",
    },
    Feature {
        icon: "⬇",
        title: "Çevrimdışı İndirme",
        desc: "Bölümleri kalite seçerek indir, kuyruktan izle",
    },
    Feature {
        icon: "🎨",
        title: "Temalar",
        desc: "Bordo, Orman, Lacivert, Mor — tek tıkla değişir",
    },
];

impl WelcomeView {
    pub fn build(
        settings: &Settings,
        on_finish: impl Fn(Settings) + 'static,
        on_toast: impl Fn(String) + 'static,
        on_preview: impl Fn(String) + 'static,
    ) -> gtk::Overlay {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);

        // Hero.
        let hero = gtk::Box::new(gtk::Orientation::Vertical, 8);
        hero.set_halign(gtk::Align::Center);
        let icon = gtk::Image::from_icon_name("tr.com.animecix");
        icon.set_pixel_size(72);
        hero.append(&icon);
        let title = gtk::Label::new(Some("AnimeciX"));
        title.add_css_class("title-1");
        hero.append(&title);
        let tag = gtk::Label::new(Some("Anime, dizi ve film — tek tıkla, kaldığın yerden."));
        tag.add_css_class("dim-label");
        tag.add_css_class("title-4");
        tag.set_wrap(true);
        tag.set_justify(gtk::Justification::Center);
        hero.append(&tag);
        root.append(&hero);

        // Özellik kartları.
        let cards = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        cards.set_halign(gtk::Align::Center);
        for f in FEATURES {
            let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
            card.add_css_class("card");
            card.set_size_request(220, -1);
            card.set_margin_top(8);
            card.set_margin_bottom(8);
            card.set_margin_start(8);
            card.set_margin_end(8);
            let emoji = gtk::Label::new(Some(f.icon));
            emoji.add_css_class("title-1");
            emoji.set_halign(gtk::Align::Center);
            emoji.set_margin_top(8);
            card.append(&emoji);
            let ft = gtk::Label::new(Some(f.title));
            ft.add_css_class("title-4");
            ft.set_halign(gtk::Align::Center);
            card.append(&ft);
            let fd = gtk::Label::new(Some(f.desc));
            fd.add_css_class("dim-label");
            fd.set_wrap(true);
            fd.set_justify(gtk::Justification::Center);
            fd.set_max_width_chars(26);
            fd.set_margin_start(12);
            fd.set_margin_end(12);
            fd.set_margin_bottom(4);
            card.append(&fd);
            cards.append(&card);
        }
        root.append(&cards);

        // Kurulum durumu.
        let setup_group = adw::PreferencesGroup::new();
        setup_group.set_title("Kurulum Durumu");
        let deps = check_all_dependencies();
        let missing = deps.iter().filter(|d| !d.installed).count();
        let dep_row = adw::ActionRow::new();
        dep_row.set_title("Sistem Araçları");
        if missing == 0 {
            dep_row.set_subtitle(&format!("{} araç hazır ✅", deps.len()));
        } else {
            let names: Vec<String> = deps
                .iter()
                .filter(|d| !d.installed)
                .map(|d| {
                    if let Some(cmd) = &d.install_cmd {
                        format!("{} ({})", d.name, cmd)
                    } else {
                        d.name.to_string()
                    }
                })
                .collect();
            dep_row.set_subtitle(&format!("Eksik: {}", names.join(", ")));
        }
        setup_group.add(&dep_row);

        let desk_row = adw::ActionRow::new();
        desk_row.set_title("Uygulama Menüsü");
        let desk_btn = gtk::Button::with_label(if check_desktop_entry_installed() {
            "Yeniden Entegre Et"
        } else {
            "Menüye Ekle"
        });
        desk_btn.add_css_class("pill");
        if !check_desktop_entry_installed() {
            desk_btn.add_css_class("suggested-action");
        } else {
            desk_btn.add_css_class("flat");
        }
        desk_row.set_subtitle(if check_desktop_entry_installed() {
            "AnimeciX menüde hazır."
        } else {
            "Tek tıkla uygulama menüsüne ekle."
        });
        {
            let on_toast = Rc::new(on_toast);
            let row_c = desk_row.clone();
            let btn_c = desk_btn.clone();
            desk_btn.connect_clicked(move |_| match install_desktop_entry() {
                Ok(_) => {
                    row_c.set_subtitle("AnimeciX menüye eklendi!");
                    btn_c.set_label("Yeniden Entegre Et");
                    btn_c.remove_css_class("suggested-action");
                    btn_c.add_css_class("flat");
                    (*on_toast)("📌 AnimeciX menüye eklendi!".to_string());
                }
                Err(e) => (*on_toast)(format!("⚠️ Menüye eklenemedi: {e}")),
            });
        }
        desk_row.add_suffix(&desk_btn);
        setup_group.add(&desk_row);
        root.append(&setup_group);

        // Hızlı tercihler.
        let pref_group = adw::PreferencesGroup::new();
        pref_group.set_title("Hızlı Tercihler");

        let (fs_row, fs_sw) = crate::ui::components::switch_row(
            "MPV Otomatik Tam Ekran",
            "Video başladığında MPV tam ekranda açılır",
            settings.auto_fullscreen,
        );
        pref_group.add(&fs_row);

        let theme_row = adw::ComboRow::new();
        theme_row.set_title("Tema");
        theme_row.set_subtitle("Hemen dene, anında uygulanır");
        let theme_names: Vec<&str> = crate::theme::THEMES.iter().map(|(_, n)| *n).collect();
        theme_row.set_model(Some(&gtk::StringList::new(&theme_names)));
        let current_theme = crate::theme::THEMES
            .iter()
            .position(|(id, _)| *id == settings.theme)
            .unwrap_or(0) as u32;
        theme_row.set_selected(current_theme);
        {
            let on_preview = Rc::new(on_preview);
            theme_row.connect_selected_notify(move |row| {
                let id = crate::theme::THEMES
                    .get(row.selected() as usize)
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_else(|| crate::theme::DEFAULT_THEME.to_string());
                (*on_preview)(id);
            });
        }
        pref_group.add(&theme_row);

        let (intro_hint_row, intro_hint_sw) = crate::ui::components::switch_row(
            "Atlama Bildirimleri",
            "İntro/outro başında atlama ipucu göster",
            settings.show_intro_hint,
        );
        pref_group.add(&intro_hint_row);

        let (music_hint_row, music_hint_sw) = crate::ui::components::switch_row(
            "Şarkı İpucu",
            "Şarkı satırında Shift+M ile tarayıcıda aç ipucunu göster",
            settings.show_music_hint,
        );
        pref_group.add(&music_hint_row);
        root.append(&pref_group);

        // Başlat.
        let start_btn = gtk::Button::with_label("Başlamaya Hazırım 🚀");
        start_btn.add_css_class("suggested-action");
        start_btn.add_css_class("pill");
        start_btn.add_css_class("title-3");
        start_btn.set_halign(gtk::Align::Center);
        start_btn.set_margin_top(12);
        {
            let base = settings.clone();
            start_btn.connect_clicked(move |_| {
                let mut s = base.clone();
                s.auto_fullscreen = fs_sw.is_active();
                s.show_intro_hint = intro_hint_sw.is_active();
                s.show_music_hint = music_hint_sw.is_active();
                s.theme = crate::theme::THEMES
                    .get(theme_row.selected() as usize)
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_else(|| crate::theme::DEFAULT_THEME.to_string());
                on_finish(s);
            });
        }
        root.append(&start_btn);

        // Orta-alt kaydırma oku: dipte değilken görünür (yazısız, silik zeminli).
        let down_hint = gtk::Image::from_icon_name("go-down-symbolic");
        down_hint.set_pixel_size(20);
        let down_bg = gtk::Box::new(gtk::Orientation::Vertical, 0);
        down_bg.add_css_class("osd");
        down_bg.add_css_class("circular");
        down_bg.set_halign(gtk::Align::Center);
        down_bg.set_valign(gtk::Align::End);
        down_bg.set_margin_bottom(16);
        down_bg.append(&down_hint);
        {
            let down_c = down_bg.clone();
            let vadj = scroll.vadjustment();
            let refresh = {
                let down_cc = down_c.clone();
                let vadj_c = vadj.clone();
                move || {
                    let max = vadj_c.upper() - vadj_c.page_size();
                    down_cc.set_visible(vadj_c.value() < max - 2.0 && max > 0.0);
                }
            };
            let refresh_c = refresh.clone();
            vadj.connect_value_changed(move |_| refresh_c());
            vadj.connect_changed(move |_| refresh());
            down_c.set_visible(false);
        }

        let page = gtk::Overlay::new();
        page.set_child(Some(&scroll));
        page.add_overlay(&down_bg);
        scroll.set_child(Some(&root));
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_cover_core_pillars() {
        assert_eq!(FEATURES.len(), 3);
        let titles: Vec<&str> = FEATURES.iter().map(|f| f.title).collect();
        assert!(titles.contains(&"Akıllı Atlama"));
        assert!(titles.contains(&"Çevrimdışı İndirme"));
        assert!(titles.contains(&"Temalar"));
    }
}
