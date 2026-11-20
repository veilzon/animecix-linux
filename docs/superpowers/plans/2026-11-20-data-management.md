# Veri Yönetimi (Yedekleme, Geri Yükleme, Veriyi Koruyarak Kaldırma) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Kullanıcı verilerini tek JSON yedekle dışa/içe aktarmak, son 5 otomatik yedeğe geri dönebilmek ve uygulamayı verileri silmeden kaldırabilmek.

**Architecture:** Yeni `src/data_management.rs` GTK'dan bağımsız saf veri katmanıdır; dosya yolları `DataPaths` ile enjekte edilir. `SettingsView` yalnızca dosya seçici/onay pencerelerini kurar; `App` dosya işlemlerini, ayar yan etkilerini ve yeniden başlatmayı yönetir. Mevcut `wipe_all_data` canlı veriyi sıfırlar ama `backups/` dizinini korur.

**Tech Stack:** Rust 2021, GTK4/libadwaita, serde/serde_json, mevcut `follow_store::write_json_atomic`, mevcut `Client` state/settings yolları.

**Spec:** `docs/superpowers/specs/2026-11-20-data-management-design.md`

## Global Constraints

- Yedek formatı: `format = "animecix-backup"`, `format_version = 1`, tek JSON dosyası.
- Yedeğe girmez: `cf_clearance`, `official_skip_secret`, `download_dir`, `discord_rpc_socket`, kapak/API önbelleği, indirilen dosyalar, `downloads.json`.
- Yedek dosyaları `0600`; içe aktarma dosyası en fazla 10 MB.
- İçe aktarma/geri yükleme öncesi otomatik yedek; en yeni 5 otomatik yedek tutulur.
- `Tüm Verileri Sıfırla` canlı veriyi, takip verisini, indirme kuyruğunu ve önbelleği temizler; `backups/` korunur.
- Kaldırma veri silmez; sıfırlama uygulamayı kaldırmaz.
- Mevcut test komutu: `cargo test --bin animecix`.
- Yeni bağımlılık eklenmez.

## Review Focus

1. Bozuk/yanlış formatlı/aşırı büyük yedek dosyası içe aktarılırken mevcut verinin bozulmaması.
2. Tam geri yüklemede mevcut `cf_clearance`, `official_skip_secret`, `download_dir`, `discord_rpc_socket` değerlerinin korunması.
3. İçe aktarma sonrası takip daemon'u/autostart'ın yeni ayarlarla tutarlı olması.
4. Sıfırlamanın yedekleri koruyup indirme kuyruğunu/önbelleği temizlemesi.
5. Kaldırma yolunun `wipe_all_data` çağırmaması; Flatpak'ta yalnızca terminal yönlendirmesi göstermesi.

---

### Task 1: Yedek çekirdeği — tipler, yollar, dışa aktarma, gizli alanlar

**Files:**
- Create: `src/data_management.rs`
- Modify: `src/main.rs` (module listesine `mod data_management;`)

**Interfaces:**
- Produces:
  - `pub const FORMAT: &str`, `pub const FORMAT_VERSION: u32`, `pub const MAX_BACKUP_BYTES: u64`, `pub const AUTO_BACKUP_KEEP: usize`
  - `pub struct DataPaths { state, settings, follows, backups, downloads, cache: PathBuf }`
  - `impl DataPaths { pub fn system() -> Self; pub fn new(state, settings, follows, backups, downloads, cache) -> Self }`
  - `pub struct BackupBundle { format, format_version, app_version, exported_at, state: api::State, settings: api::Settings, follows: Vec<follow_store::FollowedTitle> }`
  - `pub struct BackupSummary { app_version, exported_at, favorites, watched_episodes, history, marathon, follows }`
  - `pub fn suggested_filename() -> String`
  - `pub fn export_bundle(paths: &DataPaths, dest: &Path) -> Result<BackupSummary, String>`
  - `pub fn read_bundle(path: &Path) -> Result<BackupBundle, String>`
  - `pub fn summary(bundle: &BackupBundle) -> BackupSummary`
  - `pub fn apply_imported_settings(current: &api::Settings, imported: &api::Settings) -> api::Settings`

- [ ] **Step 1: Write the failing tests**

Add to `src/data_management.rs` (new file can hold tests at the end):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Settings, State, Title, Watched};
    use crate::follow_store::{FollowedTitle, FollowsFile};
    use std::path::{Path, PathBuf};

    fn temp_paths(tag: &str) -> DataPaths {
        let base = std::env::temp_dir().join(format!(
            "animecix-data-{tag}-{}",
            std::process::id()
        ));
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
            saved: vec![Title { id: 1, name: "Favori".into(), ..Default::default() }],
            watched: std::collections::HashMap::from([(
                "1".into(),
                vec![Watched { title_id: 1, season: 1, episode: 1, ..Default::default() }],
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin animecix data_management::tests::export_strips_secrets_and_reads_back`
Expected: compile error / FAIL because `data_management` does not exist.

- [ ] **Step 3: Implement the module core**

Add `mod data_management;` to `src/main.rs` (alphabetically after `covers`).

In `src/data_management.rs`:
- Add `FORMAT`, `FORMAT_VERSION`, `MAX_BACKUP_BYTES = 10 * 1024 * 1024`, `AUTO_BACKUP_KEEP = 5`.
- Define `DataPaths` with `system()` using `api::Client::state_path()`, `api::Client::settings_path()`, `follow_store::follows_path()`, settings parent + `backups`, `download::queue_file_path()`, and `$HOME/.cache/animecix`.
- Define `BackupBundle` and `BackupSummary` with `serde` derive/defaults.
- `export_bundle`: read state/settings/follows (missing file = default), normalize settings, blank `cf_clearance`/`official_skip_secret`, set `download_dir`/`discord_rpc_socket` to `None`, build bundle with `app_version = env!("CARGO_PKG_VERSION")` and `exported_at = now_secs`.
- Write pretty JSON atomically (reuse `follow_store::write_json_atomic`), set mode `0600` on Unix.
- `read_bundle`: reject files larger than `MAX_BACKUP_BYTES`; parse; require `format == FORMAT` and `format_version <= FORMAT_VERSION`; return error otherwise.
- `summary`: count favorites, watched episodes, history, marathon, follows.
- `apply_imported_settings`: clone imported, then overwrite the four local-only fields from current, normalize, return.
- `suggested_filename`: `animecix-yedek-YYYY-AA-GG.json` using `glib::DateTime::now_local()`; fall back to `animecix-yedek.json`.
- Add a private `read_json_or_default<T: DeserializeOwned + Default>`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin animecix data_management::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/data_management.rs
git commit -m "feat(data): add backup bundle and secret-safe export"
```

---

### Task 2: Birleştirme, içe aktarma, otomatik yedekler ve sıfırlama

**Files:**
- Modify: `src/data_management.rs`

**Interfaces:**
- Produces:
  - `pub enum ImportMode { Replace, Merge }`
  - `pub struct ImportReport { mode: ImportMode, auto_backup: PathBuf, favorites: usize, watched_episodes: usize, history: usize, marathon: usize, follows: usize }`
  - `pub struct BackupInfo { path: PathBuf, created_at: u64, size: u64, kind: String }`
  - `pub fn merge_state(local: &api::State, imported: &api::State) -> api::State`
  - `pub fn merge_follows(local: &[follow_store::FollowedTitle], imported: &[follow_store::FollowedTitle]) -> Vec<follow_store::FollowedTitle>`
  - `pub fn import_bundle(paths: &DataPaths, bundle: &BackupBundle, mode: ImportMode) -> Result<ImportReport, String>`
  - `pub fn auto_backup(paths: &DataPaths, kind: &str) -> Result<PathBuf, String>`
  - `pub fn list_auto_backups(paths: &DataPaths) -> Vec<BackupInfo>`
  - `pub fn delete_all_backups(paths: &DataPaths) -> Result<usize, String>`
  - `pub fn reset_live_data(paths: &DataPaths) -> Result<(), String>`

- [ ] **Step 1: Write the failing tests**

Add tests:

```rust
#[test]
fn merge_state_keeps_local_on_conflict_and_fills_missing() {
    let mut local = State::default();
    local.saved = vec![Title { id: 1, name: "Yerel".into(), ..Default::default() }];
    local.progress.insert("1:1:1".into(), (10.0, 100.0));
    local.marathon = vec![crate::api::MarathonItem {
        title: Title { id: 1, name: "Yerel".into(), ..Default::default() },
        completed: false,
        added_at: 1,
    }];
    local.watched.insert("1".into(), vec![Watched {
        title_id: 1, season: 1, episode: 1, watched_at: 10, ..Default::default()
    }]);
    local.right_click_tip_seen = false;

    let mut imported = State::default();
    imported.saved = vec![
        Title { id: 1, name: "Yedek".into(), ..Default::default() },
        Title { id: 2, name: "Yeni".into(), ..Default::default() },
    ];
    imported.progress.insert("1:1:1".into(), (90.0, 100.0));
    imported.progress.insert("2:1:1".into(), (5.0, 100.0));
    imported.marathon = vec![
        crate::api::MarathonItem {
            title: Title { id: 1, name: "Yedek".into(), ..Default::default() },
            completed: true,
            added_at: 2,
        },
        crate::api::MarathonItem {
            title: Title { id: 2, name: "Yeni".into(), ..Default::default() },
            completed: false,
            added_at: 2,
        },
    ];
    imported.watched.insert("1".into(), vec![Watched {
        title_id: 1, season: 1, episode: 1, watched_at: 20, ..Default::default()
    }]);
    imported.right_click_tip_seen = true;

    let merged = merge_state(&local, &imported);
    assert_eq!(merged.saved.len(), 2);
    assert_eq!(merged.progress["1:1:1"].0, 10.0, "yerel ilerleme korunmalı");
    assert_eq!(merged.progress["2:1:1"].0, 5.0);
    assert_eq!(merged.marathon.len(), 2);
    assert!(!merged.marathon[0].completed, "yerel maraton korunmalı");
    assert_eq!(merged.watched["1"][0].watched_at, 20, "yeni izleme kazanmalı");
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
    for _ in 0..6 {
        auto_backup(&paths, "import").expect("backup");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let list = list_auto_backups(&paths);
    assert_eq!(list.len(), AUTO_BACKUP_KEEP);
    assert!(list[0].created_at >= list[1].created_at);
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
    let saved: Settings = serde_json::from_str(&std::fs::read_to_string(&paths.settings).unwrap()).unwrap();
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
    current_state.saved = vec![Title { id: 1, name: "Yerel".into(), ..Default::default() }];
    write_json(&paths.state, &current_state);
    write_json(&paths.settings, &current);
    write_json(&paths.follows, &FollowsFile::default());

    let bundle = BackupBundle {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        app_version: "1.4.4".into(),
        exported_at: 1,
        state: State {
            saved: vec![Title { id: 2, name: "Yedek".into(), ..Default::default() }],
            ..Default::default()
        },
        settings: Settings { theme: "neon".into(), ..Default::default() },
        follows: vec![],
    };
    import_bundle(&paths, &bundle, ImportMode::Merge).expect("merge");
    let saved_state: State =
        serde_json::from_str(&std::fs::read_to_string(&paths.state).unwrap()).unwrap();
    let saved_settings: Settings =
        serde_json::from_str(&std::fs::read_to_string(&paths.settings).unwrap()).unwrap();
    assert_eq!(saved_state.saved.len(), 2);
    assert_eq!(saved_settings.theme, "koyu", "birleştirmede ayarlar korunur");
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
    let _ = std::fs::remove_dir_all(&dir);
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin animecix data_management::tests`
Expected: FAIL (missing functions/types).

- [ ] **Step 3: Implement merge/import/backup/reset**

Implement:
- `merge_state` exactly as the spec: favorites union by id (local order first), history newest `ts` and 30 max, watched newest `watched_at`, progress local wins, marathon local wins, preferred source local wins, `current` local if any, tip flags OR.
- `merge_follows`: local wins per `title_id`, then imported missing; sort by `title_id`.
- `import_bundle`: validate bundle; call `auto_backup(paths, "import")`; load local files; replace or merge; write state/settings/follows atomically; return counts + backup path.
- `auto_backup`: create `paths.backups`, filename `auto-{sanitize_kind}-{nanos}.json`, call `export_bundle`, trim to `AUTO_BACKUP_KEEP` by newest mtime.
- `list_auto_backups`: only `auto-*.json`, metadata size/mtime, newest first, take 5, `kind` parsed from filename.
- `delete_all_backups`: remove `auto-*.json`, return count.
- `reset_live_data`: write default `State`/`Settings` to their paths; remove follows file/parent follow dir, downloads file, cache dir; leave backups untouched.
- Add a private `sanitize_kind` (`import`/`reset`/`restore` only; anything else `auto`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin animecix data_management::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/data_management.rs
git commit -m "feat(data): add merge, restore and automatic backups"
```

---

### Task 3: Ayarlar UI — dışa/içe aktarma ve önceki yedekler

**Files:**
- Modify: `src/ui/views.rs:510-1378` (`SettingsView::build` and data group)
- Modify: `src/app.rs:2679-2805` (SettingsView call and data callbacks)

**Interfaces:**
- Consumes: Task 1/2 functions.
- Produces:
  - New `SettingsView::build` signature:
    ```rust
    pub fn build(
        settings: &Settings,
        on_save: impl Fn(Settings) + 'static,
        on_export: impl Fn(std::path::PathBuf) + 'static,
        on_import: impl Fn(std::path::PathBuf) + 'static,
        on_restore_backup: impl Fn(std::path::PathBuf) + 'static,
        on_delete_backups: impl Fn() + 'static,
        on_wipe: impl Fn() + 'static,
        on_uninstall: impl Fn() + 'static,
        on_check: impl Fn() + 'static,
    ) -> gtk::Box
    ```
  - `App::show_import_dialog(&self, path: PathBuf)` private method.
  - `App::apply_import(&self, bundle: data_management::BackupBundle, mode: data_management::ImportMode)` private method.
  - `App::reconcile_follow_after_import(&self, settings: &api::Settings)` private method.
  - `pub(crate) fn follow_daemon_should_run(settings: &api::Settings) -> bool` helper in `src/app.rs`.

- [ ] **Step 1: Write the failing UI integration test**

Add a small test in `src/app.rs` tests module (or `src/ui/views.rs` tests) that asserts the new data-management labels exist in the built settings view. Use the existing GTK test pattern:

```rust
#[gtk::test]
fn settings_data_group_has_export_and_previous_backups() {
    let view = crate::ui::views::SettingsView::build(
        &crate::api::Settings::default(),
        |_| {},
        |_| {},
        |_| {},
        |_| {},
        || {},
        || {},
        || {},
        || {},
    );
    let text = collect_labels(&view);
    assert!(text.contains("Verileri Dışa Aktar"));
    assert!(text.contains("Verileri İçe Aktar"));
    assert!(text.contains("Önceki Yedekler"));
    assert!(text.contains("Uygulamayı Kaldır (Verileri Koru)"));
    assert!(text.contains("Tüm Verileri Sıfırla"));
}
```

If no `collect_labels` helper exists, add a local test helper that recursively walks `gtk::Widget` children and concatenates `gtk::Label` texts. The test must fail before the UI change because the labels are absent.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin animecix settings_data_group_has_export_and_previous_backups`
Expected: FAIL (labels absent / signature mismatch).

- [ ] **Step 3: Implement the UI and App handlers**

In `src/ui/views.rs`:
- Replace the current mixed uninstall/wipe block with four separate rows/actions:
  - Export row + `Dışa Aktar` button → `FileChooserNative` Save, JSON filter, default name `data_management::suggested_filename()`; on accept call `on_export(path)`.
  - Import row + `İçe Aktar` button → `FileChooserNative` Open, JSON filter; on accept call `on_import(path)`.
  - Previous backups: call `data_management::list_auto_backups(&DataPaths::system())`; show date/size/kind rows with `Geri Yükle` buttons; empty state "Henüz otomatik yedek yok."; add `Tüm Yedekleri Sil` button with confirmation → `on_delete_backups()`.
  - Uninstall row: title `Uygulamayı Kaldır (Verileri Koru)`, subtitle `Paket/uygulama kaldırılır; izleme geçmişi, ayarlar ve yedekler korunur.`; button `Kaldır`; confirm dialog → `on_uninstall()`.
  - Reset row: title `Tüm Verileri Sıfırla`, subtitle `Canlı verileri, indirme kuyruğunu ve önbelleği siler; otomatik yedekler korunur.`; destructive `Sıfırla` button; confirm dialog → `on_wipe()`.
- Use `glib::DateTime::from_unix_local(created_at as i64)` and `format("%d.%m.%Y %H:%M")` for row titles; `crate::download::fmt_bytes(size)` for size.
- Keep Flatpak behavior: uninstall row subtitle shows `flatpak uninstall io.github.veilzon.AnimeciX`; clicking confirm calls `on_uninstall()` which only shows the command.

In `src/app.rs`:
- Update `SettingsView::build` call with the new closures.
- `on_export`: `data_management::export_bundle(&DataPaths::system(), &path)`; show success toast with path; show error toast on failure.
- `on_import` and `on_restore_backup`: call `self.show_import_dialog(path)`.
- `show_import_dialog`: read bundle, show summary (date/version/favorites/watched/history/marathon/follows), responses `Tam Geri Yükle`, `Birleştir`, `İptal`; call `apply_import`.
- `apply_import`: `import_bundle`; on success reload settings, update `self.settings`, call `client.set_cf_clearance`, `reconcile_follow_after_import`, show toast, then `glib::timeout_add_local_once(700ms, crate::restart_app)`. On error, error toast; do not touch in-memory data.
- `follow_daemon_should_run`: returns `settings.calendar_watch && settings.calendar_watch_autostart`.
- `reconcile_follow_after_import`: stop daemon, remove managed autostart, then if `follow_daemon_should_run(settings)` install autostart and spawn daemon.
- Add a unit test in `src/app.rs`:
  ```rust
  #[test]
  fn follow_daemon_after_import_requires_both_flags() {
      let mut s = crate::api::Settings::default();
      s.calendar_watch = false;
      s.calendar_watch_autostart = true;
      assert!(!super::follow_daemon_should_run(&s));
      s.calendar_watch = true;
      assert!(super::follow_daemon_should_run(&s));
  }
  ```
- `on_delete_backups`: call `data_management::delete_all_backups`, toast count, `show_page(&Page::Settings)`.
- `on_wipe`/`on_uninstall`: implement in Task 4; for this task, wire placeholders that call existing wipe/uninstall behavior so the UI compiles.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin animecix settings_data_group_has_export_and_previous_backups`
Expected: PASS.

Run: `cargo check --all-targets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/views.rs src/app.rs
git commit -m "feat(settings): add backup import/export and previous backups UI"
```

---

### Task 4: Veriyi koruyarak kaldırma ve güvenli sıfırlama

**Files:**
- Modify: `src/app.rs:2770-2805` (wipe/uninstall callbacks)
- Modify: `src/api.rs:4130-4158` (`wipe_all_data`)
- Test: `src/data_management.rs` (reset test already added)

**Interfaces:**
- Consumes: `DataPaths::system()`, `data_management::auto_backup`, `data_management::reset_live_data`, `native_package::uninstall()`, `flatpak::is_flatpak()`.
- Produces: `on_wipe` creates a `reset` auto backup, then calls `client.wipe_all_data()` and goes to Welcome; `on_uninstall` removes the package/app without any data wipe.

- [ ] **Step 1: Write the failing test**

Add/keep the reset test from Task 2 and add one assertion that the reset helper does not remove backups. If Task 2 already covers it, add a second focused test:

```rust
#[test]
fn reset_keeps_backup_but_removes_live_files() {
    let paths = temp_paths("reset-keep");
    write_json(&paths.state, &State::default());
    write_json(&paths.settings, &Settings::default());
    write_json(&paths.follows, &FollowsFile::default());
    std::fs::create_dir_all(&paths.backups).unwrap();
    std::fs::write(paths.backups.join("auto-reset-1.json"), b"{}").unwrap();
    std::fs::write(&paths.downloads, b"[]").unwrap();
    reset_live_data(&paths).expect("reset");
    assert!(!paths.downloads.exists());
    assert!(paths.backups.join("auto-reset-1.json").exists());
}
```

Run: `cargo test --bin animecix data_management::tests::reset_keeps_backup_but_removes_live_files`
Expected: PASS after Task 2; if it already passes, use it as the regression guard for Task 4 and do not alter it.

- [ ] **Step 2: Implement safe uninstall and reset**

In `src/app.rs`:
- Replace the old `on_wipe`/`on_uninstall` branch:
  - `on_wipe`: `let paths = DataPaths::system(); if let Err(e) = data_management::auto_backup(&paths, "reset") { ops.error(...); return; }` then `this_wipe.client.wipe_all_data();` clear history/show Welcome. Do **not** call `native_package::uninstall`.
  - `on_uninstall`: if Flatpak, show info toast with `flatpak uninstall io.github.veilzon.AnimeciX` and keep data. Else `native_package::uninstall()`; on success `std::process::exit(0)`; on error error toast. Do **not** call `wipe_all_data`.
- In `src/api.rs` `wipe_all_data`:
  - Clear in-memory cache.
  - Call `data_management::reset_live_data(&data_management::DataPaths::system())`.
  - Remove `/tmp/animecix-*` files.
  - Stop daemon and remove managed autostart.
  - Do not remove `backups/`.

- [ ] **Step 3: Verify the uninstall command path**

Run: `cargo test --bin animecix native_package::tests::uninstall_native_dry_run_uses_package_manager`
Expected: PASS (unchanged).

Run: `cargo check --all-targets`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/api.rs
git commit -m "feat(settings): separate uninstall from data reset"
```

---

### Task 5: Belgeler ve son doğrulama

**Files:**
- Modify: `README.md` (Yapılandırma & Veri, Ayarlar, kaldırma)
- Modify: `CHANGELOG.md` (kısa “yayınlanmadı” notu veya sonraki sürüm başlığı)
- Modify: `docs/superpowers/specs/2026-11-20-data-management-design.md` only if implementation deviates.

- [ ] **Step 1: Update docs**

Add to README:
- Yedek: `Ayarlar → Veri Yönetimi → Verileri Dışa Aktar`; içe aktarma/geri yükleme ve otomatik yedekler.
- Son 5 otomatik yedek ve `Tüm Yedekleri Sil`.
- Kaldırma: `Uygulamayı Kaldır (Verileri Koru)` verileri silmez; `Tüm Verileri Sıfırla` uygulamayı kaldırmaz.
- Data paths listesine `~/.local/share/animecix/backups/` ekle.

- [ ] **Step 2: Run the full test suite**

Run: `cargo fmt`
Run: `cargo test --bin animecix`
Run: `cargo check --all-targets`
Expected: all green.

- [ ] **Step 3: Manual UI smoke**

Run:
```bash
cargo build
```
Open the app, go to `Ayarlar → Veri Yönetimi`:
- Export a backup; confirm the file exists.
- Import it; choose `Birleştir`; confirm restart.
- Reopen settings; restore the automatic backup; choose `Tam Geri Yükle`.
- Use `Uygulamayı Kaldır (Verileri Koru)` only in a disposable test install; confirm `state.json`/`settings.json` still exist after removal.

- [ ] **Step 4: Commit**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: data backup and uninstall behavior"
```

---

## Execution Notes

- Keep `DataPaths` inject-able; all file tests use temp directories, never the real home.
- Do not log backup contents or secrets.
- Do not move or delete the existing `docs/superpowers/specs/2026-11-20-data-management-design.md`.
- If the `SettingsView` file chooser needs a parent window and `None` causes a runtime warning, pass the settings window via a new `parent: &gtk::Window` argument; keep the behavior otherwise.
