//! `animecix-calendar` ikilisinin gerçek süreç testleri: ağ yok (fixture),
//! bildirim süreç sınırında sahte (`--notify-cmd`), saat sabit (`--now`).

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

const NOW: u64 = 1_795_003_200; // 2026-11-18 12:00 UTC

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_animecix-calendar")
}

fn tmp(name: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("animecix-watchd-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn setup(name: &str, watch: bool) -> (PathBuf, PathBuf) {
    let home = tmp(&format!("{name}-home"));
    let data = tmp(&format!("{name}-data"));
    std::fs::create_dir_all(home.join(".local/share/animecix")).unwrap();
    let settings =
        format!(r#"{{"calendar_watch":{watch},"calendar_watch_interval":15,"auto_update":false}}"#);
    std::fs::write(home.join(".local/share/animecix/settings.json"), settings).unwrap();
    std::fs::write(
        data.join("follows.json"),
        r#"{"schema":1,"items":[{"title_id":500,"name":"Test","notify":true,"added_at":1}]}"#,
    )
    .unwrap();
    (home, data)
}

fn episode(ep: u64, release: &str) -> String {
    format!(
        r#"{{"id":{},"season_number":1,"episode_number":{},"release_date":"{}","name":"{} . Bölüm","title":{{"id":500,"name":"Test Anime","mal_id":1,"title_type":"anime"}}}}"#,
        1000 + ep,
        ep,
        release,
        ep
    )
}

fn fixture(home: &Path, name: &str, episodes: &[String]) -> PathBuf {
    let path = home.join(name);
    let body = format!(
        r#"{{"data":[{{"date":"2026-11-16T00:00:00.000Z","episodes":[{}]}}]}}"#,
        episodes.join(",")
    );
    std::fs::write(&path, body).unwrap();
    path
}

fn notify_script(home: &Path) -> PathBuf {
    let path = home.join("fake-notify.sh");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    writeln!(f, "printf '%s|%s\\n' \"$1\" \"$2\" >> \"$NOTIFY_LOG\"").unwrap();
    drop(f);
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

fn run(home: &Path, data: &Path, args: &[&str]) -> Output {
    let log = home.join("notify.log");
    Command::new(bin())
        .args(args)
        .env("HOME", home)
        .env("ANIMECIX_FOLLOW_DIR", data)
        .env("NOTIFY_LOG", &log)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env("XDG_RUNTIME_DIR", home)
        .output()
        .expect("animecix-calendar çalıştırılamadı")
}

fn log_lines(home: &Path) -> Vec<String> {
    std::fs::read_to_string(home.join("notify.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn invalid_settings_fails_once() {
    let (home, data) = setup("badsettings", true);
    std::fs::write(home.join(".local/share/animecix/settings.json"), "{bozuk").unwrap();
    let f = fixture(&home, "cal.json", &[episode(1, "2026-11-14T00:00:00.000Z")]);
    let out = run(
        &home,
        &data,
        &[
            "--once",
            "--fixture",
            &f.to_string_lossy(),
            "--notify-cmd",
            "/bin/true",
            "--now",
            &NOW.to_string(),
        ],
    );
    assert!(
        !out.status.success(),
        "bozuk settings.json --once'da sıfır dışı dönmeli"
    );
    assert!(log_lines(&home).is_empty());
}

#[test]
fn once_baselines_then_notifies_exactly_once() {
    let (home, data) = setup("once", true);
    let script = notify_script(&home);
    let script_s = script.to_string_lossy().to_string();
    let now_s = NOW.to_string();

    let f1 = fixture(
        &home,
        "cal1.json",
        &[episode(7, "2026-11-14T00:00:00.000Z")],
    );
    let out = run(
        &home,
        &data,
        &[
            "--once",
            "--fixture",
            &f1.to_string_lossy(),
            "--notify-cmd",
            &script_s,
            "--now",
            &now_s,
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        log_lines(&home).is_empty(),
        "ilk tur baseline almalı, bildirim göndermemeli"
    );

    let f2 = fixture(
        &home,
        "cal2.json",
        &[
            episode(7, "2026-11-14T00:00:00.000Z"),
            episode(8, "2026-11-18T09:00:00.000Z"),
        ],
    );
    let out = run(
        &home,
        &data,
        &[
            "--once",
            "--fixture",
            &f2.to_string_lossy(),
            "--notify-cmd",
            &script_s,
            "--now",
            &now_s,
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = log_lines(&home);
    assert_eq!(lines.len(), 1, "tek bildirim beklenir: {lines:?}");
    assert!(lines[0].contains("Test Anime"), "başlık: {}", lines[0]);
    assert!(lines[0].contains("S01E08"), "bölüm: {}", lines[0]);

    // Aynı veri tekrar: spam yok.
    let out = run(
        &home,
        &data,
        &[
            "--once",
            "--fixture",
            &f2.to_string_lossy(),
            "--notify-cmd",
            &script_s,
            "--now",
            &now_s,
        ],
    );
    assert!(out.status.success());
    assert_eq!(log_lines(&home).len(), 1, "aynı bölüm ikinci kez gitmemeli");

    let ledger = std::fs::read_to_string(data.join("ledger.json")).unwrap();
    assert!(ledger.contains("\"state\": \"sent\""));
    assert!(ledger.contains("\"sent\": 1"));
}

#[test]
fn gui_active_lock_defers_desktop_notification() {
    use std::os::unix::io::AsRawFd;

    let (home, data) = setup("guilock", true);
    let script = notify_script(&home);
    let script_s = script.to_string_lossy().to_string();
    let now_s = NOW.to_string();
    let f1 = fixture(
        &home,
        "cal1.json",
        &[episode(1, "2026-11-14T00:00:00.000Z")],
    );
    let args = [
        "--once",
        "--fixture",
        &f1.to_string_lossy(),
        "--notify-cmd",
        &script_s,
        "--now",
        &now_s,
    ];
    assert!(run(&home, &data, &args).status.success());

    // GUI aktif gibi davran: kilidi test süreci tutar.
    let lock_path = data.join("gui-active.lock");
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    let rc = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_eq!(rc, 0);

    let f2 = fixture(
        &home,
        "cal2.json",
        &[
            episode(1, "2026-11-14T00:00:00.000Z"),
            episode(2, "2026-11-18T09:00:00.000Z"),
        ],
    );
    let args2 = [
        "--once",
        "--fixture",
        &f2.to_string_lossy(),
        "--notify-cmd",
        &script_s,
        "--now",
        &now_s,
    ];
    assert!(run(&home, &data, &args2).status.success());
    assert!(
        log_lines(&home).is_empty(),
        "GUI aktifken masaüstü bildirimi gönderilmemeli"
    );
    let ledger = std::fs::read_to_string(data.join("ledger.json")).unwrap();
    assert!(
        ledger.contains("\"state\": \"pending\""),
        "pending kalmalı: {ledger}"
    );

    // GUI kapanınca pending gönderilir.
    drop(lock_file);
    assert!(run(&home, &data, &args2).status.success());
    assert_eq!(log_lines(&home).len(), 1);
}

#[test]
fn disabled_master_does_nothing() {
    let (home, data) = setup("disabled", false);
    let script = notify_script(&home);
    let f = fixture(&home, "cal.json", &[episode(1, "2026-11-14T00:00:00.000Z")]);
    let out = run(
        &home,
        &data,
        &[
            "--once",
            "--fixture",
            &f.to_string_lossy(),
            "--notify-cmd",
            &script.to_string_lossy(),
            "--now",
            &NOW.to_string(),
        ],
    );
    assert!(out.status.success());
    assert!(log_lines(&home).is_empty());
    assert!(
        !data.join("ledger.json").exists(),
        "kapalıyken defter yazılmamalı"
    );
}

#[test]
fn corrupt_follows_is_not_overwritten() {
    let (home, data) = setup("corrupt", true);
    std::fs::write(data.join("follows.json"), "{bozuk").unwrap();
    let f = fixture(&home, "cal.json", &[episode(1, "2026-11-14T00:00:00.000Z")]);
    let out = run(
        &home,
        &data,
        &[
            "--once",
            "--fixture",
            &f.to_string_lossy(),
            "--notify-cmd",
            "/bin/true",
            "--now",
            &NOW.to_string(),
        ],
    );
    assert!(
        !out.status.success(),
        "bozuk takip dosyası hata döndürmeli (kullanıcı uyarılsın)"
    );
    let kept = std::fs::read_to_string(data.join("follows.json")).ok();
    let backup = std::fs::read_dir(&data)
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
    assert!(
        kept.as_deref() == Some("{bozuk") || backup,
        "bozuk takip dosyası sessizce silinmemeli"
    );
    assert!(
        kept.as_deref() != Some("{\"schema\":1,\"items\":[]}"),
        "bozuk dosya boş veriyle ezilmemeli"
    );
}

#[test]
fn check_and_print_autostart_are_safe_without_display() {
    let (home, data) = setup("check", true);
    let out = run(&home, &data, &["--check"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("animecix-calendar"));
    assert!(text.contains("takip dizini:"));

    let out = run(&home, &data, &["--print-autostart"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("X-AnimeciX-Managed=1"));
    assert!(text.contains("Exec="));
    assert!(text.contains("--daemon"));
}

#[test]
fn dry_run_reports_due_without_writing() {
    let (home, data) = setup("dry", true);
    let script = notify_script(&home);
    let now_s = NOW.to_string();
    let f1 = fixture(
        &home,
        "cal1.json",
        &[episode(1, "2026-11-14T00:00:00.000Z")],
    );
    assert!(run(
        &home,
        &data,
        &[
            "--once",
            "--fixture",
            &f1.to_string_lossy(),
            "--notify-cmd",
            &script.to_string_lossy(),
            "--now",
            &now_s,
        ]
    )
    .status
    .success());

    let f2 = fixture(
        &home,
        "cal2.json",
        &[
            episode(1, "2026-11-14T00:00:00.000Z"),
            episode(2, "2026-11-18T09:00:00.000Z"),
        ],
    );
    let before = std::fs::read_to_string(data.join("ledger.json")).unwrap();
    let out = run(
        &home,
        &data,
        &[
            "--once",
            "--dry-run",
            "--fixture",
            &f2.to_string_lossy(),
            "--notify-cmd",
            &script.to_string_lossy(),
            "--now",
            &now_s,
        ],
    );
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"due\":1"), "dry-run: {text}");
    assert!(log_lines(&home).is_empty(), "dry-run bildirim göndermemeli");
    let after = std::fs::read_to_string(data.join("ledger.json")).unwrap();
    assert_eq!(before, after, "dry-run defteri değiştirmemeli");
}
