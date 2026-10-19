//! mpv yazı tipi: paketlenmiş Montserrat kurulumu + mpv bayrakları.

use std::path::{Path, PathBuf};

pub const FONT_STYLE: &str = "Montserrat SemiBold";
const FONT_FILE: &str = "Montserrat[wght].ttf";
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/Montserrat[wght].ttf");

pub fn fonts_dir() -> PathBuf {
    std::env::temp_dir().join("animecix-fonts")
}

/// Fontu bir kez yazar, dizini döner. Başarısızlıkta None
/// (mpv sistem varsayılanıyla açılır, çökme yok).
pub fn ensure_fonts() -> Option<PathBuf> {
    let dir = fonts_dir();
    let target = dir.join(FONT_FILE);
    if !target.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&target, FONT_BYTES).ok()?;
    }
    Some(dir)
}

/// mpv yazı tipi bayrakları (okunaklı OSD + altyazı).
pub fn mpv_font_args(dir: &Path) -> Vec<String> {
    let d = dir.to_string_lossy().into_owned();
    vec![
        format!("--osd-fonts-dir={d}"),
        format!("--sub-fonts-dir={d}"),
        format!("--osd-font={FONT_STYLE}"),
        "--osd-font-size=38".to_string(),
        format!("--sub-font={FONT_STYLE}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_fonts_idempotent() {
        let a = ensure_fonts().expect("font yazılmalı");
        let target = a.join(FONT_FILE);
        assert!(target.exists(), "ttf diskte olmalı");
        assert!(
            target.metadata().map(|m| m.len()).unwrap_or(0) > 100_000,
            "ttf dolu olmalı"
        );
        let b = ensure_fonts().expect("ikinci çağrı da");
        assert_eq!(a, b);
        assert!(!FONT_STYLE.is_empty());
    }

    #[test]
    fn font_args_shapes() {
        let args = mpv_font_args(Path::new("/tmp/x"));
        let joined = args.join(" ");
        assert!(joined.contains("--osd-fonts-dir=/tmp/x"), "{joined}");
        assert!(joined.contains("--sub-fonts-dir=/tmp/x"), "{joined}");
        assert!(
            joined.contains(&format!("--osd-font={FONT_STYLE}")),
            "{joined}"
        );
        assert!(joined.contains("--osd-font-size=38"), "{joined}");
        assert!(
            joined.contains(&format!("--sub-font={FONT_STYLE}")),
            "{joined}"
        );
    }
}
