//! İstatistikler sayfası: KPI kartları + Cairo grafikleri.
//!
//! Tasarım dili: koyu OLED zemin, çift katmanlı kartlar, hairline
//! çerçeveler, geniş boşluklar ve tema accent'i. Grafikler `DrawingArea`
//! üzerinde çizilir; veri agregasyonu `crate::stats` içindedir.

use gtk::cairo;
use gtk::prelude::*;
use std::f64::consts::PI;
use std::sync::OnceLock;

use crate::download::fmt_bytes;
use crate::stats::{format_duration_human, StatsSummary};

pub struct StatsView;

impl StatsView {
    pub fn build(stats: &StatsSummary, accent: (u8, u8, u8)) -> gtk::Box {
        ensure_css();
        let accent = rgb(accent);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 28);
        root.add_css_class("stats-page");
        root.set_margin_top(36);
        root.set_margin_bottom(48);
        root.set_margin_start(40);
        root.set_margin_end(40);

        root.append(&hero());
        root.append(&kpi_row(stats));

        if !stats.has_data() {
            root.append(&empty_card());
        } else {
            let grid = gtk::Grid::new();
            grid.set_column_spacing(20);
            grid.set_row_spacing(20);
            grid.set_column_homogeneous(false);
            grid.set_hexpand(true);

            let top = shell_card();
            top.set_hexpand(true);
            inner_card(&top).append(&top_anime_card(stats, accent));

            let donut = shell_card();
            donut.set_size_request(320, -1);
            inner_card(&donut).append(&donut_card(stats, accent));

            let activity = shell_card();
            activity.set_hexpand(true);
            inner_card(&activity).append(&activity_card(stats, accent));

            grid.attach(&top, 0, 0, 1, 1);
            grid.attach(&donut, 1, 0, 1, 1);
            grid.attach(&activity, 0, 1, 2, 1);
            root.append(&reveal_section(grid.upcast()));
        }

        let footer = gtk::Label::new(Some(
            "Veriler yalnızca bu cihazda tutulur. Süre, kayıtlı ilerleme yoksa tahminidir.",
        ));
        footer.add_css_class("stats-footer");
        footer.set_xalign(0.0);
        root.append(&footer);
        root
    }
}

fn hero() -> gtk::Box {
    let hero = gtk::Box::new(gtk::Orientation::Vertical, 8);
    hero.set_margin_bottom(4);

    let eyebrow_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let eyebrow = gtk::Label::new(Some("KÜTÜPHANE"));
    eyebrow.add_css_class("stats-eyebrow");
    eyebrow_row.append(&eyebrow);
    let badge = gtk::Label::new(Some("Yerel veri"));
    badge.add_css_class("stats-badge");
    eyebrow_row.append(&badge);
    hero.append(&eyebrow_row);

    let title = gtk::Label::new(Some("İstatistikler"));
    title.add_css_class("stats-title");
    title.set_xalign(0.0);
    hero.append(&title);

    let sub = gtk::Label::new(Some("İzleme alışkanlıkların, tek bakışta."));
    sub.add_css_class("stats-subtitle");
    sub.set_xalign(0.0);
    hero.append(&sub);
    hero
}

fn kpi_row(stats: &StatsSummary) -> gtk::FlowBox {
    let flow = gtk::FlowBox::new();
    flow.add_css_class("stats-flow");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(4);
    flow.set_min_children_per_line(1);
    flow.set_homogeneous(true);
    flow.set_column_spacing(16);
    flow.set_row_spacing(16);
    flow.set_valign(gtk::Align::Start);

    let duration = format_duration_human(stats.watch_seconds);
    let size = fmt_bytes(stats.downloaded_bytes);
    let anime_sub = if stats.film_count > 0 {
        format!("{} film ayrı", stats.film_count)
    } else {
        "en az bir bölüm".to_string()
    };
    let cards = [
        (
            "ANİME".to_string(),
            stats.anime_count.to_string(),
            anime_sub,
        ),
        (
            "BÖLÜM".to_string(),
            stats.episode_count.to_string(),
            "tamamlanan bölüm".to_string(),
        ),
        (
            "SÜRE".to_string(),
            duration,
            if stats.watch_estimated {
                "≈ tahmini".to_string()
            } else {
                "kayıtlı ilerleme".to_string()
            },
        ),
        (
            "İNDİRİLEN".to_string(),
            stats.downloaded_count.to_string(),
            size,
        ),
    ];

    for (label, value, sub) in cards {
        let shell = shell_card();
        let card = inner_card(&shell);
        let accent_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        accent_bar.add_css_class("stats-accent-bar");
        accent_bar.set_size_request(-1, 3);
        accent_bar.set_hexpand(false);
        card.append(&accent_bar);

        let l = gtk::Label::new(Some(&label));
        l.add_css_class("stats-kpi-label");
        l.set_xalign(0.0);
        card.append(&l);
        let v = gtk::Label::new(Some(&value));
        v.add_css_class("stats-kpi-value");
        v.set_xalign(0.0);
        card.append(&v);
        let s = gtk::Label::new(Some(&sub));
        s.add_css_class("stats-kpi-sub");
        s.set_xalign(0.0);
        s.set_ellipsize(gtk::pango::EllipsizeMode::End);
        card.append(&s);
        flow.append(&shell);
    }
    flow
}

fn top_anime_card(stats: &StatsSummary, accent: (f64, f64, f64)) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 14);
    card.append(&section_header(
        "ODAK",
        "En çok izlenen",
        "bölüm sayısına göre",
    ));

    if stats.top_anime.is_empty() {
        card.append(&muted("Henüz izlenmiş bölüm yok."));
        return card;
    }
    let max = stats
        .top_anime
        .first()
        .map(|a| a.episodes)
        .unwrap_or(1)
        .max(1);
    for (idx, anime) in stats.top_anime.iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.set_valign(gtk::Align::Center);
        let rank = gtk::Label::new(Some(&format!("{:02}", idx + 1)));
        rank.add_css_class("stats-rank");
        rank.set_width_chars(2);
        row.append(&rank);

        let name = gtk::Label::new(Some(&anime.name));
        name.add_css_class("stats-anime-name");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&name);

        let bar = fraction_bar(anime.episodes as f64 / max as f64, accent);
        bar.set_size_request(120, -1);
        row.append(&bar);

        let count = gtk::Label::new(Some(&format!("{} bölüm", anime.episodes)));
        count.add_css_class("stats-count");
        count.set_xalign(1.0);
        count.set_width_chars(9);
        row.append(&count);
        card.append(&row);
    }
    card
}

fn donut_card(stats: &StatsSummary, accent: (f64, f64, f64)) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 14);
    card.append(&section_header("DAĞILIM", "Sezonlar", "izlenen bölümler"));

    let overlay = gtk::Overlay::new();
    let area = gtk::DrawingArea::new();
    area.set_content_width(180);
    area.set_content_height(180);
    area.set_halign(gtk::Align::Center);
    let seasons = stats.seasons.clone();
    let palette = donut_palette(accent);
    let palette_c = palette.clone();
    area.set_draw_func(move |_, cr, w, h| {
        draw_donut(cr, w, h, &seasons, &palette_c);
    });
    overlay.set_child(Some(&area));

    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_halign(gtk::Align::Center);
    center.set_valign(gtk::Align::Center);
    let total = gtk::Label::new(Some(&stats.episode_count.to_string()));
    total.add_css_class("stats-donut-value");
    total.set_xalign(0.5);
    center.append(&total);
    let sub = gtk::Label::new(Some("bölüm"));
    sub.add_css_class("stats-kpi-sub");
    sub.set_xalign(0.5);
    center.append(&sub);
    overlay.add_overlay(&center);
    card.append(&overlay);

    let legend = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for (idx, s) in stats.seasons.iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let dot = color_dot(palette[idx % palette.len()]);
        row.append(&dot);
        let label = gtk::Label::new(Some(&s.label));
        label.add_css_class("stats-legend");
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        let count = gtk::Label::new(Some(&s.episodes.to_string()));
        count.add_css_class("stats-legend-count");
        count.set_xalign(1.0);
        row.append(&count);
        legend.append(&row);
    }
    if stats.seasons.is_empty() {
        legend.append(&muted("Henüz sezon verisi yok."));
    }
    card.append(&legend);
    card
}

fn activity_card(stats: &StatsSummary, accent: (f64, f64, f64)) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 14);
    card.append(&section_header(
        "RİTİM",
        "Son 8 hafta",
        "izlenen bölüm etkinliği",
    ));
    if stats.activity.iter().all(|b| b.count == 0) {
        card.append(&muted("Bu aralıkta izleme kaydı yok."));
        return card;
    }

    let area = gtk::DrawingArea::new();
    area.set_content_height(150);
    area.set_hexpand(true);
    let buckets = stats.activity.clone();
    area.set_draw_func(move |_, cr, w, h| {
        draw_activity(cr, w, h, &buckets, accent);
    });
    card.append(&area);

    let labels = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    labels.set_margin_top(6);
    for b in &stats.activity {
        let l = gtk::Label::new(Some(&b.label));
        l.add_css_class("stats-axis-label");
        l.set_xalign(0.5);
        l.set_hexpand(true);
        labels.append(&l);
    }
    card.append(&labels);
    card
}

fn empty_card() -> gtk::Box {
    let shell = shell_card();
    let card = inner_card(&shell);
    let title = gtk::Label::new(Some("Henüz istatistik yok"));
    title.add_css_class("stats-section-title");
    title.set_xalign(0.0);
    card.append(&title);
    card.append(&muted(
        "İlk bölümü izlediğinde grafikler dolmaya başlar. İndirdiğin bölümler de burada sayılır.",
    ));
    shell
}

fn shell_card() -> gtk::Box {
    let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shell.add_css_class("stats-shell");
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("stats-card");
    shell.append(&card);
    shell
}

fn inner_card(shell: &gtk::Box) -> gtk::Box {
    shell
        .first_child()
        .and_downcast::<gtk::Box>()
        .expect("stats card")
}

fn section_header(eyebrow: &str, title: &str, subtitle: &str) -> gtk::Box {
    let head = gtk::Box::new(gtk::Orientation::Vertical, 3);
    let e = gtk::Label::new(Some(eyebrow));
    e.add_css_class("stats-eyebrow");
    e.set_xalign(0.0);
    head.append(&e);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("stats-section-title");
    t.set_xalign(0.0);
    head.append(&t);
    let s = gtk::Label::new(Some(subtitle));
    s.add_css_class("stats-section-sub");
    s.set_xalign(0.0);
    head.append(&s);
    head
}

fn muted(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("stats-muted");
    l.set_xalign(0.0);
    l.set_wrap(true);
    l
}

fn reveal_section(widget: gtk::Widget) -> gtk::Revealer {
    let reveal = gtk::Revealer::new();
    reveal.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    reveal.set_transition_duration(360);
    reveal.set_child(Some(&widget));
    reveal.set_reveal_child(false);
    let r = reveal.clone();
    glib::idle_add_local_once(move || r.set_reveal_child(true));
    reveal
}

fn fraction_bar(fraction: f64, accent: (f64, f64, f64)) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(8);
    area.set_valign(gtk::Align::Center);
    area.set_hexpand(true);
    area.set_draw_func(move |_, cr, w, h| {
        let w = w as f64;
        let h = h as f64;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        rounded_rect(cr, 0.0, 0.0, w, h, h / 2.0);
        let _ = cr.fill();
        cr.set_source_rgb(accent.0, accent.1, accent.2);
        rounded_rect(
            cr,
            0.0,
            0.0,
            (w * fraction.clamp(0.0, 1.0)).max(2.0),
            h,
            h / 2.0,
        );
        let _ = cr.fill();
    });
    area
}

fn color_dot(color: (f64, f64, f64)) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(10);
    area.set_content_height(10);
    area.set_valign(gtk::Align::Center);
    area.set_draw_func(move |_, cr, w, h| {
        cr.set_source_rgb(color.0, color.1, color.2);
        cr.arc(w as f64 / 2.0, h as f64 / 2.0, 3.5, 0.0, 2.0 * PI);
        let _ = cr.fill();
    });
    area
}

fn draw_donut(
    cr: &cairo::Context,
    w: i32,
    h: i32,
    seasons: &[crate::stats::SeasonSlice],
    palette: &[(f64, f64, f64)],
) {
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;
    let radius = (w.min(h) as f64 / 2.0) - 16.0;
    cr.set_line_width(18.0);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.07);
    cr.arc(cx, cy, radius, 0.0, 2.0 * PI);
    let _ = cr.stroke();

    let total: usize = seasons.iter().map(|s| s.episodes).sum();
    if total == 0 {
        return;
    }
    let mut start = -PI / 2.0;
    for (idx, s) in seasons.iter().enumerate() {
        let angle = 2.0 * PI * s.episodes as f64 / total as f64;
        if angle <= 0.0 {
            continue;
        }
        let (r, g, b) = palette[idx % palette.len()];
        cr.set_source_rgb(r, g, b);
        cr.arc(cx, cy, radius, start, start + (angle - 0.035).max(0.02));
        let _ = cr.stroke();
        start += angle;
    }
}

fn draw_activity(
    cr: &cairo::Context,
    w: i32,
    h: i32,
    buckets: &[crate::stats::ActivityBucket],
    accent: (f64, f64, f64),
) {
    let count = buckets.len().max(1);
    let slot = w as f64 / count as f64;
    let bar_w = (slot * 0.48).clamp(12.0, 34.0);
    // Tek bir etkinlik bile tüm grafiği kaplamasın: taban ölçek 3.
    let max = buckets.iter().map(|b| b.count).max().unwrap_or(0).max(3) as f64;
    let top = 18.0;
    let baseline = h as f64 - 12.0;
    let chart_h = (baseline - top).max(12.0);

    for frac in [1.0 / 3.0, 2.0 / 3.0, 1.0] {
        let y = baseline - chart_h * frac;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
        cr.set_line_width(1.0);
        cr.move_to(0.0, y);
        cr.line_to(w as f64, y);
        let _ = cr.stroke();
    }
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.1);
    cr.move_to(0.0, baseline);
    cr.line_to(w as f64, baseline);
    let _ = cr.stroke();

    for (idx, b) in buckets.iter().enumerate() {
        let x = idx as f64 * slot + (slot - bar_w) / 2.0;
        let bh = (b.count as f64 / max) * chart_h;
        if b.count == 0 {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
            rounded_rect(cr, x, baseline - 2.0, bar_w, 2.0, 1.0);
        } else {
            cr.set_source_rgb(accent.0, accent.1, accent.2);
            rounded_rect(cr, x, baseline - bh, bar_w, bh.max(4.0), bar_w / 2.0);
        }
        let _ = cr.fill();

        if b.count > 0 {
            let text = b.count.to_string();
            cr.set_font_size(11.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.68);
            if let Ok(ext) = cr.text_extents(&text) {
                cr.move_to(
                    x + bar_w / 2.0 - ext.width() / 2.0 - ext.x_bearing(),
                    baseline - bh - 7.0,
                );
                let _ = cr.show_text(&text);
            }
        }
    }
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

fn donut_palette(accent: (f64, f64, f64)) -> Vec<(f64, f64, f64)> {
    vec![
        accent,
        (0.30, 0.78, 0.65),
        (0.96, 0.65, 0.35),
        (0.92, 0.45, 0.62),
        (0.66, 0.55, 0.96),
        (0.48, 0.70, 0.96),
    ]
}

fn rgb(color: (u8, u8, u8)) -> (f64, f64, f64) {
    (
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
    )
}

fn ensure_css() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        let css = gtk::CssProvider::new();
        css.load_from_data(
            r#"
            .stats-page { background: transparent; }
            .stats-eyebrow {
                font-size: 0.72em;
                font-weight: 800;
                color: alpha(@accent_color, 0.9);
            }
            .stats-badge {
                font-size: 0.68em;
                padding: 2px 9px;
                border-radius: 99px;
                background-color: alpha(@accent_color, 0.12);
                border: 1px solid alpha(@accent_color, 0.28);
                color: alpha(currentColor, 0.8);
            }
            .stats-title {
                font-size: 2.35em;
                font-weight: 800;
            }
            .stats-subtitle { font-size: 1.0em; color: alpha(currentColor, 0.62); }
            .stats-footer { font-size: 0.8em; color: alpha(currentColor, 0.4); margin-top: 6px; }
            .stats-flow > flowboxchild { padding: 0; }
            .stats-shell {
                background-color: alpha(currentColor, 0.035);
                border: 1px solid alpha(currentColor, 0.08);
                border-radius: 28px;
                padding: 6px;
                transition: border-color 240ms cubic-bezier(0.32, 0.72, 0, 1),
                            background-color 240ms cubic-bezier(0.32, 0.72, 0, 1);
            }
            .stats-shell:hover {
                border-color: alpha(@accent_color, 0.24);
                background-color: alpha(currentColor, 0.05);
            }
            .stats-card {
                background-color: alpha(@card_bg_color, 0.72);
                border-radius: 22px;
                padding: 22px;
                box-shadow: inset 0 1px 1px alpha(#ffffff, 0.06),
                            0 18px 40px alpha(#000000, 0.18);
            }
            .stats-accent-bar {
                background-color: @accent_color;
                border-radius: 99px;
            }
            .stats-kpi-label {
                font-size: 0.72em;
                font-weight: 800;
                color: alpha(currentColor, 0.55);
            }
            .stats-kpi-value {
                font-size: 2.15em;
                font-weight: 800;
                margin-top: 2px;
            }
            .stats-kpi-sub {
                font-size: 0.78em;
                color: alpha(currentColor, 0.48);
            }
            .stats-section-title { font-size: 1.15em; font-weight: 800; }
            .stats-section-sub { font-size: 0.8em; color: alpha(currentColor, 0.48); }
            .stats-anime-name { font-weight: 600; }
            .stats-rank {
                font-size: 0.78em;
                font-weight: 800;
                color: alpha(@accent_color, 0.72);
            }
            .stats-count, .stats-legend-count {
                font-size: 0.78em;
                color: alpha(currentColor, 0.55);
            }
            .stats-legend { font-size: 0.86em; }
            .stats-donut-value { font-size: 1.8em; font-weight: 800; }
            .stats-axis-label { font-size: 0.7em; color: alpha(currentColor, 0.42); }
            .stats-muted { color: alpha(currentColor, 0.52); font-size: 0.9em; }
            "#,
        );
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}
