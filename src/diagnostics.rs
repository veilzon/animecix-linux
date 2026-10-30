use crate::icons;
use gtk::prelude::*;

pub(crate) fn cmd_check_icons() {
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
        icons::prepend_icon_paths(&theme, &ours);
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

pub(crate) fn cmd_check_shaders() {
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
        match crate::app::resolve_upscale_shader(name) {
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
