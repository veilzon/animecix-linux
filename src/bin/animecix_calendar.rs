//! Hafif yayın takipçisi: GTK/Adwaita bağımlılığı yoktur.
//!
//! Kullanım:
//!   animecix-calendar --daemon          (oturum açılışında autostart)
//!   animecix-calendar --once            (systemd timer / elle)
//!   animecix-calendar --check           (kurulum doğrulama)
//!   animecix-calendar --print-autostart (CI: desktop-file-validate)
//!   animecix-calendar --dry-run --once  (ağa çıkmadan karar dökümü)

#[allow(dead_code)]
#[path = "../calendar_feed.rs"]
mod calendar_feed;
#[allow(dead_code)]
#[path = "../follow_feed.rs"]
mod follow_feed;
#[allow(dead_code)]
#[path = "../follow_store.rs"]
mod follow_store;
#[allow(dead_code)]
#[path = "../http.rs"]
mod http;

use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const INITIAL_DELAY: u64 = 20;

static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Daemon,
    Once,
    Check,
    PrintAutostart,
}

struct Args {
    mode: Mode,
    dry_run: bool,
    verbose: bool,
    fixture: Option<PathBuf>,
    notify_cmd: Option<String>,
    now: Option<u64>,
    interval_mins: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct WatchSettings {
    #[serde(default)]
    calendar_watch: bool,
    #[serde(default = "default_interval")]
    calendar_watch_interval: u64,
    #[serde(default)]
    cf_clearance: String,
}

fn default_interval() -> u64 {
    15
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            calendar_watch: false,
            calendar_watch_interval: default_interval(),
            cf_clearance: String::new(),
        }
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[WATCH] {e}");
            eprintln!(
                "Kullanım: animecix-calendar [--daemon|--once|--check|--print-autostart] \
                 [--dry-run] [--fixture DOSYA] [--notify-cmd YOL] [--now EPOCH|ISO] \
                 [--interval DK] [--data-dir DIZIN]"
            );
            std::process::exit(2);
        }
    };

    match args.mode {
        Mode::Check => {
            check();
            return;
        }
        Mode::PrintAutostart => {
            print_autostart();
            return;
        }
        Mode::Once => std::process::exit(run_once_mode(&args)),
        Mode::Daemon => run_daemon(&args),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        mode: Mode::Daemon,
        dry_run: false,
        verbose: false,
        fixture: None,
        notify_cmd: None,
        now: None,
        interval_mins: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--daemon" => args.mode = Mode::Daemon,
            "--once" => args.mode = Mode::Once,
            "--check" => args.mode = Mode::Check,
            "--print-autostart" => args.mode = Mode::PrintAutostart,
            "--dry-run" => args.dry_run = true,
            "--verbose" | "-v" => args.verbose = true,
            "--version" | "-V" => {
                println!("animecix-calendar {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "animecix-calendar {}\n\
                     --daemon  arka planda periyodik kontrol (varsayılan)\n\
                     --once    tek tur çalış ve çık\n\
                     --check   kurulumu doğrula\n\
                     --print-autostart  XDG autostart girdisini yazdır\n\
                     --dry-run bildirim göndermeden karar dökümü ver\n\
                     --fixture DOSYA  ağ yerine yerel bölüm satırı JSON'u kullan\n\
                     --notify-cmd YOL  test için bildirim komutu\n\
                     --now EPOCH|ISO  test için saati sabitle\n\
                     --interval DK    kontrol aralığını geçici olarak değiştir\n\
                     --data-dir DIZIN takip verisi dizinini değiştir",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            "--fixture" => {
                args.fixture = Some(PathBuf::from(it.next().ok_or("--fixture değeri yok")?));
            }
            "--notify-cmd" => {
                args.notify_cmd = Some(it.next().ok_or("--notify-cmd değeri yok")?);
            }
            "--now" => {
                let v = it.next().ok_or("--now değeri yok")?;
                args.now = Some(
                    v.parse::<u64>()
                        .ok()
                        .or_else(|| calendar_feed::iso_to_epoch(&v).map(|e| e.max(0) as u64))
                        .ok_or("--now anlaşılamadı")?,
                );
            }
            "--interval" => {
                let v = it.next().ok_or("--interval değeri yok")?;
                args.interval_mins = Some(v.parse::<u64>().map_err(|_| "--interval sayı değil")?);
            }
            "--data-dir" => {
                let v = it.next().ok_or("--data-dir değeri yok")?;
                std::env::set_var("ANIMECIX_FOLLOW_DIR", v);
            }
            other => return Err(format!("bilinmeyen argüman: {other}")),
        }
    }
    Ok(args)
}

fn check() {
    let settings = load_settings();
    println!("animecix-calendar {}", env!("CARGO_PKG_VERSION"));
    println!("takip dizini: {}", follow_store::data_dir().display());
    println!("follows: {}", follow_store::follows_path().display());
    println!("ledger: {}", follow_store::ledger_path().display());
    println!("autostart: {}", follow_store::autostart_installed());
    println!("takip açık: {}", settings.calendar_watch);
    println!("aralık: {} dk", interval_mins(&settings));
    println!("bildirim servisi: {}", notifications_available());
}

fn print_autostart() {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "animecix-calendar".to_string());
    let exec = format!("\"{exe}\" --daemon");
    print!("{}", follow_store::autostart_content(&exec));
}

fn run_once_mode(args: &Args) -> i32 {
    let settings = match load_settings_checked() {
        Ok(Some(s)) => s,
        Ok(None) => {
            eprintln!("[WATCH] ayar dosyası yok, takip kapalı");
            return 0;
        }
        Err(e) => {
            eprintln!("[WATCH] ayarlar okunamadı: {e}");
            return 1;
        }
    };
    if !settings.calendar_watch {
        eprintln!("[WATCH] takip kapalı");
        return 0;
    }
    let Some(_lock) = acquire_instance_lock() else {
        eprintln!("[WATCH] başka bir takipçi çalışıyor");
        return 0;
    };
    let now = args.now.unwrap_or_else(follow_store::now_secs);
    let mut status = follow_store::load_daemon_status();
    status.version = env!("CARGO_PKG_VERSION").to_string();
    status.pid = std::process::id();
    status.last_check = now;
    status.running = false;
    let code = match run_once(args, &settings, now) {
        Ok(n) => {
            if n > 0 {
                status.last_success = now;
            }
            status.last_error.clear();
            0
        }
        Err(e) => {
            status.last_error = truncate(&e, 200);
            eprintln!("[WATCH] {e}");
            1
        }
    };
    follow_store::save_daemon_status(&status);
    code
}

fn run_daemon(args: &Args) {
    install_signal_handlers();
    let Some(_lock) = acquire_instance_lock() else {
        eprintln!("[WATCH] başka bir takipçi çalışıyor");
        return;
    };
    let mut status = follow_store::DaemonStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        pid: std::process::id(),
        started_at: follow_store::now_secs(),
        running: true,
        ..Default::default()
    };
    follow_store::save_daemon_status(&status);
    eprintln!("[WATCH] daemon başladı (pid {})", status.pid);

    sleep_chunks(INITIAL_DELAY);
    loop {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        let settings = match load_settings_checked() {
            Ok(Some(s)) => s,
            Ok(None) => WatchSettings::default(),
            Err(e) => {
                status.running = true;
                status.last_error = truncate(&format!("ayarlar okunamadı: {e}"), 200);
                follow_store::save_daemon_status(&status);
                eprintln!("[WATCH] ayarlar okunamadı, 60 sn sonra tekrar: {e}");
                sleep_chunks(60);
                continue;
            }
        };
        if !settings.calendar_watch {
            status.running = false;
            status.last_error = "takip kapalı".to_string();
            follow_store::save_daemon_status(&status);
            eprintln!("[WATCH] takip kapalı, çıkılıyor");
            return;
        }
        let now = follow_store::now_secs();
        match run_once(args, &settings, now) {
            Ok(n) => {
                status.last_check = now;
                status.last_success = now;
                status.last_error.clear();
                if n > 0 && args.verbose {
                    eprintln!("[WATCH] {n} bildirim gönderildi");
                }
            }
            Err(e) => {
                status.last_check = now;
                status.last_error = truncate(&e, 200);
                eprintln!("[WATCH] {e}");
            }
        }
        status.running = true;
        follow_store::save_daemon_status(&status);
        let mins = args
            .interval_mins
            .unwrap_or_else(|| interval_mins(&settings))
            .clamp(1, 1440);
        sleep_chunks(mins * 60);
    }
    status.running = false;
    follow_store::save_daemon_status(&status);
    eprintln!("[WATCH] daemon durdu");
}

fn run_once(args: &Args, settings: &WatchSettings, now: u64) -> Result<usize, String> {
    let follows = follow_store::load_follows();
    if follows.corrupt {
        return Err("follows.json bozuk; yedeklendi, yapımları yeniden takibe alın".to_string());
    }
    if !follows.schema_ok {
        return Err("follows.json şeması bu sürümden yeni".to_string());
    }
    if follows.items.is_empty() {
        return Ok(0);
    }
    let ledger = follow_store::load_ledger();
    if !ledger.schema_ok {
        return Err("ledger.json şeması bu sürümden yeni".to_string());
    }

    let rows = load_follow_rows(args, &follows, settings, now)?;
    if rows.is_empty() {
        return Ok(0);
    }

    let service = args.notify_cmd.is_some() || notifications_available();
    let locked = screen_locked();
    // GUI aktifse masaüstü bildirimi gönderme; uygulama içi toast önceliklidir.
    // Kilidi yalnızca yoklamak için alıp hemen bırakırız; aksi hâlde GUI
    // penceresi aktifleştiğinde kilidi bir daha alamazdı.
    let gui_lock = follow_store::FileLock::try_acquire(&follow_store::gui_active_path());
    let gui_active = gui_lock.is_none();
    drop(gui_lock);

    if args.dry_run {
        let mut probe = follow_store::load_ledger();
        follow_store::detect_new(&follows, &mut probe, &rows, now);
        let due = follow_store::claim_due(&mut probe, now, 6);
        println!(
            "{}",
            serde_json::json!({
                "rows": rows.len(),
                "notification_service": service,
                "screen_locked": locked,
                "gui_active": gui_active,
                "due": due.len(),
                "claims": due
                    .iter()
                    .map(|c| format!("{}:{}:{}", c.title_id, c.season, c.episode))
                    .collect::<Vec<_>>(),
            })
        );
        return Ok(0);
    }

    let deliver = service && !locked && !gui_active;
    let claims = follow_store::with_ledger_locked(|l| {
        follow_store::detect_new(&follows, l, &rows, now);
        if !deliver {
            return Vec::new();
        }
        follow_store::claim_due(l, now, 6)
    })
    .ok_or_else(|| "defter kilidi alınamadı".to_string())?;
    if claims.is_empty() {
        return Ok(0);
    }

    let (summary, body) = compose(&claims);
    match send_notification(args.notify_cmd.as_deref(), &summary, &body) {
        Ok(()) => {
            let n = claims.len();
            let _ = follow_store::with_ledger_locked(|l| {
                follow_store::mark_sent(l, &claims, now);
            });
            if args.verbose {
                eprintln!("[WATCH] {n} bildirim gönderildi");
            }
            Ok(n)
        }
        Err(e) => {
            let _ = follow_store::with_ledger_locked(|l| {
                follow_store::release_claim(l, &claims, now, &e);
            });
            Err(format!("bildirim gönderilemedi: {e}"))
        }
    }
}

fn load_settings_checked() -> Result<Option<WatchSettings>, String> {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str::<WatchSettings>(&s)
            .map(Some)
            .map_err(|e| format!("settings.json ayrıştırılamadı: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("settings.json okunamadı: {e}")),
    }
}

fn load_settings() -> WatchSettings {
    load_settings_checked().ok().flatten().unwrap_or_default()
}

fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".local/share/animecix/settings.json")
}

fn interval_mins(s: &WatchSettings) -> u64 {
    match s.calendar_watch_interval {
        30 => 30,
        60 => 60,
        _ => 15,
    }
}

fn load_follow_rows(
    args: &Args,
    follows: &follow_store::FollowsFile,
    settings: &WatchSettings,
    now: u64,
) -> Result<Vec<follow_store::CalendarRow>, String> {
    if let Some(path) = &args.fixture {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("fixture okunamadı ({}): {e}", path.display()))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("fixture JSON: {e}"))?;
        let rows_value = if value.is_array() {
            value
        } else {
            value
                .get("rows")
                .cloned()
                .ok_or_else(|| "fixture içinde rows dizisi yok".to_string())?
        };
        return serde_json::from_value(rows_value)
            .map_err(|e| format!("fixture bölüm satırları: {e}"));
    }
    let http = http::Http::new(None)?;
    http.set_cf_clearance(&settings.cf_clearance);
    follow_feed::fetch_rows(&http, &follows.items, now)
}

fn notifications_available() -> bool {
    command_out(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            "org.freedesktop.Notifications",
        ],
    )
    .map(|out| out.contains("true"))
    .unwrap_or(false)
}

fn screen_locked() -> bool {
    for (dest, path) in [
        (
            "org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
        ),
        ("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver"),
    ] {
        if let Some(out) = command_out(
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                dest,
                "--object-path",
                path,
                "--method",
                &format!("{dest}.GetActive"),
            ],
        ) {
            return out.contains("true");
        }
    }
    // Sway/Hyprland gibi ScreenSaver servisi olmayan oturumlarda logind.
    if let Ok(sid) = std::env::var("XDG_SESSION_ID") {
        let sid = sid.trim();
        if !sid.is_empty() {
            if let Some(out) = command_out(
                "loginctl",
                &["show-session", sid, "-p", "LockedHint", "--value"],
            ) {
                return out.trim().eq_ignore_ascii_case("yes");
            }
        }
    }
    false
}

fn command_out(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn compose(claims: &[follow_store::Claim]) -> (String, String) {
    if claims.len() == 1 {
        let c = &claims[0];
        let detail = if c.episode_name.trim().is_empty() {
            format!("S{:02}E{:02}", c.season, c.episode)
        } else {
            format!("S{:02}E{:02} · {}", c.season, c.episode, c.episode_name)
        };
        (format!("Yeni bölüm: {}", c.title_name), detail)
    } else {
        let body = claims
            .iter()
            .take(3)
            .map(|c| format!("{} S{:02}E{:02}", c.title_name, c.season, c.episode))
            .collect::<Vec<_>>()
            .join("\n");
        (format!("{} yapımda yeni bölüm", claims.len()), body)
    }
}

fn send_notification(cmd: Option<&str>, summary: &str, body: &str) -> Result<(), String> {
    let summary = clean(summary);
    let body = clean(body);
    if let Some(path) = cmd {
        let status = Command::new(path)
            .arg(&summary)
            .arg(&body)
            .status()
            .map_err(|e| format!("notify-cmd: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!(
                "notify-cmd çıkış kodu {}",
                status.code().unwrap_or(-1)
            ))
        };
    }

    if command_out(
        "notify-send",
        &[
            "--app-name=AnimeciX",
            "--icon=tr.com.animecix",
            "--urgency=normal",
            "--expire-time=10000",
            "--",
            &summary,
            &body,
        ],
    )
    .is_some()
    {
        return Ok(());
    }

    let gv_summary = gv_quote(&summary);
    let gv_body = gv_quote(&body);
    let gv_app = gv_quote("AnimeciX");
    let gv_icon = gv_quote("tr.com.animecix");
    command_out(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.Notifications",
            "--object-path",
            "/org/freedesktop/Notifications",
            "--method",
            "org.freedesktop.Notifications.Notify",
            &gv_app,
            "0",
            &gv_icon,
            &gv_summary,
            &gv_body,
            "[]",
            "{}",
            "10000",
        ],
    )
    .map(|_| ())
    .ok_or_else(|| "bildirim aracı bulunamadı".to_string())
}

/// GVariant metin argümanı için çift tırnaklı gösterim.
fn gv_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(200));
    let mut last_space = false;
    for c in s.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if c == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        out.push(c);
        if out.chars().count() >= 200 {
            break;
        }
    }
    out.trim().to_string()
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn acquire_instance_lock() -> Option<follow_store::FileLock> {
    follow_store::FileLock::acquire(
        &follow_store::daemon_lock_path(),
        Duration::from_millis(250),
    )
}

fn sleep_chunks(secs: u64) {
    let mut left = secs;
    while left > 0 && !STOP.load(Ordering::SeqCst) {
        let step = left.min(5);
        std::thread::sleep(Duration::from_secs(step));
        left -= step;
    }
}

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_controls_and_caps_length() {
        assert_eq!(clean("a\nb\tc"), "a b c");
        let long: String = "ş".repeat(250);
        assert_eq!(clean(&long).chars().count(), 200);
        assert_eq!(clean("  bosluk  "), "bosluk");
    }

    #[test]
    fn gv_quote_escapes_quotes_and_backslashes() {
        assert_eq!(gv_quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(gv_quote("normal"), "\"normal\"");
    }
}
