use std::path::Path;
use futures::StreamExt;
use std::sync::mpsc::channel;

use glib::ControlFlow;
use gtk::prelude::*;
use adw::prelude::*;
use serde::Deserialize;

const UPDATE_REPO: &str = "veilzon/animecix-linux";
const ASSET_NAME: &str = "AnimeciX-x86_64.AppImage";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

pub fn is_appimage() -> bool {
    std::env::var("APPIMAGE").map(|v| !v.trim().is_empty()).unwrap_or(false)
}

pub fn needs_update(current: &str, latest: &str) -> bool {
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

fn http_client() -> Result<reqwest::blocking::Client, String> {
    Ok(crate::api::shared_blocking_client())
}

fn latest_release() -> Result<GithubRelease, String> {
    let url = format!("https://api.github.com/repos/{UPDATE_REPO}/releases/latest");
    let client = http_client()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "animecix-updater")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API hatası: {}", resp.status()));
    }
    resp.json().map_err(|e| e.to_string())
}

pub fn replace_target(bytes: &[u8], target: &Path) -> Result<(), String> {
    if bytes.len() < 4 || &bytes[0..4] != b"\x7fELF" {
        return Err("İndirilen dosya geçerli bir AppImage değil".into());
    }
    let tmp = std::env::temp_dir()
        .join(format!("AnimeciX-update-{}.AppImage", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp, perms).map_err(|e| e.to_string())?;
    }
    match std::fs::rename(&tmp, target) {
        Ok(_) => return Ok(()),
        Err(_) => {}
    }
    let backup = target.with_extension("AppImage.old");
    if backup.exists() {
        let _ = std::fs::remove_file(&backup);
    }
    if std::fs::rename(target, &backup).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "Güncelleme dosyası '{}' yoluna yazılamadı (farklı aygıt veya salt okunur). Eski dosya yedeklenemedi.",
            target.display()
        ));
    }
    if let Err(e) = std::fs::copy(&tmp, target) {
        let _ = std::fs::rename(&backup, target);
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Güncelleme yazılamadı: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(target) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(target, perms);
        }
    }
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

pub fn download_update<F>(url: &str, target: &Path, mut on_progress: F) -> Result<(), String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let bytes = rt.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent("animecix-updater")
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("İndirme hatası: {}", resp.status()));
        }
        let total = resp.content_length().unwrap_or(0);
        let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            buf.extend_from_slice(&chunk);
            on_progress(buf.len() as u64, total);
        }
        Ok::<Vec<u8>, String>(buf)
    })?;
    replace_target(&bytes, target)
}

pub fn check_and_prompt<W: IsA<gtk::Window> + Clone + 'static>(
    window: &W,
    show_if_uptodate: bool,
    on_suppress_uptodate: impl Fn() + 'static,
) {
    if !is_appimage() {
        if show_if_uptodate {
            present_info(window, "Güncelleme Kullanılamıyor", "Otomatik güncelleme yalnızca AppImage sürümünde çalışır.");
        }
        return;
    }
    let (tx, rx) = channel::<Result<Option<(String, String)>, String>>();
    std::thread::spawn(move || {
        let res = (|| -> Result<Option<(String, String)>, String> {
            let rel = latest_release()?;
            if !needs_update(CURRENT_VERSION, &rel.tag_name) {
                return Ok(None);
            }
            let url = rel
                .assets
                .iter()
                .find(|a| a.name == ASSET_NAME)
                .map(|a| a.browser_download_url.clone())
                .ok_or_else(|| "asset bulunamadı".to_string())?;
            Ok(Some((rel.tag_name, url)))
        })();
        let _ = tx.send(res);
    });

    let win = window.clone();
    let mut on_suppress = Some(on_suppress_uptodate);
    glib::idle_add_local(move || match rx.try_recv() {
        Ok(Ok(Some((tag, url)))) => {
            present_update_dialog(win.clone(), &tag, &url);
            ControlFlow::Break
        }
        Ok(Ok(None)) => {
            if show_if_uptodate {
                if let Some(f) = on_suppress.take() {
                    present_uptodate(&win, f);
                }
            }
            ControlFlow::Break
        }
        Ok(Err(e)) => {
            eprintln!("Güncelleme kontrolü başarısız: {e}");
            ControlFlow::Break
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => ControlFlow::Break,
    });
}

fn present_info<W: IsA<gtk::Window>>(window: &W, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.set_transient_for(Some(window));
    dialog.present();
}

fn present_uptodate<W: IsA<gtk::Window>>(window: &W, on_suppress: impl Fn() + 'static) {
    let dialog = adw::MessageDialog::builder()
        .heading("Güncel")
        .body("AnimeciX güncel sürümde 🎉")
        .close_response("ok")
        .default_response("ok")
        .build();
    dialog.add_response("suppress", "Bir Daha Gösterme");
    dialog.add_response("ok", "Tamam");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_transient_for(Some(window));
    dialog.connect_response(None, move |dlg, resp| {
        if resp == "suppress" {
            on_suppress();
        }
        dlg.close();
    });
    dialog.present();
}

#[derive(Clone)]
enum UpdMsg {
    Progress(u64, u64),
    Step(String),
    Done,
    Error(String),
}

fn present_update_dialog<W: IsA<gtk::Window> + Clone + 'static>(window: W, tag: &str, url: &str) {
    let tag = tag.trim_start_matches('v').to_string();
    let url = url.to_string();
    let target = std::env::var("APPIMAGE").unwrap_or_default();
    if target.is_empty() {
        present_info(&window, "Güncelleme Başarısız", "AppImage yolu bulunamadı.");
        return;
    }
    let dialog = adw::MessageDialog::builder()
        .heading("Yeni Sürüm Mevcut")
        .body(format!(
            "AnimeciX v{tag} yayınlandı. İndirip uygulamayı güncelleyelim mi?"
        ))
        .close_response("later")
        .default_response("later")
        .build();
    dialog.add_response("later", "Daha Sonra");
    dialog.add_response("update", "Güncelle ve Yeniden Başlat");
    dialog.set_response_appearance("update", adw::ResponseAppearance::Suggested);
    dialog.set_transient_for(Some(&window));
    dialog.connect_response(None, move |dlg, resp| {
        if resp == "update" {
            dlg.close();
            run_update_with_progress(window.clone(), &url, &target);
        }
    });
    dialog.present();
}

fn run_update_with_progress<W: IsA<gtk::Window> + Clone + 'static>(window: W, url: &str, target: &str) {
    let dlg = adw::Window::builder()
        .title("Güncelleniyor")
        .modal(true)
        .transient_for(&window)
        .default_width(380)
        .build();
    dlg.set_deletable(false);
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 12);
    box_.set_margin_top(18);
    box_.set_margin_bottom(18);
    box_.set_margin_start(18);
    box_.set_margin_end(18);
    let label = gtk::Label::new(Some("Güncelleme başlatılıyor…"));
    let bar = gtk::ProgressBar::new();
    bar.set_show_text(true);
    bar.set_text(Some("0%"));
    bar.set_fraction(0.0);
    box_.append(&label);
    box_.append(&bar);
    dlg.set_content(Some(&box_));
    dlg.present();

    let url = url.to_string();
    let target = target.to_string();
    start_update_worker(&dlg, &label, &bar, &window, &url, &target);
}

fn start_update_worker<W: IsA<gtk::Window> + Clone + 'static>(
    dlg: &adw::Window,
    label: &gtk::Label,
    bar: &gtk::ProgressBar,
    window: &W,
    url: &str,
    target: &str,
) {
    let (tx, rx) = channel::<UpdMsg>();
    let prog_tx = tx.clone();
    let url_s = url.to_string();
    let target_path = std::path::PathBuf::from(target);
    std::thread::spawn(move || {
        let _ = tx.send(UpdMsg::Step("İndiriliyor…".into()));
        let result = download_update(&url_s, &target_path, move |cur, total| {
            let _ = prog_tx.send(UpdMsg::Progress(cur, total));
        });
        match result {
            Ok(()) => {
                let _ = tx.send(UpdMsg::Step("Doğrulanıyor ve yükleniyor…".into()));
                let _ = tx.send(UpdMsg::Done);
            }
            Err(e) => {
                let _ = tx.send(UpdMsg::Error(e));
            }
        }
    });

    let dlg = dlg.clone();
    let label = label.clone();
    let bar = bar.clone();
    let window = window.clone();
    let url_c = url.to_string();
    let target_c = target.to_string();
    glib::idle_add_local(move || {
        match rx.try_recv() {
            Ok(UpdMsg::Progress(cur, total)) => {
                if total > 0 {
                    let f = (cur as f64 / total as f64).clamp(0.0, 1.0);
                    bar.set_fraction(f);
                    bar.set_text(Some(&format!("{}%", (f * 100.0) as u32)));
                } else {
                    bar.pulse();
                    bar.set_text(Some(&format!("{} KB", cur / 1024)));
                }
                ControlFlow::Continue
            }
            Ok(UpdMsg::Step(s)) => {
                label.set_text(&s);
                ControlFlow::Continue
            }
            Ok(UpdMsg::Done) => {
                label.set_text("Yeniden başlatılıyor…");
                bar.set_fraction(1.0);
                bar.set_text(Some("100%"));
                let _ = std::process::Command::new(&target_c).spawn();
                std::process::exit(0);
            }
            Ok(UpdMsg::Error(e)) => {
                label.set_text(&format!("Güncelleme başarısız: {e}"));
                bar.set_fraction(0.0);
                dlg.set_deletable(true);
                if let Some(b) = dlg.content().and_then(|w| w.downcast::<gtk::Box>().ok()) {
                    let retry = gtk::Button::with_label("Tekrar Dene");
                    retry.set_margin_top(8);
                    let dlg2 = dlg.clone();
                    let win2 = window.clone();
                    let url2 = url_c.clone();
                    let target2 = target_c.clone();
                    retry.connect_clicked(move |_| {
                        dlg2.close();
                        run_update_with_progress(win2.clone(), &url2, &target2);
                    });
                    b.append(&retry);

                    let ok = gtk::Button::with_label("Tamam");
                    ok.set_margin_top(6);
                    let dlg3 = dlg.clone();
                    ok.connect_clicked(move |_| {
                        dlg3.close();
                    });
                    b.append(&ok);
                    ok.grab_focus();
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => ControlFlow::Break,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(needs_update("3.0.0", "3.1.0"));
        assert!(needs_update("3.0.0", "4.0.0"));
        assert!(needs_update("3.0.9", "3.1.0"));
        assert!(!needs_update("3.1.0", "3.0.0"));
        assert!(!needs_update("3.0.0", "3.0.0"));
        assert!(!needs_update("v3.0.0", "3.0.0"));
        assert!(needs_update("3.0.0", "v3.0.1"));
    }

    #[test]
    fn replace_writes_and_renames() {
        let tmp = std::env::temp_dir().join(format!("animecix-replace-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let elf = b"\x7fELFfakeappimage";
        replace_target(elf, &tmp).unwrap();
        assert!(tmp.exists());
        let content = std::fs::read(&tmp).unwrap();
        assert_eq!(&content[0..4], b"\x7fELF");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn replace_rejects_non_elf() {
        let tmp = std::env::temp_dir().join(format!("animecix-replace-bad-{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        assert!(replace_target(b"not an appimage", &tmp).is_err());
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn download_update_writes_target_and_reports_progress() {
        use std::io::{Read, Write};
        let elf = b"\x7fELF-appimage-govde-icerigi-burada-1234567890";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut s = stream.unwrap();
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf); // istemci isteğini oku
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    elf.len()
                );
                s.write_all(resp.as_bytes()).unwrap();
                s.write_all(elf).unwrap();
                s.flush().unwrap();
            }
        });
        let url = format!("http://127.0.0.1:{}/AnimeciX-x86_64.AppImage", port);
        let target = std::env::temp_dir().join(format!("animecix-dl-test-{}.AppImage", std::process::id()));
        let _ = std::fs::remove_file(&target);
        let progress = std::sync::Arc::new(std::sync::Mutex::new((0u64, 0u64)));
        let prog = progress.clone();
        let r = download_update(&url, &target, move |cur, total| {
            *prog.lock().unwrap() = (cur, total);
        });
        assert!(r.is_ok(), "indirme başarısız: {:?}", r.err());
        assert!(target.exists(), "hedef dosya yazılmadı");
        assert_eq!(std::fs::read(&target).unwrap(), elf);
        let last = *progress.lock().unwrap();
        assert_eq!(last.1, elf.len() as u64, "toplam boyut bildirilmedi");
        assert_eq!(last.0, elf.len() as u64, "indirilen boyut bildirilmedi");
        std::fs::remove_file(&target).ok();
    }

    #[test]
    fn download_update_fails_on_unreachable() {
        let target = std::env::temp_dir().join(format!("animecix-dl-fail-{}.AppImage", std::process::id()));
        let _ = std::fs::remove_file(&target);
        let r = download_update("http://127.0.0.1:1/nope", &target, |_, _| {});
        assert!(r.is_err(), "erişilemeyen adres başarısız olmalı");
        std::fs::remove_file(&target).ok();
    }
}
