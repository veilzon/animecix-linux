use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;

use adw::prelude::*;
use glib::ControlFlow;
use serde::Deserialize;

const UPDATE_REPO: &str = "veilzon/animecix-linux";
const ASSET_APPIMAGE: &str = "AnimeciX-x86_64.AppImage";
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

/// Kurulum türü: güncelleme yolu buna göre seçilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    AppImage,
    Deb,
    Rpm,
    Unknown,
}

impl InstallKind {
    /// Kullanıcıya gösterilen ad.
    pub fn label(self) -> &'static str {
        match self {
            InstallKind::AppImage => "AppImage",
            InstallKind::Deb => ".deb paketi",
            InstallKind::Rpm => ".rpm paketi",
            InstallKind::Unknown => "bilinmeyen kurulum",
        }
    }
}

/// Test kancası: yalnız debug derlemede etkin (kurulu release paketinde yok sayılır).
fn fake_kind_env() -> Option<InstallKind> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("ANIMECIX_FAKE_INSTALL_KIND")
        .ok()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "deb" => Some(InstallKind::Deb),
        "rpm" => Some(InstallKind::Rpm),
        "appimage" => Some(InstallKind::AppImage),
        _ => None,
    }
}

/// Kurulum türünü algılar (süreç başına bir kez; alt komut maliyeti tek sefer).
pub fn install_kind() -> InstallKind {
    static KIND: std::sync::OnceLock<InstallKind> = std::sync::OnceLock::new();
    *KIND.get_or_init(|| {
        if std::env::var("APPIMAGE")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
        {
            return InstallKind::AppImage;
        }
        if let Some(k) = fake_kind_env() {
            return k;
        }
        if deb_package_installed() {
            return InstallKind::Deb;
        }
        if rpm_package_installed() {
            return InstallKind::Rpm;
        }
        InstallKind::Unknown
    })
}

/// Uygulama içi güncelleme bu kurulumda destekleniyor mu.
pub fn self_update_supported() -> bool {
    install_kind() != InstallKind::Unknown
}

fn deb_package_installed() -> bool {
    Command::new("dpkg-query")
        .args(["-W", "-f=${Status}", "animecix"])
        .output()
        .map(|o| {
            o.status.success()
                && String::from_utf8_lossy(&o.stdout).contains("install ok installed")
        })
        .unwrap_or(false)
}

fn rpm_package_installed() -> bool {
    Command::new("rpm")
        .args(["-q", "animecix"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Test kancası: kurulu paket sürümünü taklit eder (yalnız debug derleme).
fn fake_pkg_version_env() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("ANIMECIX_FAKE_PKG_VERSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Paket kurulumlarında güncelleme karşılaştırmasının tabanı: kurulu paket
/// sürümü. Böylece bayat bir ikili çalışsa bile kurulu sürüm güncel olduğunda
/// gereksiz uyarı gösterilmez; okunamazsa derleme sürümüne düşülür.
fn installed_package_version(kind: InstallKind) -> Option<String> {
    if let Some(v) = fake_pkg_version_env() {
        return Some(normalize_pkg_version(&v));
    }
    let out = match kind {
        InstallKind::Deb => Command::new("dpkg-query")
            .args(["-W", "-f=${Version}", "animecix"])
            .output()
            .ok()?,
        InstallKind::Rpm => Command::new("rpm")
            .args(["-q", "--qf", "%{VERSION}", "animecix"])
            .output()
            .ok()?,
        _ => return None,
    };
    if !out.status.success() {
        return None;
    }
    let v = normalize_pkg_version(&String::from_utf8_lossy(&out.stdout));
    (!v.is_empty()).then_some(v)
}

/// "v1.2.7-1.fc40" → "1.2.7": yalnız baştaki sayı+nokta parçası alınır.
fn normalize_pkg_version(raw: &str) -> String {
    let s = raw.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    s[..end].trim_end_matches('.').to_string()
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

/// Kurulum türüne göre release asset'i seçer (saf; test edilebilir).
fn pick_asset(kind: InstallKind, assets: &[GithubAsset]) -> Option<&GithubAsset> {
    match kind {
        InstallKind::AppImage => assets.iter().find(|a| a.name == ASSET_APPIMAGE),
        InstallKind::Deb => assets.iter().find(|a| a.name.ends_with("_amd64.deb")),
        InstallKind::Rpm => assets.iter().find(|a| a.name.ends_with(".x86_64.rpm")),
        InstallKind::Unknown => None,
    }
}

/// Paket sihirli baytları: deb (ar arşivi + debian-binary), rpm (lead magic).
fn package_magic_ok(kind: InstallKind, bytes: &[u8]) -> bool {
    match kind {
        InstallKind::Deb => {
            let head = &bytes[..bytes.len().min(256)];
            bytes.starts_with(b"!<arch>\n") && head.windows(13).any(|w| w == b"debian-binary")
        }
        InstallKind::Rpm => bytes.starts_with(&[0xED, 0xAB, 0xEE, 0xDB]),
        InstallKind::AppImage => bytes.starts_with(b"\x7fELF"),
        InstallKind::Unknown => false,
    }
}

pub fn replace_target(bytes: &[u8], target: &Path) -> Result<(), String> {
    if bytes.len() < 4 || &bytes[0..4] != b"\x7fELF" {
        return Err("İndirilen dosya geçerli bir AppImage değil".into());
    }
    let tmp = std::env::temp_dir().join(format!("AnimeciX-update-{}.AppImage", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp, perms).map_err(|e| e.to_string())?;
    }
    if std::fs::rename(&tmp, target).is_ok() {
        return Ok(());
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

/// Güncelleme paketlerinin indirildiği dizin (kullanıcı önbelleği).
fn updates_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("animecix").join("updates")
}

fn download_bytes<F>(url: &str, mut on_progress: F) -> Result<Vec<u8>, String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
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
    })
}

pub fn download_update<F>(url: &str, target: &Path, on_progress: F) -> Result<(), String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    let bytes = download_bytes(url, on_progress)?;
    replace_target(&bytes, target)
}

/// Paketi indirir, sihirli baytları doğrular ve önbellek dizinine yazar.
fn download_package<F>(
    kind: InstallKind,
    url: &str,
    asset_name: &str,
    on_progress: F,
) -> Result<PathBuf, String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    let bytes = download_bytes(url, on_progress)?;
    if !package_magic_ok(kind, &bytes) {
        return Err(format!(
            "İndirilen dosya geçerli bir {} değil",
            kind.label()
        ));
    }
    let dir = updates_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Güncelleme dizini oluşturulamadı: {e}"))?;
    let file_name = Path::new(asset_name)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("animecix-update.pkg");
    let dest = dir.join(file_name);
    std::fs::write(&dest, &bytes).map_err(|e| format!("Paket yazılamadı: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // pkexec ile çalışan paket yöneticisi dosyayı okuyabilmeli.
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o644));
    }
    Ok(dest)
}

/// Kurulum komutu (saf): (program, argümanlar). Paketler polkit ile kurulur.
fn install_args(kind: InstallKind, path: &Path) -> Option<(String, Vec<String>)> {
    let pkg = path.display().to_string();
    match kind {
        InstallKind::Deb => Some((
            "pkexec".to_string(),
            vec![
                "apt-get".to_string(),
                "install".to_string(),
                "-y".to_string(),
                "-o".to_string(),
                "DPkg::Lock::Timeout=120".to_string(),
                pkg,
            ],
        )),
        InstallKind::Rpm => Some((
            "pkexec".to_string(),
            vec![
                "dnf".to_string(),
                "install".to_string(),
                "-y".to_string(),
                pkg,
            ],
        )),
        _ => None,
    }
}

fn run_install(kind: InstallKind, path: &Path) -> Result<(), String> {
    let Some((program, args)) = install_args(kind, path) else {
        return Err("Bu kurulum türünde paket kurulumu desteklenmiyor".into());
    };
    let out = Command::new(&program).args(&args).output().map_err(|e| {
        format!(
            "{program} çalıştırılamadı: {e}. Paketi elle kurabilirsiniz: {}",
            path.display()
        )
    })?;
    if out.status.success() {
        return Ok(());
    }
    let code = out.status.code().unwrap_or(-1);
    if code == 126 || code == 127 {
        return Err("Kurulum için yetki verilmedi (iptal edildi).".into());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let detail = stderr
        .lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" ");
    let mut msg = format!("Paket kurulumu başarısız (çıkış {code}).");
    if !detail.is_empty() {
        msg.push(' ');
        msg.push_str(&detail);
    }
    msg.push_str(&format!(" Paketi elle kurabilirsiniz: {}", path.display()));
    Err(msg)
}

/// Yeniden başlatma kabuğu: `$0` = çalıştırılacak yol. Kabuk hemen başlar,
/// hedefi 1 sn sonra devralır (eski süreç tamamen kapanmadan yeni süreç
/// başlamasın). Çıktı, teşhis için restart.log'a eklenir.
const RESTART_SHELL: &str = "mkdir -p \"${XDG_DATA_HOME:-$HOME/.local/share}/animecix\"; \
sleep 1; exec \"$0\" >>\"${XDG_DATA_HOME:-$HOME/.local/share}/animecix/restart.log\" 2>&1";

/// Verilen yolu, oturumdan koparılmış bir yardımcı kabuk üzerinden yeniden
/// başlatır (kabuk hemen başlar, hedefi 1 sn sonra devralır).
pub fn spawn_restart(exe: &Path) {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(RESTART_SHELL)
        .arg(exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

/// Güncelleme sonrası uygulamayı yeniden başlatır (AppImage yolu veya sistem
/// ikilisi). Paket yöneticisi ikiliyi değiştirdiğinde `current_exe()`
/// "/usr/bin/animecix (deleted)" dönebildiği ve o yol spawn edilemediği için
/// (uygulama sessizce kapanıyordu) sabit sistem yolu tercih edilir.
fn restart_app(appimage_target: Option<&str>) -> ! {
    let exe = match appimage_target {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => {
            let system = PathBuf::from("/usr/bin/animecix");
            if system.is_file() {
                system
            } else {
                std::env::current_exe().unwrap_or_else(|_| PathBuf::from("animecix"))
            }
        }
    };
    spawn_restart(&exe);
    std::process::exit(0);
}

pub fn check_and_prompt<W: IsA<gtk::Window> + Clone + 'static>(
    window: &W,
    show_if_uptodate: bool,
    on_suppress_uptodate: impl Fn() + 'static,
) {
    let kind = install_kind();
    if kind == InstallKind::Unknown {
        if show_if_uptodate {
            present_info(
                window,
                "Güncelleme Kullanılamıyor",
                "Otomatik güncelleme yalnızca AppImage, .deb veya .rpm kurulumlarında çalışır.",
            );
        }
        return;
    }
    // (etiket, asset adı, indirme adresi)
    let (tx, rx) = channel::<Result<Option<(String, String, String)>, String>>();
    std::thread::spawn(move || {
        let res = (|| -> Result<Option<(String, String, String)>, String> {
            let rel = latest_release()?;
            let base =
                installed_package_version(kind).unwrap_or_else(|| CURRENT_VERSION.to_string());
            if !needs_update(&base, &rel.tag_name) {
                return Ok(None);
            }
            let asset = pick_asset(kind, &rel.assets).ok_or_else(|| {
                format!(
                    "sürümde {} için güncelleme dosyası bulunamadı",
                    kind.label()
                )
            })?;
            Ok(Some((
                rel.tag_name.clone(),
                asset.name.clone(),
                asset.browser_download_url.clone(),
            )))
        })();
        let _ = tx.send(res);
    });

    let win = window.clone();
    let mut on_suppress = Some(on_suppress_uptodate);
    glib::idle_add_local(move || match rx.try_recv() {
        Ok(Ok(Some((tag, asset_name, url)))) => {
            present_update_dialog(win.clone(), kind, &tag, &asset_name, &url);
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

/// Tek bir güncelleme işinin parametreleri (worker + tekrar dene).
#[derive(Clone)]
struct UpdateJob {
    kind: InstallKind,
    url: String,
    asset_name: String,
    appimage_target: Option<String>,
}

fn present_update_dialog<W: IsA<gtk::Window> + Clone + 'static>(
    window: W,
    kind: InstallKind,
    tag: &str,
    asset_name: &str,
    url: &str,
) {
    let tag = tag.trim_start_matches('v').to_string();
    let url = url.to_string();
    let asset_name = asset_name.to_string();
    let appimage_target = if kind == InstallKind::AppImage {
        match std::env::var("APPIMAGE") {
            Ok(t) if !t.trim().is_empty() => Some(t),
            _ => {
                present_info(&window, "Güncelleme Başarısız", "AppImage yolu bulunamadı.");
                return;
            }
        }
    } else {
        None
    };
    let body = if kind == InstallKind::AppImage {
        format!("AnimeciX v{tag} yayınlandı. İndirip uygulamayı güncelleyelim mi?")
    } else {
        format!(
            "AnimeciX v{tag} yayınlandı. {} indirilip sistem paket yöneticisiyle kurulacak; yetki istenecek. Güncellensin mi?",
            kind.label()
        )
    };
    let dialog = adw::MessageDialog::builder()
        .heading("Yeni Sürüm Mevcut")
        .body(body)
        .close_response("later")
        .default_response("later")
        .build();
    dialog.add_response("later", "Daha Sonra");
    dialog.add_response(
        "update",
        if kind == InstallKind::AppImage {
            "Güncelle ve Yeniden Başlat"
        } else {
            "Kur ve Yeniden Başlat"
        },
    );
    dialog.set_response_appearance("update", adw::ResponseAppearance::Suggested);
    dialog.set_transient_for(Some(&window));
    dialog.connect_response(None, move |dlg, resp| {
        if resp == "update" {
            dlg.close();
            let job = UpdateJob {
                kind,
                url: url.clone(),
                asset_name: asset_name.clone(),
                appimage_target: appimage_target.clone(),
            };
            run_update_with_progress(window.clone(), job);
        }
    });
    dialog.present();
}

fn run_update_with_progress<W: IsA<gtk::Window> + Clone + 'static>(window: W, job: UpdateJob) {
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

    start_update_worker(&dlg, &label, &bar, &window, job);
}

fn start_update_worker<W: IsA<gtk::Window> + Clone + 'static>(
    dlg: &adw::Window,
    label: &gtk::Label,
    bar: &gtk::ProgressBar,
    window: &W,
    job: UpdateJob,
) {
    let UpdateJob {
        kind,
        url,
        asset_name,
        appimage_target,
    } = job;
    let (tx, rx) = channel::<UpdMsg>();
    let prog_tx = tx.clone();
    let url_s = url.clone();
    let asset_s = asset_name.clone();
    let target_thread = appimage_target.clone();
    std::thread::spawn(move || {
        let _ = tx.send(UpdMsg::Step("İndiriliyor…".into()));
        let result = match kind {
            InstallKind::AppImage => {
                let target = PathBuf::from(target_thread.unwrap_or_default());
                download_update(&url_s, &target, move |cur, total| {
                    let _ = prog_tx.send(UpdMsg::Progress(cur, total));
                })
            }
            _ => {
                let prog = prog_tx.clone();
                match download_package(kind, &url_s, &asset_s, move |cur, total| {
                    let _ = prog.send(UpdMsg::Progress(cur, total));
                }) {
                    Ok(path) => {
                        let _ = tx.send(UpdMsg::Step("Kuruluyor… (yetki istenebilir)".into()));
                        match run_install(kind, &path) {
                            Ok(()) => {
                                let _ = std::fs::remove_file(&path);
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                }
            }
        };
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
    let url_c = url;
    let asset_c = asset_name;
    let target_c = appimage_target;
    glib::idle_add_local(move || match rx.try_recv() {
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
            restart_app(target_c.as_deref());
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
                let job2 = UpdateJob {
                    kind,
                    url: url_c.clone(),
                    asset_name: asset_c.clone(),
                    appimage_target: target_c.clone(),
                };
                retry.connect_clicked(move |_| {
                    dlg2.close();
                    run_update_with_progress(win2.clone(), job2.clone());
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, url: &str) -> GithubAsset {
        GithubAsset {
            name: name.to_string(),
            browser_download_url: url.to_string(),
        }
    }

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
    fn pick_asset_matches_install_kind() {
        let assets = vec![
            asset("AnimeciX-x86_64.AppImage", "tu1"),
            asset("animecix_1.2.6_amd64.deb", "tu2"),
            asset("animecix-1.2.6-1.fc40.x86_64.rpm", "tu3"),
        ];
        assert_eq!(
            pick_asset(InstallKind::AppImage, &assets).map(|a| a.browser_download_url.as_str()),
            Some("tu1")
        );
        assert_eq!(
            pick_asset(InstallKind::Deb, &assets).map(|a| a.name.as_str()),
            Some("animecix_1.2.6_amd64.deb")
        );
        assert_eq!(
            pick_asset(InstallKind::Rpm, &assets).map(|a| a.name.as_str()),
            Some("animecix-1.2.6-1.fc40.x86_64.rpm")
        );
        assert!(pick_asset(InstallKind::Unknown, &assets).is_none());
        assert!(pick_asset(InstallKind::Deb, &assets[..2]).is_some());
    }

    #[test]
    fn package_magic_checks() {
        let mut deb = b"!<arch>\ndebian-binary   padding".to_vec();
        deb.resize(64, 0);
        assert!(package_magic_ok(InstallKind::Deb, &deb));
        assert!(!package_magic_ok(InstallKind::Deb, b"not a deb package"));

        let rpm = [0xED_u8, 0xAB, 0xEE, 0xDB, 0, 0, 0, 0];
        assert!(package_magic_ok(InstallKind::Rpm, &rpm));
        assert!(!package_magic_ok(InstallKind::Rpm, b"\x7fELF"));

        assert!(package_magic_ok(InstallKind::AppImage, b"\x7fELFrest"));
        assert!(!package_magic_ok(InstallKind::AppImage, b"nope"));
        assert!(!package_magic_ok(InstallKind::Unknown, b"\x7fELF"));
    }

    #[test]
    fn install_args_use_pkexec_and_pkg_manager() {
        let (prog, args) = install_args(InstallKind::Deb, Path::new("/tmp/x.deb")).unwrap();
        assert_eq!(prog, "pkexec");
        assert_eq!(args[0], "apt-get");
        assert!(args.contains(&"/tmp/x.deb".to_string()));
        assert!(install_args(InstallKind::AppImage, Path::new("/tmp/x.AppImage")).is_none());

        let (prog, args) = install_args(InstallKind::Rpm, Path::new("/tmp/x.rpm")).unwrap();
        assert_eq!(prog, "pkexec");
        assert_eq!(args[0], "dnf");
        assert!(args.contains(&"/tmp/x.rpm".to_string()));
    }

    #[test]
    fn updates_dir_is_user_cache() {
        let d = updates_dir();
        assert!(d.ends_with("animecix/updates"), "beklenmeyen dizin: {d:?}");
    }

    #[test]
    fn replace_writes_and_renames() {
        let tmp =
            std::env::temp_dir().join(format!("animecix-replace-test-{}", std::process::id()));
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
        let target =
            std::env::temp_dir().join(format!("animecix-dl-test-{}.AppImage", std::process::id()));
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
        let target =
            std::env::temp_dir().join(format!("animecix-dl-fail-{}.AppImage", std::process::id()));
        let _ = std::fs::remove_file(&target);
        let r = download_update("http://127.0.0.1:1/nope", &target, |_, _| {});
        assert!(r.is_err(), "erişilemeyen adres başarısız olmalı");
        std::fs::remove_file(&target).ok();
    }

    #[test]
    fn normalize_pkg_version_strips_package_suffixes() {
        assert_eq!(normalize_pkg_version("1.2.7\n"), "1.2.7");
        assert_eq!(normalize_pkg_version("v1.2.7"), "1.2.7");
        assert_eq!(normalize_pkg_version("1.2.7-1.fc40"), "1.2.7");
        assert_eq!(normalize_pkg_version("1.2.7-1"), "1.2.7");
        assert_eq!(normalize_pkg_version(" 1.2.7. "), "1.2.7");
        assert_eq!(normalize_pkg_version("bozuk"), "");
    }

    #[test]
    fn restart_shell_execs_target_after_delay() {
        let dir = std::env::temp_dir().join(format!("animecix-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("marker");
        let fake = dir.join("fake-animecix");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\ntouch \"{}\"\n", marker.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(RESTART_SHELL)
            .arg(&fake)
            .env("XDG_DATA_HOME", &dir)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(marker.is_file(), "hedef çalıştırılmadı");
        assert!(
            dir.join("animecix/restart.log").is_file(),
            "restart.log yazılmadı"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_restart_starts_target() {
        let dir =
            std::env::temp_dir().join(format!("animecix-spawn-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("marker");
        let fake = dir.join("fake-animecix");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\ntouch \"{}\"\n", marker.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Log hedefini teste yönlendir; çocuk ortamı spawn anında kopyalanır.
        let old = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", &dir);
        spawn_restart(&fake);
        match old {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let mut started = false;
        for _ in 0..30 {
            if marker.is_file() {
                started = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(started, "spawn_restart hedefi başlatmadı");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
