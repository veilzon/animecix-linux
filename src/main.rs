mod api;
mod app;
mod aria;
mod auth;
mod covers;
mod desktop;
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

pub(crate) use desktop::{
    appimage_integration_blocked, appimage_integration_disabled, check_all_dependencies,
    check_desktop_entry_installed, cleanup_stale_appimage_integration, desktop_exec_target,
    install_desktop_entry, parse_desktop_exec, system_installation_present, uninstall_application,
};

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
            update::check_and_prompt(&app_inst.window, notify_uptodate, move || {
                let mut s = app_c.settings.borrow_mut();
                s.notify_uptodate = false;
                app_c.client.save_settings(&s);
            });
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
            glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || {
                inst.maybe_show_changelog()
            });
        }
    });

    let argv: Vec<&str> = filtered.iter().map(|s| s.as_str()).collect();
    app.run_with_args(&argv);
}

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
