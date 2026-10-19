use crate::api::Title;
use gtk::prelude::*;

fn create_fact_badges(facts: &[String]) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    box_.set_halign(gtk::Align::Start);
    for f in facts {
        let b = gtk::Label::new(Some(f));
        b.add_css_class("detail-badge");
        box_.append(&b);
    }
    box_
}

pub fn append_title_submeta(info_box: &gtk::Box, t: &Title) {
    if let Some(g) = t.genre_line() {
        let gl = gtk::Label::new(Some(&g));
        gl.add_css_class("dim-label");
        gl.set_xalign(0.0);
        gl.set_wrap(false);
        gl.set_single_line_mode(true);
        gl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        info_box.append(&gl);
    }
    let meta = gtk::Label::new(Some(&t.meta_line()));
    meta.add_css_class("dim-label");
    meta.set_xalign(0.0);
    meta.set_wrap(false);
    meta.set_single_line_mode(true);
    meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
    info_box.append(&meta);
}

pub fn create_title_detail_header(
    title: &Title,
    poster_widget: &gtk::Picture,
    bookmark_btn: &gtk::Button,
    marathon_btn: &gtk::Button,
    download_btn: &gtk::Button,
) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    card.add_css_class("card");
    card.add_css_class("title-detail-card");
    card.set_margin_top(8);
    card.set_margin_bottom(12);
    card.set_margin_start(12);
    card.set_margin_end(12);
    card.append(poster_widget);

    let info_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    info_box.set_hexpand(true);
    info_box.set_valign(gtk::Align::Center);

    let name_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let name_lbl = gtk::Label::new(Some(&title.display_name()));
    name_lbl.add_css_class("title-1");
    name_lbl.set_xalign(0.0);
    name_lbl.set_wrap(true);
    name_lbl.set_hexpand(true);

    name_row.append(&name_lbl);
    name_row.append(bookmark_btn);
    name_row.append(marathon_btn);
    name_row.append(download_btn);
    info_box.append(&name_row);

    // Puan hapı (Lowell uyarlaması): TMDB puanı varsa ismin altında.
    if let Some(r) = title.rating {
        let pill = gtk::Label::new(Some(&format!("★ {r:.1}")));
        pill.add_css_class("rating-pill");
        pill.set_xalign(0.0);
        info_box.append(&pill);
    }

    if let Some(genre) = title.genre_line() {
        let genre_lbl = gtk::Label::new(Some(&genre));
        genre_lbl.add_css_class("dim-label");
        genre_lbl.add_css_class("title-4");
        genre_lbl.set_xalign(0.0);
        genre_lbl.set_wrap(false);
        genre_lbl.set_single_line_mode(true);
        genre_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        genre_lbl.set_tooltip_text(Some(&genre));
        info_box.append(&genre_lbl);
    }

    let facts = title.detail_facts();
    if !facts.is_empty() {
        info_box.append(&create_fact_badges(&facts));
    }

    if let Some(desc) = &title.description {
        let clean_desc = desc.trim();
        if !clean_desc.is_empty() {
            let desc_lbl = gtk::Label::new(Some(clean_desc));
            desc_lbl.add_css_class("dim-label");
            desc_lbl.set_xalign(0.0);
            desc_lbl.set_wrap(true);
            desc_lbl.set_max_width_chars(60);
            // Tam metin: satır sınırı ve "..." yok.
            info_box.append(&desc_lbl);
        }
    }

    card.append(&info_box);
    card
}

pub fn create_quick_search_tip_banner(
    shortcut_str: &str,
    on_dismiss: impl Fn() + 'static,
) -> gtk::Box {
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    banner.add_css_class("tip-banner");

    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_icon_size(gtk::IconSize::Normal);
    icon.set_valign(gtk::Align::Center);

    let text = gtk::Label::new(Some(&format!(
        "Klavyeden '{}' kısayoluna basarak bölüm listesinde hızlıca arama yapabilirsiniz.",
        shortcut_str
    )));
    text.add_css_class("tip-banner-text");
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);

    let dismiss_btn = gtk::Button::with_label("Anladım");
    dismiss_btn.add_css_class("flat");
    dismiss_btn.add_css_class("pill");
    dismiss_btn.set_valign(gtk::Align::Center);
    let banner_clone = banner.clone();
    dismiss_btn.connect_clicked(move |_| {
        banner_clone.set_visible(false);
        on_dismiss();
    });

    banner.append(&icon);
    banner.append(&text);
    banner.append(&dismiss_btn);
    banner
}

pub fn create_right_click_tip_banner(on_dismiss: impl Fn() + 'static) -> gtk::Box {
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    banner.add_css_class("tip-banner");

    let icon = gtk::Image::from_icon_name("input-mouse-symbolic");
    icon.set_icon_size(gtk::IconSize::Normal);
    icon.set_valign(gtk::Align::Center);

    let text = gtk::Label::new(Some(
        "Bir bölüme sağ tıklayarak o bölümü izlendi / izlenmedi olarak manuel işaretleyebilirsiniz.",
    ));
    text.add_css_class("tip-banner-text");
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);

    let dismiss_btn = gtk::Button::with_label("Anladım");
    dismiss_btn.add_css_class("flat");
    dismiss_btn.add_css_class("pill");
    dismiss_btn.set_valign(gtk::Align::Center);
    let banner_clone = banner.clone();
    dismiss_btn.connect_clicked(move |_| {
        banner_clone.set_visible(false);
        on_dismiss();
    });

    banner.append(&icon);
    banner.append(&text);
    banner.append(&dismiss_btn);
    banner
}

pub fn create_movie_detail_view(
    title: &Title,
    poster_widget: &gtk::Picture,
    bookmark_btn: &gtk::Button,
    marathon_btn: &gtk::Button,
    progress: Option<(f64, f64)>,
    on_play: impl Fn() + 'static,
) -> (gtk::Box, gtk::ProgressBar, gtk::Label) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
    root.add_css_class("movie-big");
    root.set_margin_top(44);
    root.set_margin_bottom(40);
    root.set_margin_start(28);
    root.set_margin_end(28);
    // Sabit genişlikte ortalı sütun: içerik her filmde aynı hizada dursun.
    root.set_size_request(760, -1);
    root.set_halign(gtk::Align::Center);
    root.set_vexpand(true);
    root.set_valign(gtk::Align::Center);

    poster_widget.set_halign(gtk::Align::Center);
    root.append(poster_widget);

    let name_lbl = gtk::Label::new(Some(&title.display_name()));
    name_lbl.add_css_class("title-1");
    name_lbl.set_xalign(0.5);
    name_lbl.set_halign(gtk::Align::Center);
    name_lbl.set_justify(gtk::Justification::Center);
    name_lbl.set_wrap(true);
    name_lbl.set_margin_start(24);
    name_lbl.set_margin_end(24);
    root.append(&name_lbl);

    if let Some(genre) = title.genre_line() {
        let genre_lbl = gtk::Label::new(Some(&genre));
        genre_lbl.add_css_class("dim-label");
        genre_lbl.add_css_class("title-4");
        genre_lbl.set_xalign(0.5);
        genre_lbl.set_halign(gtk::Align::Center);
        genre_lbl.set_justify(gtk::Justification::Center);
        genre_lbl.set_wrap(false);
        genre_lbl.set_single_line_mode(true);
        genre_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        genre_lbl.set_tooltip_text(Some(&genre));
        genre_lbl.set_margin_start(24);
        genre_lbl.set_margin_end(24);
        root.append(&genre_lbl);
    }

    let facts = title.detail_facts();
    if !facts.is_empty() {
        let badges = create_fact_badges(&facts);
        badges.set_halign(gtk::Align::Center);
        root.append(&badges);
    }

    if let Some(desc) = &title.description {
        let clean_desc = desc.trim();
        if !clean_desc.is_empty() {
            let desc_lbl = gtk::Label::new(Some(clean_desc));
            desc_lbl.add_css_class("dim-label");
            desc_lbl.set_xalign(0.5);
            desc_lbl.set_halign(gtk::Align::Center);
            desc_lbl.set_justify(gtk::Justification::Center);
            desc_lbl.set_wrap(true);
            desc_lbl.set_max_width_chars(80);
            // Metin kutu kenarlarına değmesin.
            desc_lbl.set_margin_start(44);
            desc_lbl.set_margin_end(44);
            // Tam metin: satır sınırı ve "..." yok.
            root.append(&desc_lbl);
        }
    }

    let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    btn_row.set_halign(gtk::Align::Center);
    btn_row.set_valign(gtk::Align::Center);
    btn_row.append(bookmark_btn);
    let play_btn = gtk::Button::with_label("Filmi İzle 🎬");
    play_btn.add_css_class("suggested-action");
    play_btn.add_css_class("pill");
    play_btn.add_css_class("movie-play-btn");
    play_btn.connect_clicked(move |_| {
        on_play();
    });
    btn_row.append(&play_btn);
    btn_row.append(marathon_btn);
    root.append(&btn_row);

    let fmt_t = |s: f64| -> String {
        let s = s as u64;
        let h = s / 3600;
        let m = (s % 3600) / 60;
        let sec = s % 60;
        if h > 0 {
            format!("{h}:{:02}:{:02}", m, sec)
        } else {
            format!("{m}:{:02}", sec)
        }
    };

    let prog_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    prog_box.set_halign(gtk::Align::Center);
    prog_box.set_size_request(520, -1);
    let pb = gtk::ProgressBar::new();
    pb.add_css_class("episode-progress");
    pb.set_hexpand(true);
    let lbl = gtk::Label::new(None);
    lbl.add_css_class("dim-label");
    lbl.set_xalign(0.5);
    lbl.set_halign(gtk::Align::Center);

    if let Some((pos, dur)) = progress {
        if dur > 0.0 {
            pb.set_fraction((pos / dur).clamp(0.0, 1.0));
            lbl.set_text(&format!("İzlendi: {} / {}", fmt_t(pos), fmt_t(dur)));
            lbl.set_visible(true);
        } else {
            pb.set_visible(false);
            lbl.set_visible(false);
        }
    } else {
        pb.set_visible(false);
        lbl.set_visible(false);
    }

    prog_box.append(&pb);
    prog_box.append(&lbl);
    root.append(&prog_box);
    (root, pb, lbl)
}
