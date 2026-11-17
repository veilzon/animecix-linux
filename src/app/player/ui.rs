use gtk::prelude::*;

/// Lowell oynatıcı düzeninin tema uyumlu CSS'i.
pub fn player_css() -> String {
    r#"
    .player-root { background-color: #000000; }
    .player-shade {
        background: linear-gradient(to bottom,
            rgba(0,0,0,0.50) 0%, rgba(0,0,0,0.08) 18%,
            transparent 30%, transparent 70%,
            rgba(0,0,0,0.08) 82%, rgba(0,0,0,0.60) 100%);
    }
    .player-text { color: #FFFFFF; text-shadow: 0 1px 3px rgba(0,0,0,0.8); }
    .player-controls { background: rgba(10,10,14,0.48); border-radius: 14px; padding: 9px 12px; }
    .player-play-btn {
        -gtk-icon-size: 32px; color: #FFFFFF; min-width: 56px; min-height: 56px;
        padding: 0; border: 1px solid rgba(255,255,255,0.14);
        background: rgba(0,0,0,0.48); background-color: rgba(0,0,0,0.48);
        border-radius: 999px; box-shadow: 0 6px 24px rgba(0,0,0,0.55);
        transition: transform 120ms ease, opacity 120ms ease, background-color 120ms ease;
    }
    .player-play-btn:hover { background: rgba(0,0,0,0.64); background-color: rgba(0,0,0,0.64); transform: scale(1.06); opacity: 1; }
    .player-play-btn:active { background: rgba(0,0,0,0.72); transform: scale(0.95); }
    .player-play-btn image { color: #FFFFFF; -gtk-icon-size: 32px; }
    .player-center-btn {
        color: #FFFFFF; min-width: 46px; min-height: 46px; padding: 0;
        border: 1px solid rgba(255,255,255,0.10);
        background: rgba(0,0,0,0.38); background-color: rgba(0,0,0,0.38);
        border-radius: 999px; box-shadow: 0 4px 16px rgba(0,0,0,0.45);
        transition: transform 120ms ease, background-color 120ms ease;
    }
    .player-center-btn:hover { background: rgba(0,0,0,0.58); background-color: rgba(0,0,0,0.58); transform: scale(1.06); }
    .player-center-btn:active { transform: scale(0.95); }
    .player-center-btn label { color: #FFFFFF; font-weight: 700; text-shadow: 0 1px 4px rgba(0,0,0,0.9); }
    .embed-controls scale { padding: 0; }
    .embed-controls scale trough {
        background: rgba(255,255,255,0.3); min-height: 4px; border-radius: 2px;
        margin: 0; padding: 0;
    }
    .embed-controls scale highlight { background: #FFFFFF; min-height: 4px; border-radius: 2px; }
    .embed-controls scale fill {
        background: rgba(255,255,255,0.42); min-height: 4px; border-radius: 2px;
    }
    .player-spinner { color: rgba(255,255,255,0.92); }
    .embed-controls scale slider {
        background: #FFFFFF; border: none; min-width: 14px; min-height: 14px;
        border-radius: 7px; margin: -5px 0; opacity: 1;
        box-shadow: 0 1px 3px rgba(0,0,0,0.8);
    }
    .vol-scale trough { min-height: 4px; border-radius: 2px; background: rgba(255,255,255,0.3); }
    .vol-scale highlight { background: rgba(255,255,255,0.85); }
    .pill-dropdown, .pill-dropdown button, menubutton.pill-dropdown > button {
        color: #FFFFFF; background: transparent; background-color: transparent;
        background-image: none; border-radius: 8px; border: none;
        box-shadow: none; outline: none; padding: 3px 9px;
    }
    .pill-dropdown:hover, .pill-dropdown button:hover { background: transparent; }
    .pill-dropdown label, menubutton.pill-dropdown label {
        color: #FFFFFF; font-weight: 500; text-shadow: 0 1px 3px rgba(0,0,0,0.8);
    }
    button.player-vol-btn, .player-vol-btn {
        background: transparent; border: none; box-shadow: none; outline: none;
        color: #FFFFFF; padding: 0; border-radius: 16px; min-width: 32px; min-height: 32px;
        transition: background-color 200ms ease, transform 120ms ease;
    }
    button.player-vol-btn:hover, .player-vol-btn:hover { transform: scale(1.12); opacity: 0.85; }
    .player-vol-btn image { color: #FFFFFF; -gtk-icon-size: 20px; }
    .top-nav-btn {
        color: #FFFFFF; min-width: 36px; min-height: 36px; border-radius: 18px;
        background: rgba(0,0,0,0.60); border: none; box-shadow: none; padding: 0;
    }
    .top-nav-btn:hover { background: rgba(0,0,0,0.82); }
    .top-nav-btn image { color: #FFFFFF; -gtk-icon-size: 18px; }
    .embed-time { font-variant-numeric: tabular-nums; color: #FFFFFF; text-shadow: 0 1px 3px rgba(0,0,0,0.8); }
    .embed-bot-title { font-weight: 600; color: #FFFFFF; text-shadow: 0 1px 3px rgba(0,0,0,0.8); }
    .info-capsule {
        background-color: rgba(0,0,0,0.72); color: #FFFFFF; border-radius: 9999px;
        padding: 4px 14px; font-size: 0.82em;
    }
    .player-skip-card {
        background: rgba(14,14,18,0.92); border: 1px solid alpha(@accent_color, 0.55);
        border-radius: 14px; padding: 12px 14px; color: #FFFFFF;
    }
    .player-skip-card .title-4 { color: #FFFFFF; }
    .player-skip-card .dim-label { color: rgba(255,255,255,0.72); }
    .embed-edge { background: rgba(255,255,255,0.38); border-radius: 999px; }
    .ep-panel {
        background: rgba(10,10,14,0.92); border-radius: 14px 0 0 14px;
        padding: 10px; border-left: 1px solid alpha(@accent_color, 0.35);
    }
    .ep-panel-title { font-weight: 800; letter-spacing: 0.08em; color: #FFFFFF; }
    .season-tab { border-radius: 999px; padding: 2px 10px; }
    .ep-current { border: 2px solid alpha(@accent_color, 0.9); border-radius: 10px; }
    .section-label { font-size: 0.72em; font-weight: 800; letter-spacing: 0.09em; color: rgba(255,255,255,0.55); }
    .fan-name { font-size: 1.02em; font-weight: 600; color: #FFFFFF; }
    .fan-rating { color: #E5C07B; font-size: 0.86em; }
    .fan-check { color: @accent_color; font-weight: 800; }
    .source-name { color: rgba(255,255,255,0.86); font-size: 0.93em; }
    .source-quality { color: @accent_color; font-size: 0.84em; }
    "#.to_string()
}

pub fn install_css(widget: &impl IsA<gtk::Widget>) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&player_css());
    gtk::style_context_add_provider_for_display(
        &widget.display(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

pub struct TopBar {
    pub revealer: gtk::Revealer,
    pub back: gtk::Button,
    pub fullscreen: gtk::Button,
}

pub fn build_topbar(stage: &gtk::Overlay) -> TopBar {
    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.add_css_class("top-nav-btn");
    back.set_tooltip_text(Some("Geri"));
    let fullscreen = gtk::Button::from_icon_name("view-fullscreen-symbolic");
    fullscreen.add_css_class("top-nav-btn");
    fullscreen.set_tooltip_text(Some("Tam ekran (F)"));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.set_hexpand(true);
    row.set_valign(gtk::Align::Start);
    row.set_margin_start(16);
    row.set_margin_end(16);
    row.set_margin_top(14);
    row.append(&back);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);
    row.append(&fullscreen);

    let handle = gtk::WindowHandle::builder()
        .child(&row)
        .hexpand(true)
        .build();
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(220)
        .child(&handle)
        .build();
    revealer.set_reveal_child(true);
    revealer.set_valign(gtk::Align::Start);
    revealer.set_hexpand(true);
    stage.add_overlay(&revealer);
    TopBar {
        revealer,
        back,
        fullscreen,
    }
}

#[derive(Clone)]
pub struct SongCard {
    pub revealer: gtk::Revealer,
    pub title: gtk::Label,
    pub song: gtk::Button,
}

pub fn build_song_card(stage: &gtk::Overlay) -> SongCard {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("player-skip-card");
    card.set_halign(gtk::Align::Start);
    card.set_valign(gtk::Align::End);
    card.set_hexpand(false);
    card.set_vexpand(false);
    card.set_margin_start(16);
    card.set_margin_end(16);
    card.set_margin_bottom(150);

    let title = gtk::Label::new(None);
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_lines(2);
    title.set_max_width_chars(30);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&title);

    let song = gtk::Button::with_label("Tarayıcıda aç");
    song.add_css_class("flat");
    song.add_css_class("pill");
    song.set_halign(gtk::Align::Start);
    song.set_visible(false);
    card.append(&song);

    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(180)
        .child(&card)
        .build();
    revealer.set_reveal_child(false);
    revealer.set_halign(gtk::Align::Start);
    revealer.set_valign(gtk::Align::End);
    revealer.set_hexpand(false);
    revealer.set_vexpand(false);
    stage.add_overlay(&revealer);

    SongCard {
        revealer,
        title,
        song,
    }
}

impl SongCard {
    pub fn show(&self, title: &str, song_visible: bool, song_label: &str) {
        self.title.set_text(title);
        self.song.set_visible(song_visible);
        self.song.set_label(song_label);
        self.revealer.set_reveal_child(true);
    }

    pub fn hide(&self) {
        self.revealer.set_reveal_child(false);
    }
}

pub fn build_skip_pill(stage: &gtk::Overlay) -> (gtk::Revealer, gtk::Button) {
    let button = gtk::Button::with_label("İntroyu Atla");
    button.add_css_class("suggested-action");
    button.add_css_class("pill");
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(180)
        .child(&button)
        .build();
    revealer.set_reveal_child(false);
    revealer.set_halign(gtk::Align::End);
    revealer.set_valign(gtk::Align::End);
    revealer.set_hexpand(false);
    revealer.set_vexpand(false);
    revealer.set_margin_end(16);
    revealer.set_margin_bottom(150);
    stage.add_overlay(&revealer);
    (revealer, button)
}

pub struct ControlBar {
    pub controls: gtk::Box,
    pub controls_revealer: gtk::Revealer,
    pub center_rev: gtk::Revealer,
    pub play_btn: gtk::Button,
    pub back10_btn: gtk::Button,
    pub fwd10_btn: gtk::Button,
    pub seek: gtk::Scale,
    pub spinner: gtk::Spinner,
    pub cur_lbl: gtk::Label,
    pub dur_lbl: gtk::Label,
    pub vol_btn: gtk::Button,
    pub vol_scale: gtk::Scale,
    pub speed_btn: gtk::MenuButton,
    pub speed_pop: gtk::Popover,
    pub speed_box: gtk::Box,
    pub quality_btn: gtk::MenuButton,
    pub quality_pop: gtk::Popover,
    pub quality_box: gtk::Box,
    pub status: gtk::Label,
}

fn vol_icon(volume: f64, muted: bool) -> &'static str {
    if muted || volume <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if volume < 35.0 {
        "audio-volume-low-symbolic"
    } else if volume < 70.0 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

pub fn build_control_bar(
    stage: &gtk::Overlay,
    start_volume: f64,
    start_muted: bool,
    media_title: &str,
) -> ControlBar {
    let controls = gtk::Box::new(gtk::Orientation::Vertical, 6);
    controls.add_css_class("embed-controls");
    controls.add_css_class("player-controls");
    controls.set_valign(gtk::Align::End);
    controls.set_margin_start(16);
    controls.set_margin_end(16);
    controls.set_margin_bottom(12);

    let play_btn = gtk::Button::from_icon_name("media-playback-start-symbolic");
    play_btn.set_tooltip_text(Some("Oynat / Duraklat (Boşluk)"));
    play_btn.add_css_class("flat");
    play_btn.add_css_class("player-play-btn");

    let back10_btn = gtk::Button::with_label("−10");
    back10_btn.set_tooltip_text(Some("10 sn geri (←)"));
    back10_btn.add_css_class("flat");
    back10_btn.add_css_class("player-center-btn");

    let fwd10_btn = gtk::Button::with_label("+10");
    fwd10_btn.set_tooltip_text(Some("10 sn ileri (→)"));
    fwd10_btn.add_css_class("flat");
    fwd10_btn.add_css_class("player-center-btn");

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1000.0, 1.0);
    seek.add_css_class("pc-seek");
    seek.set_hexpand(true);
    seek.set_draw_value(false);
    // Oynatılan kısmın ötesinde mpv'nin önbelleğe aldığı bölüm `fill`
    // katmanıyla görünür. Sürükleme yine tüm çubukta serbest kalmalı.
    seek.set_show_fill_level(true);
    seek.set_restrict_to_fill_level(false);
    seek.set_fill_level(0.0);
    seek.set_tooltip_text(Some("İlerleme çubuğu — sürükle"));
    let cur_lbl = gtk::Label::new(Some("--:--"));
    let dur_lbl = gtk::Label::new(Some("--:--"));
    cur_lbl.add_css_class("embed-time");
    dur_lbl.add_css_class("embed-time");

    let vol_btn = gtk::Button::from_icon_name(vol_icon(start_volume, start_muted));
    vol_btn.set_tooltip_text(Some("Sesi aç/kapat (M)"));
    vol_btn.add_css_class("flat");
    vol_btn.add_css_class("circular");
    vol_btn.add_css_class("player-vol-btn");
    let vol_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    vol_scale.add_css_class("vol-scale");
    vol_scale.set_value(start_volume);
    vol_scale.set_draw_value(false);
    vol_scale.set_size_request(70, -1);

    let speed_btn = gtk::MenuButton::builder()
        .label("1.0x")
        .tooltip_text("Oynatma hızı")
        .build();
    speed_btn.add_css_class("pill-dropdown");
    let speed_pop = gtk::Popover::builder()
        .position(gtk::PositionType::Top)
        .build();
    speed_pop.add_css_class("quality-pop");
    speed_btn.set_popover(Some(&speed_pop));
    let speed_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    speed_box.set_margin_top(6);
    speed_box.set_margin_bottom(6);
    speed_pop.set_child(Some(&speed_box));

    let quality_btn = gtk::MenuButton::builder()
        .label("Kaynak")
        .tooltip_text("Kalite / kaynak")
        .build();
    quality_btn.add_css_class("pill-dropdown");
    let quality_pop = gtk::Popover::builder()
        .position(gtk::PositionType::Top)
        .build();
    quality_pop.add_css_class("quality-pop");
    quality_btn.set_popover(Some(&quality_pop));
    let quality_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    quality_box.set_margin_top(6);
    quality_box.set_margin_bottom(6);
    quality_pop.set_child(Some(&quality_box));

    let bot_title = gtk::Label::new(Some(media_title));
    bot_title.add_css_class("embed-bot-title");
    bot_title.add_css_class("title-4");
    bot_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    bot_title.set_xalign(0.0);
    bot_title.set_hexpand(true);

    let status = gtk::Label::new(None);
    status.add_css_class("info-capsule");
    status.set_halign(gtk::Align::Center);
    status.set_valign(gtk::Align::Center);
    status.set_visible(false);

    let top_part = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top_part.append(&bot_title);
    top_part.append(&speed_btn);
    top_part.append(&quality_btn);

    let seek_row = gtk::Overlay::new();
    seek_row.set_child(Some(&seek));
    seek_row.add_overlay(&status);

    // Yükleme dönengeci için sabit yuva: görünüp kaybolurken zaman
    // etiketini sağa sola zıplatmasın.
    let spinner_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spinner_slot.set_size_request(18, 18);
    spinner_slot.set_valign(gtk::Align::Center);
    let spinner = gtk::Spinner::new();
    spinner.add_css_class("player-spinner");
    spinner.set_size_request(16, 16);
    spinner.set_valign(gtk::Align::Center);
    spinner.set_halign(gtk::Align::Center);
    spinner.set_tooltip_text(Some("Video yükleniyor…"));
    spinner.set_visible(false);
    spinner_slot.append(&spinner);

    let bottom_part = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bottom_part.append(&spinner_slot);
    bottom_part.append(&cur_lbl);
    let time_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    time_spacer.set_hexpand(true);
    bottom_part.append(&time_spacer);
    bottom_part.append(&vol_btn);
    bottom_part.append(&vol_scale);
    bottom_part.append(&dur_lbl);

    controls.append(&top_part);
    controls.append(&seek_row);
    controls.append(&bottom_part);

    let center_box = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    center_box.set_halign(gtk::Align::Center);
    center_box.set_valign(gtk::Align::Center);
    center_box.append(&back10_btn);
    center_box.append(&play_btn);
    center_box.append(&fwd10_btn);
    let center_rev = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(220)
        .child(&center_box)
        .build();
    center_rev.set_reveal_child(true);
    center_rev.set_halign(gtk::Align::Center);
    center_rev.set_valign(gtk::Align::Center);
    stage.add_overlay(&center_rev);

    let controls_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .transition_duration(220)
        .child(&controls)
        .build();
    controls_revealer.set_reveal_child(true);
    controls_revealer.set_valign(gtk::Align::End);
    controls_revealer.set_hexpand(true);
    stage.add_overlay(&controls_revealer);

    ControlBar {
        controls,
        controls_revealer,
        center_rev,
        play_btn,
        back10_btn,
        fwd10_btn,
        seek,
        spinner,
        cur_lbl,
        dur_lbl,
        vol_btn,
        vol_scale,
        speed_btn,
        speed_pop,
        speed_box,
        quality_btn,
        quality_pop,
        quality_box,
        status,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn player_css_has_contrast_and_accent() {
        let css = super::player_css();
        assert!(css.contains("@accent_color") || css.contains("@accent_bg_color"));
        assert!(css.contains("rgba(0,0,0") || css.contains("rgba(14,14,18"));
        assert!(css.contains("#FFFFFF"));
    }

    #[test]
    fn player_icons_are_bundled() {
        const ICONS: &[&str] = &[
            "go-previous-symbolic",
            "go-next-symbolic",
            "media-playback-start-symbolic",
            "media-playback-pause-symbolic",
            "media-playback-stop-symbolic",
            "media-skip-forward-symbolic",
            "audio-volume-high-symbolic",
            "audio-volume-low-symbolic",
            "audio-volume-medium-symbolic",
            "audio-volume-muted-symbolic",
            "view-fullscreen-symbolic",
            "view-restore-symbolic",
            "camera-photo-symbolic",
            "dialog-information-symbolic",
            "folder-download-symbolic",
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/hicolor/scalable/actions");
        for icon in ICONS {
            let p = dir.join(format!("{icon}.svg"));
            assert!(p.is_file(), "eksik ikon: {}", p.display());
        }
    }
}
