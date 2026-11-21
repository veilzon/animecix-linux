//! Kullanıcı verisi için yedekleme, geri yükleme ve sıfırlama çekirdeği.
//!
//! Bu modül GTK'dan bağımsızdır; dosya yolları `DataPaths` ile enjekte
//! edilir. Böylece testler gerçek kullanıcı dizinine dokunmaz.

use crate::api::{self, Client};
use crate::follow_store::{self, FollowedTitle, FollowsFile};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FORMAT: &str = "animecix-backup";
pub const FORMAT_VERSION: u32 = 1;
pub const MAX_BACKUP_BYTES: u64 = 10 * 1024 * 1024;
pub const AUTO_BACKUP_KEEP: usize = 5;

/// Yedekleme/sıfırlama işlemlerinin dokunduğu dosya ve dizinler.
#[derive(Clone, Debug)]
pub struct DataPaths {
    pub state: PathBuf,
    pub settings: PathBuf,
    pub follows: PathBuf,
    pub backups: PathBuf,
    pub downloads: PathBuf,
    pub cache: PathBuf,
}

impl DataPaths {
    pub fn new(
        state: PathBuf,
        settings: PathBuf,
        follows: PathBuf,
        backups: PathBuf,
        downloads: PathBuf,
        cache: PathBuf,
    ) -> Self {
        Self {
            state,
            settings,
            follows,
            backups,
            downloads,
            cache,
        }
    }

    /// Gerçek kullanıcı verisi yolları.
    pub fn system() -> Self {
        let settings = Client::settings_path();
        let backups = settings
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups");
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        Self::new(
            Client::state_path(),
            settings,
            follow_store::follows_path(),
            backups,
            crate::download::queue_file_path(),
            home.join(".cache/animecix"),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupBundle {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub exported_at: u64,
    pub state: api::State,
    pub settings: api::Settings,
    #[serde(default)]
    pub follows: Vec<FollowedTitle>,
}

impl Default for BackupBundle {
    fn default() -> Self {
        Self {
            format: FORMAT.to_string(),
            format_version: FORMAT_VERSION,
            app_version: String::new(),
            exported_at: 0,
            state: api::State::default(),
            settings: api::Settings::default(),
            follows: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupSummary {
    pub app_version: String,
    pub exported_at: u64,
    pub favorites: usize,
    pub watched_episodes: usize,
    pub history: usize,
    pub marathon: usize,
    pub follows: usize,
}

pub fn summary(bundle: &BackupBundle) -> BackupSummary {
    BackupSummary {
        app_version: bundle.app_version.clone(),
        exported_at: bundle.exported_at,
        favorites: bundle.state.saved.len(),
        watched_episodes: bundle.state.watched.values().map(Vec::len).sum(),
        history: bundle.state.history.len(),
        marathon: bundle.state.marathon.len(),
        follows: bundle.follows.len(),
    }
}

pub fn suggested_filename() -> String {
    match glib::DateTime::now_local()
        .ok()
        .and_then(|d| d.format("%Y-%m-%d").ok())
    {
        Some(date) => format!("animecix-yedek-{date}.json"),
        None => "animecix-yedek.json".to_string(),
    }
}

/// Canlı veriyi tek JSON yedeğe yazar.
pub fn export_bundle(paths: &DataPaths, dest: &Path) -> Result<BackupSummary, String> {
    if is_managed_path(paths, dest) {
        return Err("canlı veri dosyasının üzerine yedek yazılamaz".to_string());
    }
    let state: api::State = read_json_or_default(&paths.state)?;
    let mut settings: api::Settings = read_json_or_default(&paths.settings)?;
    Client::normalize_settings(&mut settings);
    // Gizli ve cihaza özel alanlar yedeğe taşınmaz.
    settings.cf_clearance.clear();
    settings.official_skip_secret.clear();
    settings.download_dir = None;
    settings.discord_rpc_socket = None;

    let follows: FollowsFile = read_json_or_default(&paths.follows)?;
    let bundle = BackupBundle {
        format: FORMAT.to_string(),
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: now_secs(),
        state,
        settings,
        follows: follows.items,
    };
    write_bundle(dest, &bundle)?;
    Ok(summary(&bundle))
}

/// Yedek dosyasını doğrular ve okur.
pub fn read_bundle(path: &Path) -> Result<BackupBundle, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{} okunamadı: {e}", path.display()))?;
    if meta.len() > MAX_BACKUP_BYTES {
        return Err(format!(
            "yedek dosyası çok büyük ({} bayt; en fazla {} bayt)",
            meta.len(),
            MAX_BACKUP_BYTES
        ));
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("{} okunamadı: {e}", path.display()))?;
    let bundle: BackupBundle = serde_json::from_str(&text)
        .map_err(|e| format!("{} geçerli bir AnimeciX yedeği değil: {e}", path.display()))?;
    validate_bundle(&bundle)?;
    Ok(bundle)
}

pub fn apply_imported_settings(current: &api::Settings, imported: &api::Settings) -> api::Settings {
    let mut out = imported.clone();
    out.cf_clearance = current.cf_clearance.clone();
    out.official_skip_secret = current.official_skip_secret.clone();
    out.download_dir = current.download_dir.clone();
    out.discord_rpc_socket = current.discord_rpc_socket.clone();
    Client::normalize_settings(&mut out);
    out
}

fn validate_bundle(bundle: &BackupBundle) -> Result<(), String> {
    if bundle.format != FORMAT {
        return Err(format!(
            "desteklenmeyen yedek formatı: {} (beklenen: {FORMAT})",
            bundle.format
        ));
    }
    if bundle.format_version == 0 || bundle.format_version > FORMAT_VERSION {
        return Err(format!(
            "desteklenmeyen yedek sürümü: {} (desteklenen: {FORMAT_VERSION})",
            bundle.format_version
        ));
    }
    Ok(())
}

fn write_bundle(dest: &Path, bundle: &BackupBundle) -> Result<(), String> {
    let body = serde_json::to_string_pretty(bundle)
        .map_err(|e| format!("yedek serileştirilemedi: {e}"))?;
    if body.len() as u64 > MAX_BACKUP_BYTES {
        return Err(format!(
            "yedek çok büyük ({} bayt; en fazla {} bayt)",
            body.len(),
            MAX_BACKUP_BYTES
        ));
    }
    follow_store::write_text_atomic(dest, &body)
        .map_err(|e| format!("{} yazılamadı: {e}", dest.display()))
}

fn normalized_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(canonical_parent) = std::fs::canonicalize(parent) {
            return canonical_parent.join(name);
        }
    }
    path.to_path_buf()
}

fn is_managed_path(paths: &DataPaths, dest: &Path) -> bool {
    let dest = normalized_path(dest);
    [
        &paths.state,
        &paths.settings,
        &paths.follows,
        &paths.downloads,
    ]
    .iter()
    .any(|path| normalized_path(path) == dest)
}

fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|e| format!("{} okunamadı: {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("{} okunamadı: {e}", path.display())),
    }
}

fn read_json_lenient<T: DeserializeOwned + Default>(path: &Path) -> T {
    match read_json_or_default(path) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("[IMPORT] {e}; varsayılan değer kullanılıyor (ham kopya yedekte)");
            T::default()
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMode {
    Replace,
    Merge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub mode: ImportMode,
    pub auto_backup: PathBuf,
    pub favorites: usize,
    pub watched_episodes: usize,
    pub history: usize,
    pub marathon: usize,
    pub follows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub created_at: u64,
    pub size: u64,
    pub kind: String,
}

/// İçe aktarma öncesi mevcut canlı veriyi yedekler.
pub fn auto_backup(paths: &DataPaths, kind: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&paths.backups)
        .map_err(|e| format!("{} oluşturulamadı: {e}", paths.backups.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = paths
        .backups
        .join(format!("auto-{}-{stamp}.json", sanitize_kind(kind)));
    match export_bundle(paths, &path) {
        Ok(_) => {
            trim_auto_backups(paths)?;
            Ok(path)
        }
        Err(e) => {
            eprintln!("[BACKUP] normal yedek alınamadı: {e}; ham kopya alınıyor");
            let _ = std::fs::remove_file(&path);
            let raw = paths
                .backups
                .join(format!("auto-{}-{stamp}-raw", sanitize_kind(kind)));
            std::fs::create_dir_all(&raw)
                .map_err(|e| format!("{} oluşturulamadı: {e}", raw.display()))?;
            for (src, name) in [
                (&paths.state, "state.json"),
                (&paths.settings, "settings.json"),
                (&paths.follows, "follows.json"),
                (&paths.downloads, "downloads.json"),
            ] {
                if src.is_file() {
                    std::fs::copy(src, raw.join(name))
                        .map_err(|e| format!("{} ham yedeğe kopyalanamadı: {e}", src.display()))?;
                }
            }
            trim_raw_backups(paths)?;
            Ok(raw)
        }
    }
}

/// Son 5 otomatik yedeği (en yeniden eskiye) listeler.
pub fn list_auto_backups(paths: &DataPaths) -> Vec<BackupInfo> {
    auto_backup_files(paths)
        .into_iter()
        .take(AUTO_BACKUP_KEEP)
        .map(|(path, created_at, size)| BackupInfo {
            kind: backup_kind(&path),
            path,
            created_at,
            size,
        })
        .collect()
}

/// Otomatik yedekleri siler; kaç dosya silindiğini döndürür.
pub fn delete_all_backups(paths: &DataPaths) -> Result<usize, String> {
    let mut removed = 0;
    for (path, _, _) in auto_backup_files(paths) {
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{} silinemedi: {e}", path.display())),
        }
    }
    for path in raw_backup_dirs(paths) {
        match std::fs::remove_dir_all(&path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{} silinemedi: {e}", path.display())),
        }
    }
    Ok(removed)
}

/// Canlı veriyi varsayılana döndürür; otomatik yedeklere dokunmaz.
pub fn reset_live_data(paths: &DataPaths) -> Result<(), String> {
    follow_store::write_json_atomic(&paths.state, &api::State::default())
        .map_err(|e| format!("{} yazılamadı: {e}", paths.state.display()))?;
    follow_store::write_json_atomic(&paths.settings, &api::Settings::default())
        .map_err(|e| format!("{} yazılamadı: {e}", paths.settings.display()))?;

    remove_file_if_exists(&paths.follows)?;
    if let Some(parent) = paths.follows.parent() {
        if parent
            .file_name()
            .map(|n| n.to_string_lossy() == "follow")
            .unwrap_or(false)
        {
            remove_dir_if_exists(parent)?;
        }
    }
    remove_file_if_exists(&paths.downloads)?;
    remove_dir_if_exists(&paths.cache)?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{} silinemedi: {e}", path.display())),
    }
}

fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{} silinemedi: {e}", path.display())),
    }
}

/// Yedekteki veriyi mevcut veriyle birleştirir.
pub fn merge_state(local: &api::State, imported: &api::State) -> api::State {
    let mut out = local.clone();

    let mut saved_ids: HashSet<u64> = out.saved.iter().map(|t| t.id).collect();
    for t in &imported.saved {
        if saved_ids.insert(t.id) {
            out.saved.push(t.clone());
        }
    }

    for (key, imported_list) in &imported.watched {
        let list = out.watched.entry(key.clone()).or_default();
        for w in imported_list {
            if let Some(existing) = list
                .iter_mut()
                .find(|x| x.season == w.season && x.episode == w.episode)
            {
                if w.watched_at > existing.watched_at {
                    *existing = w.clone();
                }
            } else {
                list.push(w.clone());
            }
        }
        list.sort_by_key(|x| (x.season, x.episode));
    }

    for h in &imported.history {
        if let Some(existing) = out.history.iter_mut().find(|x| x.title.id == h.title.id) {
            if h.ts > existing.ts {
                *existing = h.clone();
            }
        } else {
            out.history.push(h.clone());
        }
    }
    out.history.sort_by(|a, b| b.ts.cmp(&a.ts));
    out.history.truncate(30);

    for (key, value) in &imported.progress {
        out.progress.entry(key.clone()).or_insert(*value);
    }

    let mut marathon_ids: HashSet<u64> = out.marathon.iter().map(|m| m.title.id).collect();
    for m in &imported.marathon {
        if marathon_ids.insert(m.title.id) {
            out.marathon.push(m.clone());
        }
    }

    for (key, value) in &imported.preferred_source {
        out.preferred_source
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }

    if out.current.is_none() {
        out.current = imported.current.clone();
    }
    out.welcome_seen |= imported.welcome_seen;
    out.quick_search_tip_seen |= imported.quick_search_tip_seen;
    out.search_tip_seen |= imported.search_tip_seen;
    out.right_click_tip_seen |= imported.right_click_tip_seen;
    out.tools_tip_seen |= imported.tools_tip_seen;
    out.player_drawer_tip_seen |= imported.player_drawer_tip_seen;
    out
}

/// Takip listesini `title_id` bazında birleştirir; yerel kayıt kazanır.
pub fn merge_follows(local: &[FollowedTitle], imported: &[FollowedTitle]) -> Vec<FollowedTitle> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in local.iter().chain(imported.iter()) {
        if seen.insert(item.title_id) {
            out.push(item.clone());
        }
    }
    out.sort_by_key(|item| item.title_id);
    out
}

/// Yedeği içe aktarır. Önce mevcut veriyi otomatik yedekler.
pub fn import_bundle(
    paths: &DataPaths,
    bundle: &BackupBundle,
    mode: ImportMode,
) -> Result<ImportReport, String> {
    validate_bundle(bundle)?;
    let auto_backup = auto_backup(paths, "import")?;

    let local_state: api::State = read_json_lenient(&paths.state);
    let local_settings: api::Settings = read_json_lenient(&paths.settings);
    let local_follows: FollowsFile = read_json_lenient(&paths.follows);

    let (new_state, new_settings, new_follows) = match mode {
        ImportMode::Replace => (
            bundle.state.clone(),
            apply_imported_settings(&local_settings, &bundle.settings),
            bundle.follows.clone(),
        ),
        ImportMode::Merge => (
            merge_state(&local_state, &bundle.state),
            local_settings.clone(),
            merge_follows(&local_follows.items, &bundle.follows),
        ),
    };

    let follows_file = FollowsFile {
        schema: follow_store::SCHEMA,
        updated_at: follow_store::now_secs(),
        items: new_follows.clone(),
        ..Default::default()
    };

    // Üç dosyayı da önce geçici adlara yaz; hepsi hazır olunca taşı.
    // Böylece normal hata yollarında canlı veri yarım kalmaz.
    let state_stage = match stage_json(&paths.state, &new_state) {
        Ok(path) => path,
        Err(e) => return Err(e),
    };
    let settings_stage = match stage_json(&paths.settings, &new_settings) {
        Ok(path) => path,
        Err(e) => {
            let _ = std::fs::remove_file(&state_stage);
            return Err(e);
        }
    };
    let follows_stage = match stage_json(&paths.follows, &follows_file) {
        Ok(path) => path,
        Err(e) => {
            let _ = std::fs::remove_file(&state_stage);
            let _ = std::fs::remove_file(&settings_stage);
            return Err(e);
        }
    };

    let commit = (|| -> Result<(), String> {
        for (stage, target) in [
            (&state_stage, &paths.state),
            (&settings_stage, &paths.settings),
            (&follows_stage, &paths.follows),
        ] {
            std::fs::rename(stage, target)
                .map_err(|e| format!("{} taşınamadı: {e}", target.display()))?;
        }
        Ok(())
    })();

    if let Err(e) = commit {
        // Taşıma yarıda kaldıysa eski değerleri geri yaz.
        let _ = follow_store::write_json_atomic(&paths.state, &local_state);
        let _ = follow_store::write_json_atomic(&paths.settings, &local_settings);
        let _ = follow_store::write_json_atomic(&paths.follows, &local_follows);
        let _ = std::fs::remove_file(&state_stage);
        let _ = std::fs::remove_file(&settings_stage);
        let _ = std::fs::remove_file(&follows_stage);
        return Err(e);
    }

    Ok(ImportReport {
        mode,
        auto_backup,
        favorites: new_state.saved.len(),
        watched_episodes: new_state.watched.values().map(Vec::len).sum(),
        history: new_state.history.len(),
        marathon: new_state.marathon.len(),
        follows: new_follows.len(),
    })
}

fn stage_json<T: serde::Serialize>(target: &Path, value: &T) -> Result<PathBuf, String> {
    let parent = target
        .parent()
        .ok_or_else(|| format!("{} için üst dizin yok", target.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("{} oluşturulamadı: {e}", parent.display()))?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("veri");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stage = parent.join(format!(".{name}.import-{}.{stamp}", std::process::id()));
    follow_store::write_json_atomic(&stage, value)
        .map_err(|e| format!("{} yazılamadı: {e}", stage.display()))?;
    Ok(stage)
}

fn auto_backup_files(paths: &DataPaths) -> Vec<(PathBuf, u64, u64)> {
    let Ok(entries) = std::fs::read_dir(&paths.backups) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("auto-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        let dur = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        out.push((path, dur.as_secs(), meta.len()));
    }
    out.sort_by_key(|(path, _, _)| std::cmp::Reverse(backup_stamp(path)));
    out
}

fn trim_auto_backups(paths: &DataPaths) -> Result<(), String> {
    for (path, _, _) in auto_backup_files(paths).into_iter().skip(AUTO_BACKUP_KEEP) {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{} silinemedi: {e}", path.display())),
        }
    }
    Ok(())
}

fn raw_backup_dirs(paths: &DataPaths) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(&paths.backups) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("auto-") && n.ends_with("-raw"))
                    .unwrap_or(false)
        })
        .collect();
    out.sort_by_key(|path| std::cmp::Reverse(raw_stamp(path)));
    out
}

fn trim_raw_backups(paths: &DataPaths) -> Result<(), String> {
    for path in raw_backup_dirs(paths).into_iter().skip(AUTO_BACKUP_KEEP) {
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{} silinemedi: {e}", path.display())),
        }
    }
    Ok(())
}

fn backup_stamp(path: &Path) -> u128 {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.trim_end_matches(".json").rsplit('-').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn raw_stamp(path: &Path) -> u128 {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.trim_end_matches("-raw").rsplit('-').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn backup_kind(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("auto-"))
        .and_then(|n| n.split('-').next())
        .unwrap_or("auto")
        .to_string()
}

fn sanitize_kind(kind: &str) -> &'static str {
    match kind {
        "import" => "import",
        "reset" => "reset",
        "restore" => "restore",
        _ => "auto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Settings, State, Title, Watched};
    use crate::follow_store::{FollowedTitle, FollowsFile};
    use std::path::Path;

    fn temp_paths(tag: &str) -> DataPaths {
        let base = std::env::temp_dir().join(format!("animecix-data-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        DataPaths::new(
            base.join("state.json"),
            base.join("settings.json"),
            base.join("follow/follows.json"),
            base.join("backups"),
            base.join("downloads.json"),
            base.join("cache"),
        )
    }

    fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn follow(id: u64, name: &str, notify: bool) -> FollowedTitle {
        FollowedTitle {
            title_id: id,
            name: name.into(),
            poster: None,
            title_type: None,
            notify,
            added_at: 0,
            season_count: None,
        }
    }

    fn sample_state() -> State {
        State {
            saved: vec![Title {
                id: 1,
                name: "Favori".into(),
                ..Default::default()
            }],
            watched: std::collections::HashMap::from([(
                "1".into(),
                vec![Watched {
                    title_id: 1,
                    season: 1,
                    episode: 1,
                    ..Default::default()
                }],
            )]),
            ..Default::default()
        }
    }

    #[test]
    fn export_strips_secrets_and_reads_back() {
        let paths = temp_paths("export");
        let mut settings = Settings::default();
        settings.theme = "aurora".into();
        settings.cf_clearance = "GIZLI".into();
        settings.official_skip_secret = "SR".into();
        settings.download_dir = Some("/özel/indirme".into());
        settings.discord_rpc_socket = Some("/özel/soket".into());
        write_json(&paths.state, &sample_state());
        write_json(&paths.settings, &settings);
        write_json(
            &paths.follows,
            &FollowsFile {
                items: vec![follow(1, "Takip", true)],
                ..Default::default()
            },
        );

        let out = paths.backups.join("test.json");
        let sum = export_bundle(&paths, &out).expect("export");
        assert_eq!(sum.favorites, 1);
        assert_eq!(sum.watched_episodes, 1);
        assert_eq!(sum.follows, 1);

        let bundle = read_bundle(&out).expect("read");
        assert_eq!(bundle.settings.theme, "aurora");
        assert!(bundle.settings.cf_clearance.is_empty());
        assert!(bundle.settings.official_skip_secret.is_empty());
        assert!(bundle.settings.download_dir.is_none());
        assert!(bundle.settings.discord_rpc_socket.is_none());
        assert_eq!(bundle.state.saved.len(), 1);
        assert_eq!(bundle.follows.len(), 1);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "yedek dosyası özel olmalı");
        }
        let _ = std::fs::remove_dir_all(paths.state.parent().unwrap());
    }

    #[test]
    fn imported_settings_keep_current_secrets_and_paths() {
        let mut current = Settings::default();
        current.cf_clearance = "MEVCUT".into();
        current.official_skip_secret = "MEVCUT_SR".into();
        current.download_dir = Some("/mevcut".into());
        current.discord_rpc_socket = Some("/mevcut.sock".into());

        let mut imported = Settings::default();
        imported.theme = "neon".into();
        imported.cf_clearance = "YEDEK".into();
        imported.download_dir = Some("/yedek".into());

        let merged = apply_imported_settings(&current, &imported);
        assert_eq!(merged.theme, "neon");
        assert_eq!(merged.cf_clearance, "MEVCUT");
        assert_eq!(merged.official_skip_secret, "MEVCUT_SR");
        assert_eq!(merged.download_dir.as_deref(), Some("/mevcut"));
        assert_eq!(merged.discord_rpc_socket.as_deref(), Some("/mevcut.sock"));
    }

    #[test]
    fn merge_state_keeps_local_on_conflict_and_fills_missing() {
        let mut local = State::default();
        local.saved = vec![Title {
            id: 1,
            name: "Yerel".into(),
            ..Default::default()
        }];
        local.progress.insert("1:1:1".into(), (10.0, 100.0));
        local.marathon = vec![crate::api::MarathonItem {
            title: Title {
                id: 1,
                name: "Yerel".into(),
                ..Default::default()
            },
            completed: false,
            added_at: 1,
        }];
        local.watched.insert(
            "1".into(),
            vec![Watched {
                title_id: 1,
                season: 1,
                episode: 1,
                watched_at: 10,
                ..Default::default()
            }],
        );

        let mut imported = State::default();
        imported.saved = vec![
            Title {
                id: 1,
                name: "Yedek".into(),
                ..Default::default()
            },
            Title {
                id: 2,
                name: "Yeni".into(),
                ..Default::default()
            },
        ];
        imported.progress.insert("1:1:1".into(), (90.0, 100.0));
        imported.progress.insert("2:1:1".into(), (5.0, 100.0));
        imported.marathon = vec![
            crate::api::MarathonItem {
                title: Title {
                    id: 1,
                    name: "Yedek".into(),
                    ..Default::default()
                },
                completed: true,
                added_at: 2,
            },
            crate::api::MarathonItem {
                title: Title {
                    id: 2,
                    name: "Yeni".into(),
                    ..Default::default()
                },
                completed: false,
                added_at: 2,
            },
        ];
        imported.watched.insert(
            "1".into(),
            vec![Watched {
                title_id: 1,
                season: 1,
                episode: 1,
                watched_at: 20,
                ..Default::default()
            }],
        );
        imported.right_click_tip_seen = true;

        let merged = merge_state(&local, &imported);
        assert_eq!(merged.saved.len(), 2);
        assert_eq!(merged.progress["1:1:1"].0, 10.0, "yerel ilerleme korunmalı");
        assert_eq!(merged.progress["2:1:1"].0, 5.0);
        assert_eq!(merged.marathon.len(), 2);
        assert!(!merged.marathon[0].completed, "yerel maraton korunmalı");
        assert_eq!(
            merged.watched["1"][0].watched_at, 20,
            "yeni izleme kazanmalı"
        );
        assert!(merged.right_click_tip_seen, "bayraklar OR");
    }

    #[test]
    fn merge_follows_local_wins() {
        let local = vec![follow(1, "Yerel", true)];
        let imported = vec![follow(1, "Yedek", false), follow(2, "Yeni", true)];
        let merged = merge_follows(&local, &imported);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "Yerel");
        assert!(merged[1].notify);
    }

    #[test]
    fn auto_backups_keep_last_five_and_can_be_deleted() {
        let paths = temp_paths("autobackup");
        write_json(&paths.state, &State::default());
        write_json(&paths.settings, &Settings::default());
        write_json(&paths.follows, &FollowsFile::default());
        let first = auto_backup(&paths, "import").expect("backup");
        for _ in 0..5 {
            auto_backup(&paths, "import").expect("backup");
        }
        let list = list_auto_backups(&paths);
        assert_eq!(list.len(), AUTO_BACKUP_KEEP);
        assert!(!first.exists(), "en eski yedek budanmalı");
        assert_eq!(delete_all_backups(&paths).unwrap(), AUTO_BACKUP_KEEP);
        assert!(list_auto_backups(&paths).is_empty());
    }

    #[test]
    fn import_replace_creates_backup_and_keeps_secrets() {
        let paths = temp_paths("import");
        let mut current = Settings::default();
        current.cf_clearance = "MEVCUT".into();
        write_json(&paths.state, &State::default());
        write_json(&paths.settings, &current);
        write_json(&paths.follows, &FollowsFile::default());

        let mut imported_settings = Settings::default();
        imported_settings.theme = "neon".into();
        let bundle = BackupBundle {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            app_version: "1.4.4".into(),
            exported_at: 1,
            state: sample_state(),
            settings: imported_settings,
            follows: vec![],
        };
        let report = import_bundle(&paths, &bundle, ImportMode::Replace).expect("import");
        assert!(report.auto_backup.exists());
        let saved: Settings =
            serde_json::from_str(&std::fs::read_to_string(&paths.settings).unwrap()).unwrap();
        assert_eq!(saved.theme, "neon");
        assert_eq!(saved.cf_clearance, "MEVCUT");
        let _ = std::fs::remove_dir_all(paths.state.parent().unwrap());
    }

    #[test]
    fn import_merge_preserves_settings_and_merges_state() {
        let paths = temp_paths("import-merge");
        let mut current = Settings::default();
        current.theme = "koyu".into();
        let mut current_state = State::default();
        current_state.saved = vec![Title {
            id: 1,
            name: "Yerel".into(),
            ..Default::default()
        }];
        write_json(&paths.state, &current_state);
        write_json(&paths.settings, &current);
        write_json(&paths.follows, &FollowsFile::default());

        let bundle = BackupBundle {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            app_version: "1.4.4".into(),
            exported_at: 1,
            state: State {
                saved: vec![Title {
                    id: 2,
                    name: "Yedek".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            settings: Settings {
                theme: "neon".into(),
                ..Default::default()
            },
            follows: vec![],
        };
        import_bundle(&paths, &bundle, ImportMode::Merge).expect("merge");
        let saved_state: State =
            serde_json::from_str(&std::fs::read_to_string(&paths.state).unwrap()).unwrap();
        let saved_settings: Settings =
            serde_json::from_str(&std::fs::read_to_string(&paths.settings).unwrap()).unwrap();
        assert_eq!(saved_state.saved.len(), 2);
        assert_eq!(
            saved_settings.theme, "koyu",
            "birleştirmede ayarlar korunur"
        );
        let _ = std::fs::remove_dir_all(paths.state.parent().unwrap());
    }

    #[test]
    fn read_bundle_rejects_wrong_format_and_oversize() {
        let dir = std::env::temp_dir().join(format!("animecix-bad-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.json");
        std::fs::write(
            &bad,
            r#"{"format":"baska","format_version":1,"state":{},"settings":{},"follows":[]}"#,
        )
        .unwrap();
        assert!(read_bundle(&bad).is_err(), "yanlış format reddedilmeli");

        let big = dir.join("big.json");
        std::fs::write(&big, vec![b'0'; (MAX_BACKUP_BYTES as usize) + 1]).unwrap();
        assert!(read_bundle(&big).is_err(), "aşırı büyük dosya reddedilmeli");

        let empty = dir.join("empty.json");
        std::fs::write(&empty, b"{}").unwrap();
        assert!(read_bundle(&empty).is_err(), "boş JSON yedek sayılmamalı");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_bundle_rejects_live_files() {
        let paths = temp_paths("live-json");
        write_json(&paths.state, &sample_state());
        write_json(&paths.settings, &Settings::default());
        write_json(&paths.follows, &FollowsFile::default());
        for path in [&paths.state, &paths.settings, &paths.follows] {
            assert!(
                read_bundle(path).is_err(),
                "canlı dosya yedek olarak kabul edilmemeli: {}",
                path.display()
            );
        }
        let _ = std::fs::remove_dir_all(paths.state.parent().unwrap());
    }

    #[test]
    fn export_rejects_live_paths() {
        let paths = temp_paths("export-live");
        write_json(&paths.state, &sample_state());
        write_json(&paths.settings, &Settings::default());
        write_json(&paths.follows, &FollowsFile::default());
        assert!(export_bundle(&paths, &paths.state).is_err());
        assert!(export_bundle(&paths, &paths.settings).is_err());
        assert!(export_bundle(&paths, &paths.follows).is_err());
        assert!(export_bundle(&paths, &paths.downloads).is_err());
        let _ = std::fs::remove_dir_all(paths.state.parent().unwrap());
    }

    #[test]
    fn auto_backup_falls_back_to_raw_copy_for_corrupt_live_file() {
        let paths = temp_paths("raw-backup");
        write_json(&paths.state, &sample_state());
        write_json(&paths.settings, &Settings::default());
        std::fs::create_dir_all(paths.follows.parent().unwrap()).unwrap();
        std::fs::write(&paths.follows, b"{bozuk-json").unwrap();

        let backup = auto_backup(&paths, "import").expect("raw backup");
        assert!(backup.is_dir(), "ham kopya dizini dönmeli");
        assert!(backup.join("state.json").exists());
        assert!(backup.join("follows.json").exists());
        let _ = std::fs::remove_dir_all(paths.state.parent().unwrap());
    }

    #[test]
    fn import_replace_with_corrupt_live_files_is_recoverable() {
        let paths = temp_paths("corrupt-import");
        std::fs::create_dir_all(paths.state.parent().unwrap()).unwrap();
        std::fs::write(&paths.state, b"{bozuk").unwrap();
        std::fs::write(&paths.settings, b"{bozuk").unwrap();
        std::fs::create_dir_all(paths.follows.parent().unwrap()).unwrap();
        std::fs::write(&paths.follows, b"{bozuk").unwrap();

        let bundle = BackupBundle {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            app_version: "1.4.4".into(),
            exported_at: 1,
            state: sample_state(),
            settings: Settings::default(),
            follows: vec![],
        };
        let report = import_bundle(&paths, &bundle, ImportMode::Replace).expect("import");
        assert!(report.auto_backup.is_dir(), "ham güvenlik yedeği alınmalı");
        let saved: State =
            serde_json::from_str(&std::fs::read_to_string(&paths.state).unwrap()).unwrap();
        assert_eq!(saved.saved.len(), 1);
        let _ = std::fs::remove_dir_all(paths.state.parent().unwrap());
    }

    #[test]
    fn reset_live_data_keeps_backups_and_clears_downloads() {
        let paths = temp_paths("reset");
        write_json(&paths.state, &sample_state());
        write_json(&paths.settings, &Settings::default());
        write_json(&paths.follows, &FollowsFile::default());
        std::fs::create_dir_all(&paths.backups).unwrap();
        std::fs::write(paths.backups.join("auto-import-1.json"), b"{}").unwrap();
        std::fs::write(&paths.downloads, b"[]").unwrap();
        std::fs::create_dir_all(&paths.cache).unwrap();

        reset_live_data(&paths).expect("reset");

        assert!(paths.settings.exists());
        assert!(paths.state.exists());
        assert!(!paths.follows.exists());
        assert!(!paths.downloads.exists());
        assert!(!paths.cache.exists());
        assert!(paths.backups.join("auto-import-1.json").exists());
    }
}
