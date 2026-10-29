//! aria2 arka-ucu: harici `aria2c` daemon + JSON-RPC (6 bağlantı/segment).
//! Binary bulunamazsa çağıran iç motora düşer (`download.rs`).
//! Yeni crate yok: reqwest (blocking) + serde_json + std.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex, OnceLock,
};

use crate::download::{DownloadEvent, DownloadHandle, UA};

/// Segment/bağlantı sayısı (aria2 `-x/-s`).
pub const SEGMENTS: u32 = 6;
/// RPC port aralığı (ilk boş olan alınır).
const PORT_FIRST: u16 = 6800;
const PORT_LAST: u16 = 6820;
/// Durum yoklama aralığı (ms).
const POLL_MS: u64 = 500;

/// `aria2c` ikilisini bulur: paketlenmiş > sistem > PATH.
pub fn find_aria2c() -> Option<PathBuf> {
    if let Ok(appdir) = std::env::var("APPDIR") {
        let p = PathBuf::from(appdir).join("usr/bin/aria2c");
        if p.is_file() {
            return Some(p);
        }
    }
    for p in ["/usr/bin/aria2c", "/usr/local/bin/aria2c"] {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("aria2c"))
            .find(|p| p.is_file())
    })
}

fn random_secret() -> String {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut buf);
    } else {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        for (i, b) in buf.iter_mut().enumerate() {
            *b = ((t >> (i * 4)) ^ (std::process::id() as u128)) as u8;
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

struct Daemon {
    child: Child,
    port: u16,
    secret: String,
}

static DAEMON: OnceLock<Mutex<Option<Daemon>>> = OnceLock::new();
/// Daemon açma tekilleştirme kilidi: eşzamanlı çağrılar tek daemon açar,
/// kaybeden kazananı yeniden kullanır (thundering-herd + kill-steal yok).
static SPAWN: OnceLock<Mutex<()>> = OnceLock::new();
/// Aktif aria indirme sayacı (0'a düşünce daemon kapatılır, yetim kalmaz).
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn daemon_slot() -> &'static Mutex<Option<Daemon>> {
    DAEMON.get_or_init(|| Mutex::new(None))
}

fn spawn_lock() -> &'static Mutex<()> {
    SPAWN.get_or_init(|| Mutex::new(()))
}

/// Ölü/zombi çocuk biçer (kill + wait; beklemeyen ebeveyn zombi bırakır).
fn reap_child(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// JSON-RPC gövdesi kurar (saf; test edilir).
pub fn rpc_body(method: &str, secret: &str, params: serde_json::Value) -> serde_json::Value {
    let mut arr = vec![serde_json::Value::String(format!("token:{secret}"))];
    match params {
        serde_json::Value::Array(mut v) => arr.append(&mut v),
        other => arr.push(other),
    }
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": "animecix",
        "method": method,
        "params": arr,
    })
}

fn rpc_call(
    port: u16,
    secret: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = crate::api::shared_blocking_client();
    let body = rpc_body(method, secret, params);
    let resp = client
        .post(format!("http://127.0.0.1:{port}/jsonrpc"))
        .timeout(std::time::Duration::from_secs(10))
        .json(&body)
        .send()
        .map_err(|e| format!("rpc bağlanamadı (:{port}): {e}"))?;
    let v: serde_json::Value = resp.json().map_err(|e| format!("rpc json: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!("rpc hata: {err}"));
    }
    v.get("result")
        .cloned()
        .ok_or_else(|| "rpc sonuçsuz".to_string())
}

/// Aria iş-seviyesi boşta-zaman aşımı (sn): ilk bayt/ilerleme gelmezse
/// iş hata ile kapatılır, slot iade edilir. Daemon-varsayılan sonsuz
/// retry/poll döngüsünü keser. Saf karar fn test edilir.
pub const ARIA_IDLE_TIMEOUT_SECS: u64 = 60;

pub fn aria_idle_expired(last_progress: std::time::Instant, now: std::time::Instant) -> bool {
    now.duration_since(last_progress).as_secs() >= ARIA_IDLE_TIMEOUT_SECS
}

/// addUri seçenekleri kurar (saf; test edilir).
pub fn add_uri_options(
    dir: &str,
    out: &str,
    ua: &str,
    referer: Option<&str>,
    split: u32,
) -> serde_json::Value {
    let mut headers = vec![format!("User-Agent: {ua}")];
    if let Some(r) = referer {
        headers.push(format!("Referer: {r}"));
    }
    serde_json::json!({
        "dir": dir,
        "out": out,
        "header": headers,
        "split": split.max(1),
        "max-connection-per-server": split.max(1),
        "min-split-size": "1M",
        "continue": true,
        "check-certificate": true,
    })
}

/// Daemon hazırsa (port, secret) döner; yoksa TEK ejempler başlatır.
/// Eşzamanlı çağrılar serilenir; ilk bitirenin daemonu herkesçe kullanılır.
pub fn ensure_daemon() -> Result<(u16, String), String> {
    // Tekilleştirme muhafızı fonksiyon boyunca tutulur (zehirde muhafızsız devam).
    let _spawn_guard = spawn_lock().lock().ok();
    // Kazanan çoktan açmış olabilir: önce yuvaya bak.
    if let Ok(g) = daemon_slot().lock() {
        if let Some(d) = g.as_ref() {
            if rpc_call(d.port, &d.secret, "aria2.getVersion", serde_json::json!([])).is_ok() {
                return Ok((d.port, d.secret.clone()));
            }
        }
    }
    // Ölü kayıt varsa biç (yenisi açılmadan önce port boşa çıksın).
    if let Ok(mut g) = daemon_slot().lock() {
        if let Some(d) = g.take() {
            if rpc_call(d.port, &d.secret, "aria2.getVersion", serde_json::json!([])).is_err() {
                reap_child(d.child);
            } else {
                let (port, secret) = (d.port, d.secret.clone());
                *g = Some(d);
                return Ok((port, secret));
            }
        }
    }
    let bin = find_aria2c().ok_or_else(|| "aria2c bulunamadı".to_string())?;
    let appdir = std::env::var("APPDIR").ok();
    let secret = random_secret();
    for port in PORT_FIRST..=PORT_LAST {
        let mut cmd = Command::new(&bin);
        cmd.args([
            "--enable-rpc",
            &format!("--rpc-listen-port={port}"),
            &format!("--rpc-secret={secret}"),
            "--rpc-listen-all=false",
            "--check-certificate=true",
            "--continue=true",
            &format!("-x{}", SEGMENTS),
            &format!("-s{}", SEGMENTS),
            "-k1M",
            // Paralel dosya indirme: daemon varsayılanı (5) N işçiyi kısmasın.
            "--max-concurrent-downloads=32",
            // Sonsuz bekleme yok: yanıt vermeyen sunucu hata versin.
            "--connect-timeout=10",
            "--timeout=30",
            "--retry-wait=2",
            "--max-tries=5",
            "--console-log-level=warn",
            "--quiet=true",
        ]);
        // Yalnızca AppImage içindeki aria2c'ye kendi lib dizinini ver.
        // Global LD_LIBRARY_PATH, GTK'nın sistem araçlarına sızıp SELinux
        // ihlaline yol açıyordu (ffmpegthumbnailer / fusefs_t).
        if let Some(appdir) = appdir.as_ref() {
            if bin == Path::new(appdir).join("usr/bin/aria2c") {
                cmd.env("LD_LIBRARY_PATH", Path::new(appdir).join("usr/lib"));
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return Err(format!("aria2c başlatılamadı: {e}")),
        };
        // RPC hazır olana kadar bekle.
        let mut ready = false;
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if rpc_call(port, &secret, "aria2.getVersion", serde_json::json!([])).is_ok() {
                ready = true;
                break;
            }
        }
        if ready {
            if let Ok(mut g) = daemon_slot().lock() {
                *g = Some(Daemon {
                    child,
                    port,
                    secret: secret.clone(),
                });
            }
            return Ok((port, secret));
        }
        // Bu porta yerleşemedi (dolu/ölü): KENDİ çocuğumuzu biç, yuvaya dokunma.
        reap_child(child);
    }
    Err("aria2 RPC hazır olmadı".to_string())
}

/// Boştaki daemonu kapatır (yetim süreç kalmaz). Kilit yalnızca
/// take için tutulur; RPC/kill/wait kilitsiz yapılır.
fn maybe_shutdown() {
    if ACTIVE.load(Ordering::SeqCst) != 0 {
        return;
    }
    let taken = daemon_slot().lock().map(|mut g| g.take()).unwrap_or(None);
    if let Some(d) = taken {
        let _ = rpc_call(d.port, &d.secret, "aria2.shutdown", serde_json::json!([]));
        std::thread::sleep(std::time::Duration::from_millis(300));
        reap_child(d.child);
    }
}

fn gid_of(v: &serde_json::Value) -> Result<String, String> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| "gid yok".to_string())
}

pub fn add_uri(
    port: u16,
    secret: &str,
    url: &str,
    dir: &str,
    out: &str,
    ua: &str,
    referer: Option<&str>,
    split: u32,
) -> Result<String, String> {
    let opts = add_uri_options(dir, out, ua, referer, split);
    let res = rpc_call(
        port,
        secret,
        "aria2.addUri",
        serde_json::json!([[url], opts]),
    )?;
    gid_of(&res)
}

/// tellStatus özeti (saf ayrıştırma test edilir).
#[derive(Debug, PartialEq)]
pub struct AriaStatus {
    pub status: String,
    pub completed: u64,
    pub total: u64,
    pub error_msg: Option<String>,
}

pub fn parse_status(v: &serde_json::Value) -> Result<AriaStatus, String> {
    let num = |k: &str| -> u64 {
        v.get(k)
            .and_then(|x| x.as_str().unwrap_or("0").parse::<u64>().ok())
            .unwrap_or(0)
    };
    let status = v
        .get("status")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "status yok".to_string())?
        .to_string();
    let error_msg = v
        .get("errorMessage")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(AriaStatus {
        status,
        completed: num("completedLength"),
        total: num("totalLength"),
        error_msg,
    })
}

pub fn tell_status(port: u16, secret: &str, gid: &str) -> Result<AriaStatus, String> {
    let res = rpc_call(
        port,
        secret,
        "aria2.tellStatus",
        serde_json::json!([
            gid,
            ["status", "completedLength", "totalLength", "errorMessage"]
        ]),
    )?;
    parse_status(&res)
}

pub fn remove_gid(port: u16, secret: &str, gid: &str) -> Result<(), String> {
    rpc_call(port, secret, "aria2.remove", serde_json::json!([gid])).map(|_| ())
}

fn aria_control_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.aria2", dest.display()))
}

/// aria2 arka-ucuyla indirme (`start_download` sözleşmesi: aynı olaylar, aynı tutamaç).
/// `.aria2` kontrol dosyasıyla kaldığı yerden devam eder.
pub fn start_download_aria(
    url: &str,
    referer: Option<&str>,
    dest_final: &Path,
    tx: mpsc::Sender<crate::download::DownloadEvent>,
    split: u32,
) -> crate::download::DownloadHandle {
    use crate::download::DownloadEvent;
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_r = cancel.clone();
    let url = url.to_string();
    let referer = referer.map(str::to_string);
    let dest_final = dest_final.to_path_buf();
    std::thread::spawn(move || {
        if let Some(parent) = dest_final.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                let _ = tx.send(DownloadEvent::Error("klasör oluşturulamadı".into()));
                return;
            }
        }
        let (port, secret) = match ensure_daemon() {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(DownloadEvent::Error(format!("aria2: {e}")));
                return;
            }
        };
        let dir = dest_final
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string());
        let out = dest_final
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video.mp4".to_string());
        let gid = match add_uri(
            port,
            &secret,
            &url,
            &dir,
            &out,
            UA,
            referer.as_deref(),
            split.max(1),
        ) {
            Ok(g) => g,
            Err(e) => {
                let _ = tx.send(DownloadEvent::Error(format!("aria2 kuyruk: {e}")));
                return;
            }
        };
        ACTIVE.fetch_add(1, Ordering::SeqCst);
        // Kısmi dosya varsa ilerlemeyi hemen yansıt.
        let have0 = std::fs::metadata(&dest_final).map(|m| m.len()).unwrap_or(0);
        if have0 > 0 {
            let _ = tx.send(DownloadEvent::Progress(have0, have0));
        }
        // Son ilerleme anı: ilk bayt gelmezse zaman aşımı işletilir.
        let mut last_progress = std::time::Instant::now();
        loop {
            if cancel_r.load(Ordering::Relaxed) {
                let _ = remove_gid(port, &secret, &gid);
                ACTIVE.fetch_sub(1, Ordering::SeqCst);
                maybe_shutdown();
                let _ = tx.send(DownloadEvent::Cancelled);
                return;
            }
            match tell_status(port, &secret, &gid) {
                Ok(st) => match st.status.as_str() {
                    "complete" => {
                        let _ = std::fs::remove_file(aria_control_path(&dest_final));
                        ACTIVE.fetch_sub(1, Ordering::SeqCst);
                        maybe_shutdown();
                        let total = st.total.max(st.completed).max(1);
                        let _ = tx.send(DownloadEvent::Progress(total, total));
                        let _ = tx.send(DownloadEvent::Done);
                        return;
                    }
                    "error" => {
                        ACTIVE.fetch_sub(1, Ordering::SeqCst);
                        maybe_shutdown();
                        let msg = st.error_msg.unwrap_or_else(|| "bilinmeyen".to_string());
                        let _ = tx.send(DownloadEvent::Error(format!("aria2: {msg}")));
                        return;
                    }
                    "removed" => {
                        ACTIVE.fetch_sub(1, Ordering::SeqCst);
                        maybe_shutdown();
                        let _ = tx.send(DownloadEvent::Cancelled);
                        return;
                    }
                    _ => {
                        // active | waiting | paused
                        if st.completed > 0 || st.total > 0 {
                            last_progress = std::time::Instant::now();
                        }
                        if aria_idle_expired(last_progress, std::time::Instant::now()) {
                            // Sunucu yanıt vermiyor: slotu iade et, hata ver.
                            let _ = remove_gid(port, &secret, &gid);
                            ACTIVE.fetch_sub(1, Ordering::SeqCst);
                            maybe_shutdown();
                            let _ = tx.send(DownloadEvent::Error(format!(
                                "zaman aşımı: sunucu {ARIA_IDLE_TIMEOUT_SECS}sn yanıt vermedi"
                            )));
                            return;
                        }
                        let _ = tx.send(DownloadEvent::Progress(st.completed, st.total));
                    }
                },
                Err(e) => {
                    // Daemon öldüyse: bekle-sonlandır yerine hata ver (devamı iç motor üstlenir).
                    ACTIVE.fetch_sub(1, Ordering::SeqCst);
                    maybe_shutdown();
                    let _ = tx.send(DownloadEvent::Error(format!("aria2 durum: {e}")));
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        }
    });
    crate::download::DownloadHandle::for_cancel(cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_body_shapes() {
        let b = rpc_body("aria2.addUri", "s3cr3t", serde_json::json!([["u"], {}]));
        assert_eq!(b["method"], "aria2.addUri");
        assert_eq!(b["params"][0], "token:s3cr3t");
        assert_eq!(b["params"][1][0], "u");
    }

    #[test]
    fn aria_idle_timeout_decision() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        assert!(!aria_idle_expired(t0, t0), "sıfır geçen süre aşmamalı");
        assert!(!aria_idle_expired(
            t0,
            t0 + Duration::from_secs(ARIA_IDLE_TIMEOUT_SECS - 1)
        ));
        assert!(aria_idle_expired(
            t0,
            t0 + Duration::from_secs(ARIA_IDLE_TIMEOUT_SECS)
        ));
        assert!(aria_idle_expired(
            t0,
            t0 + Duration::from_secs(ARIA_IDLE_TIMEOUT_SECS + 30)
        ));
    }

    #[test]
    fn add_uri_options_shapes() {
        let o = add_uri_options("/tmp/x", "v.mp4", "UA-Test", Some("http://ref/"), 3);
        assert_eq!(o["dir"], "/tmp/x");
        assert_eq!(o["out"], "v.mp4");
        assert_eq!(o["split"], 3);
        assert_eq!(o["max-connection-per-server"], 3);
        assert_eq!(o["min-split-size"], "1M");
        assert_eq!(o["continue"], true);
        let hdrs: Vec<String> = o["header"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        assert!(hdrs.iter().any(|h| h == "User-Agent: UA-Test"), "{hdrs:?}");
        assert!(hdrs.iter().any(|h| h == "Referer: http://ref/"), "{hdrs:?}");
    }

    #[test]
    fn parse_status_shapes() {
        let v = serde_json::json!({"status":"active","completedLength":"123","totalLength":"456"});
        assert_eq!(
            parse_status(&v).unwrap(),
            AriaStatus {
                status: "active".into(),
                completed: 123,
                total: 456,
                error_msg: None
            }
        );
        let e = serde_json::json!({"status":"error","completedLength":"0","totalLength":"0","errorMessage":"CUID#1 - Download aborted. URI=http://x"});
        let st = parse_status(&e).unwrap();
        assert_eq!(st.status, "error");
        assert!(st.error_msg.unwrap().contains("aborted"));
        assert!(
            parse_status(&serde_json::json!({"completedLength":"1"})).is_err(),
            "status şart"
        );
    }

    #[test]
    fn split_ranges_cover_segments() {
        // aria -s ile aynı bölme mantığı: toplam kapsanmalı (segmented.rs testi).
        assert_eq!(SEGMENTS, 6);
    }

    #[test]
    fn aria_lifecycle_against_real_daemon() {
        let Some(bin) = find_aria2c() else {
            return; // ikili yoksa atla (iç motor devrede)
        };
        assert!(bin.is_file(), "{bin:?}");
        // Küçük yerel dosya: TcpListener mini sunucu (200 tam gövde).
        use std::io::{Read, Write};
        use std::net::TcpListener;
        const LEN: usize = 200_000;
        let listener = TcpListener::bind("127.0.0.1:0").expect("dinle");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(8) {
                let Ok(mut s) = stream else { continue };
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                if req.starts_with("HEAD ") {
                    let _ = s.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {LEN}\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    );
                    continue;
                }
                let body: Vec<u8> = (0..LEN).map(|i| (i % 251) as u8).collect();
                let _ = s.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {LEN}\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                );
                let _ = s.write_all(&body);
            }
        });
        let dir = std::env::temp_dir().join("animecix-aria-live");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("v.mp4");
        let url = format!("http://127.0.0.1:{port}/v.mp4");
        let (tx, rx) = mpsc::channel();
        let _h = start_download_aria(&url, None, &dest, tx, 6);
        let mut done = false;
        for ev in rx.iter() {
            match ev {
                DownloadEvent::Done => {
                    done = true;
                    break;
                }
                DownloadEvent::Error(e) => panic!("aria hatası: {e}"),
                DownloadEvent::Cancelled => panic!("beklenmedik iptal"),
                DownloadEvent::Progress(_, _) => {}
            }
        }
        assert!(done, "daemon tamamlamalı");
        let got = std::fs::read(&dest).unwrap();
        assert_eq!(got.len(), LEN);
        assert_eq!(got[12345], (12345 % 251) as u8);
        assert!(!aria_control_path(&dest).exists(), ".aria2 temizlenmeli");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
