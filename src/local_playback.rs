//! İndirilmiş bölümlerin yerel oynatma için bulunması.
//!
//! Disk düzeni `indirme_klasörü/{seri}/{bölüm}.mp4`; burada yalnızca
//! kayıt eşleştirme ve öncelik kuralı yaşar. Dosya varlığı testlerde
//! enjekte edilebilir, böylece gerçek indirme klasörüne dokunulmaz.

use crate::download::{sanitize_filename, DownloadRecord, DownloadStatus};
use std::path::Path;

/// Bölüm için oynatılabilir ilk yerel kaydı döndürür.
///
/// Sıra: `Done` → sezon/bölüm → başlık kimliği (yoksa isim) → dosya var.
/// Aynı bölümde birden fazla indirme varsa kuyruktaki ilk kayıt kazanır.
pub fn find_for_episode(
    records: &[DownloadRecord],
    title_id: u64,
    title_name: &str,
    season: u64,
    episode: u64,
) -> Option<DownloadRecord> {
    find_matching(records, title_id, title_name, season, episode, |p| {
        p.is_file()
    })
}

fn find_matching<F>(
    records: &[DownloadRecord],
    title_id: u64,
    title_name: &str,
    season: u64,
    episode: u64,
    file_exists: F,
) -> Option<DownloadRecord>
where
    F: Fn(&Path) -> bool,
{
    records
        .iter()
        .find(|rec| {
            rec.status == DownloadStatus::Done
                && rec.season == season
                && rec.episode == episode
                && title_matches(rec, title_id, title_name)
                && file_exists(&rec.dest)
        })
        .cloned()
}

fn title_matches(rec: &DownloadRecord, title_id: u64, title_name: &str) -> bool {
    if title_id != 0 && rec.title_id != 0 {
        return rec.title_id == title_id;
    }
    let series = sanitize_filename(title_name);
    rec.title == title_name || rec.title == series
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn record(
        id: &str,
        title_id: u64,
        title: &str,
        season: u64,
        episode: u64,
        status: DownloadStatus,
    ) -> DownloadRecord {
        DownloadRecord {
            id: id.into(),
            title_id,
            title: title.into(),
            season,
            episode,
            ep_name: format!("Bölüm {episode}"),
            fansub: "Fansub".into(),
            quality: "1080p".into(),
            url: format!("https://example/{id}.mp4"),
            referer: None,
            dest: PathBuf::from(format!("/tmp/{id}.mp4")),
            total: 100,
            have: 100,
            status,
        }
    }

    #[test]
    fn done_only_matches() {
        let records = vec![
            record("queued", 7, "Dizi", 1, 1, DownloadStatus::Queued),
            record("done", 7, "Dizi", 1, 1, DownloadStatus::Done),
        ];
        let found = find_matching(&records, 7, "Dizi", 1, 1, |_| true).expect("Done kayıt");
        assert_eq!(found.id, "done");
    }

    #[test]
    fn missing_file_is_skipped() {
        let records = vec![
            record("a", 7, "Dizi", 1, 1, DownloadStatus::Done),
            record("b", 7, "Dizi", 1, 1, DownloadStatus::Done),
        ];
        let found = find_matching(&records, 7, "Dizi", 1, 1, |p| {
            p.to_string_lossy().ends_with("b.mp4")
        })
        .expect("ikinci dosya");
        assert_eq!(found.id, "b");
    }

    #[test]
    fn old_record_matches_by_sanitized_name() {
        let records = vec![record("a", 0, "Mavi Kule", 2, 3, DownloadStatus::Done)];
        assert!(find_matching(&records, 42, "Mavi Kule", 2, 3, |_| true).is_some());
    }

    #[test]
    fn first_done_record_wins() {
        let records = vec![
            record("first", 7, "Dizi", 1, 1, DownloadStatus::Done),
            record("second", 7, "Dizi", 1, 1, DownloadStatus::Done),
        ];
        let found = find_matching(&records, 7, "Dizi", 1, 1, |_| true).expect("ilk kayıt");
        assert_eq!(found.id, "first");
    }

    #[test]
    fn wrong_season_or_episode_does_not_match() {
        let records = vec![record("a", 7, "Dizi", 1, 1, DownloadStatus::Done)];
        assert!(find_matching(&records, 7, "Dizi", 1, 2, |_| true).is_none());
        assert!(find_matching(&records, 7, "Dizi", 2, 1, |_| true).is_none());
    }

    #[test]
    fn title_id_wins_over_name_when_both_known() {
        let records = vec![record("a", 8, "Dizi", 1, 1, DownloadStatus::Done)];
        assert!(find_matching(&records, 7, "Dizi", 1, 1, |_| true).is_none());
    }
}
