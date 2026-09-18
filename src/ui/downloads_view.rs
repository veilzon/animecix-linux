//! İndirilenler sayfası: kuyruk listesi + durum + aksiyonlar.

use gtk::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::download::{DownloadManager, DownloadRecord, DownloadStatus};

/// Satır tutamacı: yerinde güncelleme (bar/etiket/buton) + canlı filtre için.
#[derive(Clone)]
pub struct DlRow {
    pub card: gtk::Widget,
    pub title: gtk::Label,
    pub bar: gtk::ProgressBar,
    pub status: gtk::Label,
    pub foot: gtk::Box,
    pub filter: String,
}

pub struct DownloadsView;

impl DownloadsView {
    /// Satır metinleri: (oran, bar yazısı, durum yazısı).
    pub fn row_state(rec: &DownloadRecord) -> (f64, String, String) {
        let frac = |have: u64, total: u64| {
            if total > 0 {
                have as f64 / total as f64
            } else {
                0.0
            }
            .clamp(0.0, 1.0)
        };
        let pct = |have: u64, total: u64| {
            if total > 0 {
                ((have as f64 / total as f64) * 100.0) as u64
            } else {
                0
            }
        };
        match &rec.status {
            DownloadStatus::Done => (
                1.0,
                "Tamamlandı".to_string(),
                format!("Tamamlandı · {}", crate::download::fmt_bytes(rec.total)),
            ),
            DownloadStatus::Error(e) => (0.0, "Hata".to_string(), format!("Hata: {e}")),
            DownloadStatus::Paused => {
                let p = pct(rec.have, rec.total);
                (
                    frac(rec.have, rec.total),
                    "Duraklatıldı".to_string(),
                    format!(
                        "Duraklatıldı — %{p} · {} / {}",
                        crate::download::fmt_bytes(rec.have),
                        crate::download::fmt_bytes(rec.total)
                    ),
                )
            }
            DownloadStatus::Queued if rec.have == 0 => (
                0.0,
                "Bekleniyor".to_string(),
                "Sırada — sırayla indiriliyor".to_string(),
            ),
            DownloadStatus::Queued => {
                let p = pct(rec.have, rec.total);
                (
                    frac(rec.have, rec.total),
                    format!("%{p}"),
                    format!(
                        "Sırada — %{p} · {} / {}",
                        crate::download::fmt_bytes(rec.have),
                        crate::download::fmt_bytes(rec.total)
                    ),
                )
            }
            DownloadStatus::Downloading if rec.total == 0 => {
                (0.0, "%0".to_string(), "Bağlanıyor…".to_string())
            }
            _ => {
                let p = pct(rec.have, rec.total);
                (
                    frac(rec.have, rec.total),
                    format!("%{p}"),
                    format!(
                        "İndiriliyor — %{p} · {} / {}",
                        crate::download::fmt_bytes(rec.have),
                        crate::download::fmt_bytes(rec.total)
                    ),
                )
            }
        }
    }

    /// Satır başlığı: `{Anime} — S01E07 · {BölümAdı} [Fansub][1080p]`.
    /// Bölüm adı boşsa `· …` düşer, fansub boşsa `[]` basılmaz,
    /// kalite boş/`en iyi` ise etiket düşer.
    pub fn row_title(rec: &DownloadRecord) -> String {
        let mut t = format!("{} — S{:02}E{:02}", rec.title, rec.season, rec.episode);
        if !rec.ep_name.trim().is_empty() {
            t.push_str(&format!(" · {}", rec.ep_name.trim()));
        }
        if !rec.fansub.trim().is_empty() {
            t.push_str(&format!(" [{}]", rec.fansub.trim()));
        }
        let q = rec.quality.trim();
        if !q.is_empty() && q != "en iyi" {
            t.push_str(&format!(" [{q}]"));
        }
        t
    }

    /// Filtre havuzu (küçük harf): başlık + bölüm adı + fansub + kalite.
    fn filter_text(rec: &DownloadRecord) -> String {
        format!("{} {} {} {}", rec.title, rec.ep_name, rec.fansub, rec.quality).to_lowercase()
    }

    fn icon_btn(icon: &str, tooltip: &str) -> gtk::Button {
        let b = gtk::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.add_css_class("circular");
        b.set_tooltip_text(Some(tooltip));
        b.set_valign(gtk::Align::Center);
        b
    }

    /// `foot` aksiyon butonlarını duruma göre kurar (`status` korunur).
    fn fill_foot(
        foot: &gtk::Box,
        status: &gtk::Label,
        rec: &DownloadRecord,
        manager: &DownloadManager,
    ) {
        while let Some(c) = foot.first_child() {
            foot.remove(&c);
        }
        foot.append(status);
        match &rec.status {
            DownloadStatus::Downloading | DownloadStatus::Queued => {
                let b = Self::icon_btn("media-playback-pause-symbolic", "Duraklat");
                let m = manager.clone();
                let id = rec.id.clone();
                b.connect_clicked(move |_| m.pause(&id));
                foot.append(&b);
            }
            DownloadStatus::Paused | DownloadStatus::Error(_) => {
                let b = Self::icon_btn("media-playback-start-symbolic", "Devam");
                let m = manager.clone();
                let id = rec.id.clone();
                b.connect_clicked(move |_| m.resume(&id));
                foot.append(&b);
            }
            DownloadStatus::Done => {
                let open = Self::icon_btn("folder-open-symbolic", "Klasörde Göster");
                let dest = rec.dest.clone();
                open.connect_clicked(move |_| {
                    if let Some(parent) = dest.parent() {
                        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                    }
                });
                foot.append(&open);
            }
        }
        // Kaldır her satırda bulunur; bitmiş dosya korunur (kullanıcı kararı).
        let rm = Self::icon_btn("user-trash-symbolic", "Kaldır");
        let m = manager.clone();
        let id = rec.id.clone();
        rm.connect_clicked(move |_| m.remove(&id));
        foot.append(&rm);
    }

    /// Mevcut satırı yerinde tazeler (bar/etiket/başlık/buton; yeniden kurulum yok).
    pub fn refresh_row(row: &DlRow, rec: &DownloadRecord, manager: &DownloadManager) {
        row.title.set_text(&Self::row_title(rec));
        let (frac, txt, stxt) = Self::row_state(rec);
        row.bar.set_fraction(frac);
        row.bar.set_text(Some(&txt));
        row.status.set_text(&stxt);
        Self::fill_foot(&row.foot, &row.status, rec, manager);
    }

    /// Sayfayı kurar; ilerleme Tick'lerinde + yapısal pompa güncellemelerinde
    /// yerinde güncelleme için satır tutamaçlarını döner.
    pub fn build(
        manager: &DownloadManager,
        open_dir: PathBuf,
    ) -> (gtk::ScrolledWindow, HashMap<String, DlRow>) {
        let mut rows: HashMap<String, DlRow> = HashMap::new();
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let items = manager.snapshot();
        if items.is_empty() {
            let sp = crate::ui::components::create_status_page(
                "İndirme Yok",
                "Bölüm sayfasındaki indir düğmesiyle eklediğin bölümler burada görünür.",
                "folder-download-symbolic",
            );
            scroll.set_child(Some(&sp));
            return (scroll, rows);
        }

        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_margin_top(12);
        root.set_margin_bottom(12);
        root.set_margin_start(12);
        root.set_margin_end(12);

        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let count = gtk::Label::new(Some(&format!("{} indirme", items.len())));
        count.add_css_class("title-4");
        count.set_xalign(0.0);
        count.set_valign(gtk::Align::Center);
        head.append(&count);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("İndirilenlerde ara…"));
        search.set_hexpand(true);
        search.set_valign(gtk::Align::Center);
        head.append(&search);
        let open_btn = gtk::Button::with_label("Klasör Aç");
        open_btn.set_icon_name("folder-open-symbolic");
        open_btn.add_css_class("flat");
        open_btn.add_css_class("pill");
        open_btn.set_valign(gtk::Align::Center);
        open_btn.connect_clicked(move |_| {
            let _ = std::process::Command::new("xdg-open").arg(&open_dir).spawn();
        });
        head.append(&open_btn);
        root.append(&head);

        let mut cards: Vec<(gtk::Widget, String)> = Vec::new();
        for rec in items {
            let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
            card.add_css_class("card");
            card.set_margin_top(4);
            card.set_margin_bottom(4);
            card.set_margin_start(8);
            card.set_margin_end(8);

            let title = gtk::Label::new(Some(&Self::row_title(&rec)));
            title.add_css_class("title-4");
            title.set_xalign(0.0);
            title.set_max_width_chars(48);
            title.set_lines(1);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.set_margin_start(8);
            title.set_margin_end(8);
            title.set_margin_top(6);
            card.append(&title);

            let bar = gtk::ProgressBar::new();
            bar.set_show_text(true);
            let (frac, txt, stxt) = Self::row_state(&rec);
            bar.set_fraction(frac);
            bar.set_text(Some(&txt));
            bar.set_margin_start(8);
            bar.set_margin_end(8);
            card.append(&bar);

            let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            foot.set_margin_start(8);
            foot.set_margin_end(8);
            foot.set_margin_bottom(6);
            let status = gtk::Label::new(Some(&stxt));
            status.add_css_class("dim-label");
            status.set_xalign(0.0);
            status.set_hexpand(true);
            status.set_ellipsize(gtk::pango::EllipsizeMode::End);
            Self::fill_foot(&foot, &status, &rec, manager);
            card.append(&foot);

            let filter = Self::filter_text(&rec);
            cards.push((card.clone().upcast(), filter.clone()));
            rows.insert(
                rec.id.clone(),
                DlRow {
                    card: card.clone().upcast(),
                    title: title.clone(),
                    bar: bar.clone(),
                    status: status.clone(),
                    foot: foot.clone(),
                    filter,
                },
            );
            root.append(&card);
        }

        let total = rows.len();
        let cards_rc = std::rc::Rc::new(std::cell::RefCell::new(cards));
        let count_c = count.clone();
        search.connect_search_changed(move |e| {
            let query = e.text().trim().to_lowercase();
            let mut vis = 0;
            for (card, hay) in cards_rc.borrow().iter() {
                let show = query.is_empty() || hay.contains(&query);
                card.set_visible(show);
                if show {
                    vis += 1;
                }
            }
            if query.is_empty() {
                count_c.set_text(&format!("{total} indirme"));
            } else {
                count_c.set_text(&format!("{vis}/{total} indirme"));
            }
        });

        scroll.set_child(Some(&root));
        (scroll, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn rec(
        status: DownloadStatus,
        have: u64,
        total: u64,
        ep_name: &str,
        fansub: &str,
        quality: &str,
    ) -> DownloadRecord {
        DownloadRecord {
            id: "t".into(),
            title: "Frieren".into(),
            season: 1,
            episode: 7,
            ep_name: ep_name.into(),
            fansub: fansub.into(),
            quality: quality.into(),
            url: "https://ornek/v.mp4".into(),
            referer: None,
            dest: PathBuf::from("/tmp/v.mp4"),
            total,
            have,
            status,
        }
    }

    #[test]
    fn title_format_full() {
        let r = rec(DownloadStatus::Queued, 0, 0, "Gün Batımı", "Raion", "1080p");
        assert_eq!(r.ep_name, "Gün Batımı");
        assert_eq!(
            DownloadsView::row_title(&r),
            "Frieren — S01E07 · Gün Batımı [Raion] [1080p]"
        );
    }

    #[test]
    fn title_format_drops_empty_parts() {
        // Bölüm adı boşsa `· …` düşer.
        let r = rec(DownloadStatus::Queued, 0, 0, "", "Raion", "1080p");
        assert_eq!(DownloadsView::row_title(&r), "Frieren — S01E07 [Raion] [1080p]");
        // Fansub boşsa `[]` basılmaz.
        let r = rec(DownloadStatus::Queued, 0, 0, "Gün Batımı", "", "1080p");
        assert_eq!(
            DownloadsView::row_title(&r),
            "Frieren — S01E07 · Gün Batımı [1080p]"
        );
        // Kalite boş/`en iyi` ise etiket düşer.
        let r = rec(DownloadStatus::Queued, 0, 0, "Gün Batımı", "Raion", "en iyi");
        assert_eq!(
            DownloadsView::row_title(&r),
            "Frieren — S01E07 · Gün Batımı [Raion]"
        );
        let r = rec(DownloadStatus::Queued, 0, 0, "", "", "");
        assert_eq!(DownloadsView::row_title(&r), "Frieren — S01E07");
    }

    #[test]
    fn row_state_strings() {
        let (f, bar, st) = DownloadsView::row_state(&rec(DownloadStatus::Done, 100, 100, "", "", ""));
        assert_eq!((bar.as_str(), st.as_str()), ("Tamamlandı", "Tamamlandı · 100 B"));
        assert_eq!(f, 1.0);

        let (_, bar, st) =
            DownloadsView::row_state(&rec(DownloadStatus::Error("ağ koptu".into()), 0, 0, "", "", ""));
        assert_eq!((bar.as_str(), st.as_str()), ("Hata", "Hata: ağ koptu"));

        let (f, bar, st) =
            DownloadsView::row_state(&rec(DownloadStatus::Paused, 50, 100, "", "", ""));
        assert_eq!(f, 0.5);
        assert_eq!(bar, "Duraklatıldı");
        assert_eq!(st, "Duraklatıldı — %50 · 50 B / 100 B");

        let (f, bar, st) = DownloadsView::row_state(&rec(DownloadStatus::Queued, 0, 0, "", "", ""));
        assert_eq!(f, 0.0);
        assert_eq!(bar, "Bekleniyor");
        assert_eq!(st, "Sırada — sırayla indiriliyor");

        let (_, bar, st) = DownloadsView::row_state(&rec(DownloadStatus::Queued, 30, 100, "", "", ""));
        assert_eq!(bar, "%30");
        assert_eq!(st, "Sırada — %30 · 30 B / 100 B");

        let (_, bar, st) =
            DownloadsView::row_state(&rec(DownloadStatus::Downloading, 0, 0, "", "", ""));
        assert_eq!((bar.as_str(), st.as_str()), ("%0", "Bağlanıyor…"));

        let (f, bar, st) =
            DownloadsView::row_state(&rec(DownloadStatus::Downloading, 1536, 3072, "", "", ""));
        assert_eq!(f, 0.5);
        assert_eq!(bar, "%50");
        assert_eq!(st, "İndiriliyor — %50 · 1.5 KB / 3.0 KB");
    }

    #[test]
    fn row_state_has_no_emoji() {
        for (status, have, total) in [
            (DownloadStatus::Done, 100, 100),
            (DownloadStatus::Error("x".into()), 0, 0),
            (DownloadStatus::Paused, 10, 100),
            (DownloadStatus::Queued, 0, 0),
            (DownloadStatus::Queued, 10, 100),
            (DownloadStatus::Downloading, 0, 0),
            (DownloadStatus::Downloading, 10, 100),
        ] {
            let (_, bar, st) = DownloadsView::row_state(&rec(status, have, total, "", "", ""));
            for s in [&bar, &st] {
                assert!(
                    !s.chars().any(|c| c as u32 > 0x2500 && !matches!(c, '—' | '·' | '…')),
                    "emoji/simge olmamalı: {s}"
                );
            }
        }
    }
}
