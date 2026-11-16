/// Dosya adında yasak karakterleri temizler.
pub fn safe_fname(s: &str) -> String {
    let mut out: String = s.chars().filter(|c| !"/\\:*?\"<>|".contains(*c)).collect();
    out = out.trim().to_string();
    if out.chars().count() > 80 {
        out = out.chars().take(80).collect();
    }
    if out.is_empty() {
        out = "animecix".into();
    }
    out
}

pub fn screenshot_filename(title: &str, season: u64, episode: u64, timestamp: u64) -> String {
    format!(
        "{}-S{:02}E{:02}-{timestamp}.png",
        safe_fname(title),
        season,
        episode
    )
}

pub fn info_text(info: &[(String, String)]) -> String {
    if info.is_empty() {
        return "Bilgi yok (video henüz yüklenmedi mi?)".to_string();
    }
    info.iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn screenshot_filename_sanitizes_and_has_context() {
        let n = super::screenshot_filename("Witch/Hat: Atelier", 1, 3, 42);
        assert!(n.starts_with("WitchHat Atelier-S01E03-42"));
        assert!(!n.contains('/') && !n.contains(':'));
    }

    #[test]
    fn info_text_empty_and_lines() {
        assert!(super::info_text(&[]).contains("Bilgi yok"));
        let info = vec![
            ("Çözünürlük".to_string(), "1920x1080".to_string()),
            ("FPS".to_string(), "23.98".to_string()),
        ];
        let t = super::info_text(&info);
        assert!(t.contains("Çözünürlük: 1920x1080"));
        assert!(t.contains("FPS: 23.98"));
    }
}
