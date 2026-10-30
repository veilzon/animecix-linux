use std::process::Command;

const ANIMECIX_PNG_ICON: &[u8] =
    include_bytes!("../assets/hicolor/256x256/apps/tr.com.animecix.png");

const ANIMECIX_SVG_ICON: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#7c3aed"/>
      <stop offset="1" stop-color="#db2777"/>
    </linearGradient>
  </defs>
  <rect width="256" height="256" rx="48" fill="url(#g)"/>
  <path d="M70 200 L128 56 L186 200 L160 200 L148 168 L108 168 L96 200 Z M116 148 L140 148 L128 116 Z" fill="#ffffff"/>
</svg>"##;

pub struct DepStatus {
    pub name: &'static str,
    pub installed: bool,
    pub install_cmd: Option<String>,
}

pub fn check_all_dependencies() -> Vec<DepStatus> {
    let (distro_name, mpv_cmd) = detect_distro_info();
    let (_, pkg_cmd) = detect_package_manager_cmd();

    let has_mpv = check_command("mpv");
    let has_curl = check_command("curl");
    let has_xdg = check_command("xdg-open");

    vec![
        DepStatus {
            name: "Video Oynatıcı (mpv)",
            installed: has_mpv,
            install_cmd: Some(format!("{mpv_cmd}   # ({distro_name})")),
        },
        DepStatus {
            name: "Sistem Medya İndirici (curl)",
            installed: has_curl,
            install_cmd: Some(format!("{pkg_cmd} curl")),
        },
        DepStatus {
            name: "Masaüstü Bağlantı Araçları (xdg-utils)",
            installed: has_xdg,
            install_cmd: Some(format!("{pkg_cmd} xdg-utils")),
        },
    ]
}

pub fn check_desktop_entry_installed() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let p1 = std::path::Path::new(&format!(
        "{home}/.local/share/applications/tr.com.animecix.desktop"
    ))
    .exists();
    let p2 = std::path::Path::new(&format!(
        "{home}/.local/share/applications/animecix.desktop"
    ))
    .exists();
    let p3 = std::path::Path::new("/usr/share/applications/tr.com.animecix.desktop").exists();
    let p4 = std::path::Path::new("/usr/share/applications/animecix.desktop").exists();
    p1 || p2 || p3 || p4
}

pub const STABLE_APPIMAGE_NAME: &str = "AnimeciX-x86_64.AppImage";

pub fn stable_appimage_path(home: &str) -> String {
    format!("{home}/.local/bin/{STABLE_APPIMAGE_NAME}")
}

/// Paket (deb/rpm) kurulumunda sistem ikilisinin yolu.
pub const SYSTEM_BIN_PATH: &str = "/usr/bin/animecix";

/// Sistemde rpm/deb kurulumu var mı? Varsa menü girdisinin sahibi sistem
/// paketidir; AppImage kullanıcı kısayolu oluşturmaz.
pub fn system_installation_present() -> bool {
    std::path::Path::new(SYSTEM_BIN_PATH).is_file()
}

/// AppImage + sistem kurulumu birlikteyse otomatik entegrasyon kapalıdır.
/// AppImage bu durumda yalnızca taşınabilir çalışır; menüyü gölgelemez.
pub(crate) fn appimage_integration_blocked(
    appimage: Option<&str>,
    system_bin_exists: bool,
) -> bool {
    appimage.map(|s| !s.trim().is_empty()).unwrap_or(false) && system_bin_exists
}

/// UI ve kurulum yolu için: AppImage entegrasyonu bu ortamda kapalı mı?
pub fn appimage_integration_disabled() -> bool {
    appimage_integration_blocked(
        std::env::var("APPIMAGE").ok().as_deref(),
        system_installation_present(),
    )
}

/// Kısayolun çalıştıracağı yol: sistem paketi varsa sistem ikilisi
/// (/usr/bin/animecix), yoksa AppImage'in sabit kopyası. Eski sürümlerin
/// ~/.local/bin/animecix'e yaptığı kopya güncelleme sonrası bayat kalıp uyarı
/// döngüsü yarattığı için artık kullanılmaz.
pub fn desktop_exec_target(home: &str) -> String {
    let appimage = std::env::var("APPIMAGE").ok();
    let current = std::env::current_exe().ok();
    desktop_exec_target_for(
        appimage.as_deref(),
        home,
        system_installation_present(),
        current.as_deref(),
    )
}

fn desktop_exec_target_for(
    appimage: Option<&str>,
    home: &str,
    system_bin_exists: bool,
    current_exe: Option<&std::path::Path>,
) -> String {
    if system_bin_exists {
        return SYSTEM_BIN_PATH.to_string();
    }
    if appimage.filter(|s| !s.trim().is_empty()).is_some() {
        return stable_appimage_path(home);
    }
    current_exe
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("{home}/.local/bin/animecix"))
}

pub fn parse_desktop_exec(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("Exec=") {
            let v = v.trim();
            let unquoted = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v);
            return Some(unquoted.to_string());
        }
    }
    None
}

/// Sistem paketi kuruluyken AppImage'in bıraktığı kullanıcı entegrasyonunu
/// temizler: yalnızca sabit AppImage yolunu gösteren .desktop dosyaları ve
/// (o an çalışmıyorsa) sabit kopya silinir. Kullanıcının kendi girdilerine
/// dokunulmaz.
pub(crate) fn cleanup_stale_appimage_integration(home: &str, current_appimage: &str) {
    let stable = stable_appimage_path(home);
    let apps_dir = format!("{home}/.local/share/applications");
    let mut removed = false;

    for name in ["tr.com.animecix.desktop", "animecix.desktop"] {
        let path = format!("{apps_dir}/{name}");
        if parse_desktop_exec(&path).as_deref() == Some(stable.as_str())
            && std::fs::remove_file(&path).is_ok()
        {
            removed = true;
        }
    }

    // Çalışan dosyayı silmek güvenli olsa da yeniden başlatma/otomatik
    // güncelleme yolu kaybolmasın: yalnızca başka bir kopyadan çalışırken sil.
    let running_from_stable = match (
        std::fs::canonicalize(current_appimage),
        std::fs::canonicalize(&stable),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !running_from_stable && std::fs::remove_file(&stable).is_ok() {
        removed = true;
    }

    if removed {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&apps_dir)
            .output();
    }
}

pub fn install_desktop_entry() -> Result<(), String> {
    if appimage_integration_disabled() {
        return Err(
            "Sistem kurulumu (RPM/DEB) algılandı; menü kısayolu sistem paketi tarafından yönetiliyor."
                .to_string(),
        );
    }
    let home = std::env::var("HOME").map_err(|_| "HOME klasörü bulunamadı".to_string())?;

    let exec_target = desktop_exec_target(&home);
    if let Some(ai) = std::env::var("APPIMAGE")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        let _ = std::fs::remove_file(format!("{home}/.local/bin/animecix"));
        let stable = stable_appimage_path(&home);
        if ai != stable {
            std::fs::create_dir_all(format!("{home}/.local/bin")).map_err(|e| e.to_string())?;
            std::fs::copy(&ai, &stable)
                .map_err(|e| format!("AppImage sabit konuma kopyalanamadı: {e}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&stable, std::fs::Permissions::from_mode(0o755));
            }
        }
    } else {
        // Eski sürümler ikiliyi ~/.local/bin/animecix'e kopyalıyordu; kopya
        // güncelleme sonrası bayat kalıp "güncelleme var" uyarı döngüsü
        // yaratıyordu. Kısayol artık sistem ikilisini gösterir; eski kopya
        // silinir (çalışan dosyayı silmek güvenli, süreç inode'undan devam eder).
        let _ = std::fs::remove_file(format!("{home}/.local/bin/animecix"));
    }

    let apps_dir = format!("{home}/.local/share/applications");
    let icons_dir = format!("{home}/.local/share/icons/hicolor/256x256/apps");
    let icons_dir_simple = format!("{home}/.local/share/icons");

    let _ = std::fs::create_dir_all(&apps_dir);
    let _ = std::fs::create_dir_all(&icons_dir);
    let _ = std::fs::create_dir_all(&icons_dir_simple);

    let target_icon = format!("{icons_dir}/tr.com.animecix.png");
    let target_icon_simple = format!("{icons_dir_simple}/tr.com.animecix.png");
    let target_icon_lg =
        format!("{home}/.local/share/icons/hicolor/512x512/apps/tr.com.animecix.png");
    let _ = std::fs::create_dir_all(format!("{home}/.local/share/icons/hicolor/512x512/apps"));
    let target_icon_lg_simple = format!("{icons_dir_simple}/tr.com.animecix.svg");
    let _ = std::fs::write(&target_icon_lg_simple, ANIMECIX_SVG_ICON.as_bytes());

    let icon_bytes: Option<Vec<u8>> = None
        .or_else(|| {
            let p = std::path::Path::new("assets/hicolor/256x256/apps/tr.com.animecix.png");
            std::fs::read(p).ok()
        })
        .or_else(|| {
            let p =
                std::path::Path::new("/usr/share/icons/hicolor/256x256/apps/tr.com.animecix.png");
            std::fs::read(p).ok()
        })
        .or_else(|| {
            if let Ok(ai) = std::env::var("APPIMAGE") {
                if !ai.trim().is_empty() {
                    let p = std::path::Path::new(&ai)
                        .parent()
                        .map(|d| d.join("assets/hicolor/256x256/apps/tr.com.animecix.png"));
                    if let Some(pp) = p {
                        return std::fs::read(pp).ok();
                    }
                }
            }
            None
        })
        .or_else(|| Some(ANIMECIX_PNG_ICON.to_vec()));

    if let Some(content) = icon_bytes {
        let _ = std::fs::write(&target_icon, &content);
        let _ = std::fs::write(&target_icon_simple, &content);
        let _ = std::fs::write(&target_icon_lg, &content);
    }

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=AnimeciX\n\
         Comment=Anime, dizi ve film izleme istemcisi\n\
         Exec=\"{exec_target}\"\n\
         Icon=tr.com.animecix\n\
         Terminal=false\n\
         Categories=AudioVideo;Video;Network;\n\
         X-AppImage-Version=0.1.0\n"
    );

    let target_desktop = format!("{apps_dir}/tr.com.animecix.desktop");
    std::fs::write(&target_desktop, desktop_content).map_err(|e| e.to_string())?;

    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .output();

    Ok(())
}

pub fn uninstall_application() {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return;
    }

    let bin_path = format!("{home}/.local/bin/animecix");
    let _ = std::fs::remove_file(&bin_path);
    let _ = std::fs::remove_file(stable_appimage_path(&home));

    let d1 = format!("{home}/.local/share/applications/tr.com.animecix.desktop");
    let d2 = format!("{home}/.local/share/applications/animecix.desktop");
    let _ = std::fs::remove_file(&d1);
    let _ = std::fs::remove_file(&d2);

    let i1 = format!("{home}/.local/share/icons/hicolor/256x256/apps/tr.com.animecix.png");
    let i2 = format!("{home}/.local/share/icons/hicolor/256x256/apps/animecix.png");
    let i3 = format!("{home}/.local/share/icons/tr.com.animecix.png");
    let i4 = format!("{home}/.local/share/icons/animecix.png");
    let _ = std::fs::remove_file(&i1);
    let _ = std::fs::remove_file(&i2);
    let _ = std::fs::remove_file(&i3);
    let _ = std::fs::remove_file(&i4);

    let apps_dir = format!("{home}/.local/share/applications");
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .output();
}

fn check_command(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn detect_package_manager_cmd() -> (&'static str, &'static str) {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        let content_lower = content.to_lowercase();
        if content_lower.contains("ubuntu")
            || content_lower.contains("debian")
            || content_lower.contains("mint")
        {
            return ("APT", "sudo apt update && sudo apt install -y");
        } else if content_lower.contains("fedora") {
            return ("DNF", "sudo dnf install -y");
        } else if content_lower.contains("arch") || content_lower.contains("manjaro") {
            return ("Pacman", "sudo pacman -S --noconfirm");
        } else if content_lower.contains("suse") {
            return ("Zypper", "sudo zypper install -y");
        }
    }
    ("Paket Yöneticisi", "sudo apt install -y")
}

fn detect_distro_info() -> (&'static str, &'static str) {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        let content_lower = content.to_lowercase();
        if content_lower.contains("ubuntu")
            || content_lower.contains("debian")
            || content_lower.contains("mint")
        {
            return (
                "Ubuntu / Debian / Mint",
                "sudo apt update && sudo apt install mpv",
            );
        } else if content_lower.contains("fedora") {
            return ("Fedora", "sudo dnf install mpv");
        } else if content_lower.contains("arch") || content_lower.contains("manjaro") {
            return ("Arch / Manjaro", "sudo pacman -S mpv");
        } else if content_lower.contains("suse") {
            return ("openSUSE", "sudo zypper install mpv");
        }
    }
    (
        "Linux",
        "sudo apt install mpv   # ya da dnf/pacman/zypper install mpv",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_exec_target_uses_stable_copy_for_appimage() {
        let ai = Some("/opt/AnimeciX-x86_64.AppImage");
        assert_eq!(
            desktop_exec_target_for(ai, "/home/x", false, None),
            "/home/x/.local/bin/AnimeciX-x86_64.AppImage"
        );
        // Sistem kurulumu varsa menü girdisi sistem ikilisini gösterir.
        assert_eq!(
            desktop_exec_target_for(ai, "/home/x", true, None),
            "/usr/bin/animecix"
        );
    }

    #[test]
    fn appimage_integration_blocked_only_with_system_install() {
        assert!(appimage_integration_blocked(
            Some("/opt/AnimeciX-x86_64.AppImage"),
            true
        ));
        assert!(!appimage_integration_blocked(
            Some("/opt/AnimeciX-x86_64.AppImage"),
            false
        ));
        assert!(!appimage_integration_blocked(None, true));
        assert!(!appimage_integration_blocked(Some("  "), true));
    }

    #[test]
    fn cleanup_removes_only_appimage_managed_entry() {
        let dir = std::env::temp_dir().join(format!(
            "animecix_cleanup_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let apps = dir.join(".local/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        let stable = stable_appimage_path(dir.to_str().unwrap());
        std::fs::create_dir_all(std::path::Path::new(&stable).parent().unwrap()).unwrap();
        std::fs::write(&stable, b"stable-appimage").unwrap();

        let managed = apps.join("tr.com.animecix.desktop");
        std::fs::write(
            &managed,
            format!("[Desktop Entry]\nType=Application\nName=AnimeciX\nExec=\"{stable}\"\n"),
        )
        .unwrap();
        let system = apps.join("animecix.desktop");
        std::fs::write(
            &system,
            "[Desktop Entry]\nType=Application\nName=AnimeciX\nExec=animecix\n",
        )
        .unwrap();

        cleanup_stale_appimage_integration(dir.to_str().unwrap(), "/opt/current.AppImage");

        assert!(!managed.exists(), "AppImage kullanıcı kısayolu silinmedi");
        assert!(system.exists(), "sistem kısayolu yanlışlıkla silindi");
        assert!(
            !std::path::Path::new(&stable).exists(),
            "stable kopya silinmedi"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_keeps_running_stable_copy() {
        let dir = std::env::temp_dir().join(format!(
            "animecix_cleanup_run_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let apps = dir.join(".local/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        let stable = stable_appimage_path(dir.to_str().unwrap());
        std::fs::create_dir_all(std::path::Path::new(&stable).parent().unwrap()).unwrap();
        std::fs::write(&stable, b"stable-appimage").unwrap();
        let managed = apps.join("tr.com.animecix.desktop");
        std::fs::write(
            &managed,
            format!("[Desktop Entry]\nType=Application\nName=AnimeciX\nExec=\"{stable}\"\n"),
        )
        .unwrap();

        cleanup_stale_appimage_integration(dir.to_str().unwrap(), &stable);

        assert!(!managed.exists(), "AppImage kullanıcı kısayolu silinmedi");
        assert!(
            std::path::Path::new(&stable).exists(),
            "çalışan stable kopya silinmemeli"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stable_appimage_path_shape() {
        assert_eq!(
            stable_appimage_path("/home/x"),
            "/home/x/.local/bin/AnimeciX-x86_64.AppImage"
        );
    }

    #[test]
    fn parse_desktop_exec_reads_and_unquotes() {
        let dir = std::env::temp_dir().join(format!("animecix_dtest_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.desktop");
        std::fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=AnimeciX\nExec=\"/opt/x y/AnimeciX.AppImage\"\n",
        )
        .unwrap();
        assert_eq!(
            parse_desktop_exec(path.to_str().unwrap()),
            Some("/opt/x y/AnimeciX.AppImage".to_string())
        );
        let p2 = dir.join("empty.desktop");
        std::fs::write(&p2, "[Desktop Entry]\nName=x\n").unwrap();
        assert_eq!(parse_desktop_exec(p2.to_str().unwrap()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn desktop_exec_target_prefers_system_binary() {
        let cur = std::path::Path::new("/home/x/indir/animecix");
        assert_eq!(
            desktop_exec_target_for(None, "/home/x", true, Some(cur)),
            "/usr/bin/animecix"
        );
        assert_eq!(
            desktop_exec_target_for(Some("  "), "/home/x", true, None),
            "/usr/bin/animecix"
        );
        assert_eq!(
            desktop_exec_target_for(None, "/home/x", false, Some(cur)),
            "/home/x/indir/animecix"
        );
        assert_eq!(
            desktop_exec_target_for(None, "/home/x", false, None),
            "/home/x/.local/bin/animecix"
        );
    }
}
