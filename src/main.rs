mod api;
mod app;
mod aria;
mod covers;
mod download;
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

use app::App;
use adw::prelude::*;
use std::process::Command;

const ANIMECIX_PNG_ICON: &[u8] = include_bytes!("../assets/hicolor/256x256/apps/tr.com.animecix.png");

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
        let _ = std::process::Command::new(exe).spawn();
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
    let light_mode = api::Client::new().load_settings().light_mode;
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
            theme.add_search_path(base.join("assets"));
            theme.add_search_path(base.join("usr/share/icons"));

            let css = gtk::CssProvider::new();
            css.load_from_data(
                r#"/* === FlowBoxChild Temizliği (Çift arka plan engelleme) === */
                flowboxchild {
                    padding: 0;
                    margin: 0;
                    background: none;
                    border: none;
                    outline: none;
                    box-shadow: none;
                }
                flowboxchild:hover, flowboxchild:selected, flowboxchild:focus {
                    background: none;
                    border: none;
                    outline: none;
                    box-shadow: none;
                }

                /* === Kapak Fotoğrafları ve Kesin Boyut Sınırlamaları === */
                .cover {
                    background-color: alpha(currentColor, 0.07);
                    border-radius: 10px;
                    transition: transform 150ms ease, box-shadow 150ms ease;
                }

                /* === Kart Hover: parlama + hafif büyüme === */
                .title-btn:hover {
                    cursor: pointer;
                }
                .title-btn:hover .cover {
                    transform: scale(1.04);
                    box-shadow: 0 0 22px 3px alpha(@accent_color, 0.55);
                }

                .cover-thumb {
                    min-width: 48px;
                    max-width: 48px;
                    min-height: 72px;
                    max-height: 72px;
                }

                .cover-header {
                    min-width: 120px;
                    max-width: 120px;
                    min-height: 180px;
                    max-height: 180px;
                }

                .cover-movie-header {
                    min-width: 160px;
                    max-width: 160px;
                    min-height: 240px;
                    max-height: 240px;
                }

                .cover-shelf {
                    min-width: 140px;
                    max-width: 140px;
                    min-height: 210px;
                    max-height: 210px;
                }

                /* === Kart Hover Animasyonu === */
                .title-btn {
                    padding: 4px;
                    border-radius: 12px;
                    transition: transform 140ms ease, background-color 140ms ease;
                }
                .title-btn:hover {
                    transform: scale(1.04);
                    background-color: alpha(currentColor, 0.08);
                }
                .title-btn:active {
                    transform: scale(0.97);
                }

                /* === Dizi Detay Kartı === */
                .title-detail-card {
                    background-color: alpha(currentColor, 0.04);
                    border: 1px solid alpha(currentColor, 0.08);
                    border-radius: 14px;
                    padding: 14px;
                    transition: background-color 150ms ease;
                }

                /* === Detay görünümü bilgi rozetleri (süre / bölüm / yayın) === */
                .detail-badge {
                    background-color: alpha(currentColor, 0.08);
                    border: 1px solid alpha(currentColor, 0.12);
                    border-radius: 999px;
                    padding: 3px 12px;
                    color: alpha(currentColor, 0.75);
                    font-size: 0.85em;
                }

                /* === Tek Seferlik İpucu Kartı === */
                .tip-banner {
                    background-color: alpha(@accent_color, 0.1);
                    border: 1px solid alpha(@accent_color, 0.28);
                    border-radius: 10px;
                    padding: 10px 14px;
                    margin: 0 12px 8px 12px;
                }
                .tip-banner-text {
                    font-size: 0.9em;
                }

                /* === Kart Başlık Yazısı === */
                .card-title {
                    font-weight: 600;
                    font-size: 0.85em;
                    text-align: center;
                    margin-top: 2px;
                }

                /* === Raf Başlıkları: büyük + nefes alan parlaklık === */
                @keyframes shelf-breathe {
                    0%, 100% { opacity: 0.72; }
                    50% { opacity: 1.0; }
                }
                .shelf-title {
                    font-size: 1.02em;
                    font-weight: 800;
                    letter-spacing: 0.1em;
                    text-transform: uppercase;
                    color: @accent_color;
                    opacity: 0.9;
                    text-shadow: 0 0 14px alpha(@accent_color, 0.35);
                    animation-name: shelf-breathe;
                    animation-duration: 3.2s;
                    animation-timing-function: ease-in-out;
                    animation-iteration-count: infinite;
                }

                /* === Koyu temalarda sabit okunaklı raf başlığı (temaya sadık değil) === */
                .theme-dark .shelf-title {
                    color: #FFFFFF;
                    text-shadow: 0 1px 4px alpha(black, 0.85);
                }

                /* === Arayüz Ölçeği (Ayarlar > Görünüm) === */
                .ui-scale-125 { font-size: 20px; }
                .ui-scale-150 { font-size: 24px; }

                /* === Dikey Film Sayfası === */
                .movie-big {
                    border-radius: 18px;
                }
                .movie-tint {
                    border-radius: 18px;
                    transition: background 1s ease;
                }
                .movie-play-btn { font-size: 1.1em; padding: 12px 36px; font-weight: 700; }

                /* === Bookmark Butonu (eski floating) === */
                .lg-icon { -gtk-icon-size: 26px; }
                .bookmark-btn {
                    background-color: alpha(black, 0.55);
                    backdrop-filter: blur(8px);
                    border-radius: 20px;
                    transition: background-color 120ms, transform 120ms;
                }
                .bookmark-btn:hover {
                    background-color: alpha(black, 0.75);
                    transform: scale(1.1);
                }

                /* === Inline Bookmark Butonu (Bölüm sayfası kart içi) === */
                .inline-bookmark-btn {
                    transition: transform 120ms, color 120ms;
                }
                .inline-bookmark-btn:hover {
                    transform: scale(1.15);
                }

                /* === Bölüm Listesi Paneli: oluklar panelde erir, siyah çerçeve biter === */
                /* NOT: GtkListBox'un CSS düğümü `list`tir (`listbox` ölü kural olur). */
                list.content-list {
                    background-color: alpha(@row_tint, 0.04);
                    border: none;
                    box-shadow: none;
                    border-radius: 14px;
                    padding: 6px 8px;
                }
                list.content-list > separator {
                    background-color: transparent;
                    opacity: 0;
                }
                .content-list > row {
                    background-color: alpha(@row_tint, 0.10);
                    border: none;
                    outline: none;
                    box-shadow: none;
                    border-radius: 10px;
                    margin: 3px 0;
                    transition: background-color 120ms ease;
                }
                .content-list > row:hover {
                    background-color: alpha(@row_tint, 0.17);
                }
                .content-list > row:focus-visible {
                    background-color: alpha(@row_tint, 0.22);
                    outline: none;
                }
                .content-list > row:selected,
                .content-list > row:selected:focus {
                    background-color: alpha(@row_tint, 0.28);
                    outline: none;
                    box-shadow: none;
                }

                /* === Şeffaf kaydırma: viewport dahil tema görünür === */
                .clear-scroll,
                .clear-scroll viewport,
                .clear-scroll list:not(.content-list),
                .clear-scroll list:not(.content-list) > row,
                .clear-scroll flowbox {
                    background-color: transparent;
                    background-image: none;
                    border: none;
                    box-shadow: none;
                }

                /* === Yüzen indirme hapı: kapsül kabı, düğmeleri sarar === */
                .dl-float-pill {
                    background-color: alpha(@card_bg_color, 0.95);
                    color: @card_fg_color;
                    border: 1px solid alpha(currentColor, 0.12);
                    border-radius: 9999px;
                    padding: 6px;
                    box-shadow: 0 4px 18px alpha(black, 0.45);
                }
                .dl-float-pill button.pill {
                    margin: 0;
                }

                /* === Bölüm kaydırıcısı: kenar gölgesi şeridi + hap altı boşluk === */
                scrolledwindow.clear-scroll undershoot.top,
                scrolledwindow.clear-scroll undershoot.bottom,
                scrolledwindow.clear-scroll overshoot.top,
                scrolledwindow.clear-scroll overshoot.bottom {
                    background: none;
                    background-image: none;
                    box-shadow: none;
                    border: none;
                }
                scrolledwindow.clear-scroll scrollbar {
                    background: transparent;
                    border: none;
                }
                scrolledwindow.clear-scroll viewport {
                    padding-bottom: 72px;
                }

                /* === İzleme Maratonu Zengin Tasarım Stilleri === */
                list.marathon-list-box {
                    background: transparent;
                }
                list.marathon-list-box > row {
                    background: transparent;
                    border: none;
                    padding: 0;
                    margin: 0;
                    box-shadow: none;
                }
                list.marathon-list-box > row:hover,
                list.marathon-list-box > row:selected,
                list.marathon-list-box > row:focus {
                    background: transparent;
                }

                .marathon-summary-card {
                    background-color: alpha(currentColor, 0.04);
                    border: 1px solid alpha(@accent_color, 0.25);
                    border-radius: 16px;
                    padding: 16px 20px;
                    margin-bottom: 14px;
                }

                .marathon-percent-pill {
                    background-color: alpha(@accent_color, 0.18);
                    color: @accent_color;
                    border: 1px solid alpha(@accent_color, 0.35);
                    border-radius: 14px;
                    padding: 4px 12px;
                    font-weight: 700;
                    font-size: 0.9em;
                }

                .marathon-item-card {
                    background-color: alpha(currentColor, 0.03);
                    border: 1px solid alpha(currentColor, 0.07);
                    border-radius: 14px;
                    padding: 12px 16px;
                    margin-bottom: 8px;
                    cursor: grab;
                    transition: background-color 140ms ease, border-color 140ms ease;
                }
                .marathon-item-card:hover {
                    background-color: alpha(currentColor, 0.06);
                    border-color: alpha(@accent_color, 0.3);
                }
                .marathon-item-card:active {
                    cursor: grabbing;
                }
                .marathon-item-card:drop(active) {
                    border-color: @accent_color;
                    background-color: alpha(@accent_color, 0.12);
                }

                .fav-item-card {
                    background-color: alpha(currentColor, 0.03);
                    border: 1px solid alpha(currentColor, 0.07);
                    border-radius: 14px;
                    padding: 12px 16px;
                    margin-bottom: 8px;
                    transition: background-color 140ms ease, border-color 140ms ease;
                }
                .fav-item-card:hover {
                    background-color: alpha(currentColor, 0.06);
                    border-color: alpha(@accent_color, 0.3);
                }

                .marathon-index {
                    min-width: 26px;
                    min-height: 26px;
                    padding: 2px 8px;
                    color: @accent_color;
                    background-color: alpha(@accent_color, 0.12);
                    border: 1px solid alpha(@accent_color, 0.45);
                    border-radius: 9px;
                    font-weight: 700;
                    font-size: 13px;
                }

                .history-item-card {
                    background-color: alpha(currentColor, 0.035);
                    border: 1px solid alpha(currentColor, 0.08);
                    border-radius: 12px;
                    padding: 5px 12px;
                    margin-bottom: 5px;
                    transition: background-color 140ms ease, border-color 140ms ease;
                }
                .history-item-card:hover {
                    background-color: alpha(currentColor, 0.07);
                    border-color: alpha(@accent_color, 0.35);
                }

                .status-badge-completed {
                    background-color: alpha(#2ec27e, 0.18);
                    color: #2ec27e;
                    border: 1px solid alpha(#2ec27e, 0.4);
                    border-radius: 12px;
                    padding: 2px 10px;
                    font-size: 0.82em;
                    font-weight: 600;
                }

                .status-badge-progress {
                    background-color: alpha(@accent_color, 0.22);
                    color: @accent_color;
                    border: 1px solid alpha(@accent_color, 0.6);
                    border-radius: 12px;
                    padding: 2px 10px;
                    font-size: 0.82em;
                    font-weight: 600;
                }

                /* === Durum Renkleri === */
                .success { color: #2ec27e; font-weight: bold; }
                .error   { color: #e01b24; font-weight: bold; }

                /* === Bölüm Progress Bar === */
                .episode-progress {
                    min-height: 6px;
                    border-radius: 3px;
                }
                .episode-progress trough {
                    border-radius: 3px;
                    min-height: 6px;
                    background-color: alpha(currentColor, 0.12);
                }
                .episode-progress progress {
                    border-radius: 3px;
                    background-color: @accent_color;
                }"#,
            );
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            if check_desktop_entry_installed() {
                let home = std::env::var("HOME").unwrap_or_default();
                let desktop_path =
                    format!("{home}/.local/share/applications/tr.com.animecix.desktop");
                if !home.is_empty() {
                    if let Some(old_exec) = parse_desktop_exec(&desktop_path) {
                        migrated = old_exec != desktop_exec_target(&home);
                    }
                }
                let _ = install_desktop_entry();
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
        } else if auto_update && update::is_appimage() {
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
    let p1 = std::path::Path::new(&format!("{home}/.local/share/applications/tr.com.animecix.desktop")).exists();
    let p2 = std::path::Path::new(&format!("{home}/.local/share/applications/animecix.desktop")).exists();
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

pub fn desktop_exec_target(home: &str) -> String {
    if std::env::var("APPIMAGE").ok().filter(|s| !s.trim().is_empty()).is_some() {
        stable_appimage_path(home)
    } else {
        format!("{home}/.local/bin/animecix")
    }
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
    if let Some(ai) = std::env::var("APPIMAGE").ok().filter(|s| !s.trim().is_empty()) {
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
        let bin_dir = format!("{home}/.local/bin");
        let _ = std::fs::create_dir_all(&bin_dir);
        if let Ok(exe_path) = std::env::current_exe() {
            if let Ok(bytes) = std::fs::read(&exe_path) {
                let _ = std::fs::write(&exec_target, &bytes);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&exec_target, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
    }

    let apps_dir = format!("{home}/.local/share/applications");
    let icons_dir = format!("{home}/.local/share/icons/hicolor/256x256/apps");
    let icons_dir_simple = format!("{home}/.local/share/icons");

    let _ = std::fs::create_dir_all(&apps_dir);
    let _ = std::fs::create_dir_all(&icons_dir);
    let _ = std::fs::create_dir_all(&icons_dir_simple);

    let target_icon = format!("{icons_dir}/tr.com.animecix.png");
    let target_icon_simple = format!("{icons_dir_simple}/tr.com.animecix.png");
    let target_icon_lg = format!("{home}/.local/share/icons/hicolor/512x512/apps/tr.com.animecix.png");
    let _ = std::fs::create_dir_all(format!("{home}/.local/share/icons/hicolor/512x512/apps"));
    let target_icon_lg_simple = format!("{icons_dir_simple}/tr.com.animecix.svg");
    let _ = std::fs::write(&target_icon_lg_simple, ANIMECIX_SVG_ICON.as_bytes());

    let icon_bytes: Option<Vec<u8>> = None
        .or_else(|| {
            let p = std::path::Path::new("assets/hicolor/256x256/apps/tr.com.animecix.png");
            std::fs::read(p).ok()
        })
        .or_else(|| {
            let p = std::path::Path::new("/usr/share/icons/hicolor/256x256/apps/tr.com.animecix.png");
            std::fs::read(p).ok()
        })
        .or_else(|| {
            if let Ok(ai) = std::env::var("APPIMAGE") {
                if !ai.trim().is_empty() {
                    let p = std::path::Path::new(&ai).parent()
                        .map(|d| d.join("assets/hicolor/256x256/apps/tr.com.animecix.png"));
                    if let Some(pp) = p { return std::fs::read(pp).ok(); }
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
    if home.is_empty() { return; }

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
        if content_lower.contains("ubuntu") || content_lower.contains("debian") || content_lower.contains("mint") {
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
        if content_lower.contains("ubuntu") || content_lower.contains("debian") || content_lower.contains("mint") {
            return ("Ubuntu / Debian / Mint", "sudo apt update && sudo apt install mpv");
        } else if content_lower.contains("fedora") {
            return ("Fedora", "sudo dnf install mpv");
        } else if content_lower.contains("arch") || content_lower.contains("manjaro") {
            return ("Arch / Manjaro", "sudo pacman -S mpv");
        } else if content_lower.contains("suse") {
            return ("openSUSE", "sudo zypper install mpv");
        }
    }
    ("Linux", "sudo apt install mpv   # ya da dnf/pacman/zypper install mpv")
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
    fn desktop_exec_target_falls_back_to_local_bin() {
        std::env::remove_var("APPIMAGE");
        assert_eq!(
            desktop_exec_target("/home/x"),
            "/home/x/.local/bin/animecix"
        );
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
                let status = if exists && size > 1000 { "OK" } else { "MISSING/EMPTY" };
                if status == "OK" { ok += 1; } else { fail += 1; }
                println!(
                    "  [{status:5}] {name:38} mode={mode:22} size={size:7}B  head={head:.60}"
                );
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