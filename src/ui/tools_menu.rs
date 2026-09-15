//! Sayfalar menüsü: sayfa kısayolları + animasyonlu popover + kendi CSS'i.
//! main.rs şişmesin diye stil bu modülün sağlayıcısındadır (covers.rs deseni).

use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

/// Menüdeki sayfalar: (anahtar, görünen ad). Sıra popover gezinme sırasıdır.
pub const TOOL_PAGES: [(&str, &str); 6] = [
    ("home", "Ana Sayfa"),
    ("fav", "Favoriler"),
    ("marathon", "Maraton"),
    ("history", "Geçmiş"),
    ("downloads", "İndirilenler"),
    ("settings", "Ayarlar"),
];

/// Ayarlardaki kısayol seçenekleri. Sıra ComboRow indeksleriyle eşleşir.
pub const TOOL_SHORTCUT_OPTIONS: [&str; 4] = ["Ctrl+T", "Alt+T", "F10", "T"];

/// Açılış/kapanış animasyon süresi (ms). Kapanış beklemesiyle eşleşir.
const REVEAL_MS: u64 = 180;

/// Kısayol eşleşmesi (saf fonksiyon; GTK'sız test edilir).
/// `focus_is_editable`: odak bir metin alanındaysa çıplak `T` yutulur.
pub fn match_tools_shortcut(
    shortcut: &str,
    key_name: &str,
    ctrl: bool,
    alt: bool,
    focus_is_editable: bool,
) -> bool {
    match shortcut {
        "Alt+T" => alt && !ctrl && (key_name == "t" || key_name == "T"),
        "F10" => key_name == "F10",
        "T" => {
            if focus_is_editable {
                return false;
            }
            !ctrl && !alt && (key_name == "t" || key_name == "T")
        }
        _ => ctrl && !alt && (key_name == "t" || key_name == "T"), // Ctrl+T
    }
}

/// Modül CSS'i (kapsamlı seçiciler; genel sağlayıcıya dokunmaz).
pub fn tools_menu_css() -> String {
    r#"
                button.tools-header-btn {
                    border-radius: 20px;
                    padding: 4px 10px;
                    font-size: 0.88em;
                    min-height: 0;
                    min-width: 0;
                }
                button.tools-header-btn:hover {
                    background-color: alpha(currentColor, 0.1);
                }

                /* NOT: süs iç düğüme çizilir; dış popover çıplaktır (çift çerçeve olur). */
                popover.tools-pop,
                popover.tools-pop.background {
                    background-color: transparent;
                    background-image: none;
                    border: none;
                    box-shadow: none;
                    outline: none;
                    padding: 0;
                }
                popover.tools-pop > contents {
                    background-color: var(--popover-bg-color);
                    color: var(--popover-fg-color);
                    border: 1px solid alpha(currentColor, 0.12);
                    border-radius: 14px;
                    padding: 6px;
                    outline: none;
                    box-shadow: 0 4px 18px alpha(black, 0.45);
                }
                popover.tools-pop > arrow {
                    background-color: transparent;
                    background-image: none;
                    border: none;
                    box-shadow: none;
                    outline: none;
                    padding: 0;
                }
                popover.tools-pop list.tools-list {
                    background-color: transparent;
                }
                popover.tools-pop list.tools-list > row {
                    border-radius: 8px;
                    padding: 8px 12px;
                }
                popover.tools-pop list.tools-list > row:selected,
                popover.tools-pop list.tools-list > row:focus {
                    background-color: alpha(@accent_color, 0.22);
                    outline: none;
                }

                entry.tools-search-pill {
                    border-radius: 9999px;
                    min-width: 420px;
                }
                entry.tools-search-pill:focus {
                    border-color: alpha(@accent_color, 0.6);
                }
                "#
    .to_string()
}

/// CSS sağlayıcısını bir kez yükler (covers.rs deseni).
pub fn ensure_tools_css() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        let css = gtk::CssProvider::new();
        css.load_from_data(&tools_menu_css());
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

/// İlk satıra odak verir (görsel seçim + klavye aktivasyonu birlikte).
/// `select_row` tek başına boyar; `Enter` (row-activated) için satır odağı şart.
/// popup() ile aynı tick'teki grab tutmayabildiği için idle'da denenir.
fn focus_first_row(list: &gtk::ListBox) {
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
        let list_c = list.clone();
        glib::idle_add_local_once(move || {
            if first.is_visible() {
                first.grab_focus();
            } else {
                list_c.grab_focus();
            }
        });
    }
}

/// Animasyonlu Sayfalar menüsü tutamacı (Clone edilebilir).
#[derive(Clone)]
pub struct ToolsMenu {
    button: gtk::Button,
    popover: gtk::Popover,
    reveal: gtk::Revealer,
    list: gtk::ListBox,
    close_gen: Rc<Cell<u64>>,
}

impl ToolsMenu {
    /// `goto`: menü satır indeksi (0..TOOL_PAGES) → sayfaya git.
    /// `toast`: ipucu toast'ları için overlay. `client`: ilk-açılış bayrağı.
    /// `shortcut_label`: güncel kısayol metni (toast açıklamasında kullanılır).
    pub fn build(
        goto: Rc<dyn Fn(usize)>,
        toast: adw::ToastOverlay,
        client: Arc<crate::api::Client>,
        shortcut_label: Rc<dyn Fn() -> String>,
    ) -> Self {
        let button = gtk::Button::with_label("Sayfalar");
        button.add_css_class("flat");
        button.add_css_class("tools-header-btn");
        button.set_tooltip_text(Some("Sayfalar menüsü (kısayol atanabilir)"));

        let close_gen = Rc::new(Cell::new(0u64));

        let reveal = gtk::Revealer::new();
        reveal.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        reveal.set_transition_duration(REVEAL_MS as u32);

        let popover = gtk::Popover::new();
        popover.add_css_class("tools-pop");
        popover.set_child(Some(&reveal));
        popover.set_has_arrow(true);
        popover.set_autohide(true);

        // Animasyonlu kapatma: önce perdeyi topla, sonra popdown.
        let close_animated: Rc<dyn Fn()> = {
            let reveal_c = reveal.clone();
            let popover_c = popover.clone();
            let gen_c = close_gen.clone();
            Rc::new(move || {
                let g = gen_c.get() + 1;
                gen_c.set(g);
                reveal_c.set_reveal_child(false);
                let popover_cc = popover_c.clone();
                let gen_cc = gen_c.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(REVEAL_MS + 10),
                    move || {
                        if gen_cc.get() == g {
                            popover_cc.popdown();
                        }
                    },
                );
            })
        };

        // TEK gerçek: tıklama VE Enter ikisi de buradan geçer (satır kimliğiyle).
        let list = gtk::ListBox::new();
        list.add_css_class("tools-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        for (_, name) in TOOL_PAGES.iter() {
            let lbl = gtk::Label::new(Some(name));
            lbl.set_xalign(0.0);
            lbl.add_css_class("title-4");
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&lbl));
            row.set_activatable(true);
            row.set_selectable(true);
            list.append(&row);
        }
        {
            let goto_c = goto.clone();
            let close_c = close_animated.clone();
            list.connect_row_activated(move |_, row| {
                goto_c(row.index() as usize);
                close_c();
            });
        }
        reveal.set_child(Some(&list));

        // Otohide/Esc ile kapanınca perdeyi sıfırla (animasyonsuz yol).
        {
            let reveal_c = reveal.clone();
            let gen_c = close_gen.clone();
            popover.connect_closed(move |_| {
                gen_c.set(gen_c.get() + 1);
                reveal_c.set_reveal_child(false);
            });
        }

        // Odak haritalamada alınır: popup() ile aynı tick'teki grab tutmaz.
        // Satıra odak verilir ki Enter direkt çalışsın (seçim yetmez).
        {
            let list_c = list.clone();
            popover.connect_map(move |_| {
                focus_first_row(&list_c);
            });
        }

        let menu = Self {
            button: button.clone(),
            popover: popover.clone(),
            reveal: reveal.clone(),
            list: list.clone(),
            close_gen: close_gen.clone(),
        };

        // Buton: aç + ilk-açılış ipucu.
        {
            let menu_c = menu.clone();
            button.connect_clicked(move |_| {
                menu_c.open(&toast, &client, &shortcut_label);
            });
        }
        popover.set_parent(&button);

        menu
    }

    /// Menüyü animasyonla açar (gerekirse 15sn ipucu gösterir).
    pub fn open(
        &self,
        toast: &adw::ToastOverlay,
        client: &Arc<crate::api::Client>,
        shortcut_label: &Rc<dyn Fn() -> String>,
    ) {
        self.close_gen.set(self.close_gen.get() + 1);
        self.popover.popup();
        self.reveal.set_visible(true);
        self.reveal.set_reveal_child(true);
        focus_first_row(&self.list);
        if !client.is_tools_tip_seen() {
            client.set_tools_tip_seen(true);
            let t = adw::Toast::new(&format!(
                "Sayfalar: {} ile aç • Ok ile seç • Enter/tık ile aç",
                glib::markup_escape_text(&shortcut_label())
            ));
            t.set_timeout(15);
            toast.add_toast(t);
        }
    }

    pub fn button(&self) -> gtk::Button {
        self.button.clone()
    }

    pub fn is_open(&self) -> bool {
        self.popover.is_visible()
    }

    /// Animasyonlu kapatma (dışarıdan: kısayol aç/kapa).
    pub fn close(&self) {
        let g = self.close_gen.get() + 1;
        self.close_gen.set(g);
        self.reveal.set_reveal_child(false);
        let popover_c = self.popover.clone();
        let gen_c = self.close_gen.clone();
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(REVEAL_MS + 10),
            move || {
                if gen_c.get() == g {
                    popover_c.popdown();
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_pages_count_and_order() {
        assert_eq!(TOOL_PAGES.len(), 6);
        assert_eq!(TOOL_PAGES[0].0, "home");
        assert_eq!(TOOL_PAGES[1].0, "fav");
        assert_eq!(TOOL_PAGES[5].0, "settings");
        for (_, name) in TOOL_PAGES {
            assert!(
                name.chars().all(|c| c.is_alphanumeric() || c == ' '),
                "menüde emoji/sembol olmamalı: {name}"
            );
        }
    }

    #[test]
    fn shortcut_options_count() {
        assert_eq!(TOOL_SHORTCUT_OPTIONS.len(), 4);
        assert_eq!(TOOL_SHORTCUT_OPTIONS[0], "Ctrl+T");
    }

    #[test]
    fn matcher_ctrl_t() {
        assert!(match_tools_shortcut("Ctrl+T", "t", true, false, false));
        assert!(match_tools_shortcut("Ctrl+T", "T", true, false, true));
        assert!(!match_tools_shortcut("Ctrl+T", "t", false, false, false));
        assert!(!match_tools_shortcut("Ctrl+T", "t", true, true, false));
        assert!(!match_tools_shortcut("bilinmeyen", "x", true, false, false));
    }

    #[test]
    fn matcher_alt_t_and_f10() {
        assert!(match_tools_shortcut("Alt+T", "T", false, true, false));
        assert!(!match_tools_shortcut("Alt+T", "T", true, true, false));
        assert!(match_tools_shortcut("F10", "F10", false, false, false));
        assert!(match_tools_shortcut("F10", "F10", false, false, true));
        assert!(!match_tools_shortcut("F10", "F9", false, false, false));
    }

    #[test]
    fn matcher_bare_t_guarded_by_editable_focus() {
        assert!(match_tools_shortcut("T", "t", false, false, false));
        assert!(!match_tools_shortcut("T", "t", false, false, true));
        assert!(!match_tools_shortcut("T", "t", true, false, false));
    }

    #[test]
    fn css_contains_scoped_selectors() {
        let css = tools_menu_css();
        for sel in [
            ".tools-header-btn",
            "popover.tools-pop",
            "popover.tools-pop > contents",
            "list.tools-list",
            "entry.tools-search-pill",
        ] {
            assert!(css.contains(sel), "{sel} yok");
        }
        assert!(!css.contains(" height:"), "ölü özellik sızmamalı");
        assert!(!css.contains(" width:"), "ölü özellik sızmamalı");
    }
}
