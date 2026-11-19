use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::api::Episode;

#[derive(Clone)]
pub struct EpisodeDrawer {
    pub revealer: gtk::Revealer,
    pub season_bar: gtk::Box,
    pub list: gtk::Box,
    pub count: gtk::Label,
    pub strip: gtk::Revealer,
    pub tip: gtk::Revealer,
    pub tip_shown: Rc<Cell<bool>>,
    pub selected_season: Rc<Cell<u64>>,
}

/// İpucu yalnız çok bölümlü yapımlarda ve kullanıcı çekmeceyi hiç açmadıysa.
pub fn tip_needed(seen: bool, episode_count: usize) -> bool {
    !seen && episode_count > 1
}

/// Oynatma başladıktan sonra ipucunun görünmesi için gereken süre.
pub const TIP_DELAY_SECS: u64 = 20;

pub fn build(stage: &gtk::Overlay, on_open: Rc<dyn Fn()>) -> EpisodeDrawer {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.add_css_class("ep-panel");
    panel.set_size_request(300, -1);
    panel.set_margin_top(12);
    panel.set_margin_bottom(12);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("BÖLÜMLER"));
    title.add_css_class("ep-panel-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    let count = gtk::Label::new(None);
    count.add_css_class("dim-label");
    count.add_css_class("caption");
    head.append(&title);
    head.append(&count);
    panel.append(&head);

    let season_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    season_bar.set_halign(gtk::Align::Start);
    panel.append(&season_bar);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    list.set_hexpand(true);
    scroll.set_child(Some(&list));
    panel.append(&scroll);

    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .transition_duration(200)
        .child(&panel)
        .build();
    revealer.set_reveal_child(false);
    revealer.set_halign(gtk::Align::End);
    revealer.set_valign(gtk::Align::Fill);
    revealer.set_vexpand(true);
    stage.add_overlay(&revealer);

    // Sağ kenardaki ince tutamaç; fareyle üzerine gelince panel açılır.
    let pill = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pill.add_css_class("embed-edge");
    pill.set_size_request(4, 110);
    pill.set_halign(gtk::Align::Center);
    pill.set_valign(gtk::Align::Center);
    let zone = gtk::Box::new(gtk::Orientation::Vertical, 0);
    zone.set_size_request(18, 240);
    zone.set_halign(gtk::Align::End);
    zone.set_valign(gtk::Align::Center);
    zone.append(&pill);
    let strip = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .transition_duration(160)
        .child(&zone)
        .build();
    strip.set_reveal_child(true);
    strip.set_halign(gtk::Align::End);
    strip.set_valign(gtk::Align::Center);
    strip.set_visible(false);
    stage.add_overlay(&strip);

    // Sağ kenardaki çekmeceyi ilk kez keşfettirmek için 20 sn sonra
    // gösterilen ipucu; kullanıcı çekmeceyi açınca kalıcı olarak kapanır.
    let tip_label = gtk::Label::new(Some("Bölümler →"));
    tip_label.add_css_class("player-drawer-tip");
    let tip = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .transition_duration(180)
        .child(&tip_label)
        .build();
    tip.set_reveal_child(false);
    tip.set_visible(false);
    tip.set_halign(gtk::Align::End);
    tip.set_valign(gtk::Align::Center);
    tip.set_margin_end(26);
    tip.set_can_target(false);
    stage.add_overlay(&tip);

    let opened_once = Rc::new(Cell::new(false));
    {
        let rev = revealer.clone();
        let tip_c = tip.clone();
        let opened = opened_once.clone();
        let on_open = on_open.clone();
        let enter = gtk::EventControllerMotion::new();
        enter.connect_enter(move |_, _, _| {
            rev.set_reveal_child(true);
            tip_c.set_reveal_child(false);
            tip_c.set_visible(false);
            if !opened.replace(true) {
                on_open();
            }
        });
        zone.add_controller(enter);
    }
    {
        let rev = revealer.clone();
        let leave = gtk::EventControllerMotion::new();
        leave.connect_leave(move |_| rev.set_reveal_child(false));
        panel.add_controller(leave);
    }

    EpisodeDrawer {
        revealer,
        season_bar,
        list,
        count,
        strip,
        tip,
        tip_shown: Rc::new(Cell::new(false)),
        selected_season: Rc::new(Cell::new(0)),
    }
}

/// 20 saniyelik oynatma sonrası ipucunu gösterir; 8 sn sonra kendiliğinden
/// gizlenir ama kullanıcı çekmeceyi açana kadar bir sonraki oyunda tekrar denenir.
pub fn show_tip(drawer: &EpisodeDrawer) {
    if drawer.tip_shown.replace(true) {
        return;
    }
    drawer.tip.set_visible(true);
    drawer.tip.set_reveal_child(true);
    let tip = drawer.tip.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(8), move || {
        tip.set_reveal_child(false);
        let tip2 = tip.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(220), move || {
            tip2.set_visible(false);
        });
    });
}

pub fn render(
    drawer: &EpisodeDrawer,
    episodes: &[Episode],
    current_season: u64,
    current_episode: u64,
    runtime: Option<i64>,
    is_movie: bool,
    on_play: &Rc<dyn Fn(Episode)>,
) {
    drawer.strip.set_visible(!is_movie && episodes.len() > 1);
    if episodes.is_empty() {
        clear(&drawer.season_bar);
        clear(&drawer.list);
        let l = gtk::Label::new(Some("Bölüm listesi yükleniyor…"));
        l.add_css_class("dim-label");
        l.set_xalign(0.0);
        l.set_margin_start(8);
        drawer.list.append(&l);
        drawer.count.set_text("");
        return;
    }

    let mut seasons: Vec<u64> = episodes.iter().map(|e| e.season).collect();
    seasons.sort_unstable();
    seasons.dedup();
    let selected = if seasons.contains(&drawer.selected_season.get()) {
        drawer.selected_season.get()
    } else if seasons.contains(&current_season) {
        current_season
    } else {
        seasons[0]
    };
    drawer.selected_season.set(selected);
    render_season(
        drawer,
        episodes,
        selected,
        current_season,
        current_episode,
        runtime,
        is_movie,
        on_play,
    );
}

fn render_season(
    drawer: &EpisodeDrawer,
    episodes: &[Episode],
    season: u64,
    current_season: u64,
    current_episode: u64,
    runtime: Option<i64>,
    is_movie: bool,
    on_play: &Rc<dyn Fn(Episode)>,
) {
    clear(&drawer.season_bar);
    clear(&drawer.list);

    let mut seasons: Vec<u64> = episodes.iter().map(|e| e.season).collect();
    seasons.sort_unstable();
    seasons.dedup();
    for s in seasons.into_iter().filter(|_| !is_movie) {
        let b = gtk::Button::with_label(&format!("S{s}"));
        b.add_css_class("season-tab");
        if s == season {
            b.add_css_class("suggested-action");
        }
        let drawer_c = drawer.clone();
        let eps = episodes.to_vec();
        let cb = on_play.clone();
        b.connect_clicked(move |_| {
            drawer_c.selected_season.set(s);
            render_season(
                &drawer_c,
                &eps,
                s,
                current_season,
                current_episode,
                runtime,
                is_movie,
                &cb,
            );
        });
        drawer.season_bar.append(&b);
    }

    let mut current: Vec<&Episode> = episodes.iter().filter(|e| e.season == season).collect();
    current.sort_by_key(|e| e.episode);
    let count_text = if is_movie {
        "Film".to_string()
    } else {
        format!("{} bölüm", current.len())
    };
    drawer.count.set_text(&count_text);
    for e in current {
        let is_current = e.season == current_season && e.episode == current_episode;
        let b = gtk::Button::new();
        b.add_css_class("flat");
        b.set_halign(gtk::Align::Fill);
        let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
        vb.set_hexpand(true);
        vb.set_margin_top(6);
        vb.set_margin_bottom(6);
        vb.set_margin_start(8);
        vb.set_margin_end(8);
        let name = if e.name.trim().is_empty() {
            if is_movie {
                "Film".to_string()
            } else {
                format!("Bölüm {}", e.episode)
            }
        } else {
            e.name.clone()
        };
        let nm = gtk::Label::new(Some(&name));
        nm.set_xalign(0.0);
        nm.set_wrap(true);
        nm.set_lines(2);
        nm.set_ellipsize(gtk::pango::EllipsizeMode::End);
        nm.set_hexpand(true);
        vb.append(&nm);
        let mut sub = if is_movie {
            "Film".to_string()
        } else {
            format!("S{:02}E{:02}", e.season, e.episode)
        };
        if let Some(rt) = runtime.filter(|r| *r > 0) {
            sub.push_str(&format!(" · {rt} dk"));
        }
        let meta = gtk::Label::new(Some(&sub));
        meta.add_css_class("dim-label");
        meta.add_css_class("caption");
        meta.set_xalign(0.0);
        vb.append(&meta);
        b.set_child(Some(&vb));
        if is_current {
            b.add_css_class("ep-current");
            b.set_sensitive(false);
            b.set_tooltip_text(Some("Şu an izlenen bölüm"));
        } else {
            b.set_tooltip_text(Some("Bu bölümü oynat"));
        }
        let cb = on_play.clone();
        let ep = e.clone();
        let rev = drawer.revealer.clone();
        b.connect_clicked(move |_| {
            rev.set_reveal_child(false);
            cb(ep.clone());
        });
        drawer.list.append(&b);
    }
}

fn clear(box_: &gtk::Box) {
    let mut cur = box_.first_child();
    while let Some(child) = cur {
        let next = child.next_sibling();
        box_.remove(&child);
        cur = next;
    }
}

#[cfg(test)]
mod tests {
    use super::tip_needed;

    #[test]
    fn drawer_tip_only_for_multi_episode_and_until_seen() {
        assert!(tip_needed(false, 2));
        assert!(tip_needed(false, 12));
        assert!(!tip_needed(true, 12));
        assert!(!tip_needed(false, 1));
        assert!(!tip_needed(false, 0));
    }
}
