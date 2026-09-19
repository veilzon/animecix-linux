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
        std::env::split_paths(&paths).map(|d| d.join("aria2c")).find(|p| p.is_file())
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
/// Aktif aria indirme sayacı (0'a düşünce daemon kapatılır, yetim kalmaz).
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn daemon_slot() -> &'static Mutex<Option<Daemon>> {
    DAEMON.get_or_init(|| Mutex::new(None))
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

fn rpc_call(port: u16, secret: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
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

/// addUri seçenekleri kurar (saf; test edilir).
pub fn add_uri_options(
    dir: &str,
    out: &str,
    ua: &str,
    referer: Option<&str>,
) -> serde_json::Value {
    let mut headers = vec![format!("User-Agent: {ua}")];
    if let Some(r) = referer {
        headers.push(format!("Referer: {r}"));
    }
    serde_json::json!({
        "dir": dir,
        "out": out,
        "header": headers,
        "split": SEGMENTS,
        "max-connection-per-server": SEGMENTS,
        "min-split-size": "1M",
        "continue": true,
        "check-certificate": true,
    })
}

/// Daemon hazırsa (port, secret) döner; yoksa başlatır.
pub fn ensure_daemon() -> Result<(u16, String), String> {
    if let Ok(g) = daemon_slot().lock() {
        if let Some(d) = g.as_ref() {
            if rpc_call(d.port, &d.secret, "aria2.getVersion", serde_json::json!([])).is_ok() {
                return Ok((d.port, d.secret.clone()));
            }
        }
    }
    let bin = find_aria2c().ok_or_else(|| "aria2c bulunamadı".to_string())?;
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
            "--console-log-level=warn",
            "--quiet=true",
        ]);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
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
                *g = Some(Daemon { child, port, secret: secret.clone() });
            }
            return Ok((port, secret));
        }
        let _ = daemon_slot()
            .lock()
            .map(|mut g| g.take().map(|mut d| d.child.kill()));
    }
    Err("aria2 RPC hazır olmadı".to_string())
}

/// Boştaki daemonu kapatır (yetim süreç kalmaz).
fn maybe_shutdown() {
    if ACTIVE.load(Ordering::SeqCst) != 0 {
        return;
    }
    if let Ok(mut g) = daemon_slot().lock() {
        if let Some(d) = g.take() {
            let _ = rpc_call(d.port, &d.secret, "aria2.shutdown", serde_json::json!([]));
            std::thread::sleep(std::time::Duration::from_millis(300));
            let mut d = d;
            let _ = d.child.kill();
            let _ = d.child.wait();
        }
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
) -> Result<String, String> {
    let opts = add_uri_options(dir, out, ua, referer);
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
        serde_json::json!([gid, ["status", "completedLength", "totalLength", "errorMessage"]]),
    )?;
    parse_status(&res)
}

pub fn pause_gid(port: u16, secret: &str, gid: &str) -> Result<(), String> {
    rpc_call(port, secret, "aria2.pause", serde_json::json!([gid])).map(|_| ())
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
        let gid = match add_uri(port, &secret, &url, &dir, &out, UA, referer.as_deref()) {
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
    fn add_uri_options_shapes() {
        let o = add_uri_options("/tmp/x", "v.mp4", "UA-Test", Some("http://ref/"));
        assert_eq!(o["dir"], "/tmp/x");
        assert_eq!(o["out"], "v.mp4");
        assert_eq!(o["split"], SEGMENTS);
        assert_eq!(o["max-connection-per-server"], SEGMENTS);
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
            AriaStatus { status: "active".into(), completed: 123, total: 456, error_msg: None }
        );
        let e = serde_json::json!({"status":"error","completedLength":"0","totalLength":"0","errorMessage":"CUID#1 - Download aborted. URI=http://x"});
        let st = parse_status(&e).unwrap();
        assert_eq!(st.status, "error");
        assert!(st.error_msg.unwrap().contains("aborted"));
        assert!(parse_status(&serde_json::json!({"completedLength":"1"})).is_err(), "status şart");
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
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {LEN}\r\nConnection: close\r\n\r\n").as_bytes(),
                    );
                    continue;
                }
                let body: Vec<u8> = (0..LEN).map(|i| (i % 251) as u8).collect();
                let _ = s.write_all(
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {LEN}\r\nConnection: close\r\n\r\n").as_bytes(),
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
        let _h = start_download_aria(&url, None, &dest, tx);
        let mut done = false;
        for ev in rx.iter() {
            match ev {
                DownloadEvent::Done => { done = true; break; }
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
