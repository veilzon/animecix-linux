//! Satır içerik menüsü: düz Popover + Revealer + ListBox (Tools deseni).
//! PopoverMenu'nun iç düğüm boşluklarıyla uğraşılmaz; dolgu tam kontrol altındadır.
//! main.rs şişmesin diye stil bu modülün sağlayıcısındadır (covers.rs deseni).

use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;

/// Açılış/kapanış animasyon süresi (ms).
const REVEAL_MS: u64 = 150;

/// Modül CSS'i (dış çıplak, süs içte — çift çerçeve yok).
pub fn row_menu_css() -> String {
    r#"
                popover.row-pop,
                popover.row-pop.background {
                    background-color: transparent;
                    background-image: none;
                    border: none;
                    box-shadow: none;
                    outline: none;
                    padding: 0;
                }
                popover.row-pop > contents {
                    background-color: var(--popover-bg-color);
                    color: var(--popover-fg-color);
                    border: 1px solid alpha(currentColor, 0.12);
                    border-radius: 12px;
                    padding: 4px;
                    outline: none;
                    box-shadow: 0 4px 18px alpha(black, 0.45);
                }
                popover.row-pop > arrow {
                    background-color: transparent;
                    background-image: none;
                    border: none;
                    box-shadow: none;
                    outline: none;
                    padding: 0;
                }
                popover.row-pop list.row-list {
                    background-color: transparent;
                }
                popover.row-pop list.row-list > row {
                    border-radius: 8px;
                    padding: 8px 12px;
                }
                popover.row-pop list.row-list > row:selected,
                popover.row-pop list.row-list > row:focus {
                    background-color: alpha(@accent_color, 0.22);
                    outline: none;
                }
                "#
    .to_string()
}

/// CSS sağlayıcısını bir kez yükler (covers.rs deseni).
pub fn ensure_row_menu_css() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        let css = gtk::CssProvider::new();
        css.load_from_data(&row_menu_css());
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

/// İki-eylemli satır menüsü tutamacı.
#[derive(Clone)]
pub struct RowMenu {
    popover: gtk::Popover,
    reveal: gtk::Revealer,
    close_gen: Rc<Cell<u64>>,
}

impl RowMenu {
    /// `items`: (etiket, eylem). Eylem çalışınca menü animasyonla kapanır.
    pub fn build(items: Vec<(String, Rc<dyn Fn()>)>) -> Self {
        ensure_row_menu_css();

        let close_gen = Rc::new(Cell::new(0u64));

        let list = gtk::ListBox::new();
        list.add_css_class("row-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        for (label, _) in items.iter() {
            let lbl = gtk::Label::new(Some(label));
            lbl.set_xalign(0.0);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&lbl));
            row.set_activatable(true);
            row.set_selectable(true);
            list.append(&row);
        }

        let reveal = gtk::Revealer::new();
        reveal.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        reveal.set_transition_duration(REVEAL_MS as u32);
        reveal.set_child(Some(&list));

        let popover = gtk::Popover::new();
        popover.add_css_class("row-pop");
        popover.set_child(Some(&reveal));
        popover.set_has_arrow(false);
        popover.set_autohide(true);

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
        {
            let actions: Vec<Rc<dyn Fn()>> = items.into_iter().map(|(_, f)| f).collect();
            let close_c = close_animated.clone();
            list.connect_row_activated(move |_, row| {
                if let Some(act) = actions.get(row.index() as usize) {
                    act();
                }
                close_c();
            });
        }
        {
            let reveal_c = reveal.clone();
            let gen_c = close_gen.clone();
            popover.connect_closed(move |p| {
                gen_c.set(gen_c.get() + 1);
                reveal_c.set_reveal_child(false);
                p.unparent();
            });
        }
        {
            let list_c = list.clone();
            popover.connect_map(move |_| {
                focus_first_row(&list_c);
            });
        }

        Self {
            popover,
            reveal,
            close_gen,
        }
    }

    /// Menüyü verilen satırda, tıklanan noktada açar.
    pub fn popup_at(&self, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        self.close_gen.set(self.close_gen.get() + 1);
        self.popover.set_parent(parent);
        let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
        self.popover.set_pointing_to(Some(&rect));
        self.popover.popup();
        self.reveal.set_visible(true);
        self.reveal.set_reveal_child(true);
        if let Some(list) = self
            .reveal
            .child()
            .and_downcast::<gtk::ListBox>()
        {
            focus_first_row(&list);
        }
    }
}

/// İlk satıra odak verir: `Enter` (row-activated) için satır odağı şart,
/// `select_row` tek başına boyar. popup() aynı-tick grab tutmayabilir,
// idle'da dene.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_scopes_inner_frame_only() {
        let css = row_menu_css();
        for sel in [
            "popover.row-pop",
            "popover.row-pop > contents",
            "popover.row-pop > arrow",
            "list.row-list",
        ] {
            assert!(css.contains(sel), "{sel} yok");
        }
        assert!(!css.contains(" height:"), "ölü özellik sızmamalı");
        assert!(!css.contains(" width:"), "ölü özellik sızmamalı");
    }
}
