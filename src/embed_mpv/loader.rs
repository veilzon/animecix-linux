use std::path::Path;

pub use libloading::Library;

pub struct LoadedLibmpv {
    /// Sembollerin çözülebilmesi için kütüphane yükleme boyunca yaşamalı.
    #[allow(dead_code)]
    pub library: Library,
    pub path: String,
    pub api_version: u64,
}

/// libmpv istemci API'sinin ana sürümü 2 olan sürümlerle uyumluyuz.
/// Alt sürüm yükselse de C ABI geriye uyumludur.
pub fn api_version_compatible(version: u64) -> bool {
    (version >> 16) == 2
}

/// Denenecek kütüphane yolları: paket içi kopya, Flatpak, sonra sistem soname'leri.
pub fn candidates(appdir: Option<&str>, override_path: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        out.push(p.to_string());
    }
    if let Some(dir) = appdir.map(str::trim).filter(|p| !p.is_empty()) {
        let base = Path::new(dir).join("usr/lib");
        out.push(base.join("libmpv.so.2").to_string_lossy().into_owned());
        out.push(base.join("libmpv.so.1").to_string_lossy().into_owned());
    }
    out.push("/app/lib/libmpv.so.2".to_string());
    out.push("libmpv.so.2".to_string());
    out.push("libmpv.so.1".to_string());
    out.push("libmpv.so".to_string());
    out
}

pub fn version_label(version: u64) -> String {
    format!("{}.{}", version >> 16, version & 0xffff)
}

pub fn load() -> Result<LoadedLibmpv, String> {
    let appdir = std::env::var("APPDIR").ok();
    let override_path = std::env::var("ANIMECIX_LIBMPV_PATH").ok();
    let mut errors = Vec::new();

    for candidate in candidates(appdir.as_deref(), override_path.as_deref()) {
        let library = match unsafe { Library::new(&candidate) } {
            Ok(library) => library,
            Err(e) => {
                errors.push(format!("{candidate}: {e}"));
                continue;
            }
        };

        let version_fn = match unsafe {
            library.get::<unsafe extern "C" fn() -> u64>(b"mpv_client_api_version\0")
        } {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("{candidate}: mpv_client_api_version yok ({e})"));
                continue;
            }
        };
        let api_version = unsafe { version_fn() };
        if !api_version_compatible(api_version) {
            errors.push(format!(
                "{candidate}: uyumsuz libmpv API {}",
                version_label(api_version)
            ));
            continue;
        }

        return Ok(LoadedLibmpv {
            library,
            path: candidate,
            api_version,
        });
    }

    Err(if errors.is_empty() {
        "libmpv adayı yok".to_string()
    } else {
        errors.join(" | ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_accepts_2x_only() {
        assert!(api_version_compatible((2 << 16) | 5));
        assert!(api_version_compatible((2 << 16) | 0));
        assert!(!api_version_compatible((1 << 16) | 9));
        assert!(!api_version_compatible((3 << 16) | 0));
    }

    #[test]
    fn candidates_prefer_override_then_bundled_then_soname() {
        let c = candidates(Some("/tmp/AppDir"), Some("/opt/mpv/libmpv.so.2"));
        assert_eq!(c[0], "/opt/mpv/libmpv.so.2");
        assert_eq!(c[1], "/tmp/AppDir/usr/lib/libmpv.so.2");
        assert_eq!(c[3], "/app/lib/libmpv.so.2");
        assert!(c.ends_with(&[
            "libmpv.so.2".into(),
            "libmpv.so.1".into(),
            "libmpv.so".into()
        ]));
        assert_eq!(version_label((2 << 16) | 5), "2.5");
    }
}
