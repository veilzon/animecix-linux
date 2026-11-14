//! Flatpak sandbox algılama.
//!
//! Flatpak, menü girdisini ve güncellemeleri kendi yönetir; uygulama içi
//! kendini güncelleme ve kullanıcı `.desktop` entegrasyonu çalıştırılmaz.

use std::path::Path;

/// Flatpak sandbox'ında mıyız? `/.flatpak-info` Flatpak tarafından
/// oluşturulur; `FLATPAK_ID` de aynı bilgiyi taşır.
pub fn is_flatpak() -> bool {
    detect(
        Path::new("/.flatpak-info").exists(),
        std::env::var("FLATPAK_ID").ok().as_deref(),
    )
}

fn detect(info_exists: bool, flatpak_id: Option<&str>) -> bool {
    info_exists || flatpak_id.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_flatpak_info_or_id() {
        assert!(detect(true, None));
        assert!(detect(false, Some("io.github.veilzon.AnimeciX")));
        assert!(!detect(false, None));
        assert!(!detect(false, Some("   ")));
    }
}
