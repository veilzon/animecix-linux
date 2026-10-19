//! Oynatma kalite seçimi: saf çözümleme mantığı (GTK'sız, mpv'siz).
//! İndirme hattına dokunmaz; yalnızca oynatma aday sıralamasını etkiler.

/// Seçilebilir oynatma kalitesi: etikete karşılık gelen doğrudan URL + kaynak ayna.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayQuality {
    pub label: String,
    pub url: String,
    pub mirror_url: String,
}

/// Etiket sıralaması: 1080p > 720p > 480p > diğerleri (alfabetik).
pub fn order_labels(mut labels: Vec<String>) -> Vec<String> {
    labels.sort_by_key(|l| match l.as_str() {
        "1080p" => (0, l.clone()),
        "720p" => (1, l.clone()),
        "480p" => (2, l.clone()),
        _ => (3, l.clone()),
    });
    labels
}

/// Ham isabetleri tekilleştir + sırala: (etiket, url, ayna).
pub fn collect_play_qualities(hits: Vec<(String, String, String)>) -> Vec<PlayQuality> {
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<PlayQuality> = Vec::new();
    for (label, url, mirror_url) in hits {
        if label.is_empty() || seen.iter().any(|s| s == &label) {
            continue;
        }
        seen.push(label.clone());
        out.push(PlayQuality {
            label,
            url,
            mirror_url,
        });
    }
    let ordered = order_labels(seen);
    let mut sorted: Vec<PlayQuality> = Vec::new();
    for l in ordered {
        if let Some(q) = out.iter().find(|q| q.label == l).cloned() {
            sorted.push(q);
        }
    }
    sorted
}

/// Seçili çevirinin aynalarından oynatılabilir kalite listesi.
/// İlk çok-kaliteli aynayı döner; tek kalite/çözülemezse Err (dialog atlanır).
pub fn available_play_qualities(
    client: &crate::api::Client,
    mirrors: &[crate::api::FansubMirror],
) -> Result<Vec<PlayQuality>, String> {
    for m in mirrors {
        let mut hits: Vec<(String, String, String)> = Vec::new();
        for q in ["1080p", "720p", "480p"] {
            match client.resolve_mirror_quality_strict(&m.url, q) {
                Ok((label, url)) if !label.is_empty() => hits.push((label, url, m.url.clone())),
                _ => {}
            }
        }
        let quals = collect_play_qualities(hits);
        if quals.len() > 1 {
            return Ok(quals);
        }
    }
    Err("tek kalite".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_labels_ranks_hd_first() {
        let v = order_labels(vec![
            "480p".to_string(),
            "abc".to_string(),
            "1080p".to_string(),
            "720p".to_string(),
        ]);
        assert_eq!(v, vec!["1080p", "720p", "480p", "abc"]);
    }

    #[test]
    fn collect_dedupes_and_orders() {
        let quals = collect_play_qualities(vec![
            ("480p".to_string(), "u480".to_string(), "m".to_string()),
            ("1080p".to_string(), "u1080".to_string(), "m".to_string()),
            ("480p".to_string(), "u480b".to_string(), "m".to_string()),
            ("".to_string(), "ux".to_string(), "m".to_string()),
        ]);
        let labels: Vec<&str> = quals.iter().map(|q| q.label.as_str()).collect();
        assert_eq!(labels, vec!["1080p", "480p"]);
        assert_eq!(quals[0].url, "u1080");
    }

    #[test]
    fn collect_empty_in_empty_out() {
        assert!(collect_play_qualities(Vec::new()).is_empty());
    }
}
