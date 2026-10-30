mod api;
mod app;
mod aria;
mod auth;
mod covers;
mod download;
mod download_folders;
mod font;
mod http;
mod music;
mod play_quality;
mod player;
mod segmented;
mod skip;
mod theme;
mod ui;
mod update;

use adw::prelude::*;
use app::App;
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

pub fn restart_app() {
    let exe = std::env::var("APPIMAGE")
        .map(std::path::PathBuf::from)
        .ok()
        .filter(|p| p.is_file())
        .or_else(|| std::env::current_exe().ok());
    if let Some(exe) = exe {
        update::spawn_restart(&exe);
        std::process::exit(0);
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.iter().any(|a| a == "--check-shaders") {
        return cmd_check_shaders();
    }
    if raw_args.iter().any(|a| a == "--check-icons") {
        return cmd_check_icons();
    }
    let mut filtered: Vec<String> = Vec::new();
    let mut it = raw_args.iter();
    if let Some(p) = it.next() {
        filtered.push(p.clone());
    }
    while let Some(a) = it.next() {
        if a == "--goto" {
            let _ = it.next(); // değeri atla
        } else if a.starts_with("--goto=") {
        } else {
            filtered.push(a.clone());
        }
    }

    std::env::set_var("MALLOC_ARENA_MAX", "2");
    // Hafif mod bayrağı: tüm Client kurmadan doğrudan ayar dosyasından
    // okunur (Client::new HTTP/runtime kurar, açılışı yavaşlatır).
    let light_mode = std::fs::read_to_string(api::Client::settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("light_mode").and_then(|b| b.as_bool()))
        .unwrap_or(false);
    if light_mode {
        std::env::set_var("GSK_RENDERER", "cairo");
        eprintln!("[STARTUP] hafif mod aktif (cairo renderer)");
    }

    let app = adw::Application::builder()
        .application_id("tr.com.animecix")
        .build();

    app.connect_activate(|app| {
        let mut migrated = false;
        if let Some(display) = gtk::gdk::Display::default() {
            let mut base = std::path::PathBuf::new();
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    base = dir.to_path_buf();
                }
            }
            if let Ok(ad) = std::env::var("APPDIR") {
                if !ad.is_empty() {
                    base = std::path::PathBuf::from(ad);
                }
            }

            let theme = gtk::IconTheme::for_display(&display);
            // Gömülü Adwaita öncelikli: ours-önce + set_search_path
            // (add_search_path sona eklerdi; ~/.local + /usr/share/Adwaita
            // kazanır, gemideki 58 GitHub SVG hiç seçilmezdi).
            // Sıra: önce AppImage düzeni, sonra `cargo run` kökleri.
            {
                let mut ours: Vec<std::path::PathBuf> = vec![
                    base.join("usr/share/icons"),
                    base.join("usr/share/animecix/assets/hicolor"),
                    base.join("usr/share/animecix/assets"),
                    base.join("assets/hicolor"),
                    base.join("assets"),
                    // FHS kurulumu (/usr/bin/animecix): gömülü set /usr/share/animecix altında.
                    std::path::PathBuf::from("/usr/share/animecix/assets/hicolor"),
                    std::path::PathBuf::from("/usr/share/animecix/assets"),
                ];
                // `cargo run`: exe `target/debug/` altında, assetler repo kökünde.
                if let Some(root) = base.ancestors().nth(2) {
                    ours.push(root.join("assets/hicolor"));
                    ours.push(root.join("assets"));
                }
                crate::prepend_icon_paths(&theme, &ours);
                // Kilit: tema Adwaita'ya sabitlenir. GTK önce AKTİF temayı
                // arar (breeze-dark'ta sistem kazanırdı); hicolor yedekliği
                // yetmezdi. Display singleton'unda set_theme_name yasak
                // olduğundan ayar üzerinden yapılır. Gömülü Adwaita dizini
                // search-path'in önünde olduğundan GitHub çizimleri seçilir
                // (CC-BY-SA, atıf gemide).
                gtk::Settings::for_display(&display).set_gtk_icon_theme_name(Some("Adwaita"));
            }

            let css = gtk::CssProvider::new();
            css.load_from_data(include_str!("../assets/style.css"));
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            let appimage = std::env::var("APPIMAGE").ok();
            let system_install = system_installation_present();
            if appimage_integration_blocked(appimage.as_deref(), system_install) {
                // Sistem paketi menü girdisinin sahibidir: AppImage yalnızca
                // taşınabilir çalışır. Eski sürümlerin bıraktığı kullanıcı
                // kısayolu ve sabit kopya arka planda temizlenir.
                let home = std::env::var("HOME").unwrap_or_default();
                let current = appimage.unwrap_or_default();
                if !home.is_empty() {
                    eprintln!("[STARTUP] sistem kurulumu algılandı; AppImage entegrasyonu atlandı");
                    std::thread::spawn(move || {
                        cleanup_stale_appimage_integration(&home, &current);
                    });
                }
            } else if check_desktop_entry_installed() {
                let home = std::env::var("HOME").unwrap_or_default();
                let desktop_path =
                    format!("{home}/.local/share/applications/tr.com.animecix.desktop");
                if !home.is_empty() {
                    if let Some(old_exec) = parse_desktop_exec(&desktop_path) {
                        migrated = old_exec != desktop_exec_target(&home);
                    }
                }
                // AppImage kopyası + update-desktop-database UI thread'i
                // bloklar (saniyeler); arka plana alınır.
                std::thread::spawn(|| {
                    if let Err(e) = install_desktop_entry() {
                        eprintln!("[STARTUP] kısayol kurulumu atlandı: {e}");
                    }
                });
                if migrated {
                    eprintln!("[STARTUP] kurulu kısayol yeni çalıştırılan AppImage'a taşındı");
                }
            }
        }
        let app_inst = App::new(app);
        {
            let sweep_client = app_inst.client.clone();
            std::thread::spawn(move || sweep_client.sweep_expired_covers());
        }
        let (auto_update, notify_uptodate) = {
            let s = app_inst.settings.borrow();
            (s.auto_update, s.notify_uptodate)
        };
        if migrated {
            let dlg = adw::MessageDialog::builder()
                .heading("AnimeciX Güncellendi")
                .body(format!(
                    "AnimeciX sürümünüz v{} olarak değiştirildi.",
                    update::CURRENT_VERSION
                ))
                .close_response("ok")
                .default_response("ok")
                .build();
            dlg.add_response("ok", "Tamam");
            dlg.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
            dlg.set_transient_for(Some(&app_inst.window));
            dlg.present();
        } else if auto_update && update::self_update_supported() {
            let app_c = app_inst.clone();
            update::check_and_prompt(
                &app_inst.window,
                notify_uptodate,
                move || {
                    let mut s = app_c.settings.borrow_mut();
                    s.notify_uptodate = false;
                    app_c.client.save_settings(&s);
                },
            );
        }
        app_inst.window.present();
        // Kapak wipe'ı bu açılışta yapıldıysa (bayrak tüketildi) bilgi ver:
        // silme zaten bitti, tek butonla kapatılır (zorunlu modal).
        if app_inst.client.covers_wiped_this_boot {
            let dlg = adw::MessageDialog::builder()
                .heading("Önbellek Sıfırlandı")
                .body(
                    "Kapak önbelleği yeni biçime geçirildi. \
                     Ayarların, geçmişin ve indirmelerin silinmedi.",
                )
                .close_response("ok")
                .default_response("ok")
                .build();
            dlg.add_response("ok", "Tamam");
            dlg.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
            dlg.set_transient_for(Some(&app_inst.window));
            dlg.present();
        }
        // Yenilik popup'ı açılış kalabalığına karışmasın: 2.5sn gecikmeli,
        // sürüm başına bir kez (guard maybe_show_changelog içinde).
        {
            let inst = app_inst.clone();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(2500),
                move || inst.maybe_show_changelog(),
            );
        }
    });

    let argv: Vec<&str> = filtered.iter().map(|s| s.as_str()).collect();
    app.run_with_args(&argv);
}

pub struct DepStatus {
    pub name: &'static str,
    pub desc: &'static str,
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
            desc: "Anime, dizi ve filmleri sorunsuz oynatabilmek için gereklidir.",
            installed: has_mpv,
            install_cmd: Some(format!("{mpv_cmd}   # ({distro_name})")),
        },
        DepStatus {
            name: "Sistem Medya İndirici (curl)",
            desc: "Sunuculardan hızlı veri çekmek ve görsel kapakları için gereklidir.",
            installed: has_curl,
            install_cmd: Some(format!("{pkg_cmd} curl")),
        },
        DepStatus {
            name: "Masaüstü Bağlantı Araçları (xdg-utils)",
            desc: "Masaüstü entegrasyonu ve bağlantıları açmak için gereklidir.",
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

pub fn check_and_auto_update_installation() {
    if check_desktop_entry_installed() {
        let _ = install_desktop_entry();
    }
}

pub const STABLE_APPIMAGE_NAME: &str = "AnimeciX-x86_64.AppImage";

pub fn stable_appimage_path(home: &str) -> String {
    format!("{home}/.local/bin/{STABLE_APPIMAGE_NAME}")
}

/// Paket (deb/rpm) kurulumunda sistem ikilisinin yolu.
pub const SYSTEM_BIN_PATH: &str = "/usr/bin/animecix";

/// Kısayolun çalıştıracağı yol: AppImage'de sabit kopya, paket kurulumunda
/// sistem ikilisi (/usr/bin/animecix). Eski sürümlerin ~/.local/bin/animecix'e
/// yaptığı kopya güncelleme sonrası bayat kalıp uyarı döngüsü yarattığı için
/// artık kullanılmaz.
pub fn desktop_exec_target(home: &str) -> String {
    let appimage = std::env::var("APPIMAGE").ok();
    let current = std::env::current_exe().ok();
    desktop_exec_target_for(
        appimage.as_deref(),
        home,
        std::path::Path::new(SYSTEM_BIN_PATH).is_file(),
        current.as_deref(),
    )
}

fn desktop_exec_target_for(
    appimage: Option<&str>,
    home: &str,
    system_bin_exists: bool,
    current_exe: Option<&std::path::Path>,
) -> String {
    if appimage.filter(|s| !s.trim().is_empty()).is_some() {
        return stable_appimage_path(home);
    }
    if system_bin_exists {
        return SYSTEM_BIN_PATH.to_string();
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

pub fn install_desktop_entry() -> Result<(), String> {
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
        std::env::set_var("APPIMAGE", "/opt/AnimeciX-x86_64.AppImage");
        assert_eq!(
            desktop_exec_target("/home/x"),
            "/home/x/.local/bin/AnimeciX-x86_64.AppImage"
        );
        std::env::remove_var("APPIMAGE");
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

/// Bizim ikon yollarını listenin başına al (gtk4 0.8'de `prepend_search_path`
/// yok; `search_path` + `set_search_path` ile aynı iş).
/// Var olmayan yol zararsız (yok sayılır), tekrar eklenmez.
fn prepend_icon_paths(theme: &gtk::IconTheme, ours: &[std::path::PathBuf]) {
    let mut combined: Vec<std::path::PathBuf> = ours.to_vec();
    for p in theme.search_path() {
        if !combined.contains(&p) {
            combined.push(p);
        }
    }
    let refs: Vec<&std::path::Path> = combined.iter().map(|p| p.as_path()).collect();
    theme.set_search_path(&refs);
}

fn cmd_check_icons() {
    // GTK'yı başlatsız tema okunamaz; init başarısızsa liste basılamaz.
    if gtk::init().is_err() {
        eprintln!("--check-icons: gtk::init başarısız (displaysiz ortam?)");
        std::process::exit(2);
    }
    let Some(display) = gtk::gdk::Display::default() else {
        eprintln!("--check-icons: Display yok");
        std::process::exit(2);
    };
    // Gerçek çalışma düzeniyle aynı yollar (connect_activate ile eşleşmeli).
    let mut base = std::path::PathBuf::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            base = dir.to_path_buf();
        }
    }
    if let Ok(ad) = std::env::var("APPDIR") {
        if !ad.is_empty() {
            base = std::path::PathBuf::from(ad);
        }
    }
    let theme = gtk::IconTheme::for_display(&display);
    {
        let mut ours: Vec<std::path::PathBuf> = vec![
            base.join("usr/share/icons"),
            base.join("usr/share/animecix/assets/hicolor"),
            base.join("usr/share/animecix/assets"),
            base.join("assets/hicolor"),
            base.join("assets"),
            // FHS kurulumu (/usr/bin/animecix): gömülü set /usr/share/animecix altında.
            std::path::PathBuf::from("/usr/share/animecix/assets/hicolor"),
            std::path::PathBuf::from("/usr/share/animecix/assets"),
        ];
        if let Some(root) = base.ancestors().nth(2) {
            ours.push(root.join("assets/hicolor"));
            ours.push(root.join("assets"));
        }
        prepend_icon_paths(&theme, &ours);
        gtk::Settings::for_display(&display).set_gtk_icon_theme_name(Some("Adwaita"));
    }
    println!("search_path={:?}", theme.search_path());
    println!("theme_name={:?}", theme.theme_name());
    // Kodda geçen kritik set + bilerek-eksik bir ad (fallback kanıtı).
    let names = [
        "go-home-symbolic",
        "view-grid-symbolic",
        "starred-symbolic",
        "non-starred-symbolic",
        "media-playlist-consecutive-symbolic",
        "document-open-recent-symbolic",
        "x-office-calendar-symbolic",
        "view-list-symbolic",
        "folder-download-symbolic",
        "folder-open-symbolic",
        "emblem-system-symbolic",
        "system-search-symbolic",
        "go-previous-symbolic",
        "go-next-symbolic",
        "media-playback-start-symbolic",
        "media-playback-pause-symbolic",
        "bu-ad-yok-symbolic",
    ];
    let mut ok = 0u32;
    let mut fail = 0u32;
    for n in &names {
        // Önce hafif yoklama: lookup_icon(bulunamayan) bazı GTK
        // zincirlerinde son-çare image-missing aramasını döndürüp
        // taşırdığından doğrudan lookup'a girilmez.
        if !theme.has_icon(n) {
            if *n == "bu-ad-yok-symbolic" {
                ok += 1;
                println!("  [ABSENT-OK] {n} (beklendiği gibi yok)");
            } else {
                fail += 1;
                println!("  [MISSING] {n}");
            }
            continue;
        }
        let found = theme.lookup_icon(
            n,
            &[],
            48,
            1,
            gtk::TextDirection::None,
            gtk::IconLookupFlags::empty(),
        );
        match found.file().and_then(|f| f.path()) {
            Some(p) => {
                let pstr = p.to_string_lossy();
                let ours = pstr.contains("appimage_extracted")
                    || pstr.contains("animecix")
                    || pstr.contains("assets/hicolor");
                let tag = if ours { "OURS" } else { "SYSTEM" };
                if *n == "bu-ad-yok-symbolic" {
                    fail += 1;
                    println!("  [UNEXPECTED] {n} -> {tag} {}", p.display());
                } else {
                    // OURS veya SYSTEM fark etmez: ikon çözümleniyor, arayüz çalışır.
                    // (Aktif tema Adwaita ikonu bulunca gömülü hicolor yedeğine
                    // bakmaz; SYSTEM de geçerli sonuçtur.)
                    ok += 1;
                    println!("  [{tag:6}] {n} -> {}", p.display());
                }
            }
            None => {
                if *n == "bu-ad-yok-symbolic" {
                    ok += 1;
                    println!("  [ABSENT-OK] {n} (beklendiği gibi yok)");
                } else {
                    fail += 1;
                    println!("  [MISSING] {n}");
                }
            }
        }
    }
    println!("Sonuç: {ok} OK, {fail} eksik");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn cmd_check_shaders() {
    let shaders = [
        ("Anime4K_Upscale_DTD_x2.glsl", "hafif / hafif_keskin"),
        ("Anime4K_Upscale_CNN_x2_UL.glsl", "ultra"),
        ("Anime4K_Upscale_CNN_x2_M.glsl", "(yedek)"),
        ("Anime4K_Upscale_Original_x2.glsl", "(yedek)"),
    ];
    println!("AnimeciX upscale shader doğrulaması");
    println!("===================================");
    let mut ok = 0u32;
    let mut fail = 0u32;
    for (name, mode) in &shaders {
        match app::resolve_upscale_shader(name) {
            Some(p) => {
                let exists = std::path::Path::new(&p).exists();
                let size = if exists {
                    std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                };
                let head = if exists {
                    std::fs::read_to_string(&p)
                        .ok()
                        .and_then(|s| s.lines().next().map(|l| l.to_string()))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                let status = if exists && size > 1000 {
                    "OK"
                } else {
                    "MISSING/EMPTY"
                };
                if status == "OK" {
                    ok += 1;
                } else {
                    fail += 1;
                }
                println!("  [{status:5}] {name:38} mode={mode:22} size={size:7}B  head={head:.60}");
                println!("           path: {p}");
            }
            None => {
                fail += 1;
                println!("  [NONE ] {name} (gömülü içerik bulunamadı)");
            }
        }
    }
    println!("===================================");
    println!("Sonuç: {ok} OK, {fail} hata");
    if fail > 0 {
        std::process::exit(1);
    }
}
