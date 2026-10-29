//! Flashcard çevirmen turu: toplu indirmede bölüm başına çevirmen seçimi.
//! Ortak-çevirmen giriş kartı (varsa) + tek tek bölüm kartları + özet.

use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::api::{Episode, FansubInfo, Title};

/// Bölümlerin tümünde ortak çevirmenler (template_id kesişimi, puana göre).
pub fn common_fansubs(items: &[(Episode, Vec<FansubInfo>)]) -> Vec<FansubInfo> {
    let mut iter = items.iter();
    let Some((_, first)) = iter.next() else {
        return Vec::new();
    };
    let mut common: Vec<i64> = first.iter().map(|f| f.template_id).collect();
    for (_, list) in iter {
        common.retain(|t| list.iter().any(|f| &f.template_id == t));
        if common.is_empty() {
            break;
        }
    }
    let mut out: Vec<FansubInfo> = first
        .iter()
        .filter(|f| common.contains(&f.template_id))
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        b.rating
            .partial_cmp(&a.rating)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

pub fn find_template(list: &[FansubInfo], template_id: i64) -> Option<FansubInfo> {
    list.iter().find(|f| f.template_id == template_id).cloned()
}

struct State {
    items: Vec<(Episode, Vec<FansubInfo>)>,
    /// tour[pos] bölümünün seçimi.
    choice: Vec<Option<FansubInfo>>,
    page: usize,
    pages: Vec<PageKind>,
}

#[derive(Clone, Copy, PartialEq)]
enum PageKind {
    Common,
    Ep(usize),
    Summary,
}

fn fansub_card(fs: &FansubInfo, selected: bool) -> gtk::Button {
    let row = gtk::Button::new();
    row.add_css_class("card");
    if selected {
        row.add_css_class("suggested-action");
    }
    row.set_halign(gtk::Align::Fill);
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    hbox.set_margin_top(10);
    hbox.set_margin_bottom(10);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);
    let name = gtk::Label::new(Some(&fs.name));
    name.add_css_class("title-4");
    name.set_hexpand(true);
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    hbox.append(&name);
    if fs.rating > 0.0 {
        let rate = gtk::Label::new(Some(&format!("★ {:.1}", fs.rating)));
        rate.add_css_class("accent");
        hbox.append(&rate);
    }
    if !fs.approved_only {
        let old = gtk::Label::new(Some("eski"));
        old.add_css_class("dim-label");
        hbox.append(&old);
    }
    row.set_child(Some(&hbox));
    row
}

/// Seçim listesini görsel duruma yansıtır (sadece verilen buton seçili).
fn mark_selected(listbox: &gtk::Box, selected_btn: &gtk::Button) {
    let mut cur = listbox.first_child();
    while let Some(w) = cur {
        let next = w.next_sibling();
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            if b == selected_btn {
                b.add_css_class("suggested-action");
            } else {
                b.remove_css_class("suggested-action");
            }
        }
        cur = next;
    }
}

/// Flashcard turunu açar. Bitince seçilen (bölüm, çevirmen) çiftleri döner;
/// kapatılırsa hiçbir şey dönmez (çöpe atılır).
pub fn show_flashcard_wizard(
    parent: &adw::ApplicationWindow,
    title: &Title,
    items: Vec<(Episode, Vec<FansubInfo>)>,
    on_done: impl Fn(Vec<(Episode, FansubInfo)>) + 'static,
) {
    // Tur: çevirisi olan bölümler (boşlar özetten atlanır).
    let tour: Rc<Vec<usize>> = Rc::new(
        items
            .iter()
            .enumerate()
            .filter(|(_, (_, l))| !l.is_empty())
            .map(|(i, _)| i)
            .collect(),
    );
    let skipped_empty = items.len() - tour.len();
    let commons = common_fansubs(&items);

    // Sayfa listesi: [ortak?] + tur + özet.
    let mut pages: Vec<PageKind> = Vec::new();
    if !commons.is_empty() {
        pages.push(PageKind::Common);
    }
    for &t in tour.iter() {
        pages.push(PageKind::Ep(t));
    }
    pages.push(PageKind::Summary);
    let summary_idx = pages.len() - 1;

    // Tek çevirmenli bölümler önceden seçili gelir.
    let mut choice: Vec<Option<FansubInfo>> = vec![None; tour.len()];
    for (pos, &t) in tour.iter().enumerate() {
        if items[t].1.len() == 1 {
            choice[pos] = Some(items[t].1[0].clone());
        }
    }

    let st = Rc::new(RefCell::new(State {
        items,
        choice,
        page: 0,
        pages,
    }));
    let on_done = Rc::new(on_done);

    let dlg = gtk::Window::builder()
        .modal(true)
        .transient_for(parent)
        .default_width(420)
        .default_height(480)
        .resizable(false)
        .title("Çevirmen Seç")
        .build();

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(16);
    root.set_margin_bottom(12);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let counter = gtk::Label::new(None);
    counter.add_css_class("title-4");
    counter.set_xalign(0.0);
    root.append(&counter);

    let bar = gtk::ProgressBar::new();
    bar.set_show_text(false);
    root.append(&bar);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_vexpand(true);
    root.append(&stack);

    let nav = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    nav.set_halign(gtk::Align::Center);
    let back_btn = gtk::Button::with_label("← Geri");
    back_btn.add_css_class("flat");
    back_btn.add_css_class("pill");
    let skip_btn = gtk::Button::with_label("Atla");
    skip_btn.add_css_class("flat");
    skip_btn.add_css_class("pill");
    let apply_all = gtk::CheckButton::with_label("Hepsine uygula");
    apply_all.set_tooltip_text(Some("Bu seçimi kalan bölümlere de uygular"));
    let next_btn = gtk::Button::with_label("İleri →");
    next_btn.add_css_class("suggested-action");
    next_btn.add_css_class("pill");
    nav.append(&back_btn);
    nav.append(&skip_btn);
    nav.append(&apply_all);
    nav.append(&next_btn);
    root.append(&nav);

    // Sayfa geçişi + sayaç + nav durumu (içerik yeniden kurulmaz).
    let render: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let render_c = render.clone();
    let st_r = st.clone();
    let tour_r = tour.clone();
    let stack_r = stack.clone();
    let counter_r = counter.clone();
    let bar_r = bar.clone();
    let back_btn_r = back_btn.clone();
    let skip_btn_r = skip_btn.clone();
    let next_btn_r = next_btn.clone();
    let apply_all_r = apply_all.clone();
    *render.borrow_mut() = Some(Rc::new(move || {
        let s = st_r.borrow();
        let kind = s.pages[s.page];
        let total = s.pages.len();
        let (caption, pos_of_tour) = match kind {
            PageKind::Common => ("Ortak Çevirmenler".to_string(), None),
            PageKind::Ep(t) => {
                let pos = tour_r.iter().position(|&x| x == t).unwrap_or(0);
                (format!("Bölüm {}/{}", pos + 1, tour_r.len()), Some(pos))
            }
            PageKind::Summary => ("Özet".to_string(), None),
        };
        counter_r.set_text(&caption);
        bar_r.set_fraction(if total > 1 {
            s.page as f64 / (total - 1) as f64
        } else {
            1.0
        });

        let name = match kind {
            PageKind::Common => "pg_common".to_string(),
            PageKind::Ep(t) => format!("pg_ep{t}"),
            PageKind::Summary => "pg_summary".to_string(),
        };
        if kind == PageKind::Summary {
            if let Some(old) = stack_r.child_by_name("pg_summary") {
                stack_r.remove(&old);
            }
            stack_r.add_named(
                &build_summary(&s.items, &s.choice, &tour_r, skipped_empty),
                Some("pg_summary"),
            );
        }
        stack_r.set_visible_child_name(&name);

        let is_summary = matches!(kind, PageKind::Summary);
        let at_last_ep = matches!(kind, PageKind::Ep(_))
            && s.page + 1 < total
            && matches!(s.pages[s.page + 1], PageKind::Summary);
        next_btn_r.set_label(if is_summary {
            "Bitir ✓"
        } else if at_last_ep {
            "Özete Git →"
        } else if matches!(kind, PageKind::Common) {
            "Tek tek seç →"
        } else {
            "İleri →"
        });
        let can_next = match (kind, pos_of_tour) {
            (PageKind::Common, _) => true,
            (PageKind::Summary, _) => s.choice.iter().any(|c| c.is_some()),
            (PageKind::Ep(_), Some(pos)) => s.choice.get(pos).and_then(|c| c.as_ref()).is_some(),
            (PageKind::Ep(_), None) => false,
        };
        next_btn_r.set_sensitive(can_next);
        skip_btn_r.set_visible(matches!(kind, PageKind::Ep(_)));
        let show_apply = matches!(kind, PageKind::Ep(_));
        apply_all_r.set_visible(show_apply);
        if !show_apply {
            apply_all_r.set_active(false);
        }
        back_btn_r.set_sensitive(s.page > 0);
    }));

    // Ortak kart sayfası.
    if !commons.is_empty() {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let head = gtk::Label::new(Some(&format!(
            "Bu çevirmenler seçili tüm bölümlerde var — birini seç, hepsi bitsin.\n{}",
            title.name
        )));
        head.set_xalign(0.0);
        head.add_css_class("dim-label");
        head.set_wrap(true);
        vbox.append(&head);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for fs in &commons {
            let btn = fansub_card(fs, false);
            let st_c = st.clone();
            let render_c = render_c.clone();
            let list_c = list.clone();
            let fs_c = fs.clone();
            let tour_c = tour.clone();
            btn.connect_clicked(move |b| {
                {
                    let mut s = st_c.borrow_mut();
                    for (pos, &t) in tour_c.iter().enumerate() {
                        if let Some(f) = find_template(&s.items[t].1, fs_c.template_id) {
                            s.choice[pos] = Some(f);
                        }
                    }
                    s.page = summary_idx;
                }
                mark_selected(&list_c, b);
                render_c.borrow().as_ref().unwrap()();
            });
            list.append(&btn);
        }
        scroll.set_child(Some(&list));
        vbox.append(&scroll);
        stack.add_named(&vbox, Some("pg_common"));
    }

    // Bölüm sayfaları (bağlantılı).
    for &t in tour.iter() {
        let (ep, list) = {
            let s = st.borrow();
            (s.items[t].0.clone(), s.items[t].1.clone())
        };
        let pos = tour.iter().position(|&x| x == t).unwrap_or(0);
        let preselected = st.borrow().choice[pos].as_ref().map(|f| f.template_id);

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let head = gtk::Label::new(Some(&format!(
            "S{:02}E{:02} — {}",
            ep.season, ep.episode, ep.name
        )));
        head.set_xalign(0.0);
        head.add_css_class("title-2");
        vbox.append(&head);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        let listbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for fs in &list {
            let sel = preselected == Some(fs.template_id);
            let btn = fansub_card(fs, sel);
            let st_c = st.clone();
            let render_c = render_c.clone();
            let list_c = listbox.clone();
            let fs_c = fs.clone();
            btn.connect_clicked(move |b| {
                st_c.borrow_mut().choice[pos] = Some(fs_c.clone());
                mark_selected(&list_c, b);
                render_c.borrow().as_ref().unwrap()();
            });
            listbox.append(&btn);
        }
        scroll.set_child(Some(&listbox));
        vbox.append(&scroll);
        stack.add_named(&vbox, Some(&format!("pg_ep{t}")));
    }

    // Nav bağlantıları.
    {
        let st_c = st.clone();
        let render_c = render_c.clone();
        back_btn.connect_clicked(move |_| {
            let mut s = st_c.borrow_mut();
            if s.page > 0 {
                s.page -= 1;
            }
            drop(s);
            render_c.borrow().as_ref().unwrap()();
        });
    }
    {
        let st_c = st.clone();
        let render_c = render_c.clone();
        skip_btn.connect_clicked(move |_| {
            let mut s = st_c.borrow_mut();
            if s.page + 1 < s.pages.len() {
                s.page += 1;
            }
            drop(s);
            render_c.borrow().as_ref().unwrap()();
        });
    }
    {
        let st_c = st.clone();
        let render_c = render_c.clone();
        let apply_c = apply_all.clone();
        let dlg_c = dlg.clone();
        let on_done_c = on_done.clone();
        let tour_c = tour.clone();
        next_btn.connect_clicked(move |_| {
            let finish = {
                let mut s = st_c.borrow_mut();
                // Hepsine uygula: mevcut seçimi kalan kararsızlara yay.
                if apply_c.is_active() {
                    if let Some(cur) = current_choice(&s, &tour_c) {
                        for (pos, &t) in tour_c.iter().enumerate() {
                            if s.choice[pos].is_none() {
                                if let Some(f) = find_template(&s.items[t].1, cur.template_id) {
                                    s.choice[pos] = Some(f);
                                }
                            }
                        }
                    }
                    apply_c.set_active(false);
                }
                let is_summary = matches!(s.pages[s.page], PageKind::Summary);
                if !is_summary && s.page + 1 < s.pages.len() {
                    s.page += 1;
                }
                is_summary
            };
            if finish {
                let done: Vec<(Episode, FansubInfo)> = {
                    let s = st_c.borrow();
                    tour_c
                        .iter()
                        .enumerate()
                        .filter_map(|(pos, &t)| {
                            s.choice[pos].clone().map(|fs| (s.items[t].0.clone(), fs))
                        })
                        .collect()
                };
                dlg_c.close();
                on_done_c(done);
            } else {
                render_c.borrow().as_ref().unwrap()();
            }
        });
    }

    dlg.set_child(Some(&root));
    render.borrow().as_ref().unwrap()();
    dlg.present();
}

/// O anki sayfadaki seçim (Hepsine uygula kaynağı).
fn current_choice(s: &State, tour: &[usize]) -> Option<FansubInfo> {
    let PageKind::Ep(t) = s.pages[s.page] else {
        return None;
    };
    let pos = tour.iter().position(|&x| x == t)?;
    s.choice.get(pos).cloned().flatten()
}

fn build_summary(
    items: &[(Episode, Vec<FansubInfo>)],
    choice: &[Option<FansubInfo>],
    tour: &[usize],
    skipped_empty: usize,
) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
    vbox.set_margin_top(8);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    for (pos, &t) in tour.iter().enumerate() {
        let (ep, _) = &items[t];
        let txt = match choice.get(pos).and_then(|c| c.as_ref()) {
            Some(fs) => format!(
                "{} → {} ({:.1}★)",
                crate::ui::components::episode_label_compact(ep.season, ep.episode, &ep.name),
                fs.name,
                fs.rating
            ),
            None => format!(
                "{} → atlanacak",
                crate::ui::components::episode_label_compact(ep.season, ep.episode, &ep.name)
            ),
        };
        let lbl = gtk::Label::new(Some(&txt));
        lbl.set_xalign(0.0);
        lbl.add_css_class("title-4");
        list.append(&lbl);
    }
    if skipped_empty > 0 {
        let lbl = gtk::Label::new(Some(&format!(
            "{skipped_empty} bölümde çeviri yok (atlanacak)"
        )));
        lbl.set_xalign(0.0);
        lbl.add_css_class("dim-label");
        list.append(&lbl);
    }
    scroll.set_child(Some(&list));
    vbox.append(&scroll);
    vbox
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(season: u64, episode: u64) -> Episode {
        Episode {
            season,
            episode,
            name: format!("B{episode}"),
        }
    }

    fn fs(tpl: i64, name: &str, rating: f64) -> FansubInfo {
        FansubInfo {
            template_id: tpl,
            name: name.into(),
            rating,
            total_votes: 0,
            language: "tr".into(),
            approved_only: true,
            mirror_count: 1,
            hosts: Vec::new(),
            mirrors: Vec::new(),
        }
    }

    #[test]
    fn common_fansubs_intersection_and_rating_order() {
        let items = vec![
            (
                ep(1, 1),
                vec![fs(1, "A", 9.0), fs(2, "B", 7.0), fs(3, "C", 8.0)],
            ),
            (ep(1, 2), vec![fs(2, "B", 7.0), fs(3, "C", 8.0)]),
            (ep(1, 3), vec![fs(3, "C", 8.0), fs(4, "D", 9.5)]),
        ];
        let c = common_fansubs(&items);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].template_id, 3);
        let items2 = vec![
            (ep(1, 1), vec![fs(1, "A", 9.0), fs(2, "B", 7.0)]),
            (ep(1, 2), vec![fs(2, "B", 7.0), fs(1, "A", 9.0)]),
        ];
        let c2 = common_fansubs(&items2);
        assert_eq!(c2.len(), 2);
        assert_eq!(c2[0].template_id, 1, "yüksek puan önce");
    }

    #[test]
    fn common_fansubs_empty_cases() {
        let empty: Vec<(Episode, Vec<FansubInfo>)> = Vec::new();
        assert!(common_fansubs(&empty).is_empty());
        let no_overlap = vec![
            (ep(1, 1), vec![fs(1, "A", 9.0)]),
            (ep(1, 2), vec![fs(2, "B", 9.0)]),
        ];
        assert!(
            common_fansubs(&no_overlap).is_empty(),
            "kesişim yoksa kart çıkmaz"
        );
        let one_empty = vec![(ep(1, 1), vec![fs(1, "A", 9.0)]), (ep(1, 2), Vec::new())];
        assert!(common_fansubs(&one_empty).is_empty());
    }

    #[test]
    fn find_template_shapes() {
        let list = vec![fs(1, "A", 9.0), fs(2, "B", 7.0)];
        assert_eq!(
            find_template(&list, 2).map(|f| f.name),
            Some("B".to_string())
        );
        assert!(find_template(&list, 9).is_none());
    }
}
