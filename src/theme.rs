//! Tam-uygulama tema katmanı: sabit palet + gradyan CSS üretimi.
//! Baz CSS (main.rs) taşınmaz; bu katman window/accent üzerine eklenir.

use gtk::prelude::*;

/// (id, görünen ad). Sıra ComboRow indeksleriyle eşleşir.
/// Yalnızca koyu aile: açık tema kaldırıldı.
pub const THEMES: [(&str, &str); 5] = [
    ("koyu", "Koyu"),
    ("bordo", "Bordo"),
    ("orman", "Orman"),
    ("lacivert", "Lacivert"),
    ("mor", "Mor"),
];

pub const DEFAULT_THEME: &str = "koyu";

struct Palette {
    base: &'static str,
    over1: &'static str,
    over2: &'static str,
    accent: &'static str,
    on_accent: &'static str,
    /// Geniş yüzey boyası (bölüm satırları): koyu temada nötr, renklilerde accent.
    row_tint: &'static str,
    /// Üst çubuk (headerbar) zemini: her temanın kendi koyu tonu.
    header_bg: &'static str,
}

fn palette(id: &str) -> Palette {
    match id {
        "bordo" => Palette {
            base: "#1A0E12",
            over1: "rgba(229,72,77,0.24)",
            over2: "rgba(124,20,40,0.22)",
            accent: "#E5484D",
            on_accent: "#FFFFFF",
            row_tint: "#E5484D",
            header_bg: "#261016",
        },
        "orman" => Palette {
            base: "#0D1A13",
            over1: "rgba(46,194,126,0.22)",
            over2: "rgba(20,80,50,0.24)",
            accent: "#2EC27E",
            on_accent: "#FFFFFF",
            row_tint: "#2EC27E",
            header_bg: "#10241B",
        },
        "lacivert" => Palette {
            base: "#0E1626",
            over1: "rgba(98,160,234,0.26)",
            over2: "rgba(30,60,110,0.24)",
            accent: "#62A0EA",
            on_accent: "#10131A",
            row_tint: "#62A0EA",
            header_bg: "#152238",
        },
        "mor" => Palette {
            base: "#170F2B",
            over1: "rgba(124,58,237,0.26)",
            over2: "rgba(219,39,119,0.20)",
            accent: "#7C3AED",
            on_accent: "#FFFFFF",
            row_tint: "#7C3AED",
            header_bg: "#211640",
        },
        "koyu" => Palette {
            base: "#1E1E1E",
            over1: "rgba(255,255,255,0.04)",
            over2: "rgba(0,0,0,0.0)",
            accent: "#3584E4",
            on_accent: "#FFFFFF",
            row_tint: "#FFFFFF",
            header_bg: "#2A2A2A",
        },
        _ => Palette {
            // Bilinmeyen: güvenli varsayılan = koyu.
            base: "#1E1E1E",
            over1: "rgba(255,255,255,0.04)",
            over2: "rgba(0,0,0,0.0)",
            accent: "#3584E4",
            on_accent: "#FFFFFF",
            row_tint: "#FFFFFF",
            header_bg: "#2A2A2A",
        },
    }
}

/// Koyu zorunluluğu: tüm temalar koyu ailedendir (açık tema kaldırıldı).
pub fn wants_force_dark(_theme_id: &str) -> bool {
    true
}

/// Tema CSS'i. Bilinmeyen id → koyu (güvenli varsayılan).
pub fn theme_css(theme_id: &str) -> String {
    let id = if THEMES.iter().any(|(i, _)| *i == theme_id) {
        theme_id
    } else {
        DEFAULT_THEME
    };
    let p = palette(id);
    format!(
        "window {{ background-color: {base}; \
         background-image: linear-gradient(135deg, {o1}, {o2}); }}\n\
         @define-color accent_bg_color {a};\n\
         @define-color accent_fg_color {fg};\n\
         @define-color accent_color {a};\n\
         @define-color row_tint {r};\n\
         @define-color headerbar_bg_color {hb};\n\
         @define-color headerbar_fg_color #FFFFFF;\n\
         headerbar {{ background-color: {hb}; \
         background-image: linear-gradient(135deg, {o1}, {o2}); \
         border-bottom: 1px solid alpha({a}, 0.35); }}\n\
         headerbar button.flat {{ color: #FFFFFF; }}\n",
        base = p.base,
        o1 = p.over1,
        o2 = p.over2,
        a = p.accent,
        fg = p.on_accent,
        r = p.row_tint,
        hb = p.header_bg,
    )
}

/// Tek kalıcı provider ile temayı uygular (birikim yok; ana thread).
/// Koyu temalarda `theme-dark` sınıfı takılır (sabit okunaklı yazılar için).
pub fn apply_theme(window: &adw::ApplicationWindow, theme_id: &str) {
    use std::sync::OnceLock;
    static ADDED: OnceLock<bool> = OnceLock::new();
    thread_local! {
        static PROVIDER: gtk::CssProvider = gtk::CssProvider::new();
    }
    ADDED.get_or_init(|| {
        if let Some(display) = gtk::gdk::Display::default() {
            PROVIDER.with(|p| {
                gtk::style_context_add_provider_for_display(
                    &display,
                    p,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            });
        }
        true
    });
    PROVIDER.with(|p| p.load_from_data(&theme_css(theme_id)));
    if wants_force_dark(theme_id) {
        window.add_css_class("theme-dark");
    } else {
        window.remove_css_class("theme-dark");
    }
    adw::StyleManager::default().set_color_scheme(if wants_force_dark(theme_id) {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::Default
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_themes_produce_contrast_safe_base() {
        assert_eq!(THEMES.len(), 5, "yalnızca koyu aile");
        for (id, _) in THEMES {
            let css = theme_css(id);
            assert!(!css.is_empty(), "{id}: css boş olamaz");
            assert!(css.contains("background-color: #"), "{id}: baz yok");
            assert!(css.contains("linear-gradient"), "{id}: gradyan yok");
            assert!(css.contains("@define-color accent_bg_color"), "{id}: vurgu yok");
            assert!(css.contains("@define-color row_tint"), "{id}: satır boyası yok");
        }
    }

    #[test]
    fn unknown_theme_falls_back_to_dark() {
        assert_eq!(theme_css("uzay"), theme_css("koyu"));
    }

    #[test]
    fn all_themes_force_dark() {
        for (id, _) in THEMES {
            assert!(wants_force_dark(id), "{id}");
        }
    }

    #[test]
    fn headerbar_follows_theme() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for (id, _) in THEMES {
            let css = theme_css(id);
            assert!(css.contains("@define-color headerbar_bg_color #"), "{id}: başlık rengi yok");
            assert!(css.contains("headerbar {"), "{id}: headerbar kuralı yok");
            assert!(css.contains("headerbar button.flat"), "{id}: başlık buton rengi yok");
            let hb = css
                .lines()
                .find(|l| l.starts_with("@define-color headerbar_bg_color"))
                .unwrap()
                .to_string();
            assert!(seen.insert(hb.clone()), "iki tema aynı başlık rengini kullanmamalı: {hb}");
        }
    }

    #[test]
    fn koyu_rows_are_neutral() {
        let css = theme_css("koyu");
        assert!(css.contains("@define-color row_tint #FFFFFF"), "koyu satırlar nötr olmalı");
        assert!(!css.contains("98,160,234"), "koyu zeminde mavi ışıltı kalmamalı");
    }

    #[test]
    fn theme_index_roundtrip() {
        assert_eq!(THEMES.len(), 5);
        assert_eq!(THEMES[0].0, "koyu");
        assert!(THEMES.iter().any(|(i, _)| *i == "mor"));
        assert!(!THEMES.iter().any(|(i, _)| *i == "acik"), "açık tema kaldırıldı");
    }
}
