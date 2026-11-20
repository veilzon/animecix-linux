//! Kullanıcı verisi için yedekleme, geri yükleme ve sıfırlama çekirdeği.
//!
//! Bu modül GTK'dan bağımsızdır; dosya yolları `DataPaths` ile enjekte
//! edilir. Böylece testler gerçek kullanıcı dizinine dokunmaz.

use crate::api::{self, Client};
use crate::follow_store::{self, FollowedTitle, FollowsFile};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
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
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub exported_at: u64,
    #[serde(default)]
    pub state: api::State,
    #[serde(default)]
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

fn default_format() -> String {
    FORMAT.to_string()
}

fn default_format_version() -> u32 {
    FORMAT_VERSION
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
    let date = glib::DateTime::now_local()
        .ok()
        .and_then(|d| d.format("%Y-%m-%d").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "yedek".to_string());
    format!("animecix-yedek-{date}.json")
}

/// Canlı veriyi tek JSON yedeğe yazar.
pub fn export_bundle(paths: &DataPaths, dest: &Path) -> Result<BackupSummary, String> {
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
    follow_store::write_json_atomic(dest, bundle)
        .map_err(|e| format!("{} yazılamadı: {e}", dest.display()))
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

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Settings, State, Title, Watched};
    use crate::follow_store::{FollowedTitle, FollowsFile};
    use std::path::{Path, PathBuf};

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
}
