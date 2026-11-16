# Uygulama İçi mpv Oynatıcı Uygulama Planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Varsayılan oynatmayı uygulama içi libmpv/GtkGLArea oynatıcısına taşımak; harici mpv'yi ayar olarak korumak ve mevcut tüm oynatma özelliklerini entegre etmek.

**Architecture:** `libmpv` çalışma anında `libloading` ile aranır ve ham FFI üzerinden `GtkGLArea`'ya çizilir. `src/app/player/` altındaki oynatıcı, `Page::Player` olarak mevcut `Stack`'e girer; `play_candidates` gömülü/harici dallara ayrılır. Ortak `SkipPlan`, progres, Discord RPC, kaynak failover ve bölüm geçişi iki yol tarafından paylaşılır.

**Tech Stack:** Rust 2021, GTK4 0.8, libadwaita 0.6, `libloading`, ham libmpv C FFI, OpenGL 3.0, mevcut `Client`/`SkipPlan`/`DownloadManager`.

**Spec:** `docs/superpowers/specs/2026-11-16-embedded-player-design.md`

## Global Constraints

- Üretimde `libmpv-sys` kullanılmaz; `libloading` + yerel FFI kullanılır.
- Yükleme sırası: `ANIMECIX_LIBMPV_PATH`, `$APPDIR/usr/lib/libmpv.so.2`, `$APPDIR/usr/lib/libmpv.so.1`, `/app/lib/libmpv.so.2`, `libmpv.so.2`, `libmpv.so.1`, `libmpv.so`.
- `mpv_client_api_version()` ana sürümü `2` değilse gömülü oynatıcı reddedilir; uygulama çökmez, harici mpv yoluna düşer.
- `Settings.embedded_player` varsayılanı `true`; eski ayar dosyaları da `true` okur. Ayar yalnızca bir sonraki oynatmada etkilidir.
- Lowell'in Focus Mod'u, arka plan AniSkip fetch'i ve otomatik intro/outro atlaması getirilmez. Mevcut `SkipPlan`, `s`/`e`, `show_intro_hint`, `show_music_hint`, `auto_next` korunur.
- Fansub seçimi ve mevcut harici mpv süpervizörü değişmez.
- Sürüm `1.3.5` kalır; release/tag üretilmez.
- `packaging/post-install-cleanup.sh`, `inf`, `specs/` kullanıcı dosyaları commit edilmez.
- Arch testi yalnızca CI'da yapılır; yerelde Arch container çalıştırılmaz.
- Tema: player video üstünde koyu scrim + açık metin; vurgu rengi mevcut `@accent_color`/`@accent_bg_color` üzerinden gelir.

## Review Focus

- Eksik veya ABI uyumsuz libmpv: uygulama açılmaya devam etmeli, oynatma harici mpv'ye düşmeli, kullanıcı bir kez bilgilendirilmeli (Task 1, 3).
- Render bağlamı mpv handle'ından önce yok edilmeli; ikinci `shutdown` panic üretmemeli (Task 3 smoke, `--player-smoke`).
- Kaynak değişiminde konum korunmalı, sıfıra dönmemeli; aynı konum tekrar tekrar seek edilmemeli (Task 4).
- RefCell guard tutarken GTK callback ile aynı hücreye mutable erişim panic üretmemeli (Task 3-7; mevcut `tests/refcell_guard.rs` kuralı).
- Uzun başlık/bölüm adları taşmamalı; 5 temada kart ve kontroller okunabilmeli (Task 3, 6; görsel kontrol Task 9).
- Geç gelen bölüm listesi kapanmış/eski oynatıcıya yazılmamalı (Task 5, generation testi).
- Aynı intro/outro kartı her tick'te yeniden gösterilmemeli; `s`/`e` tekrar seek üretmemeli (Task 6).
- Otomatik sonraki bölüm EOF'ta bir kez tetiklenmeli (Task 5, mevcut `should_auto_next` testi).
- AppImage'da libmpv ve bağımlılıkları bulunmalı; boyut artışı ölçülüp raporlanmalı (Task 8).

---

### Task 1: Dinamik libmpv yükleyici ve `--check-libmpv`

**Files:**
- Create: `src/embed_mpv/mod.rs`
- Create: `src/embed_mpv/loader.rs`
- Modify: `Cargo.toml` (`libloading = "0.8"`)
- Modify: `src/main.rs` (`mod embed_mpv;`, `--check-libmpv` erken dalı)
- Test: `src/embed_mpv/loader.rs` içindeki `#[cfg(test)]`

**Interfaces:**
- Produces: `pub struct LoadedLibmpv { pub library: libloading::Library, pub path: PathBuf, pub api_version: u64 }`
- Produces: `pub fn load() -> Result<LoadedLibmpv, String>`
- Produces: `pub fn api_version_compatible(version: u64) -> bool`
- Produces: `pub fn candidates(appdir: Option<&str>, override_path: Option<&str>) -> Vec<String>`
- Produces: `pub fn version_label(version: u64) -> String` (`2.5` biçimi)

- [ ] **Step 1: Yükleyici testlerini yaz**

```rust
#[test]
fn api_version_accepts_2x_only() {
    assert!(api_version_compatible((2 << 16) | 5));
    assert!(api_version_compatible((2 << 16) | 0));
    assert!(!api_version_compatible((1 << 16) | 9));
    assert!(!api_version_compatible((3 << 16) | 0));
}

#[test]
fn candidates_prefer_override_then_bundled_then_soname() {
    let c = candidates(Some("/tmp/AppDir"), Some("/opt/mpv/libmpv.so.2"));
    assert_eq!(c[0], "/opt/mpv/libmpv.so.2");
    assert_eq!(c[1], "/tmp/AppDir/usr/lib/libmpv.so.2");
    assert_eq!(c[3], "/app/lib/libmpv.so.2");
    assert!(c.ends_with(&["libmpv.so.2".into(), "libmpv.so.1".into(), "libmpv.so".into()]));
    assert_eq!(version_label((2 << 16) | 5), "2.5");
}
```

- [ ] **Step 2: Testlerin kırmızı olduğunu doğrula**

Run: `cargo test --release --bin animecix loader::tests -- --test-threads=1`
Expected: FAIL — `api_version_compatible`/`candidates` yok.

- [ ] **Step 3: `loader.rs` ve `mod.rs`'i uygula**

`load()` her adayı sırayla dener; dosya yoksa/hata verirse sonraki adaya geçer. Bulunan kütüphaneden `mpv_client_api_version` sembolü `unsafe { lib.get::<unsafe extern "C" fn() -> u64>(b"mpv_client_api_version\0") }` ile alınır. Sürüm uyumsuzsa kütüphane kapatılır ve hata metni aday yolunu içerir. `LoadedLibmpv` sembol tablosunu değil yalnızca yüklenmiş kütüphaneyi taşır; sonraki görev aynı struct'a fonksiyon işaretçilerini ekler.

- [ ] **Step 4: `--check-libmpv` tanı komutunu ekle**

`src/main.rs` içinde GTK/`Client` kurulmadan önce:
```rust
if raw_args.iter().any(|a| a == "--check-libmpv") {
    return diagnostics::cmd_check_libmpv();
}
```
`src/diagnostics.rs`'e ekle:
- başarı: `libmpv: <path> (API 2.5)` + `exit 0`
- hata: `libmpv bulunamadı: <neden>` + `exit 1`

- [ ] **Step 5: Testleri ve komutu doğrula**

Run: `cargo test --release --bin animecix loader::tests -- --test-threads=1`
Expected: PASS.
Run: `ANIMECIX_LIBMPV_PATH=/tmp/opencode/animecix-mpv-dev/usr/lib64/libmpv.so.2 cargo run --release --bin animecix -- --check-libmpv`
Expected: `libmpv: /tmp/.../libmpv.so.2 (API 2.5)` ve `exit 0`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/embed_mpv src/diagnostics.rs
git commit -m "feat(player): libmpv dinamik yükleyici ve tanı komutu"
```

---

### Task 2: Ham libmpv FFI çekirdeği ve OpenGL render bağlamı

**Files:**
- Create: `src/embed_mpv/ffi.rs`
- Create: `src/embed_mpv/render.rs`
- Create: `src/embed_mpv/player.rs`
- Modify: `src/embed_mpv/mod.rs`
- Modify: `src/diagnostics.rs` (check komutu artık başsız instance açar)
- Test: `src/embed_mpv/player.rs` içindeki `#[cfg(test)]`

**Interfaces:**
- Consumes: `LoadedLibmpv` (Task 1).
- Produces: `pub struct MpvApi` (kütüphane + fonksiyon tablosu), `pub fn MpvApi::from_loaded(loaded: LoadedLibmpv) -> Result<Self, String>`.
- Produces: `pub struct MpvEmbed`; `MpvEmbed::new(api: Rc<MpvApi>, opts: &[(&str, &str)], start_pos: Option<f64>) -> Result<Self, String>`.
- Produces: `RenderContext::new(api: Rc<MpvApi>, handle: *mut MpvHandle) -> Result<Self, String>`; `render(fbo: i32, w: i32, h: i32, flip_y: bool)`; `report_swap()`; `set_update_callback(F)`.
- Produces `MpvEmbed` metotları: `load_url`, `set_http_headers`, `toggle_pause`, `set_pause`, `seek_abs`, `seek_rel`, `stop`, `pos`, `dur`, `volume`, `set_volume`, `muted`, `set_mute`, `speed`, `set_speed`, `paused`, `idle`, `eof`, `core_idle`, `video_height`, `panscan`, `set_panscan`, `tracks`, `set_audio`, `set_sub`, `show_text`, `screenshot_to_file`, `video_info`.

- [ ] **Step 1: Saf yardımcı testlerini yaz**

```rust
#[test]
fn mpv_cli_args_become_options() {
    let args = crate::api::upscale_mpv_args("hafif", Some("/tmp/x.glsl"), Some(1080));
    let opts = options_from_cli(&args);
    assert!(opts.iter().any(|(k, _)| k == "glsl-shaders"));
    assert!(opts.iter().all(|(k, _)| !k.starts_with("--")));
}

#[test]
fn resume_only_after_five_seconds() {
    assert_eq!(start_pos_option(None), None);
    assert_eq!(start_pos_option(Some(4.9)), None);
    assert_eq!(start_pos_option(Some(5.0)), Some(5.0));
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix embed_mpv::player::tests -- --test-threads=1`
Expected: FAIL — modül/fonksiyonlar yok.

- [ ] **Step 3: FFI ve render'ı yaz**

- `ffi.rs`: `#[repr(C)]` `mpv_handle`, `mpv_render_context`, `mpv_opengl_init_params`, `mpv_opengl_fbo`, `mpv_render_param`.
- Sabitler: `MPV_RENDER_PARAM_API_TYPE=1`, `OPENGL_INIT_PARAMS=2`, `OPENGL_FBO=3`, `FLIP_Y=4`; `MPV_FORMAT_STRING=1`, `FLAG=3`, `INT64=4`, `DOUBLE=5`; `MPV_RENDER_API_TYPE_OPENGL = b"opengl\0"`.
- Fonksiyon işaretçileri: `mpv_create`, `mpv_initialize`, `mpv_terminate_destroy`, `mpv_error_string`, `mpv_set_option_string`, `mpv_set_property`, `mpv_get_property`, `mpv_command`, `mpv_free`, `mpv_render_context_create`, `mpv_render_context_render`, `mpv_render_context_report_swap`, `mpv_render_context_free`, `mpv_render_context_set_update_callback`.
- `get_proc_addr` `libc::dlsym(libc::RTLD_DEFAULT, name)` kullanır; Wayland EGL ve X11 GLX aynı yoldan çalışır.
- `MpvEmbed::new`: `vo=libmpv` initialize öncesi; `hwdec=auto-safe`, `hwdec-fallback=yes`, `hr-seek=yes`, `keep-open=yes`, `cache=yes`, `network-timeout=10`, mevcut Chromium UA, `osc=no`, `msg-level=all=warn`; verilen opts + `start` (yalnız >5 sn). Initialize sonrası `volume=100`.
- `RenderContext` GL current iken oluşturulur; `Drop` önce update callback'i `None` yapar, sonra `mpv_render_context_free`. `MpvEmbed` handle'ı render bağlamından sonra düşecek şekilde alan sırası kullanır.
- `render.rs` geçici olarak `#[allow(dead_code)]` taşır; Task 3'te kaldırılır.

- [ ] **Step 4: `--check-libmpv` komutunu başsız instance ile güçlendir**

`diagnostics::cmd_check_libmpv` yükleyici + `MpvApi::from_loaded` + `MpvEmbed::new(api, &[], None)` yapar; başarılıysa `libmpv: <path> (API 2.5), başsız mpv hazır` yazar. Hata durumunda süreç `1` döner.

- [ ] **Step 5: Testleri/komutu doğrula**

Run: `cargo test --release --bin animecix embed_mpv:: -- --test-threads=1`
Expected: PASS.
Run: `ANIMECIX_LIBMPV_PATH=/tmp/opencode/animecix-mpv-dev/usr/lib64/libmpv.so.2 cargo run --release --bin animecix -- --check-libmpv`
Expected: başsız mpv hazır, `exit 0`.

- [ ] **Step 6: Commit**

```bash
git add src/embed_mpv src/diagnostics.rs
git commit -m "feat(player): ham libmpv FFI ve GtkGLArea render bağlamı"
```

---

### Task 3: Gömülü oynatıcı iskeleti, Page::Player ve varsayılan ayar

**Files:**
- Create: `src/app/player/mod.rs`
- Create: `src/app/player/ui.rs`
- Modify: `src/app.rs` (`mod player;`, `Page::Player`, `App.player`, `sidebar_sep`, `clone_ref`, `show_page`, `update_sidebar_selection`, kapatma)
- Modify: `src/app/playback.rs` (`play_candidates` gömülü dalı, `play_embedded`)
- Modify: `src/api.rs` (`embedded_player: bool`, default true, `Settings::default`)
- Modify: `src/ui/views.rs` (Ayarlar → Oynatıcı: gömülü oynatıcı switch'i)
- Modify: `src/main.rs` (`mod embed_mpv` zaten; gerekirse player modülü yok)
- Test: `src/app/player/mod.rs`, `src/api.rs` testleri

**Interfaces:**
- Produces: `pub struct PlayerHost { pub client: Arc<Client>, pub ops: OperationStack, pub discord: DiscordRpc, pub progress: Rc<RefCell<HashMap<String, (f64, f64)>>>, pub on_back: Rc<dyn Fn()>, pub on_play_episode: Rc<dyn Fn(Episode)>, pub on_neighbor: Rc<dyn Fn(i8)>, pub on_download: Rc<dyn Fn(Episode)>, pub on_ready: Rc<dyn Fn()>, pub on_terminal: Rc<dyn Fn(PlayTerminal, String)>, pub on_external: Rc<dyn Fn()> }`
- Produces: `pub struct EmbedRequest { pub title: Title, pub ep: Episode, pub candidates: Vec<String>, pub fast_embeds: Vec<String>, pub fallback_embeds: Vec<String>, pub quality_map: HashMap<String, String>, pub plan: Option<SkipPlan>, pub saved_pos: Option<f64>, pub upscale: String, pub auto_fullscreen: bool, pub show_intro_hint: bool, pub show_music_hint: bool, pub cache_forward_mb: u64, pub cache_back_mb: u64, pub patience_secs: u64, pub auto_next: bool, pub episodes: Vec<Episode>, pub prev_ep: Option<Episode>, pub next_ep: Option<Episode> }`
- Produces: `pub struct EmbeddedPlayer`; `pub fn build(parent: &adw::ApplicationWindow, host: PlayerHost, req: EmbedRequest) -> Option<Self>`; `pub fn widget(&self) -> &gtk::Box`; `pub fn shutdown(&self)`; `pub fn position(&self) -> f64`; `pub fn duration(&self) -> f64`.
- Produces: `App::try_embedded(...) -> bool` (iç kullanım) ve `App::shutdown_player()`.

- [ ] **Step 1: Ayar ve saf yardımcı testlerini yaz**

```rust
#[test]
fn settings_default_to_embedded_player() {
    let s: Settings = serde_json::from_str("{}").expect("defaults");
    assert!(s.embedded_player);
    assert!(Settings::default().embedded_player);
}

#[test]
fn player_page_name_is_stable() {
    assert_eq!(player_page_name(42, 1, 3), "player-42-1-3");
}

#[test]
fn fmt_time_handles_hours_and_invalid() {
    assert_eq!(fmt_time(65.0), "1:05");
    assert_eq!(fmt_time(3661.0), "1:01:01");
    assert_eq!(fmt_time(-1.0), "--:--");
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix settings_default_to_embedded_player -- --test-threads=1`
Expected: FAIL — alan yok.

- [ ] **Step 3: Ayarı ve sayfa/yaşam döngüsünü uygula**

- `Settings`'e `#[serde(default = "default_true")] pub embedded_player: bool`; `Default` içinde `true`.
- `Page`'e `Player { title: Title, ep: Episode }`; sidebar seçiminde `Home` gibi davran.
- `App`'e `pub player: Rc<RefCell<Option<EmbeddedPlayer>>>` ve `pub sidebar_sep: gtk::Separator` ekle; `App::new`/`clone_ref` güncelle.
- `show_page`: Player değilse `shutdown_player()`; Player ise header + sidebar + separator gizle, aksi halde geri getir. Player sayfası adı `player-{id}-{season}-{episode}`.
- `go_back` Player'dan çıkınca önceki sayfaya döner; `show_page` shutdown'ı yapar.
- Pencere `close_request` içinde player'ı `shutdown` et.
- `play_candidates` başlangıcındaki mevcut `current/history/put_title` korunur; hemen sonra `embedded_player` açıksa `try_embedded` denenir. Başsız yükleme başarısızsa bir kez `ops.info("Gömülü oynatıcı yok", "libmpv bulunamadı; mpv penceresinde açılıyor.", 5s)` göster ve harici yola devam et.
- `play_embedded`: `PlayerHost` callback'leri `Weak<App>` ile kur; `EmbedRequest` doldur; eski player'ı kapat; yeni player'ı `App.player`'e koy; `Page::Player`'ı history'ye ekleyip `show_page` çağır. Başarısızsa `None` döner ve harici yola düşer.
- `PlayerHost.on_ready`: mevcut `opening_finish_delay` ile açılış satırını kapat; `on_terminal`: hata/done durumunu OperationStack'e yaz, RPC temizle; `on_back`/`on_play_episode` ilgili App metotlarına bağlanır.

- [ ] **Step 4: Oyuncu iskeletini ve kontrol barını uygula**

- `EmbeddedPlayer::build`: `MpvApi` + `MpvEmbed` + GLArea + overlay + topbar + controls + alt bar.
- İlk kaynak `req.candidates.first()`; yoksa hata rozeti ve `None`. Kaynak yönetimi Task 4'te genişler.
- Video GLArea: realize'te render ctx, 8 ms'lik queue_render poll, render callback, unrealize/shutdown sırası.
- Topbar: solda geri, sağda tam ekran; `adw::ApplicationWindow` fullscreen.
- Merkez: `−10`, oynat/duraklat, `+10`; özel CSS `player-fab`, beyaz/saydam daire, tema accent'i yalnız focus/skip aksiyonunda.
- Alt bar: başlık (ellipsize) · hız · kalite; tam genişlik seek; süre · ses · toplam süre. Tema scrim'i:
  `background: linear-gradient(... rgba(0,0,0,...))`, metin `#FFFFFF`, kart `rgba(14,14,18,.88)`; test `player_css_has_contrast_and_accent`.
- Otomatik gizleme: 1 sn imleç, 2 sn krom; popover/klavye hareketi sayılır.
- Klavye: Space, ←/→, ↑/↓, `m`, `f`, `a`, `n`/`p` callback; tek tık/çift tık ayrımı. `Escape` global davranışa bırakılır.
- `shutdown`: `alive` atomiğini düşür, pause+stop, GL current iken render ctx'i bırak, timer'lar `Break`, son konumu kaydet. İkinci çağrı no-op (test `shutdown_guard_is_idempotent`).
- Ayarlar switch'i: `embedded_player` satırı; `save_all` içinde `updated.embedded_player = emb_r.is_active()`; `active_notify` ile kaydet.

- [ ] **Step 5: Testleri çalıştır**

Run: `cargo test --release --bin animecix player -- --test-threads=1`
Expected: PASS.
Run: `cargo check --release`
Expected: PASS (geçici dead-code uyarıları kabul; final Task 9'da temizlenecek).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/app src/api.rs src/ui/views.rs src/embed_mpv docs/superpowers
git commit -m "feat(player): uygulama içi oynatıcı sayfası ve gömülü varsayılan"
```

---

### Task 4: Kaynak çözme, kalite, failover ve oynatma kaydı

**Files:**
- Create: `src/app/player/source.rs`
- Modify: `src/app/player/mod.rs`, `src/app/player/ui.rs`
- Modify: `src/app.rs` / `src/app/playback.rs` (Discord, progress, op, preferred host)
- Test: `src/app/player/source.rs`

**Interfaces:**
- Produces: `pub enum Source { Direct(String), Embed(String) }`, `impl Source { pub fn hint_url(&self) -> &str; }`
- Produces: `pub struct SourceQueue { pub sources: Vec<Source>, pub quality: Vec<Option<String>>, pub embeds: Vec<Option<String>>, pub index: usize }`
- Produces: `SourceQueue::new(candidates, fast_embeds, fallback_embeds, quality_map) -> Self`
- Produces: `SourceQueue::next_after_failure(&self) -> Option<usize>`
- Produces: `pub fn resume_after_switch(pos: f64, dur: f64) -> Option<f64>` (>=1.0 sn ise Some(pos), sıfır/negatifse None)
- Produces: `App::mark_embedded_progress(...)` iç kullanım.

- [ ] **Step 1: Kaynak testlerini yaz**

```rust
#[test]
fn queue_prioritizes_direct_then_embed_fallbacks() {
    let q = SourceQueue::new(
        vec!["https://a/1.mp4".into()],
        vec!["https://fast/e1".into()],
        vec!["https://fallback/e2".into()],
        HashMap::from([("https://fast/e1".into(), "1080p".into())]),
    );
    assert_eq!(q.sources.len(), 2);
    assert_eq!(q.quality[0].as_deref(), Some("1080p"));
    assert_eq!(q.next_after_failure(), Some(1));
}

#[test]
fn failover_preserves_position_not_zero() {
    assert_eq!(resume_after_switch(123.4, 1440.0), Some(123.4));
    assert_eq!(resume_after_switch(0.5, 1440.0), None);
    assert_eq!(resume_after_switch(-1.0, 1440.0), None);
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix player::source -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Kaynak yöneticisini ve yüklemeyi uygula**

- Direct: `load_url`, sibnet referer varsa `set_http_headers`.
- Embed: arka planda `client.resolve_single(url)`, sonucu `mpsc` ile ana thread'e taşı; polling'de uygula.
- Yükleme başında `resume = saved_pos` (ilk) veya `resume_after_switch(pos, dur)` (failover). Medya süresi >0 olunca bir kez `seek_abs(resume)`.
- `source_quality` etiketleri ve `tau_qualities` (`client.embed_quality_list`) ile kalite popover'ı; kaynak seçimi `load_source_at` üzerinden.
- Kaynak 25 sn'de yüklenmezse/core idle ise `next_after_failure`; tüm kaynaklar bittiyse `on_terminal(Failed, ...)`.
- İlk video hazır olduğunda `on_ready`; hata durumunda `on_terminal(Failed)`.

- [ ] **Step 4: Progress, RPC ve tercih edilen host'u bağla**

- 500 ms tick: `pos/dur`, seek/volume/hız senkronu, `progress` map + `client.save_progress`, 60 sn'de bir `client.report_pos`, `played_enough` olunca `save_watched` (title_name/title_type dahil), `set_preferred_host`.
- Discord aktivitesi oynatma başında `Activity::from_playback`; bitişte `playback_finished` ile temizle. `shutdown`/terminal durumunda da temizle.
- `OperationStack` açılış satırını `on_ready`/`on_terminal` ile mevcut 10 sn kuralına uydur; hata halinde 6 sn kırmızı satır.
- Buffer: `mpv_cache_forward_mb`/`back_mb` değerlerini `demuxer-max-bytes`/`demuxer-max-back-bytes` olarak uygula; 0/None güvenli varsayılana clamp.

- [ ] **Step 5: Testleri doğrula**

Run: `cargo test --release --bin animecix player::source -- --test-threads=1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app/player src/app.rs src/app/playback.rs
git commit -m "feat(player): kaynak failover, kalite, ilerleme ve RPC entegrasyonu"
```

---

### Task 5: Sağ bölüm çekmecesi, bölüm geçişi ve otomatik sonraki

**Files:**
- Create: `src/app/player/episodes.rs`
- Modify: `src/app/player/mod.rs`, `src/app/player/ui.rs`
- Modify: `src/app.rs` (`request_neighbor` görünürlüğü gerekirse)
- Test: `src/app/player/episodes.rs`

**Interfaces:**
- Consumes: `crate::api::neighbor_episode`.
- Produces: `pub fn page_visible_episodes(all: &[Episode], season: u64) -> Vec<Episode>`
- Produces: `pub fn episode_fetch_is_current(response_gen: u64, current_gen: u64) -> bool`
- Produces: `pub fn auto_next_should_fire(...) -> bool` (mevcut `crate::player::should_auto_next` delegasyonu)

- [ ] **Step 1: Testleri yaz**

```rust
#[test]
fn page_visible_episodes_filters_and_sorts() {
    let eps = vec![ep(1, 3), ep(1, 1), ep(2, 1)];
    let out = page_visible_episodes(&eps, 1);
    assert_eq!(out.iter().map(|e| e.episode).collect::<Vec<_>>(), vec![1, 3]);
}

#[test]
fn stale_episode_fetch_is_ignored() {
    assert!(episode_fetch_is_current(7, 7));
    assert!(!episode_fetch_is_current(6, 7));
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix player::episodes -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Çekmeceyi ve gezinmeyi uygula**

- Sağ kenarda 4 px tutamaç; hover/click ile `SlideLeft` revealer. İçeride sezon sekmeleri (`S1`, `S2`) ve bölüm satırları; satırda isim (2 satır ellipsize), `S01E02 · 25 dk`, isteğe bağlı thumbnail.
- Thumbnail: `client.get_bytes` arka planda; `CoverManager::scale_texture` veya mevcut kapak yardımcılarıyla `gtk::Picture`.
- `req.episodes` boşsa `client.episodes(&client.enrich_title(&title))` arka planda; dönen sonuç yalnızca `episode_fetch_is_current` ise uygulanır. Kapanmış oynatıcıya yazılmaz.
- Bölüm tıklaması `host.on_play_episode(ep)`; yeni oynatma App `play` üzerinden. Mevcut bölüm vurgulu, tıklanamaz.
- `n`/`p` ve sağ tık satırları `host.on_neighbor(+1/-1)`; App `request_neighbor` bölüm listesini gerekirse yeniden çeker.
- EOF'ta `should_auto_next(pos, dur, eof, auto_next, already_fired)` true ise tek kez `on_neighbor(1)`; film/tek bölüm/`auto_next=false` durumunda hiç tetiklenmez.

- [ ] **Step 4: Testleri doğrula**

Run: `cargo test --release --bin animecix player::episodes -- --test-threads=1`
Expected: PASS.
Run: `cargo test --release --test next_episode -- --test-threads=1`
Expected: PASS (mevcut davranış korunur).

- [ ] **Step 5: Commit**

```bash
git add src/app/player src/app.rs
git commit -m "feat(player): bölüm çekmecesi ve güvenli bölüm geçişi"
```

---

### Task 6: Modern skip/şarkı kartları

**Files:**
- Create: `src/app/player/notices.rs`
- Modify: `src/app/player/mod.rs`, `src/app/player/ui.rs`
- Modify: `src/music.rs` (tarayıcı açma yardımcısı, opsiyonel saf komut testi)
- Test: `src/app/player/notices.rs`, `src/music.rs`

**Interfaces:**
- Produces: `pub enum SkipKind { Intro, Outro }`
- Produces: `pub struct SkipNotice { pub kind: SkipKind, pub target: f64, pub music: Option<String>, pub song_url: Option<String> }`
- Produces: `pub fn notice_for(times: &SkipTimes, pos: f64, op_done: bool, ed_done: bool, plan: &SkipPlan, hints: bool) -> Option<SkipNotice>`
- Produces: `Music` yardımcısı: `pub fn song_open_command(url: &str) -> (&'static str, Vec<&str>)` (yalnız güvenli https hostları).

- [ ] **Step 1: Testleri yaz**

```rust
#[test]
fn notice_only_once_per_segment_and_only_in_window() {
    let times = SkipTimes { op_start: Some(10.0), op_end: Some(30.0), ..Default::default() };
    let plan = SkipPlan { times: times.clone(), music_op: Some("🎵 Açılış: X".into()), ..Default::default() };
    assert!(notice_for(&times, 8.0, false, false, &plan, true).is_some());
    assert!(notice_for(&times, 9.9, false, false, &plan, true).is_some());
    assert!(notice_for(&times, 31.0, false, false, &plan, true).is_none());
    assert!(notice_for(&times, 12.0, true, false, &plan, true).is_none());
    assert!(notice_for(&times, 12.0, false, false, &plan, false).is_none());
}

#[test]
fn cards_carry_music_and_skip_target() {
    let plan = SkipPlan { times: SkipTimes { ed_start: Some(100.0), ed_end: Some(130.0), ..Default::default() }, music_ed: Some("🏁 Kapanış: Y".into()), song_url: Some("https://open.spotify.com/track/x".into()), ..Default::default() };
    let n = notice_for(&plan.times, 99.0, false, false, &plan, true).unwrap();
    assert_eq!(n.target, 130.0);
    assert!(n.music.unwrap().contains("Kapanış"));
    assert!(n.song_url.is_some());
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix player::notices -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Kart UI'sini ve durum makinesini uygula**

- Video üstünde sağ-alt yüzen `adw::Toast` yerine özel `gtk::Box` kart: sol accent çizgisi, ikon, başlık (`İntro başladı` / `Outro başladı`), şarkı satırı, altında `Atla` ve `Şarkıyı aç` düğmeleri, `S`/`E`/`Shift+M` ipucu.
- `show_intro_hint` kapalıysa kart gösterilmez; `S`/`E` ve sağ-alt atla düğmesi çalışmaya devam eder. `show_music_hint` yalnız ipucu görünürlüğünü etkiler.
- Kart yalnızca segment başına bir kez; `op_done`/`ed_done` ve `op_prompted`/`ed_prompted` ayrı tutulur. `notice_for` bunu sağlar.
- Kart 8 sn sonra kendiliğinden gizlenir; yeni bir kart açılırsa eski timeout yeni kartı gizlemez (generation `Cell<u64>`).
- `Atla`/`S`/`E` aynı `seek_abs(target)` yolunu kullanır; target zaten geçildiyse tekrar seek yok.
- `Shift+M`: URL varsa `xdg-open` (yoksa `gio open`) arka planda; geçersiz URL'yi açma. `music::song_open_command` yalnız `https` + izinli host döndürür.

- [ ] **Step 4: Testleri doğrula**

Run: `cargo test --release --bin animecix player::notices music::tests -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/player src/music.rs
git commit -m "feat(player): modern intro/outro ve şarkı kartları"
```

---

### Task 7: Sağ tık menüsü, ekran görüntüsü, bölüm indirme ve video bilgisi

**Files:**
- Create: `src/app/player/menu.rs`
- Modify: `src/app/player/mod.rs`, `src/app/player/ui.rs`
- Test: `src/app/player/menu.rs`

**Interfaces:**
- Consumes: `PlayerHost.on_download`, `MpvEmbed` metotları.
- Produces: `pub fn screenshot_filename(title: &str, season: u64, ep: u64, timestamp: u64) -> String`
- Produces: `pub fn menu_actions(...) -> Vec<MenuAction>` veya doğrudan `rebuild_menu`; `MenuAction` oynat/duraklat, tam ekran, atla, komşu, oran, ekran görüntüsü, video bilgisi, indir.

- [ ] **Step 1: Dosya adı testini yaz**

```rust
#[test]
fn screenshot_filename_sanitizes_and_has_context() {
    let n = screenshot_filename("Witch/Hat: Atelier", 1, 3, 42);
    assert!(n.starts_with("WitchHat Atelier-S01E03-42"));
    assert!(!n.contains('/') && !n.contains(':'));
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix player::menu -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Menü ve eylemleri uygula**

- Video alanında sağ tık `Popover`; satırlar `ctx_row` ile ikon + etiket + kısayol.
- Oynat/duraklat, tam ekran, intro/outro atla, önceki/sonraki bölüm, ekranı doldur/orijinal oran, ekran görüntüsü, video bilgisi, bölümü indir.
- Screenshot: `pictures/AnimeciX/{sanitized}.png`; `player.screenshot_to_file`; başarılıysa pano (`gdk::Texture::from_file` + clipboard), kullanıcıya kart/toast. Dosya adı güvenli.
- Video bilgisi: codec, çözünürlük, FPS, süre, boyut (`MpvEmbed::video_info`).
- İndir: `host.on_download(ep.clone())`; App bunu mevcut `download_episode(title, ep)` akışına bağlar. Kaynak yoksa "önce kaynağın açılmasını bekle" mesajı.
- Ekranı doldur: `panscan 1.0` / orijinal `0.0`; durum metni kısa kart.
- Bağlam menüsü açıkken krom oto-gizlenmez; kapanınca sayaç sıfırlanır.

- [ ] **Step 4: Testleri doğrula**

Run: `cargo test --release --bin animecix player::menu -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/player
git commit -m "feat(player): bağlam menüsü, ekran görüntüsü ve indirme"
```

---

### Task 8: CI smoke, Flatpak/AppImage/deb/rpm paketleme

**Files:**
- Create: `src/player_smoke.rs`
- Modify: `src/main.rs` (`--player-smoke <path>` erken dalı)
- Modify: `src/embed_mpv/mod.rs` (gerekirse smoke için public API)
- Modify: `.github/workflows/check.yml`, `.github/workflows/release.yml`, `.github/workflows/appimage.yml`, `.github/workflows/package.yml`
- Modify: `packaging/flatpak/io.github.veilzon.AnimeciX.yml`
- Modify: `packaging/flathub-submission/io.github.veilzon.AnimeciX.yml`
- Modify: `build_appimage.sh`
- Modify: `packaging/rpm/animecix.spec` (gerekirse `Requires: mpv`)
- Test: `src/player_smoke.rs` (saf argüman testi) + CI shell

**Interfaces:**
- Consumes: `EmbeddedPlayer::build`, `PlayerHost`, `EmbedRequest`.
- Produces: `pub fn run(path: &str) -> i32` (0 başarı, 1 hata).
- Produces: `--check-libmpv` ve `--player-smoke <dosya>` tanı komutları.

- [ ] **Step 1: Smoke testini yaz**

`src/player_smoke.rs`:
```rust
#[test]
fn smoke_rejects_missing_path() {
    assert_eq!(crate::player_smoke::run("/tmp/animecix-smoke-yok.mp4"), 1);
}
```
Testi ağır GTK açmadan çalıştırmak için `run` önce `Path::is_file()` kontrolü yapar; eksikse `1`.

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix player_smoke -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Smoke runner'ı uygula**

- `--player-smoke <path>` main'de GTK öncesi ayrıştırılır; `gtk::Application`/`adw::ApplicationWindow` açar, test video dosyasını `EmbedRequest` ile yükler.
- `PlayerHost`: gerçek `Client`, boş `OperationStack`, `DiscordRpc::start(enabled:false)`, no-op callback'ler.
- Smoke, `EmbeddedPlayer::position()` 0.3 sn'yi geçince `PLAYER_SMOKE_OK`, süreç 0; 12 sn içinde geçmezse `PLAYER_SMOKE_FAIL`, süreç 1. Pencere kapanınca `shutdown`.
- `--check-libmpv` mevcut kalır; smoke libmpv yoksa `1`.

- [ ] **Step 4: Yerelde smoke çalıştır**

```bash
ffmpeg -y -f lavfi -i testsrc=size=320x180:rate=24 -t 2 -pix_fmt yuv420p /tmp/opencode/animecix-smoke.mp4
xvfb-run -a env LIBGL_ALWAYS_SOFTWARE=1 \
  ANIMECIX_LIBMPV_PATH=/tmp/opencode/animecix-mpv-dev/usr/lib64/libmpv.so.2 \
  cargo run --release -- --player-smoke /tmp/opencode/animecix-smoke.mp4
```
Expected: `PLAYER_SMOKE_OK` `exit 0`.

- [ ] **Step 5: Paketleme ve CI'yı güncelle**

- Flatpak: `-Dlibmpv=true`; LuaJIT/OSC, GnuTLS, `b_ndebug`, union patch korunur. CI'da `flatpak run ... --check-libmpv`.
- AppImage: build script libmpv'yi `ldconfig -p`/`find` ile bulur, `$APPDIR/usr/lib` altına gerçek `.so.2` + symlink kopyalar; `patchelf --set-rpath '$ORIGIN'` uygular; libmpv'nin `ldd` bağımlılıklarını glibc/ld/libstdc++ hariç kopyalar. AppRun'a global `LD_LIBRARY_PATH` eklenmez; loader `$APPDIR` adayını kullanır. CI'ya `libmpv-dev` ve `patchelf` ekle.
- AppImage testi: `./AnimeciX-x86_64.AppImage --check-libmpv` + `--player-smoke /tmp/test.mp4` (xvfb). Boyutu `stat -c%s` ile ölç ve eski `9.0 MB` tabanına göre raporla.
- deb/rpm: mevcut `Requires: mpv`/`Depends: mpv` korunur; kurulum testinde `--check-libmpv` çalıştır.
- Arch: `depends=('mpv')` zaten libmpv sağlar; Arch CI ayrı repoda olduğu için bu repoda yalnız README notu; yerel container yok.
- `release.yml`/`appimage.yml`/`check.yml` sistem bağımlılıklarına `libmpv-dev` ekle (derleme için gerekmez, smoke için runtime).

- [ ] **Step 6: Doğrula ve commit**

Run: `cargo test --release --bin animecix player_smoke -- --test-threads=1`
Expected: PASS.
Run: `bash -n build_appimage.sh`
Expected: PASS.
```bash
git add src/player_smoke.rs src/main.rs src/embed_mpv .github build_appimage.sh packaging
git commit -m "build(player): libmpv paketleme, AppImage taşıma ve smoke CI"
```

---

### Task 9: Kısayollar/README, tam doğrulama ve final inceleme

**Files:**
- Modify: `src/ui/shortcuts.rs` (gömülü oynatıcı tuşları)
- Modify: `README.md` (varsayılan gömülü, harici mpv ayarı, libmpv gereksinimi)
- Modify: `CHANGELOG.md` (yalnız geliştirme notu; sürüm değişmez)
- Test: mevcut ve yeni tüm testler

- [ ] **Step 1: Kısayol testini güncelle**

`xml_includes_settings_rows` testine ekle:
```rust
assert!(with.contains("Gömülü oynatıcı") || with.contains("Oynatıcı"));
assert!(with.contains("İntroyu atla") || with.contains("Sonraki bölüm"));
```
`ui_xml` "Oynatıcı" grubunu gömülü + harici ortak tuşlarla güncelle.

- [ ] **Step 2: README'yi güncelle**

- Özellikler: "Uygulama içi oynatıcı (libmpv/GtkGLArea), varsayılan; harici mpv Ayarlar'dan seçilebilir."
- Gereksinimler: mpv/libmpv; AppImage kendi libmpv runtime'ını taşır; Flatpak kendi mpv'sini kullanır.
- Harici mpv fallback ve eksik libmpv davranışı.
- Eski `mpv` süpervizörü ve `n`/`p`/`s`/`e`/`Shift+M` korunur.

- [ ] **Step 3: Tam test/format kapısını çalıştır**

```bash
cargo fmt --all
cargo fmt --all -- --check
xvfb-run -a env LIBGL_ALWAYS_SOFTWARE=1 cargo test --release --bin animecix -- --test-threads=1
cargo test --release --test refcell_guard --test next_episode
python3 tests/skip_harness.py
python3 tests/flatpak_updater.py
RUSTFLAGS="-D dead-code" cargo test --release --bin animecix --no-run
cargo clippy --release --all-targets --message-format=short
cargo machete
cargo deny check
```
Expected: hepsi PASS / temiz; yeni libmpv testleri yerelde `ANIMECIX_LIBMPV_PATH` ile geçer.

- [ ] **Step 4: Smoke ve ikon/shader kontrolleri**

```bash
ANIMECIX_LIBMPV_PATH=/tmp/opencode/animecix-mpv-dev/usr/lib64/libmpv.so.2 \
  ./target/release/animecix --check-libmpv
xvfb-run -a env LIBGL_ALWAYS_SOFTWARE=1 ANIMECIX_LIBMPV_PATH=/tmp/opencode/animecix-mpv-dev/usr/lib64/libmpv.so.2 \
  ./target/release/animecix --player-smoke /tmp/opencode/animecix-smoke.mp4
./target/release/animecix --check-icons
./target/release/animecix --check-shaders
```
Expected: tümü `exit 0`.

- [ ] **Step 5: Görsel kontrol**

- 5 temada player'ı aç: `koyu`, `bordo`, `orman`, `lacivert`, `mor`.
- 1280×800 ve 960×640: üst bar, merkez küme, alt bar, bölüm çekmecesi, skip kartı, uzun anima adı.
- `xvfb` ekran görüntüsü al veya gerçek hostta `--player-smoke` penceresini gözle kontrol et. Taşma/kırpılma varsa düzelt.
- Theme testi `player_css_has_contrast_and_accent` geçmeli; kart metni video üstünde okunmalı.

- [ ] **Step 6: Final review/commit**

`specs/` hariç tüm değişiklikleri incele; final review subagent'ı ile diff'i denetle. Sonra:
```bash
git add src README.md CHANGELOG.md packaging .github docs/superpowers
git commit -m "feat: varsayılan uygulama içi mpv oynatıcısı"
```
(Kullanıcı onayı olmadan push/release yok.)

---

## Self-Review

**Spec coverage:**
- Gömülü varsayılan + harici seçenek: Task 3.
- Dinamik libmpv/ABI/fallback: Task 1, 2, 8.
- Fansub dokunulmaz: Task 3 (`play_candidates` öncesi akış korunur).
- SkipPlan/şarkı kartları: Task 6; mevcut ayarlar.
- DC RPC: Task 4.
- Progress/izlendi/tercih host: Task 4.
- Bölüm çekmecesi/auto-next: Task 5.
- İndirme/ekran görüntüsü/video bilgisi: Task 7.
- Flatpak/AppImage/deb/rpm/Arch: Task 8.
- Tema/ölçek/kısayollar: Task 3, 6, 9.
- Focus Mod yok: Global constraint; Task 6 mevcut manuel skip.

**Step scan:** Her görev test → kırmızı → minimum uygulama → yeşil → commit döngüsü taşır. Geçici `allow(dead_code)` yalnız Task 2'de, Task 3'te kaldırılır.

**Type consistency:** `EmbeddedPlayer`, `PlayerHost`, `EmbedRequest`, `SourceQueue`, `SkipNotice`, `PlayTerminal` adları tüm görevlerde aynı.

**Review Focus:** Eksik libmpv/ABI (Task 1/3), teardown (Task 3 smoke), konum koruma (Task 4), RefCell (Task 3-7 + mevcut test), taşma/tema (Task 9), geç gelen bölüm listesi (Task 5), skip tekrarı (Task 6), auto-next (Task 5), AppImage boyutu (Task 8).
