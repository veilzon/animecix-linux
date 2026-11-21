//! Yayın takibi verisi: takip listesi, bildirim defteri, kilitleme ve
//! atomik yazma. GTK'dan bağımsızdır; GUI ve `animecix-calendar` paylaşır.
//!
//! Dosya sahipliği:
//! - `follows.json` yalnız GUI tarafından yazılır, daemon salt okur.
//! - `ledger.json` iki tarafça `flock` altında güncellenir.
//! - `gui-active.lock` GUI penceresi aktifken tutulur; daemon bunu görüp
//!   masaüstü bildirimi göndermez (uygulama içi toast önceliklidir).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SCHEMA: u32 = 1;
pub const DAILY_LIMIT: u32 = 6;
pub const PENDING_TTL_SECS: u64 = 7 * 86_400;
pub const NEW_WINDOW_SECS: i64 = 72 * 3600;
pub const SENT_RETENTION_SECS: u64 = 30 * 86_400;
/// Uygulama kapalıyken gönderilen bildirimin uygulama içinde en fazla bu
/// kadar sonra gösterilmesi.
pub const IN_APP_WINDOW_SECS: u64 = 72 * 3600;

pub const STATE_PENDING: &str = "pending";
pub const STATE_SENDING: &str = "sending";
pub const STATE_SENT: &str = "sent";

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Yollar
// ---------------------------------------------------------------------------

pub fn data_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("ANIMECIX_FOLLOW_DIR") {
        return PathBuf::from(v);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".local/share/animecix/follow")
}

pub fn follows_path() -> PathBuf {
    data_dir().join("follows.json")
}

pub fn ledger_path() -> PathBuf {
    data_dir().join("ledger.json")
}

#[allow(dead_code)] // animecix-calendar ikilisi kullanır.
pub fn daemon_path() -> PathBuf {
    data_dir().join("daemon.json")
}

pub fn lock_path() -> PathBuf {
    data_dir().join("ledger.lock")
}

pub fn gui_active_path() -> PathBuf {
    data_dir().join("gui-active.lock")
}

#[allow(dead_code)] // animecix-calendar / testler kullanır.
pub fn daemon_lock_path() -> PathBuf {
    data_dir().join("daemon.lock")
}

/// XDG autostart girdisi (yalnız GUI yazar/siler).
pub fn autostart_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/autostart/tr.com.animecix.calendar.desktop")
}

pub fn autostart_content(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=AnimeciX Yayın Takibi\n\
         Comment=Yeni bölümleri arka planda denetler\n\
         Exec={exec}\n\
         Icon=tr.com.animecix\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=20\n\
         X-AnimeciX-Managed=1\n"
    )
}

pub fn write_autostart(exec: &str) -> std::io::Result<()> {
    write_atomic(&autostart_path(), autostart_content(exec).as_bytes())
}

pub fn remove_autostart() {
    let _ = std::fs::remove_file(autostart_path());
}

#[allow(dead_code)] // animecix-calendar / testler kullanır.
pub fn autostart_installed() -> bool {
    autostart_path().is_file()
}

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// PID'in gerçekten takipçi olduğunu doğrular (PID yeniden kullanımına karşı).
pub fn pid_is_watch_daemon(pid: u32) -> bool {
    if !pid_alive(pid) {
        return false;
    }
    let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let cmd = String::from_utf8_lossy(&raw).replace('\0', " ");
    cmd.contains("animecix-calendar") || cmd.contains("animecix_calendar")
}

/// Çalışan takipçiye nazikçe SIGTERM gönderir (yalnız doğrulanmış PID).
pub fn stop_daemon() {
    let st = load_daemon_status();
    if !st.running || !pid_is_watch_daemon(st.pid) {
        return;
    }
    unsafe {
        libc::kill(st.pid as libc::pid_t, libc::SIGTERM);
    }
}

/// Takipçiyi durdurur ve süreç gerçekten çıkana kadar bekler.
/// SIGTERM yeterli olmazsa doğrulanmış PID'e SIGKILL gönderir.
pub fn stop_daemon_and_wait(timeout: Duration) -> bool {
    let st = load_daemon_status();
    if !st.running || !pid_is_watch_daemon(st.pid) {
        return true;
    }
    stop_daemon();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(st.pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if pid_is_watch_daemon(st.pid) {
        unsafe {
            libc::kill(st.pid as libc::pid_t, libc::SIGKILL);
        }
    }
    for _ in 0..10 {
        if !pid_alive(st.pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !pid_alive(st.pid)
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DaemonStatus {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub started_at: u64,
    #[serde(default)]
    pub last_check: u64,
    #[serde(default)]
    pub last_success: u64,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub running: bool,
}

pub fn load_daemon_status_from(path: &Path) -> DaemonStatus {
    read_json::<DaemonStatus>(path).0.unwrap_or_default()
}

#[allow(dead_code)] // animecix-calendar / testler kullanır.
pub fn save_daemon_status_to(path: &Path, st: &DaemonStatus) -> std::io::Result<()> {
    write_json_atomic(path, st)
}

// ---------------------------------------------------------------------------
// Takip listesi
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FollowedTitle {
    pub title_id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub title_type: Option<String>,
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default)]
    pub added_at: u64,
    /// Bölüm listesi kontrolü için sezon sayısı; eski kayıtlarda boş olabilir.
    #[serde(default)]
    pub season_count: Option<u64>,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FollowsFile {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub app_min: String,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub items: Vec<FollowedTitle>,
    /// Yüklenen dosyanın şeması bu sürümce destekleniyor mu? (serde dışı)
    #[serde(skip)]
    pub schema_ok: bool,
    /// Dosya okunamadı/ayrıştırılamadı ve yedeklendi mi? (serde dışı)
    #[serde(skip)]
    pub corrupt: bool,
}

fn default_schema() -> u32 {
    SCHEMA
}

impl Default for FollowsFile {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            app_min: String::new(),
            updated_at: 0,
            items: Vec::new(),
            schema_ok: true,
            corrupt: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Daily {
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub sent: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LedgerEntry {
    #[serde(default)]
    pub season: u64,
    #[serde(default)]
    pub episode: u64,
    #[serde(default)]
    pub episode_name: String,
    #[serde(default)]
    pub release_epoch: i64,
    #[serde(default)]
    pub detected_at: u64,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub next_attempt: u64,
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub last_error: String,
    /// Uygulama içi bildirim gösterildi mi? Masaüstü bildirimi gönderilse
    /// bile uygulama açılınca kaçırılan bölüm bir kez gösterilir.
    #[serde(default)]
    pub in_app_seen: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TitleState {
    #[serde(default)]
    pub name: String,
    /// Sezon → görülen en yüksek bölüm numarası.
    #[serde(default)]
    pub seen: BTreeMap<u64, u64>,
    #[serde(default)]
    pub entries: Vec<LedgerEntry>,
}

impl Default for TitleState {
    fn default() -> Self {
        Self {
            name: String::new(),
            seen: BTreeMap::new(),
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub daily: Daily,
    #[serde(default)]
    pub titles: BTreeMap<String, TitleState>,
    #[serde(skip)]
    pub schema_ok: bool,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            daily: Daily::default(),
            titles: BTreeMap::new(),
            schema_ok: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalendarRow {
    pub title_id: u64,
    pub alt_ids: Vec<u64>,
    pub title_name: String,
    pub season: u64,
    pub episode: u64,
    pub episode_name: String,
    pub release_epoch: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Claim {
    pub title_id: u64,
    pub title_name: String,
    pub season: u64,
    pub episode: u64,
    pub episode_name: String,
}

// ---------------------------------------------------------------------------
// Kilit ve atomik dosya yazımı
// ---------------------------------------------------------------------------

pub struct FileLock {
    file: std::fs::File,
}

impl FileLock {
    pub fn try_acquire(path: &Path) -> Option<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .ok()?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            Some(Self { file })
        } else {
            None
        }
    }

    pub fn acquire(path: &Path, timeout: Duration) -> Option<Self> {
        let start = Instant::now();
        loop {
            if let Some(lock) = Self::try_acquire(path) {
                return Some(lock);
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "dosya yolu dizinsiz")
    })?;
    std::fs::create_dir_all(parent)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("state");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(".{name}.tmp.{}.{stamp}", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let body = serde_json::to_vec_pretty(value).unwrap_or_default();
    write_atomic(path, &body)
}

#[allow(dead_code)] // animecix-calendar / testler kullanır.
pub fn write_text_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    write_atomic(path, text.as_bytes())
}

pub fn load_daemon_status() -> DaemonStatus {
    load_daemon_status_from(&daemon_path())
}

#[allow(dead_code)] // animecix-calendar / testler kullanır.
pub fn save_daemon_status(st: &DaemonStatus) {
    let _ = save_daemon_status_to(&daemon_path(), st);
}

/// GUI'nin yaptığı takip kontrollerinin durumu (daemon.json'dan ayrı).
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct UiStatus {
    #[serde(default)]
    pub last_check: u64,
    #[serde(default)]
    pub last_success: u64,
    #[serde(default)]
    pub last_error: String,
}

pub fn ui_status_path() -> PathBuf {
    data_dir().join("ui-status.json")
}

pub fn load_ui_status_from(path: &Path) -> UiStatus {
    read_json::<UiStatus>(path).0.unwrap_or_default()
}

pub fn save_ui_status_to(path: &Path, st: &UiStatus) -> std::io::Result<()> {
    write_json_atomic(path, st)
}

pub fn load_ui_status() -> UiStatus {
    load_ui_status_from(&ui_status_path())
}

pub fn save_ui_status(st: &UiStatus) {
    let _ = save_ui_status_to(&ui_status_path(), st);
}

fn read_json<T: DeserializeOwned>(path: &Path) -> (Option<T>, bool) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, false);
    };
    match serde_json::from_str::<T>(&text) {
        Ok(v) => (Some(v), false),
        Err(_) => {
            let backup = path.with_extension(format!("corrupt.{}", now_secs()));
            let _ = std::fs::rename(path, &backup);
            (None, true)
        }
    }
}

// ---------------------------------------------------------------------------
// Takip listesi işlemleri
// ---------------------------------------------------------------------------

pub fn load_follows_from(path: &Path) -> FollowsFile {
    let (parsed, corrupt) = read_json::<FollowsFile>(path);
    match parsed {
        Some(mut f) => {
            if f.schema > SCHEMA {
                f.items.clear();
                f.schema_ok = false;
            } else {
                f.schema_ok = true;
            }
            f.corrupt = corrupt;
            f
        }
        None => FollowsFile {
            corrupt,
            ..Default::default()
        },
    }
}

pub fn save_follows_to(path: &Path, f: &FollowsFile) -> std::io::Result<()> {
    if !f.schema_ok {
        return Ok(());
    }
    let body = serde_json::to_vec_pretty(f).unwrap_or_default();
    write_atomic(path, &body)
}

pub fn load_follows() -> FollowsFile {
    load_follows_from(&follows_path())
}

pub fn save_follows(f: &FollowsFile) {
    let _ = save_follows_to(&follows_path(), f);
}

pub fn followed_ids() -> HashSet<u64> {
    load_follows()
        .items
        .into_iter()
        .filter(|i| i.notify)
        .map(|i| i.title_id)
        .collect()
}

pub fn is_followed(title_id: u64) -> bool {
    load_follows()
        .items
        .iter()
        .any(|i| i.title_id == title_id && i.notify)
}

pub fn toggle_item(f: &mut FollowsFile, item: FollowedTitle) -> bool {
    let existing = f.items.iter().position(|i| i.title_id == item.title_id);
    let following = existing.is_none();
    match existing {
        Some(i) => {
            f.items.remove(i);
        }
        None => f.items.push(item),
    }
    following
}

/// Takip durumunu değiştirir ve yeni durumu döndürür (true = takipte).
pub fn toggle_follow(item: FollowedTitle) -> bool {
    let mut f = load_follows();
    let following = toggle_item(&mut f, item);
    f.schema = SCHEMA;
    f.updated_at = now_secs();
    save_follows(&f);
    following
}

/// Kopya grubunu tek bir takip olarak açar/kapatır. Gruptaki herhangi bir
/// kimlik takipteyse tümü kaldırılır (yinelenen takip oluşmaz).
pub fn apply_group_toggle(
    f: &mut FollowsFile,
    primary: u64,
    group: &[u64],
    item: FollowedTitle,
) -> bool {
    let matches = |id: u64| id == primary || group.contains(&id);
    let existing = f.items.iter().any(|i| matches(i.title_id));
    if existing {
        f.items.retain(|i| !matches(i.title_id));
    } else {
        f.items.push(item);
    }
    !existing
}

pub fn toggle_group(primary: u64, group: &[u64], item: FollowedTitle) -> bool {
    let mut f = load_follows();
    let following = apply_group_toggle(&mut f, primary, group, item);
    f.schema = SCHEMA;
    f.updated_at = now_secs();
    save_follows(&f);
    following
}

// ---------------------------------------------------------------------------
// Defter
// ---------------------------------------------------------------------------

pub fn load_ledger_from(path: &Path) -> Ledger {
    let (parsed, _) = read_json::<Ledger>(path);
    match parsed {
        Some(mut l) => {
            if l.schema > SCHEMA {
                l.titles.clear();
                l.schema_ok = false;
            } else {
                l.schema_ok = true;
            }
            l
        }
        None => Ledger::default(),
    }
}

pub fn save_ledger_to(path: &Path, l: &Ledger) -> std::io::Result<()> {
    if !l.schema_ok {
        return Ok(());
    }
    let body = serde_json::to_vec_pretty(l).unwrap_or_default();
    write_atomic(path, &body)
}

#[allow(dead_code)] // animecix-calendar ikilisi kullanır.
pub fn load_ledger() -> Ledger {
    load_ledger_from(&ledger_path())
}

#[allow(dead_code)] // animecix-calendar ikilisi kullanır.
pub fn save_ledger(l: &Ledger) {
    let _ = save_ledger_to(&ledger_path(), l);
}

/// Defteri kilit altında oku, `f` uygula, atomik yaz. Kilit alınamazsa `None`.
pub fn with_ledger_locked<T>(f: impl FnOnce(&mut Ledger) -> T) -> Option<T> {
    let lpath = ledger_path();
    let _lock = FileLock::acquire(&lock_path(), Duration::from_millis(500))?;
    let mut ledger = load_ledger_from(&lpath);
    let out = f(&mut ledger);
    save_ledger_to(&lpath, &ledger).ok()?;
    Some(out)
}

/// GUI penceresi aktifken tutulan kilit: daemon bunu alabiliyorsa GUI aktif
/// değildir. `None` dönerse kilit başkasında.
pub fn try_gui_active_lock() -> Option<FileLock> {
    FileLock::try_acquire(&gui_active_path())
}

// ---------------------------------------------------------------------------
// Yeni bölüm tespiti
// ---------------------------------------------------------------------------

/// Takvim satırlarından yeni bölümleri tespit eder; ilk görülen sezonda
/// baseline alır (geçmiş bölümler toplu bildirilmez). `queued` sayısını döner.
pub fn detect_new(
    follows: &FollowsFile,
    ledger: &mut Ledger,
    rows: &[CalendarRow],
    now: u64,
) -> usize {
    let now_i = now as i64;
    let followed: Vec<&FollowedTitle> = follows.items.iter().filter(|f| f.notify).collect();

    // Takipten çıkarılan yapımların bekleyen kayıtları da iptal olur.
    ledger.titles.retain(|k, _| {
        k.parse::<u64>()
            .map(|id| followed.iter().any(|f| f.title_id == id))
            .unwrap_or(true)
    });

    // Defter kimliği, takvimde tutulan title_id değil, kullanıcının takip
    // ettiği kimliktir. Kopya projeksiyonu günler arasında değişse bile
    // (ör. E7 -> 13562, E8 yalnız 13445 + alt [13562]) baseline korunur.
    let mut by_follow: BTreeMap<u64, Vec<&CalendarRow>> = BTreeMap::new();
    for row in rows {
        let mut matched: Vec<u64> = followed
            .iter()
            .filter(|f| f.title_id == row.title_id || row.alt_ids.contains(&f.title_id))
            .map(|f| f.title_id)
            .collect();
        matched.sort_unstable();
        matched.dedup();
        if let Some(&follow_id) = matched.first() {
            by_follow.entry(follow_id).or_default().push(row);
        }
    }

    let mut queued = 0usize;
    for (title_id, list) in by_follow {
        let key = title_id.to_string();
        let st = ledger.titles.entry(key).or_default();
        if st.name.is_empty() {
            if let Some(r) = list.first() {
                st.name = r.title_name.clone();
            }
        }
        let mut by_season: BTreeMap<u64, Vec<&CalendarRow>> = BTreeMap::new();
        for r in list {
            by_season.entry(r.season).or_default().push(r);
        }
        for (season, season_rows) in by_season {
            let released: Vec<&CalendarRow> = season_rows
                .iter()
                .copied()
                .filter(|r| r.release_epoch > 0 && r.release_epoch <= now_i)
                .collect();
            if released.is_empty() {
                // Yalnız gelecek bölümler görüldü: sezonu 0 ile işaretle ki
                // ilk bölüm yayınlandığında "yeni" sayılsın (prömiyer kaybı yok).
                if !st.seen.contains_key(&season) {
                    st.seen.insert(season, 0);
                }
                continue;
            }
            let max_ep = released.iter().map(|r| r.episode).max().unwrap_or(0);
            match st.seen.get(&season).copied() {
                None => {
                    st.seen.insert(season, max_ep);
                }
                Some(last) => {
                    for r in &released {
                        if r.episode <= last {
                            continue;
                        }
                        if now_i - r.release_epoch > NEW_WINDOW_SECS {
                            continue;
                        }
                        if st
                            .entries
                            .iter()
                            .any(|e| e.season == r.season && e.episode == r.episode)
                        {
                            continue;
                        }
                        st.entries.push(LedgerEntry {
                            season: r.season,
                            episode: r.episode,
                            episode_name: r.episode_name.clone(),
                            release_epoch: r.release_epoch,
                            detected_at: now,
                            state: STATE_PENDING.to_string(),
                            attempts: 0,
                            next_attempt: now,
                            expires_at: now + PENDING_TTL_SECS,
                            last_error: String::new(),
                            in_app_seen: false,
                        });
                        queued += 1;
                    }
                    if max_ep > last {
                        st.seen.insert(season, max_ep);
                    }
                }
            }
        }
    }

    prune(ledger, now);
    queued
}

fn prune(ledger: &mut Ledger, now: u64) {
    for st in ledger.titles.values_mut() {
        st.entries.retain(|e| {
            if e.state == STATE_SENT {
                now.saturating_sub(e.detected_at) <= SENT_RETENTION_SECS
            } else {
                e.expires_at == 0 || e.expires_at > now
            }
        });
        if st.entries.len() > 50 {
            let excess = st.entries.len() - 50;
            st.entries.drain(0..excess);
        }
    }
    ledger
        .titles
        .retain(|_, st| !st.entries.is_empty() || !st.seen.is_empty());
}

/// Gönderilmeye hazır kayıtları sahiplenir; `sending` işaretler. Başarılı
/// gönderimden sonra `mark_sent`, hatada `release_claim` çağrılır.
pub fn claim_due(ledger: &mut Ledger, now: u64, limit: usize) -> Vec<Claim> {
    // Çökme/kesinti sonrası "sending" kalmış kayıtları geri al. Bu,
    // bildirimin kaybolması yerine en fazla bir kez daha denenmesini sağlar.
    for st in ledger.titles.values_mut() {
        for e in st.entries.iter_mut() {
            if e.state == STATE_SENDING && e.next_attempt <= now {
                e.state = STATE_PENDING.to_string();
                e.next_attempt = now;
            }
        }
    }
    roll_daily(ledger, now);
    let remaining = DAILY_LIMIT.saturating_sub(ledger.daily.sent) as usize;
    if remaining == 0 || limit == 0 {
        return Vec::new();
    }
    let cap = remaining.min(limit);
    let mut out = Vec::new();
    'outer: for (tid, st) in ledger.titles.iter_mut() {
        for e in st.entries.iter_mut() {
            if out.len() >= cap {
                break 'outer;
            }
            if e.state == STATE_PENDING && e.next_attempt <= now && e.expires_at > now {
                e.state = STATE_SENDING.to_string();
                e.next_attempt = now + 120;
                out.push(Claim {
                    title_id: tid.parse::<u64>().unwrap_or(0),
                    title_name: st.name.clone(),
                    season: e.season,
                    episode: e.episode,
                    episode_name: e.episode_name.clone(),
                });
            }
        }
    }
    out
}

pub fn mark_sent(ledger: &mut Ledger, claims: &[Claim], now: u64) {
    roll_daily(ledger, now);
    for c in claims {
        if let Some(st) = ledger.titles.get_mut(&c.title_id.to_string()) {
            if let Some(e) = st
                .entries
                .iter_mut()
                .find(|e| e.season == c.season && e.episode == c.episode)
            {
                e.state = STATE_SENT.to_string();
                e.last_error.clear();
            }
        }
    }
    ledger.daily.sent = ledger.daily.sent.saturating_add(claims.len() as u32);
}

/// Masaüstü bildirimi gönderilmiş ama uygulama içinde gösterilmemiş son
/// bölümleri döndürür. `mark_in_app_seen` çağrılana kadar tekrar döner.
pub fn claim_in_app_unread(ledger: &mut Ledger, now: u64, limit: usize) -> Vec<Claim> {
    if limit == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    'outer: for (tid, st) in ledger.titles.iter_mut() {
        for e in st.entries.iter_mut() {
            if out.len() >= limit {
                break 'outer;
            }
            if e.state == STATE_SENT
                && !e.in_app_seen
                && e.detected_at > 0
                && now.saturating_sub(e.detected_at) <= IN_APP_WINDOW_SECS
            {
                out.push(Claim {
                    title_id: tid.parse::<u64>().unwrap_or(0),
                    title_name: st.name.clone(),
                    season: e.season,
                    episode: e.episode,
                    episode_name: e.episode_name.clone(),
                });
            }
        }
    }
    out
}

/// Uygulama içi bildirimi gösterilen kayıtları okundu işaretler.
pub fn mark_in_app_seen(ledger: &mut Ledger, claims: &[Claim]) {
    for c in claims {
        if let Some(st) = ledger.titles.get_mut(&c.title_id.to_string()) {
            if let Some(e) = st
                .entries
                .iter_mut()
                .find(|e| e.season == c.season && e.episode == c.episode)
            {
                e.in_app_seen = true;
            }
        }
    }
}

pub fn daemon_running() -> bool {
    let st = load_daemon_status();
    st.running && pid_is_watch_daemon(st.pid)
}

#[allow(dead_code)] // animecix-calendar / testler kullanır.
pub fn release_claim(ledger: &mut Ledger, claims: &[Claim], now: u64, err: &str) {
    for c in claims {
        if let Some(st) = ledger.titles.get_mut(&c.title_id.to_string()) {
            if let Some(e) = st
                .entries
                .iter_mut()
                .find(|e| e.season == c.season && e.episode == c.episode)
            {
                e.state = STATE_PENDING.to_string();
                e.attempts = e.attempts.saturating_add(1);
                e.next_attempt = now + backoff_secs(e.attempts);
                e.last_error = err.chars().take(200).collect();
            }
        }
    }
}

#[allow(dead_code)] // animecix-calendar / testler kullanır.
pub fn backoff_secs(attempts: u32) -> u64 {
    match attempts {
        0 | 1 => 15 * 60,
        2 => 30 * 60,
        3 => 3600,
        4 => 2 * 3600,
        _ => 4 * 3600,
    }
}

fn roll_daily(ledger: &mut Ledger, now: u64) {
    let day = utc_day(now);
    if ledger.daily.day != day {
        ledger.daily.day = day;
        ledger.daily.sent = 0;
    }
}

pub fn utc_day(epoch: u64) -> String {
    let (y, m, d) = civil_from_days((epoch / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// Testler
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmp(name: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("animecix-follow-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn follow(id: u64) -> FollowedTitle {
        FollowedTitle {
            title_id: id,
            name: format!("T{id}"),
            poster: None,
            title_type: Some("anime".into()),
            notify: true,
            added_at: 1,
            season_count: Some(1),
        }
    }

    fn row(id: u64, season: u64, episode: u64, rel: i64) -> CalendarRow {
        CalendarRow {
            title_id: id,
            alt_ids: Vec::new(),
            title_name: format!("T{id}"),
            season,
            episode,
            episode_name: format!("{episode}. Bölüm"),
            release_epoch: rel,
        }
    }

    fn follows(ids: &[u64]) -> FollowsFile {
        FollowsFile {
            items: ids.iter().copied().map(follow).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn first_run_baselines_without_notifying() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let rows = vec![
            row(1, 1, 1, now as i64 - 5 * 86_400),
            row(1, 1, 5, now as i64 - 3600),
        ];
        let queued = detect_new(&follows(&[1]), &mut ledger, &rows, now);
        assert_eq!(queued, 0);
        assert_eq!(ledger.titles["1"].seen.get(&1), Some(&5));
        assert!(ledger.titles["1"].entries.is_empty());
    }

    #[test]
    fn new_episode_queues_pending() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 1, 5, now as i64 - 7 * 86_400)],
            now,
        );
        let queued = detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 1, 6, now as i64 - 60)],
            now,
        );
        assert_eq!(queued, 1);
        let st = &ledger.titles["1"];
        assert_eq!(st.seen.get(&1), Some(&6));
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.entries[0].state, STATE_PENDING);
        assert_eq!(st.entries[0].episode, 6);
    }

    #[test]
    fn future_episode_is_not_notified_early() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 1, 5, now as i64 - 86_400)],
            now,
        );
        let queued = detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 1, 6, now as i64 + 3600)],
            now,
        );
        assert_eq!(queued, 0);
        assert_eq!(ledger.titles["1"].seen.get(&1), Some(&5));
    }

    #[test]
    fn old_episode_advances_baseline_without_spam() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 1, 5, now as i64 - 20 * 86_400)],
            now,
        );
        let queued = detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 1, 6, now as i64 - 10 * 86_400)],
            now,
        );
        assert_eq!(queued, 0, "72 saatten eski bölüm bildirilmemeli");
        assert_eq!(ledger.titles["1"].seen.get(&1), Some(&6));
        assert!(ledger.titles["1"].entries.is_empty());
    }

    #[test]
    fn alt_title_id_matches_followed_title() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let t = |s, e, rel| CalendarRow {
            title_id: 13562,
            alt_ids: vec![13445],
            title_name: "Hotaru no Yomeiri".into(),
            season: s,
            episode: e,
            episode_name: String::new(),
            release_epoch: rel,
        };
        detect_new(
            &follows(&[13445]),
            &mut ledger,
            &[t(1, 7, now as i64 - 86_400)],
            now,
        );
        // Defter kimliği takip edilen id'dir, o gün tutulan projeksiyon değil.
        assert_eq!(ledger.titles["13445"].seen.get(&1), Some(&7));
        assert!(!ledger.titles.contains_key("13562"));
    }

    #[test]
    fn kept_id_flip_does_not_swallow_new_episode() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let f = follows(&[13562]);
        // Gün 1: E7, tutulan projeksiyon 13562.
        detect_new(
            &f,
            &mut ledger,
            &[row(13562, 1, 7, now as i64 - 86_400)],
            now,
        );
        assert_eq!(ledger.titles["13562"].seen.get(&1), Some(&7));
        // Gün 2: E8 yalnız 13445'te, alt kimlik 13562.
        let mut r = row(13445, 1, 8, now as i64 - 60);
        r.alt_ids = vec![13562];
        let queued = detect_new(&f, &mut ledger, &[r], now);
        assert_eq!(queued, 1, "tutulan kimlik değişse de bölüm kaçmamalı");
        assert_eq!(ledger.titles["13562"].entries.len(), 1);
        assert_eq!(ledger.titles["13562"].entries[0].episode, 8);
    }

    #[test]
    fn unfollow_prunes_pending_notifications() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let f = follows(&[500, 600]);
        let rows = vec![
            row(500, 1, 1, now as i64 - 86_400),
            row(600, 1, 1, now as i64 - 86_400),
        ];
        detect_new(&f, &mut ledger, &rows, now);
        let rows = vec![
            row(500, 1, 2, now as i64 - 60),
            row(600, 1, 2, now as i64 - 60),
        ];
        detect_new(&f, &mut ledger, &rows, now);
        assert_eq!(ledger.titles["500"].entries.len(), 1);

        let mut smaller = f;
        smaller.items.retain(|i| i.title_id != 500);
        detect_new(&smaller, &mut ledger, &rows, now);
        assert!(
            !ledger.titles.contains_key("500"),
            "takipten çıkan yapının bekleyeni iptal olmalı"
        );
        let claims = claim_due(&mut ledger, now, 10);
        assert!(claims.iter().all(|c| c.title_id != 500));
    }

    #[test]
    fn future_premiere_is_notified_on_release() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let f = follows(&[1]);
        // Sezon 2 henüz yayınlanmadı; ilk görüş.
        detect_new(&f, &mut ledger, &[row(1, 2, 1, now as i64 + 3600)], now);
        assert_eq!(ledger.titles["1"].seen.get(&2), Some(&0));
        // Prömiyer yayınlandı.
        let queued = detect_new(&f, &mut ledger, &[row(1, 2, 1, now as i64 - 60)], now);
        assert_eq!(queued, 1, "sezon prömiyeri bildirilmeli");
        assert_eq!(ledger.titles["1"].entries[0].episode, 1);
    }

    #[test]
    fn stale_sending_entries_are_recovered() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let st = ledger.titles.entry("1".into()).or_default();
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 2,
            state: STATE_SENDING.into(),
            next_attempt: now - 1,
            expires_at: now + 10_000,
            ..Default::default()
        });
        assert_eq!(claim_due(&mut ledger, now, 1).len(), 1);

        let st = ledger.titles.get_mut("1").unwrap();
        st.entries[0].state = STATE_SENDING.into();
        st.entries[0].next_attempt = now + 100;
        assert!(
            claim_due(&mut ledger, now, 1).is_empty(),
            "henüz süresi dolmamış sending geri alınmamalı"
        );
    }

    #[test]
    fn group_toggle_adds_and_removes_alt_ids() {
        let mut f = follows(&[13445]);
        assert!(!apply_group_toggle(&mut f, 13562, &[13445], follow(13562)));
        assert!(f.items.is_empty(), "gruptaki kimlik kaldırılmalı");
        assert!(apply_group_toggle(&mut f, 13562, &[13445], follow(13562)));
        assert_eq!(f.items.len(), 1);
        assert_eq!(f.items[0].title_id, 13562);
    }

    #[test]
    fn seasons_track_separately() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let rows = vec![
            row(1, 1, 33, now as i64 - 86_400),
            row(1, 2, 7, now as i64 - 86_400),
        ];
        detect_new(&follows(&[1]), &mut ledger, &rows, now);
        assert_eq!(ledger.titles["1"].seen.get(&1), Some(&33));
        assert_eq!(ledger.titles["1"].seen.get(&2), Some(&7));

        let queued = detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 2, 8, now as i64 - 60)],
            now,
        );
        assert_eq!(queued, 1);
        assert_eq!(ledger.titles["1"].entries[0].season, 2);
        assert_eq!(ledger.titles["1"].entries[0].episode, 8);
    }

    #[test]
    fn repeated_detect_does_not_duplicate() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let f = follows(&[1]);
        detect_new(&f, &mut ledger, &[row(1, 1, 5, now as i64 - 86_400)], now);
        let r = [row(1, 1, 6, now as i64 - 60)];
        detect_new(&f, &mut ledger, &r, now);
        detect_new(&f, &mut ledger, &r, now + 60);
        assert_eq!(ledger.titles["1"].entries.len(), 1);
    }

    #[test]
    fn claim_mark_sent_and_daily_cap() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let f = follows(&[1, 2, 3]);
        let mut rows = Vec::new();
        for id in 1..=3 {
            rows.push(row(id, 1, 1, now as i64 - 86_400));
        }
        detect_new(&f, &mut ledger, &rows, now);
        for id in 1..=3 {
            rows.push(row(id, 1, 2, now as i64 - 60));
        }
        detect_new(&f, &mut ledger, &rows, now);

        let claims = claim_due(&mut ledger, now, 10);
        assert_eq!(claims.len(), 3);
        assert_eq!(ledger.titles["1"].entries[0].state, STATE_SENDING);
        assert!(claim_due(&mut ledger, now, 10).is_empty());

        mark_sent(&mut ledger, &claims, now);
        assert_eq!(ledger.titles["1"].entries[0].state, STATE_SENT);
        assert_eq!(ledger.daily.sent, 3);
    }

    #[test]
    fn daily_cap_limits_claims() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        ledger.daily.day = utc_day(now);
        ledger.daily.sent = DAILY_LIMIT - 1;
        let st = ledger.titles.entry("9".into()).or_default();
        for ep in 1..=5 {
            st.entries.push(LedgerEntry {
                season: 1,
                episode: ep,
                state: STATE_PENDING.into(),
                next_attempt: 0,
                expires_at: now + 1000,
                ..Default::default()
            });
        }
        let claims = claim_due(&mut ledger, now, 10);
        assert_eq!(claims.len(), 1);
    }

    #[test]
    fn failed_delivery_backs_off_then_retries() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let st = ledger.titles.entry("1".into()).or_default();
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 2,
            state: STATE_PENDING.into(),
            next_attempt: 0,
            expires_at: now + 10_000,
            ..Default::default()
        });

        let claims = claim_due(&mut ledger, now, 1);
        assert_eq!(claims.len(), 1);
        release_claim(&mut ledger, &claims, now, "bildirim servisi yok");
        let e = &ledger.titles["1"].entries[0];
        assert_eq!(e.state, STATE_PENDING);
        assert_eq!(e.attempts, 1);
        assert_eq!(e.next_attempt, now + 15 * 60);
        assert!(e.last_error.contains("servis"));

        assert!(claim_due(&mut ledger, now + 14 * 60, 1).is_empty());
        assert_eq!(claim_due(&mut ledger, now + 16 * 60, 1).len(), 1);
    }

    #[test]
    fn pending_survives_reload() {
        let dir = tmp("ledger-roundtrip");
        let path = dir.join("ledger.json");
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let st = ledger.titles.entry("1".into()).or_default();
        st.name = "T1".into();
        st.seen.insert(1, 4);
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 5,
            state: STATE_PENDING.into(),
            next_attempt: now,
            expires_at: now + PENDING_TTL_SECS,
            ..Default::default()
        });
        save_ledger_to(&path, &ledger).unwrap();
        let back = load_ledger_from(&path);
        assert_eq!(back.titles["1"].entries.len(), 1);
        assert_eq!(back.titles["1"].seen.get(&1), Some(&4));
        assert_eq!(back.titles["1"].entries[0].state, STATE_PENDING);
    }

    #[test]
    fn prune_removes_expired_pending_and_old_sent() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let st = ledger.titles.entry("1".into()).or_default();
        st.seen.insert(1, 10);
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 1,
            state: STATE_SENT.into(),
            detected_at: now - SENT_RETENTION_SECS - 1,
            ..Default::default()
        });
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 2,
            state: STATE_PENDING.into(),
            expires_at: now - 1,
            ..Default::default()
        });
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 3,
            state: STATE_PENDING.into(),
            expires_at: now + 1000,
            ..Default::default()
        });
        prune(&mut ledger, now);
        let st = &ledger.titles["1"];
        assert_eq!(st.entries.len(), 1);
        assert_eq!(st.entries[0].episode, 3);
    }

    #[test]
    fn follows_roundtrip_and_toggle() {
        let dir = tmp("follows-roundtrip");
        let path = dir.join("follows.json");
        let mut f = follows(&[1, 2]);
        assert!(!toggle_item(&mut f, follow(2)));
        assert!(!f.items.iter().any(|i| i.title_id == 2));
        assert!(toggle_item(&mut f, follow(3)));
        save_follows_to(&path, &f).unwrap();
        let back = load_follows_from(&path);
        assert_eq!(back.items.len(), 2);
        assert!(back.items.iter().any(|i| i.title_id == 1));
        assert!(back.items.iter().any(|i| i.title_id == 3));
        assert!(back.schema_ok);
    }

    #[test]
    fn corrupt_files_are_backed_up() {
        let dir = tmp("corrupt");
        let fpath = dir.join("follows.json");
        std::fs::write(&fpath, "{bozuk").unwrap();
        let f = load_follows_from(&fpath);
        assert!(f.items.is_empty());
        assert!(f.corrupt, "bozuk dosya işareti taşınmalı");
        let backup = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(backup, "bozuk dosya yedeklenmeli");
    }

    #[test]
    fn newer_schema_is_not_rewritten() {
        let dir = tmp("schema");
        let path = dir.join("follows.json");
        std::fs::write(&path, r#"{"schema":99,"items":[{"title_id":1}]}"#).unwrap();
        let f = load_follows_from(&path);
        assert!(!f.schema_ok);
        assert!(f.items.is_empty());
        save_follows_to(&path, &f).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"schema\":99"), "yeni şema ezilmemeli");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let dir = tmp("atomic");
        let path = dir.join("ledger.json");
        save_ledger_to(&path, &Ledger::default()).unwrap();
        for e in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            assert!(!name.contains(".tmp."), "geçici dosya kaldı: {name}");
        }
    }

    #[test]
    fn flock_blocks_second_holder() {
        let dir = tmp("flock");
        let path = dir.join("lock");
        let a = FileLock::acquire(&path, Duration::from_millis(10)).unwrap();
        let b = FileLock::acquire(&path, Duration::from_millis(40));
        assert!(b.is_none(), "ikinci kilit alınmamalı");
        drop(a);
        let c = FileLock::acquire(&path, Duration::from_millis(40));
        assert!(c.is_some(), "kilit bırakılınca alınabilmeli");
    }

    #[test]
    fn utc_day_uses_utc_calendar_date() {
        assert_eq!(utc_day(0), "1970-01-01");
        assert_eq!(utc_day(1_794_960_000), "2026-11-18");
        assert_eq!(utc_day(1_795_000_000), "2026-11-18");
    }

    #[test]
    fn unfollowed_titles_are_ignored() {
        let now = 1_800_000_000u64;
        let mut f = FollowsFile::default();
        let mut item = follow(1);
        item.notify = false;
        f.items.push(item);
        let mut ledger = Ledger::default();
        let queued = detect_new(&f, &mut ledger, &[row(1, 1, 1, now as i64 - 60)], now);
        assert_eq!(queued, 0);
        assert!(ledger.titles.is_empty());
    }

    #[test]
    fn autostart_entry_is_managed() {
        let c = autostart_content("\"/home/u/AnimeciX-x86_64.AppImage\" --calendar-daemon");
        assert!(c.contains("X-AnimeciX-Managed=1"));
        assert!(c.contains("Exec=\"/home/u/AnimeciX-x86_64.AppImage\" --calendar-daemon"));
        assert!(c.contains("NoDisplay=true"));
    }

    #[test]
    fn daemon_status_roundtrip() {
        let dir = tmp("daemon-status");
        let path = dir.join("daemon.json");
        let st = DaemonStatus {
            version: "1.4.0".into(),
            pid: 42,
            last_check: 123,
            running: true,
            ..Default::default()
        };
        save_daemon_status_to(&path, &st).unwrap();
        let back = load_daemon_status_from(&path);
        assert_eq!(back.pid, 42);
        assert_eq!(back.last_check, 123);
        assert!(back.running);
    }

    #[test]
    fn ui_status_roundtrip() {
        let dir = tmp("ui-status");
        let path = dir.join("ui-status.json");
        let st = UiStatus {
            last_check: 456,
            last_success: 400,
            last_error: "ağ hatası".into(),
        };
        save_ui_status_to(&path, &st).unwrap();
        let back = load_ui_status_from(&path);
        assert_eq!(back.last_check, 456);
        assert_eq!(back.last_success, 400);
        assert_eq!(back.last_error, "ağ hatası");
    }

    #[test]
    fn in_app_unread_claims_only_recent_unseen_sent_entries() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let st = ledger.titles.entry("1".into()).or_default();
        st.name = "Test".into();
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 7,
            state: STATE_SENT.into(),
            detected_at: now - 3600,
            ..Default::default()
        });
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 6,
            state: STATE_SENT.into(),
            detected_at: now - IN_APP_WINDOW_SECS - 3600,
            ..Default::default()
        });
        st.entries.push(LedgerEntry {
            season: 1,
            episode: 5,
            state: STATE_SENT.into(),
            detected_at: now - 60,
            in_app_seen: true,
            ..Default::default()
        });

        let claims = claim_in_app_unread(&mut ledger, now, 10);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].episode, 7);
        mark_in_app_seen(&mut ledger, &claims);
        assert!(claim_in_app_unread(&mut ledger, now, 10).is_empty());
    }

    #[test]
    fn episode_list_row_is_notified_when_new() {
        let now = 1_800_000_000u64;
        let mut ledger = Ledger::default();
        let st = ledger.titles.entry("1".into()).or_default();
        st.seen.insert(1, 6);
        let queued = detect_new(
            &follows(&[1]),
            &mut ledger,
            &[row(1, 1, 7, now as i64)],
            now,
        );
        assert_eq!(queued, 1);
        assert_eq!(ledger.titles["1"].entries[0].state, STATE_PENDING);
    }

    #[test]
    fn pid_liveness_checks_current_process() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }

    #[test]
    fn pid_guard_rejects_non_daemon_command_line() {
        assert!(!pid_is_watch_daemon(0));
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        assert!(!pid_is_watch_daemon(child.id()));
        let _ = child.kill();
        let _ = child.wait();
    }
}
