use std::path::{Path, PathBuf};

use adw::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangelogVideo {
    pub file: &'static str,
    pub title: &'static str,
    pub caption: &'static str,
}

pub const PREVIEW_VERSION: &str = "1.4.0";

const VIDEOS: &[ChangelogVideo] = &[
    ChangelogVideo {
        file: "ekrangoruntusu.mp4",
        title: "Sağ tık menüsü",
        caption: "Ekran görüntüsü, video bilgisi ve indirme",
    },
    ChangelogVideo {
        file: "fansubsecimi.mp4",
        title: "Fansub ve kalite seçimi",
        caption: "Tek menüden çeviri ve kaynak seçimi",
    },
    ChangelogVideo {
        file: "hizsecimi.mp4",
        title: "Oynatma hızı",
        caption: "0.5x–2.0x arası hız seçimi",
    },
];

pub fn videos_for(version: &str) -> &'static [ChangelogVideo] {
    if version == PREVIEW_VERSION {
        VIDEOS
    } else {
        &[]
    }
}

pub fn video_path(file: &str) -> Option<PathBuf> {
    let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
    let exe = std::env::current_exe().ok();
    video_candidates(appdir.as_deref(), exe.as_deref(), file)
        .into_iter()
        .find(|p| p.is_file())
}

fn video_candidates(appdir: Option<&Path>, exe: Option<&Path>, file: &str) -> Vec<PathBuf> {
    let name = Path::new(file);
    let mut out = Vec::new();
    if let Some(dir) = appdir {
        out.push(dir.join("usr/share/animecix/assets/changelog").join(name));
    }
    out.push(PathBuf::from("/app/share/animecix/assets/changelog").join(name));
    out.push(PathBuf::from("/usr/share/animecix/assets/changelog").join(name));
    if let Some(exe) = exe {
        if let Some(parent) = exe.parent() {
            out.push(parent.join("assets/changelog").join(name));
            out.push(parent.join("../../assets/changelog").join(name));
        }
    }
    out.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/changelog")
            .join(name),
    );
    out
}

/// Sürüm notlarını ve varsa tanıtım videolarını gösterir.
pub fn present(
    parent: &adw::ApplicationWindow,
    heading: &str,
    items: &[&str],
    videos: &[ChangelogVideo],
) {
    let dlg = adw::Window::new();
    dlg.set_title(Some(heading));
    dlg.set_modal(true);
    dlg.set_transient_for(Some(parent));
    let (parent_w, parent_h) = if parent.width() > 0 && parent.height() > 0 {
        (parent.width(), parent.height())
    } else {
        (1280, 800)
    };
    let (width, height, video_side) = popup_metrics(parent_w, parent_h);
    dlg.set_default_size(width, if videos.is_empty() { 560 } else { height });

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let titlebar = adw::HeaderBar::new();
    let title = adw::WindowTitle::new(heading, "");
    titlebar.set_title_widget(Some(&title));
    root.append(&titlebar);

    // Carousel + madde listesi tek kaydırma alanında: fare tekerleği
    // popup'ın her yerinde (video üzerindeyken de) içeriği kaydırır.
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_hexpand(true);

    let mut close_video: Option<gtk::Video> = None;
    if !videos.is_empty() {
        let (carousel_box, video) = carousel(videos, video_side);
        close_video = Some(video);
        content.append(&carousel_box);
    }

    let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
    list.set_margin_top(16);
    list.set_margin_bottom(18);
    list.set_margin_start(18);
    list.set_margin_end(18);
    list.set_hexpand(true);
    for it in items {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let dot = gtk::Label::new(Some("•"));
        dot.set_valign(gtk::Align::Start);
        dot.add_css_class("dim-label");
        row.append(&dot);
        let lbl = gtk::Label::new(Some(it));
        lbl.set_xalign(0.0);
        lbl.set_wrap(true);
        lbl.set_hexpand(true);
        row.append(&lbl);
        list.append(&row);
    }
    content.append(&list);
    scroll.set_child(Some(&content));
    root.append(&scroll);
    dlg.set_content(Some(&root));

    if let Some(video) = close_video {
        dlg.connect_close_request(move |_| {
            if let Some(stream) = video.media_stream() {
                stream.set_playing(false);
            }
            gtk::glib::Propagation::Proceed
        });
    }

    dlg.present();
}

fn popup_metrics(parent_w: i32, parent_h: i32) -> (i32, i32, i32) {
    let width = ((parent_w as f64 * 0.92) as i32).clamp(560, 960);
    let height = ((parent_h as f64 * 0.94) as i32).clamp(600, 900);
    let video = (width - 80).min(height - 240).clamp(280, 620);
    (width, height, video)
}

fn next_video_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (current + 1) % len
    }
}

fn carousel(videos: &[ChangelogVideo], video_side: i32) -> (gtk::Box, gtk::Video) {
    let videos = videos.to_vec();
    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    section.set_margin_top(14);
    section.set_margin_start(18);
    section.set_margin_end(18);

    let wrap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    wrap.add_css_class("changelog-video-wrap");
    wrap.set_overflow(gtk::Overflow::Hidden);
    wrap.set_size_request(video_side, video_side);
    wrap.set_halign(gtk::Align::Center);

    let video = gtk::Video::new();
    video.set_autoplay(true);
    video.set_loop(false);
    video.set_hexpand(true);
    video.set_vexpand(true);
    wrap.append(&video);

    let fallback = gtk::Label::new(Some("Video yüklenemedi"));
    fallback.add_css_class("dim-label");
    fallback.set_halign(gtk::Align::Center);
    fallback.set_valign(gtk::Align::Center);
    fallback.set_visible(false);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&wrap));
    overlay.add_overlay(&fallback);
    overlay.set_halign(gtk::Align::Center);
    section.append(&overlay);

    let title = gtk::Label::new(None);
    title.add_css_class("changelog-video-title");
    title.set_xalign(0.0);
    title.set_wrap(true);
    section.append(&title);

    let caption = gtk::Label::new(None);
    caption.add_css_class("changelog-video-caption");
    caption.set_xalign(0.0);
    caption.set_wrap(true);
    section.append(&caption);

    let dots = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    dots.set_halign(gtk::Align::Center);
    dots.set_margin_top(4);
    let mut buttons = Vec::with_capacity(videos.len());
    for (i, v) in videos.iter().enumerate() {
        let b = gtk::Button::new();
        b.add_css_class("changelog-dot");
        b.set_tooltip_text(Some(&format!("Video {}: {}", i + 1, v.title)));
        dots.append(&b);
        buttons.push(b);
    }
    section.append(&dots);

    let video_c = video.clone();
    let fallback_c = fallback.clone();
    let title_c = title.clone();
    let caption_c = caption.clone();
    let buttons_c = buttons.clone();
    let videos_c = videos.clone();
    let switch_slot: std::rc::Rc<std::cell::RefCell<Option<std::rc::Weak<dyn Fn(usize)>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let switch_slot_c = switch_slot.clone();
    let switch: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(move |idx| {
        let Some(item) = videos_c.get(idx) else {
            return;
        };
        title_c.set_text(item.title);
        caption_c.set_text(item.caption);
        for (i, b) in buttons_c.iter().enumerate() {
            if i == idx {
                b.add_css_class("active");
            } else {
                b.remove_css_class("active");
            }
        }
        if let Some(stream) = video_c.media_stream() {
            stream.set_playing(false);
        }
        match video_path(item.file) {
            Some(path) => {
                fallback_c.set_visible(false);
                video_c.set_visible(true);
                video_c.set_filename(Some(&path));
                if let Some(stream) = video_c.media_stream() {
                    stream.set_muted(true);
                    let video_e = video_c.clone();
                    let fallback_e = fallback_c.clone();
                    stream.connect_error_notify(move |s| {
                        if s.error().is_some() {
                            video_e.set_visible(false);
                            fallback_e.set_visible(true);
                        }
                    });
                    let slot_e = switch_slot_c.clone();
                    let next = next_video_index(idx, videos_c.len());
                    stream.connect_ended_notify(move |s| {
                        if s.is_ended() {
                            if let Some(f) = slot_e.borrow().as_ref().and_then(|w| w.upgrade()) {
                                f(next);
                            }
                        }
                    });
                }
            }
            None => {
                video_c.set_visible(false);
                fallback_c.set_visible(true);
            }
        }
    });
    for (i, b) in buttons.iter().enumerate() {
        let switch = switch.clone();
        b.connect_clicked(move |_| switch(i));
    }
    *switch_slot.borrow_mut() = Some(std::rc::Rc::downgrade(&switch));
    switch(0);

    (section, video)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_has_three_videos_in_order() {
        let v = videos_for(PREVIEW_VERSION);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].file, "ekrangoruntusu.mp4");
        assert_eq!(v[1].file, "fansubsecimi.mp4");
        assert_eq!(v[2].file, "hizsecimi.mp4");
    }

    #[test]
    fn bundled_videos_exist_and_are_not_empty() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/changelog");
        for v in videos_for(PREVIEW_VERSION) {
            let p = dir.join(v.file);
            let len = std::fs::metadata(&p)
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
                .len();
            assert!(len > 100_000, "{} çok küçük: {len}", p.display());
        }
    }

    #[test]
    fn candidates_prefer_appdir_then_flatpak_then_fhs() {
        let c = video_candidates(Some(std::path::Path::new("/tmp/AppDir")), None, "x.mp4");
        assert!(c[0].ends_with("usr/share/animecix/assets/changelog/x.mp4"));
        assert!(c
            .iter()
            .any(|p| p == std::path::Path::new("/app/share/animecix/assets/changelog/x.mp4")));
        assert!(c
            .iter()
            .any(|p| p == std::path::Path::new("/usr/share/animecix/assets/changelog/x.mp4")));
    }

    #[test]
    fn missing_video_returns_none() {
        assert!(video_path("boyle-bir-video-yok-12345.mp4").is_none());
    }

    #[test]
    fn popup_metrics_grow_with_parent_and_cap() {
        assert_eq!(popup_metrics(800, 800), (736, 752, 512));
        assert_eq!(popup_metrics(1920, 1080), (960, 900, 620));
        assert_eq!(popup_metrics(640, 480), (588, 600, 360));
    }

    #[test]
    fn next_video_wraps() {
        assert_eq!(next_video_index(0, 3), 1);
        assert_eq!(next_video_index(1, 3), 2);
        assert_eq!(next_video_index(2, 3), 0);
    }
}
