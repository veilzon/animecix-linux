//! İndirilenleri seri klasörlerine ayıran saf model.
//!
//! Disk düzeni zaten `indirme_klasörü/{seri}/{bölüm}.mp4`; burada yalnızca
//! UI için gruplama, toplamlar ve arama filtresi üretilir.

use crate::download::DownloadRecord;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DownloadFolder {
    pub title_id: u64,
    pub title: String,
    /// `(sezon, bölüm)` sırasına dizilmiş kopyalar.
    pub episodes: Vec<DownloadRecord>,
}

impl DownloadFolder {
    pub fn done_count(&self) -> usize {
        self.episodes
            .iter()
            .filter(|e| e.status == crate::download::DownloadStatus::Done)
            .count()
    }

    pub fn total_bytes(&self) -> u64 {
        self.episodes.iter().map(|e| e.total.max(e.have)).sum()
    }
}

/// Kayıtları `(title_id, title)` bazında klasörlere ayırır.
/// Klasörler başlık adına göre; bölümler `(sezon, bölüm)` sırasına göre.
pub fn group_downloads(records: &[DownloadRecord]) -> Vec<DownloadFolder> {
    let mut map: BTreeMap<(u64, String), Vec<DownloadRecord>> = BTreeMap::new();
    for rec in records {
        map.entry((rec.title_id, rec.title.clone()))
            .or_default()
            .push(rec.clone());
    }
    let mut folders: Vec<DownloadFolder> = map
        .into_iter()
        .map(|((title_id, title), mut episodes)| {
            episodes.sort_by_key(|e| (e.season, e.episode));
            DownloadFolder {
                title_id,
                title,
                episodes,
            }
        })
        .collect();
    folders.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.title_id.cmp(&b.title_id))
    });
    folders
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::DownloadStatus;
    use std::path::PathBuf;

    fn rec(id: &str, title: &str, title_id: u64, season: u64, episode: u64) -> DownloadRecord {
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
            status: DownloadStatus::Done,
        }
    }

    #[test]
    fn groups_by_title_and_sorts_episodes() {
        let records = vec![
            rec("a2", "B Dizi", 2, 1, 2),
            rec("b1", "A Dizi", 1, 1, 1),
            rec("a1", "B Dizi", 2, 1, 1),
        ];
        let folders = group_downloads(&records);
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].title, "A Dizi");
        assert_eq!(folders[1].title, "B Dizi");
        assert_eq!(
            folders[1]
                .episodes
                .iter()
                .map(|e| e.episode)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(folders[1].done_count(), 2);
        assert_eq!(folders[1].total_bytes(), 200);
    }
}
