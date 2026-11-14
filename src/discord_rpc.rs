//! Discord Rich Presence: yerel IPC soketi üzerinden, harici crate olmadan.
//!
//! Discord masaüstü uygulaması kapalıysa sessizce hiçbir şey yapılmaz.
//! Soket yolu otomatik bulunur; Ayarlar'dan manuel de verilebilir.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Discord Developer Portal'daki AnimeciX uygulamasının Application ID'si.
pub const CLIENT_ID: &str = "1568654241290125382";

const MAX_FRAME: usize = 1024 * 1024;
const RECONNECT_AFTER: Duration = Duration::from_secs(30);
const READY_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscordConfig {
    pub enabled: bool,
    /// Boşsa otomatik; tam soket yolu, klasör veya `{0}` içeren şablon.
    pub socket: Option<String>,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            socket: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub details: String,
    pub state: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
}

impl Activity {
    /// Oynatma bilgisinden Discord aktivitesi kurar (saf).
    ///
    /// v1.3.1'de süre/geri sayım bilinçli olarak gönderilmiyor.
    pub fn from_playback(
        details: &str,
        state: &str,
        _pos: f64,
        _dur: f64,
        _paused: bool,
        _poster: Option<&str>,
    ) -> Activity {
        Activity {
            details: truncate(details, 128),
            state: truncate(state, 128),
            start_ms: None,
            end_ms: None,
            large_image: None,
            large_text: Some(truncate(details, 128)),
            small_image: None,
            small_text: None,
        }
    }
}

/// Bir bölümün bitip bitmediği (saf): MPV `--keep-open=yes` ile açık kaldığı
/// için EOF'ta `Closed` gelmez; presence'ı bu eşikte temizleriz.
pub fn playback_finished(pos: f64, dur: f64) -> bool {
    dur > 0.0 && pos >= 0.0 && pos / dur >= 0.995
}

#[derive(Clone)]
pub struct DiscordRpc {
    tx: Sender<Command>,
}

enum Command {
    Activity(Activity),
    Clear,
    Configure(DiscordConfig),
    Shutdown,
}

impl DiscordRpc {
    pub fn start(config: DiscordConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || run_worker(rx, config));
        Self { tx }
    }

    pub fn set_activity(&self, activity: Activity) {
        let _ = self.tx.send(Command::Activity(activity));
    }

    pub fn clear(&self) {
        let _ = self.tx.send(Command::Clear);
    }

    pub fn configure(&self, config: DiscordConfig) {
        let _ = self.tx.send(Command::Configure(config));
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

pub fn candidate_paths(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let dirs = [
        base.join("app/com.discordapp.Discord"),
        base.join("app/com.discordapp.DiscordCanary"),
        base.join(".flatpak/com.discordapp.Discord/xdg-run"),
        base.join(".flatpak/com.discordapp.DiscordCanary/xdg-run"),
        base.join(".flatpak/dev.vencord.Vesktop/xdg-run"),
        base.join("snap.discord"),
        base.join("snap.discord-canary"),
    ];
    for dir in &dirs {
        for i in 0..10 {
            out.push(dir.join(format!("discord-ipc-{i}")));
        }
    }
    for i in 0..10 {
        out.push(base.join(format!("discord-ipc-{i}")));
    }
    out
}

/// Manuel yol/klasör/şablon veya otomatik adaylardan var olan soketler.
pub fn candidate_socket_paths(config: &DiscordConfig) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(raw) = config
        .socket
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if raw.contains("{0}") {
            for i in 0..10 {
                let p = PathBuf::from(raw.replace("{0}", &i.to_string()));
                if is_socket(&p) {
                    out.push(p);
                }
            }
        } else {
            let p = PathBuf::from(raw);
            if is_socket(&p) {
                out.push(p);
            } else if p.is_dir() {
                for i in 0..10 {
                    let c = p.join(format!("discord-ipc-{i}"));
                    if is_socket(&c) {
                        out.push(c);
                    }
                }
            }
        }
        return out;
    }

    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    let mut paths = candidate_paths(&base);
    let tmp = std::env::temp_dir();
    if tmp != base {
        paths.extend(candidate_paths(&tmp));
    }
    for p in paths {
        if is_socket(&p) && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

#[allow(dead_code)]
pub fn resolve_socket_path(config: &DiscordConfig) -> Option<PathBuf> {
    candidate_socket_paths(config).into_iter().next()
}

fn is_socket(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| std::os::unix::fs::FileTypeExt::is_socket(&m.file_type()))
        .unwrap_or(false)
}

pub fn encode_frame(opcode: u32, payload: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(payload).unwrap_or_default();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&opcode.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

pub fn decode_frame(bytes: &[u8]) -> Option<(u32, Value)> {
    if bytes.len() < 8 {
        return None;
    }
    let opcode = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let len = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    if len > MAX_FRAME || bytes.len() < 8 + len {
        return None;
    }
    let value = serde_json::from_slice(&bytes[8..8 + len]).ok()?;
    Some((opcode, value))
}

pub fn activity_payload(activity: &Activity) -> Value {
    let mut obj = json!({
        "details": activity.details.as_str(),
        "state": activity.state.as_str(),
        "instance": false,
    });
    if let (Some(start), Some(end)) = (activity.start_ms, activity.end_ms) {
        obj["timestamps"] = json!({ "start": start, "end": end });
    }
    let mut assets = serde_json::Map::new();
    if let Some(v) = &activity.large_image {
        assets.insert("large_image".into(), json!(v));
    }
    if let Some(v) = &activity.large_text {
        assets.insert("large_text".into(), json!(v));
    }
    if let Some(v) = &activity.small_image {
        assets.insert("small_image".into(), json!(v));
    }
    if let Some(v) = &activity.small_text {
        assets.insert("small_text".into(), json!(v));
    }
    if !assets.is_empty() {
        obj["assets"] = Value::Object(assets);
    }
    obj
}

pub fn episode_state(season: u64, episode: u64, name: &str, is_movie: bool) -> String {
    if is_movie {
        return "Film".to_string();
    }
    let label = format!("S{:02}B{:02}", season, episode);
    let name = name.trim();
    if name.is_empty() {
        label
    } else {
        format!("{label} · {name}")
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn read_frame(stream: &mut UnixStream) -> Option<(u32, Value)> {
    let mut head = [0u8; 8];
    stream.read_exact(&mut head).ok()?;
    let len = u32::from_le_bytes(head[4..8].try_into().ok()?) as usize;
    if len > MAX_FRAME {
        return None;
    }
    let mut bytes = Vec::with_capacity(8 + len);
    bytes.extend_from_slice(&head);
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).ok()?;
    bytes.extend_from_slice(&body);
    decode_frame(&bytes)
}

fn write_frame(stream: &mut UnixStream, opcode: u32, payload: &Value) -> std::io::Result<()> {
    stream.write_all(&encode_frame(opcode, payload))
}

/// Tek bir Discord bağlantısı: yazma tarafı kilitli, okuma/PING/PONG ayrı
/// thread'de. Böylece Discord'un yanıtları birikip soketi tıkamaz.
struct Connection {
    write: Arc<Mutex<UnixStream>>,
    closed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Connection {
    fn open(path: &Path) -> Option<Self> {
        let mut stream = UnixStream::connect(path).ok()?;
        let _ = stream.set_read_timeout(Some(READY_TIMEOUT));
        let _ = stream.set_write_timeout(Some(READY_TIMEOUT));
        write_frame(&mut stream, 0, &json!({ "v": 1, "client_id": CLIENT_ID })).ok()?;
        let (_, hello) = read_frame(&mut stream)?;
        if hello["evt"].as_str() != Some("READY") {
            return None;
        }
        let _ = stream.set_read_timeout(None);
        let _ = stream.set_write_timeout(Some(READY_TIMEOUT));
        let read_stream = stream.try_clone().ok()?;
        let write = Arc::new(Mutex::new(stream));
        let closed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let write = write.clone();
            let closed = closed.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("discord-rpc-read".into())
                .spawn(move || {
                    let mut read_stream = read_stream;
                    while !stop.load(Ordering::Relaxed) {
                        match read_frame(&mut read_stream) {
                            Some((3, payload)) => {
                                if let Ok(mut s) = write.lock() {
                                    let _ = write_frame(&mut s, 4, &payload);
                                }
                            }
                            Some((2, _)) => break,
                            Some(_) => {}
                            None => break,
                        }
                    }
                    closed.store(true, Ordering::Relaxed);
                })
                .ok()?
        };
        Some(Self {
            write,
            closed,
            stop,
            reader: Some(reader),
        })
    }

    fn alive(&self) -> bool {
        !self.closed.load(Ordering::Relaxed)
    }

    fn send(&self, activity: Option<&Activity>) -> bool {
        if !self.alive() {
            return false;
        }
        let activity = activity.map(activity_payload).unwrap_or(Value::Null);
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": std::process::id(),
                "activity": activity,
            },
            "nonce": nonce,
        });
        let mut s = match self.write.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if write_frame(&mut s, 1, &payload).is_ok() {
            true
        } else {
            self.closed.store(true, Ordering::Relaxed);
            false
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(s) = self.write.lock() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
    }
}

fn connect(config: &DiscordConfig) -> Option<Connection> {
    for path in candidate_socket_paths(config) {
        if let Some(conn) = Connection::open(&path) {
            return Some(conn);
        }
    }
    None
}

fn try_send(
    conn: &mut Option<Connection>,
    activity: Option<&Activity>,
    last_activity: &mut Option<Activity>,
    sent_on_conn: &mut bool,
    next_retry: &mut Instant,
) {
    let Some(c) = conn.as_ref() else {
        return;
    };
    if !c.alive() {
        *conn = None;
        *sent_on_conn = false;
        *next_retry = Instant::now();
        return;
    }
    if activity.is_some() && *sent_on_conn && last_activity.as_ref() == activity {
        return;
    }
    if c.send(activity) {
        *last_activity = activity.cloned();
        *sent_on_conn = activity.is_some();
    } else {
        *conn = None;
        *sent_on_conn = false;
        *next_retry = Instant::now() + RECONNECT_AFTER;
    }
}

fn run_worker(rx: Receiver<Command>, mut config: DiscordConfig) {
    let mut conn: Option<Connection> = None;
    let mut pending: Option<Activity> = None;
    let mut last_activity: Option<Activity> = None;
    let mut sent_on_conn = false;
    let mut next_retry = Instant::now();

    loop {
        if let Some(c) = conn.as_ref() {
            if !c.alive() {
                conn = None;
                sent_on_conn = false;
                next_retry = Instant::now();
            }
        }
        if conn.is_none() && config.enabled && Instant::now() >= next_retry {
            conn = connect(&config);
            sent_on_conn = false;
            if conn.is_none() {
                next_retry = Instant::now() + RECONNECT_AFTER;
            }
        }
        if conn.is_some() && pending.is_some() {
            try_send(
                &mut conn,
                pending.as_ref(),
                &mut last_activity,
                &mut sent_on_conn,
                &mut next_retry,
            );
        }

        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(Command::Activity(activity)) => {
                pending = Some(activity);
                if !config.enabled {
                    continue;
                }
                if conn.is_none() {
                    conn = connect(&config);
                    sent_on_conn = false;
                    if conn.is_none() {
                        next_retry = Instant::now() + RECONNECT_AFTER;
                    }
                }
                continue;
            }
            Ok(Command::Clear) => {
                pending = None;
                last_activity = None;
                if let Some(c) = conn.as_ref() {
                    if !c.send(None) {
                        conn = None;
                        next_retry = Instant::now() + RECONNECT_AFTER;
                    }
                }
                sent_on_conn = false;
                continue;
            }
            Ok(Command::Configure(new_config)) => {
                let socket_changed = new_config.socket != config.socket;
                config = new_config;
                if !config.enabled {
                    if let Some(c) = conn.as_ref() {
                        let _ = c.send(None);
                    }
                    conn = None;
                    pending = None;
                    last_activity = None;
                    sent_on_conn = false;
                } else if socket_changed || conn.is_none() {
                    conn = None;
                    sent_on_conn = false;
                    next_retry = Instant::now();
                }
                continue;
            }
            Ok(Command::Shutdown) => {
                if let Some(c) = conn.as_ref() {
                    let _ = c.send(None);
                }
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if conn.is_none() && config.enabled && Instant::now() >= next_retry {
                    conn = connect(&config);
                    sent_on_conn = false;
                    if conn.is_none() {
                        next_retry = Instant::now() + RECONNECT_AFTER;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::time::Duration;

    fn temp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "animecix-discord-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn candidate_paths_cover_flatpak_and_snap() {
        let paths = candidate_paths(Path::new("/run/user/1000"));
        let text: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert!(text.iter().any(|p| p.ends_with("/discord-ipc-0")));
        assert!(text
            .iter()
            .any(|p| p.contains("/app/com.discordapp.Discord/discord-ipc-0")));
        assert!(text
            .iter()
            .any(|p| p.contains("/snap.discord/discord-ipc-0")));
        assert!(text
            .iter()
            .any(|p| p.contains("/.flatpak/dev.vencord.Vesktop/xdg-run/discord-ipc-0")));
        assert!(text
            .iter()
            .any(|p| p.contains("/.flatpak/com.discordapp.Discord/xdg-run/discord-ipc-0")));
    }

    #[test]
    fn manual_socket_file_is_used() {
        let dir = temp_dir("manual-file");
        let path = dir.join("discord-ipc-0");
        let _listener = UnixListener::bind(&path).unwrap();
        let cfg = DiscordConfig {
            enabled: true,
            socket: Some(path.to_string_lossy().into_owned()),
        };
        assert_eq!(resolve_socket_path(&cfg), Some(path));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn manual_socket_dir_scans_ipc_files() {
        let dir = temp_dir("manual-dir");
        let path = dir.join("discord-ipc-3");
        let _listener = UnixListener::bind(&path).unwrap();
        let cfg = DiscordConfig {
            enabled: true,
            socket: Some(dir.to_string_lossy().into_owned()),
        };
        assert_eq!(resolve_socket_path(&cfg), Some(path));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn manual_placeholder_expands() {
        let dir = temp_dir("manual-placeholder");
        let path = dir.join("discord-ipc-2");
        let _listener = UnixListener::bind(&path).unwrap();
        let cfg = DiscordConfig {
            enabled: true,
            socket: Some(dir.join("discord-ipc-{0}").to_string_lossy().into_owned()),
        };
        assert_eq!(resolve_socket_path(&cfg), Some(path));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn frame_roundtrip() {
        let payload = json!({ "v": 1, "client_id": CLIENT_ID });
        let bytes = encode_frame(0, &payload);
        let (op, decoded) = decode_frame(&bytes).unwrap();
        assert_eq!(op, 0);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn activity_payload_has_text_and_timestamps() {
        let a = Activity {
            details: "Frieren".into(),
            state: "S01B05 · Bölüm".into(),
            start_ms: Some(1_700_000_000_000),
            end_ms: Some(1_700_001_440_000),
            large_image: None,
            large_text: Some("Frieren".into()),
            small_image: None,
            small_text: None,
        };
        let v = activity_payload(&a);
        assert_eq!(v["details"], "Frieren");
        assert_eq!(v["state"], "S01B05 · Bölüm");
        assert_eq!(v["timestamps"]["start"], 1_700_000_000_000i64);
        assert_eq!(v["timestamps"]["end"], 1_700_001_440_000i64);
        assert_eq!(v["assets"]["large_text"], "Frieren");
    }

    #[test]
    fn playback_activity_has_no_timestamps() {
        let a = Activity::from_playback("Frieren", "S01B05", 10.0, 100.0, false, None);
        assert_eq!(a.start_ms, None);
        assert_eq!(a.end_ms, None);
        let v = activity_payload(&a);
        assert!(v.get("timestamps").is_none());
    }

    #[test]
    fn playback_finished_threshold() {
        assert!(!playback_finished(99.4, 100.0));
        assert!(playback_finished(99.5, 100.0));
        assert!(playback_finished(100.0, 100.0));
        assert!(!playback_finished(100.0, 0.0));
    }

    #[test]
    fn short_or_oversized_frame_is_rejected() {
        assert!(decode_frame(&[0, 0, 0]).is_none());
        let mut bytes = vec![1, 0, 0, 0];
        bytes.extend_from_slice(&(MAX_FRAME as u32 + 1).to_le_bytes());
        assert!(decode_frame(&bytes).is_none());
    }

    #[test]
    fn long_text_is_truncated() {
        let a = Activity::from_playback(&"x".repeat(200), &"y".repeat(200), 0.0, 0.0, false, None);
        assert_eq!(a.details.chars().count(), 128);
        assert_eq!(a.state.chars().count(), 128);
    }

    #[test]
    fn episode_state_shapes() {
        assert_eq!(
            episode_state(1, 5, "Başlangıç", false),
            "S01B05 · Başlangıç"
        );
        assert_eq!(episode_state(1, 5, "", false), "S01B05");
        assert_eq!(episode_state(1, 1, "Suzume", true), "Film");
    }

    fn fake_ready_and_activity(
        listener: UnixListener,
        tx_activity: std::sync::mpsc::Sender<serde_json::Value>,
        tx_pong: std::sync::mpsc::Sender<serde_json::Value>,
        tx_clear: std::sync::mpsc::Sender<serde_json::Value>,
    ) {
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (op, hello) = read_frame(&mut stream).unwrap();
            assert_eq!(op, 0);
            assert_eq!(hello["client_id"], CLIENT_ID);
            write_frame(
                &mut stream,
                1,
                &json!({ "cmd": "DISPATCH", "evt": "READY", "data": {} }),
            )
            .unwrap();
            let (op2, payload) = read_frame(&mut stream).unwrap();
            assert_eq!(op2, 1);
            let _ = tx_activity.send(payload);
            write_frame(&mut stream, 3, &json!({ "ping": 1 })).unwrap();
            let (op3, pong) = read_frame(&mut stream).unwrap();
            assert_eq!(op3, 4);
            let _ = tx_pong.send(pong);
            let (op4, clear) = read_frame(&mut stream).unwrap();
            assert_eq!(op4, 1);
            let _ = tx_clear.send(clear);
            std::thread::sleep(Duration::from_secs(2));
        });
    }

    #[test]
    fn fake_discord_receives_handshake_activity_and_pong() {
        let dir = temp_dir("fake-discord");
        let path = dir.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx_activity, rx_activity) = std::sync::mpsc::channel();
        let (tx_pong, rx_pong) = std::sync::mpsc::channel();
        let (tx_clear, rx_clear) = std::sync::mpsc::channel();
        fake_ready_and_activity(listener, tx_activity, tx_pong, tx_clear);
        let rpc = DiscordRpc::start(DiscordConfig {
            enabled: true,
            socket: Some(path.to_string_lossy().into_owned()),
        });
        rpc.set_activity(Activity {
            details: "Frieren".into(),
            state: "S01B05".into(),
            start_ms: None,
            end_ms: None,
            large_image: None,
            large_text: None,
            small_image: None,
            small_text: None,
        });
        let payload = rx_activity
            .recv_timeout(Duration::from_secs(5))
            .expect("activity frame");
        assert_eq!(payload["cmd"], "SET_ACTIVITY");
        assert_eq!(payload["args"]["activity"]["details"], "Frieren");
        let pong = rx_pong
            .recv_timeout(Duration::from_secs(5))
            .expect("pong frame");
        assert_eq!(pong["ping"], 1);
        rpc.clear();
        let clear = rx_clear
            .recv_timeout(Duration::from_secs(5))
            .expect("clear frame");
        assert!(clear["args"]["activity"].is_null());
        rpc.shutdown();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_socket_falls_back_to_next_candidate() {
        let dir = temp_dir("stale-fallback");
        let stale = dir.join("discord-ipc-0");
        {
            let _listener = UnixListener::bind(&stale).unwrap();
        }
        let live = dir.join("discord-ipc-1");
        let listener = UnixListener::bind(&live).unwrap();
        let (tx_activity, rx_activity) = std::sync::mpsc::channel();
        let (tx_pong, _rx_pong) = std::sync::mpsc::channel();
        let (tx_clear, _rx_clear) = std::sync::mpsc::channel();
        fake_ready_and_activity(listener, tx_activity, tx_pong, tx_clear);
        let rpc = DiscordRpc::start(DiscordConfig {
            enabled: true,
            socket: Some(dir.to_string_lossy().into_owned()),
        });
        rpc.set_activity(Activity {
            details: "Fallback".into(),
            state: "S01B01".into(),
            start_ms: None,
            end_ms: None,
            large_image: None,
            large_text: None,
            small_image: None,
            small_text: None,
        });
        let payload = rx_activity
            .recv_timeout(Duration::from_secs(5))
            .expect("fallback activity frame");
        assert_eq!(payload["args"]["activity"]["details"], "Fallback");
        rpc.shutdown();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[ignore]
    fn live_discord_socket_is_detected() {
        let found = resolve_socket_path(&DiscordConfig::default());
        assert!(found.is_some(), "Discord IPC soketi bulunamadı");
    }
}
