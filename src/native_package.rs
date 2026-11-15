//! Native paket entegrasyonu: gömülü `packaging/native-package.sh` köprüsü.
//!
//! AppImage/kaynak kurulumu kullanıcı seviyesinde kalır; deb/rpm paketleri
//! sistem paket yöneticisiyle kurulur/kaldırılır.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const SCRIPT: &str = include_str!("../packaging/native-package.sh");

pub fn system_package_managed_for(kind: crate::update::InstallKind) -> bool {
    matches!(
        kind,
        crate::update::InstallKind::Deb | crate::update::InstallKind::Rpm
    )
}

/// Uygulama deb/rpm olarak mı kurulu? (AppImage her zaman kendi yolunu kullanır.)
pub fn system_package_managed() -> bool {
    system_package_managed_for(crate::update::install_kind())
}

fn script_path() -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "animecix-native-{}-{}.sh",
        std::process::id(),
        nanos
    ));
    std::fs::write(&path, SCRIPT).map_err(|e| format!("native script yazılamadı: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
    }
    Ok(path)
}

fn run(args: &[&str]) -> Result<Output, String> {
    let path = script_path()?;
    let out = Command::new("/bin/bash")
        .arg(&path)
        .args(args)
        .output()
        .map_err(|e| format!("native script çalıştırılamadı: {e}"));
    let _ = std::fs::remove_file(&path);
    let out = out?;
    if out.status.success() {
        return Ok(out);
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    Err(if detail.is_empty() {
        format!("native script başarısız (çıkış {:?})", out.status.code())
    } else {
        detail
    })
}

/// GitHub'dan indirilen deb/rpm dosyasını doğru paket yöneticisiyle kurar.
pub fn install(path: &Path) -> Result<(), String> {
    let p = path.to_string_lossy().into_owned();
    run(&["install", &p]).map(|_| ())
}

pub fn uninstall_native() -> Result<(), String> {
    run(&["uninstall", "--native"]).map(|_| ())
}

pub fn uninstall_user() -> Result<(), String> {
    run(&["uninstall", "--user"]).map(|_| ())
}

/// Kurulum türüne göre doğru kaldırma yolunu seçer.
pub fn uninstall() -> Result<(), String> {
    if system_package_managed() {
        uninstall_native()
    } else {
        uninstall_user()
    }
}

/// deb/rpm kurulumunda eski AppImage kullanıcı kısayollarını temizler.
pub fn cleanup_user() -> Result<(), String> {
    run(&["cleanup", "--user"]).map(|_| ())
}

/// Flatpak remote'unda bu uygulama için güncelleme var mı?
/// Flatpak dışında `flatpak-spawn` olmadığından güvenli biçimde `false` döner.
pub fn flatpak_update_available() -> bool {
    let Ok(path) = script_path() else {
        return false;
    };
    let status = Command::new("/bin/bash")
        .arg(&path)
        .arg("check-flatpak")
        .status();
    let _ = std::fs::remove_file(&path);
    status.map(|s| s.success()).unwrap_or(false)
}

/// Flatpak güncellemesini host üzerinde çalıştırır; polkit gerekirse sorar.
pub fn update_flatpak() -> Result<(), String> {
    run(&["update-flatpak"]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::InstallKind;
    use std::io::Write;

    #[test]
    fn system_package_mapping() {
        assert!(system_package_managed_for(InstallKind::Deb));
        assert!(system_package_managed_for(InstallKind::Rpm));
        assert!(!system_package_managed_for(InstallKind::AppImage));
        assert!(!system_package_managed_for(InstallKind::Unknown));
    }

    #[test]
    fn script_status_runs() {
        let out = run(&["status"]).expect("status");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("owner="));
        assert!(text.contains("manager="));
    }

    #[test]
    fn install_dry_run_uses_package_manager() {
        let path =
            std::env::temp_dir().join(format!("animecix-native-test-{}.deb", std::process::id()));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"test").unwrap();
        let out = run(&["--dry-run", "install", path.to_str().unwrap()]).expect("dry run");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("apt-get")
                || text.contains("dnf")
                || text.contains("zypper")
                || text.contains("pacman"),
            "paket yöneticisi komutu yok: {text}"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn uninstall_native_dry_run_uses_package_manager() {
        let out = run(&["--dry-run", "uninstall", "--native"]).expect("dry run");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("remove") || text.contains("pacman"),
            "kaldırma komutu yok: {text}"
        );
    }
}
