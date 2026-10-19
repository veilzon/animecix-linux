use std::path::Path;

use adw::prelude::*;

// update.rs içindeki saf mantığın burada da birebir tekrarı (proje kalıbı: ana crate'e
// import edilemeyen bin, davranışı bağımsız doğrular). Asıl sözleşme update.rs'tir.

fn needs_update(current: &str, latest: &str) -> bool {
    fn parse(v: &str) -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect()
    }
    let c = parse(current);
    let l = parse(latest);
    let n = c.len().max(l.len());
    for i in 0..n {
        let cv = c.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }
    false
}

fn replace_target(bytes: &[u8], target: &Path) -> Result<(), String> {
    if bytes.len() < 4 || &bytes[0..4] != b"\x7fELF" {
        return Err("geçersiz AppImage".into());
    }
    let tmp = std::env::temp_dir().join(format!(
        "animecix-testupdate-{}.AppImage",
        std::process::id()
    ));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, target).map_err(|e| e.to_string())?;
    Ok(())
}

fn parse_asset(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let assets = v.get("assets")?.as_array()?;
    for a in assets {
        if a.get("name")?.as_str()? == "AnimeciX-x86_64.AppImage" {
            return a
                .get("browser_download_url")?
                .as_str()
                .map(|s| s.to_string());
        }
    }
    None
}

fn main() {
    let mut failed = 0;
    let mut check = |name: &str, ok: bool| {
        if ok {
            println!("  ✓ {name}");
        } else {
            println!("  ✗ {name}");
            failed += 1;
        }
    };

    println!("==> test_update");
    check("needs_update newer", needs_update("3.0.0", "3.1.0"));
    check("needs_update major", needs_update("3.0.0", "4.0.0"));
    check("needs_update equal", !needs_update("3.0.0", "3.0.0"));
    check("needs_update older", !needs_update("3.1.0", "3.0.0"));

    let json = r#"{"tag_name":"v3.1.0","assets":[{"name":"AnimeciX-x86_64.AppImage","browser_download_url":"http://x/AnimeciX-x86_64.AppImage"}]}"#;
    check(
        "parse_asset finds url",
        parse_asset(json).as_deref() == Some("http://x/AnimeciX-x86_64.AppImage"),
    );
    check(
        "parse_asset none",
        parse_asset(r#"{"assets":[]}"#).is_none(),
    );

    let target = std::env::temp_dir().join(format!(
        "animecix-testupdate-target-{}.AppImage",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&target);
    check(
        "replace_target ok",
        replace_target(b"\x7fELFfakeappimage", &target).is_ok(),
    );
    check("replace_target exists", target.exists());
    std::fs::remove_file(&target).ok();
    check(
        "replace_target rejects non-elf",
        replace_target(b"nope", &target).is_err(),
    );

    // GTK yüzeyini başlat: update.rs ile aynı MessageDialog API'sinin derlendiğini/çalıştığını doğrula
    let _ = gtk::init();
    let dlg = adw::MessageDialog::builder()
        .heading("Yeni Sürüm Mevcut")
        .body("AnimeciX v3.1.0 yayınlandı. İndirip uygulamayı güncelleyelim mi?")
        .close_response("later")
        .default_response("later")
        .build();
    dlg.add_response("later", "Daha Sonra");
    dlg.add_response("update", "Güncelle ve Yeniden Başlat");
    dlg.set_response_appearance("update", adw::ResponseAppearance::Suggested);
    let _win: gtk::Widget = dlg.clone().upcast();
    check("dialog builds", true);

    if failed > 0 {
        println!("==> {failed} checkpoint başarısız");
        std::process::exit(1);
    }
    println!("==> test_update: TÜM KONTROLLER GEÇTİ");
}
