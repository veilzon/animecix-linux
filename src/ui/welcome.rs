//! Karşılama ekranı: hero + özellikler + kurulum + tercihler.

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
        icon: "media-skip-forward-symbolic",
        title: "Akıllı Atlama",
        desc: "'s' / 'e' ile intro/outro sonuna atla, şarkıyı gör ('Shift+M' ile aç)",
    },
    Feature {
        icon: "folder-download-symbolic",
        title: "Çevrimdışı İndirme",
        desc: "Bölümleri kalite seçerek indir, kuyruktan izle",
    },
    Feature {
        icon: "applications-graphics-symbolic",
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
        let flatpak = crate::flatpak::is_flatpak();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(28);
        root.set_margin_bottom(32);
        root.set_margin_start(28);
        root.set_margin_end(28);

        root.add_css_class("welcome-root");

        // Hero.
        let hero = gtk::Box::new(gtk::Orientation::Vertical, 10);
        hero.add_css_class("welcome-hero");
        hero.set_halign(gtk::Align::Fill);
        hero.set_hexpand(true);
        hero.set_margin_bottom(6);

        let badge = gtk::Label::new(Some("YENİ OYNATICI · GÖMÜLÜ LIBMPV"));
        badge.add_css_class("welcome-badge");
        badge.set_halign(gtk::Align::Center);
        hero.append(&badge);

        let icon = gtk::Image::from_icon_name("tr.com.animecix");
        icon.add_css_class("welcome-icon");
        icon.set_pixel_size(76);
        hero.append(&icon);

        let title = gtk::Label::new(Some("AnimeciX"));
        title.add_css_class("welcome-title");
        title.set_halign(gtk::Align::Center);
        hero.append(&title);

        let tag = gtk::Label::new(Some(if flatpak {
            "Flatpak sürümü — bağımlılıklar ve menü girdisi paketle gelir."
        } else {
            "Anime, dizi ve film — uygulama içi oynatıcıyla, kaldığın yerden."
        }));
        tag.add_css_class("welcome-tagline");
        tag.set_halign(gtk::Align::Center);
        tag.set_wrap(true);
        tag.set_justify(gtk::Justification::Center);
        hero.append(&tag);
        root.append(&hero);

        // Özellik kartları (dar pencerede alt satıra sarar).
        let cards = gtk::FlowBox::new();
        cards.set_selection_mode(gtk::SelectionMode::None);
        cards.set_max_children_per_line(3);
        cards.set_min_children_per_line(1);
        cards.set_homogeneous(true);
        cards.set_column_spacing(12);
        cards.set_row_spacing(12);
        cards.set_halign(gtk::Align::Fill);
        for f in FEATURES {
            let card = gtk::Box::new(gtk::Orientation::Vertical, 7);
            card.add_css_class("welcome-feature");
            card.set_margin_top(2);
            card.set_margin_bottom(2);
            card.set_hexpand(true);
            let icon = gtk::Image::from_icon_name(f.icon);
            icon.add_css_class("welcome-feature-icon");
            icon.set_pixel_size(28);
            icon.set_halign(gtk::Align::Start);
            card.append(&icon);
            let ft = gtk::Label::new(Some(f.title));
            ft.add_css_class("welcome-feature-title");
            ft.set_xalign(0.0);
            card.append(&ft);
            let fd = gtk::Label::new(Some(f.desc));
            fd.add_css_class("welcome-feature-desc");
            fd.set_xalign(0.0);
            fd.set_wrap(true);
            fd.set_max_width_chars(34);
            card.append(&fd);
            cards.append(&card);
        }
        root.append(&cards);

        // Kurulum durumu (Flatpak'ta bağımlılık ve menü girdisi paketle gelir).
        if !flatpak {
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
            let system_managed = crate::native_package::system_package_managed();
            let appimage_blocked = crate::appimage_integration_disabled();
            desk_row.set_subtitle(if system_managed || appimage_blocked {
                "Menü kısayolunu sistem paketi yönetiyor."
            } else if check_desktop_entry_installed() {
                "AnimeciX menüde hazır."
            } else {
                "Tek tıkla uygulama menüsüne ekle."
            });
            if !system_managed {
                let desk_btn = gtk::Button::with_label(if appimage_blocked {
                    "Sistem Kurulumu Etkin"
                } else if check_desktop_entry_installed() {
                    "Yeniden Entegre Et"
                } else {
                    "Menüye Ekle"
                });
                desk_btn.add_css_class("pill");
                desk_btn.set_sensitive(!appimage_blocked);
                if !appimage_blocked && !check_desktop_entry_installed() {
                    desk_btn.add_css_class("suggested-action");
                } else {
                    desk_btn.add_css_class("flat");
                }
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
            }
            setup_group.add(&desk_row);
            root.append(&setup_group);
        }

        // Hızlı tercihler.
        let pref_group = adw::PreferencesGroup::new();
        pref_group.set_title("Hızlı Tercihler");

        let (fs_row, fs_sw) = crate::ui::components::switch_row(
            "Oynatıcı Otomatik Tam Ekran",
            "Video başladığında oynatıcı tam ekranda açılır",
            settings.auto_fullscreen,
        );
        pref_group.add(&fs_row);

        let (auto_next_row, auto_next_sw) = crate::ui::components::switch_row(
            "Otomatik Sonraki Bölüm",
            "Bölüm bitince sıradakini kendiliğinden açar",
            settings.auto_next,
        );
        pref_group.add(&auto_next_row);

        let (outro_skip_row, outro_skip_sw) = crate::ui::components::switch_row(
            "Outro'yu Otomatik Atla",
            "Outro başlayınca sonunu beklemeden sonraki bölüme geç",
            settings.auto_skip_outro,
        );
        outro_skip_row.set_sensitive(settings.auto_next);
        {
            let row_c = outro_skip_row.clone();
            auto_next_sw.connect_active_notify(move |sw| row_c.set_sensitive(sw.is_active()));
        }
        pref_group.add(&outro_skip_row);

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
            "MPV Atlama Bildirimleri",
            "Harici mpv'de intro/outro başında atlama ipucu göster",
            settings.show_intro_hint,
        );
        pref_group.add(&intro_hint_row);

        let (music_hint_row, music_hint_sw) = crate::ui::components::switch_row(
            "MPV Şarkı İpucu",
            "Harici mpv'nin şarkı satırında Shift+M ipucunu göster",
            settings.show_music_hint,
        );
        pref_group.add(&music_hint_row);
        root.append(&pref_group);

        let on_finish = Rc::new(on_finish);
        // Başlat.
        let start_btn = gtk::Button::with_label(if flatpak { "Başla" } else { "Hemen Başla" });
        start_btn.add_css_class("suggested-action");
        start_btn.add_css_class("pill");
        start_btn.add_css_class("welcome-start");
        start_btn.set_halign(gtk::Align::Center);
        start_btn.set_margin_top(14);
        {
            let base = settings.clone();
            let f = on_finish.clone();
            start_btn.connect_clicked(move |_| {
                let mut s = base.clone();
                s.auto_fullscreen = fs_sw.is_active();
                s.auto_next = auto_next_sw.is_active();
                s.auto_skip_outro = outro_skip_sw.is_active();
                s.show_intro_hint = intro_hint_sw.is_active();
                s.show_music_hint = music_hint_sw.is_active();
                s.theme = crate::theme::THEMES
                    .get(theme_row.selected() as usize)
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_else(|| crate::theme::DEFAULT_THEME.to_string());
                f(s);
            });
        }
        root.insert_child_after(&start_btn, Some(&cards));

        // Atla: ayarla oynamadan çık (bayrak görünüm kurulurken yazıldı).
        let skip_btn = gtk::Button::with_label("Atla →");
        skip_btn.add_css_class("flat");
        skip_btn.add_css_class("pill");
        skip_btn.add_css_class("welcome-skip");
        skip_btn.set_halign(gtk::Align::Center);
        {
            let f = on_finish.clone();
            let s = settings.clone();
            skip_btn.connect_clicked(move |_| {
                f(s.clone());
            });
        }
        root.insert_child_after(&skip_btn, Some(&start_btn));

        let footnote = gtk::Label::new(Some(
            "F1 ile tüm kısayollar · Oynatıcı ve önbellek ayarları Ayarlar'da",
        ));
        footnote.add_css_class("welcome-footnote");
        footnote.set_halign(gtk::Align::Center);
        footnote.set_margin_top(2);
        footnote.set_wrap(true);
        root.append(&footnote);

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
        // Esc de çıkıştır (butona ulaşamayan klavye kullanıcısı için).
        {
            let f = on_finish.clone();
            let s = settings.clone();
            let esc = gtk::EventControllerKey::new();
            esc.connect_key_pressed(move |_, keyval, _, _| {
                if keyval.name().map(|s| s.to_string()).unwrap_or_default() == "Escape" {
                    f(s.clone());
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            page.add_controller(esc);
        }
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
