# AnimeciX Refactor + Feature Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `app.rs`/`main.rs` dosyalarını davranış değiştirmeden modüllere ayırmak, derleyici uyarılarını sıfırlamak, yorum disiplinini uygulamak ve bekleyen yerel oynatma, indirme, RAM, izlendi-progress ve istatistik özelliklerini eklemek.

**Architecture:** `App` tek GTK koordinatörü olarak kalır. Sayfa kurucuları `src/app/pages/` altına, oynatma state machine'i `src/app/playback.rs` altına taşınır. Saf iş kuralları (`local_playback`, `stats`, `memory`) ayrı modüllerde tutulur ve GTK'dan bağımsız test edilir. Her refactor adımı saf taşımadır; özellik değişiklikleri ayrı commit'lerde yapılır.

**Tech Stack:** Rust 2021, GTK4 0.8 / libadwaita 0.6, mpv IPC, Cairo (`gtk::cairo`), `libc` (zaten bağımlılık), serde/serde_json. Yeni crate yok.

**Spec:** Bu belgenin “Onaylanmış Kararlar” ve “Global Kısıtlar” bölümleri; 2026-10-30 tarihli konuşmada onaylandı.

## Global Kısıtlar

- Sürüm `1.2.7` kalır; `Cargo.toml` sürümü değişmez.
- Push, tag, GitHub release veya paket yayını yapılmaz.
- `~/state.json`, `~/.local/share/animecix`, `~/.cache/animecix` gerçek kullanıcı verilerine dokunulmaz.
- Kirli kullanıcı dosyaları korunur: `packaging/post-install-cleanup.sh`, `src/ui/views.rs`, `src/ui/welcome.rs` ve `src/main.rs` içindeki kullanıcı hunk'ları.
- `src/main.rs` değişiklikleri `git add -p` ile yalnızca bu plana ait hunk'larla sahnelenir.
- Refactor commit'lerinde davranış değişmez; yalnızca kod taşınır ve görünürlük/import düzeltilir.
- Özellik commit'lerinde refactor ile özellik karıştırılmaz.
- Yeni crate eklenmez; `libc` mevcut bağımlılıktır.
- Kaynak koddaki yorumlar otomatik silinmez. Yalnızca yorum disiplini uygulanır.
- Hedef: `cargo check --release --bin animecix` ve `--tests` için 0 warning.
- Her faz sonunda `cargo fmt --all -- --check` ve ilgili testler çalıştırılır.
- Testlerde gerçek kullanıcı state'i kullanılmaz; mevcut `use_isolated_state()` deseni izlenir.

## Kirli Worktree Stratejisi

Bu depoda `src/main.rs`, `src/ui/views.rs` ve `src/ui/welcome.rs` üzerinde kullanıcıya ait commitlenmemiş değişiklikler var. Refactor sırasında bunlar kaybolmayacak.

- Başlamadan önce `git diff -- src/main.rs src/ui/views.rs src/ui/welcome.rs > /tmp/animecix-user-dirty.patch` ile yedek alınacak.
- `src/main.rs` CSS bloğunda kullanıcıya ait `.history-time` hunk'ı var. CSS `assets/style.css`e taşınırken bu satırlar yeni dosyada korunacak, ancak **bizim commit'imize dahil edilmeyecek**:
  - `git add -N assets/style.css`
  - `git add -p assets/style.css` ile yalnızca bizim taşıdığımız satırlar sahnelenecek; kullanıcı hunk'ı unstaged kalacak.
- `src/main.rs` için her commit'te `git add src/main.rs` kullanılmayacak; yalnızca `git add -p src/main.rs`.
- `src/ui/views.rs` ve `src/ui/welcome.rs` yalnızca zorunlu warning import temizliği için `git add -p` ile sahnelenecek. **Yorum disiplini bu iki dosyada uygulanmayacak.**
- `src/main.rs` içindeki CSS yorumlarına dokunulmayacak; CSS Task 3'te birebir taşınacak.
- `src/bin/probe.rs` `.gitignore` içinde; uyarı temizliği yerel kalacak, commit edilmeyecek.

## Onaylanmış Kararlar

1. **Sıra:** Önce warning + yorum disiplini, sonra `main.rs`, sonra `app.rs` bölüm sayfası, sonra playback, en son özellikler.
2. **Yorumlar:** Otomatik stripper yok. Doc comment'ler ve “neden” yorumları kalır; kodu tekrar eden yorumlar temizlenir.
3. **Yerel skip:** Oynatma anında `fetch_plan_for_embeds` ile çekilir. Plan yoksa/çevrimdışıysa yerel oynatma skip'siz devam eder. İndirme anında plan kalıcılığı bu planın kapsamı dışındadır.
4. **İzleme süresi:** Kayıtlı progress + eksiklerde tahmini süre; arayüzde `≈` ve “tahmini” etiketi.
5. **İstatistik grafikleri:** En çok izlenen 6 anime, sezon donut'u, son 8 hafta etkinliği.
6. **App state:** `App` tek koordinatör kalır; state ayrı struct'lara bölünmez.
7. **Sürüm/dağıtım:** v1.2.7; push/tag/release yok. AppImage yalnızca yerel doğrulama için üretilir.

## Review Focus

Bu maddeler, planın testleriyle özellikle kilitlenecek riskli davranışlardır:

- **Startup/AppImage:** `main.rs` bölünürken `APPDIR`, ikon tema yolu, `.desktop` kurulumu, SELinux `LD_LIBRARY_PATH` davranışı bozulmamalı. Test: `--check-icons`, `--check-shaders`, izole Xvfb smoke ve AppImage paket doğrulaması.
- **Playback:** `play_candidates` taşınırken yeni mpv başlatma, eski mpv'yi sonlandırma, `n`/`p` geçişi, failover ve skip planı aynı kalmalı. Test: mevcut supervisor testleri + manuel iki bölüm geçişi.
- **Eski indirmeler:** `DownloadRecord.title_id == 0` olan kayıtlar isim eşleşmesiyle bulunmalı; dosya silinmişse yerel oynatma yerine akışa düşülmeli. Test: `local_playback` birim testleri.
- **Eski state:** `Watched` yeni alanları olmayan `state.json` sorunsuz okunmalı; kullanıcı verisi kaybolmamalı. Test: eski JSON deserialize testi.
- **İstatistikler:** Hiç veri yokken, kısmi progress varken ve eski kayıtlarda isim/tarih yokken sayfa çökmemeli; süre tahmini `≈` ile dürüst gösterilmeli. Test: `stats` birim testleri + boş state manuel kontrolü.
- **GTK thread disiplini:** Dosya/ağ işleri worker'da, GTK widget güncellemeleri ana thread'de olmalı. Test: `Msg::PlayLocal` ve memory trim akışının `spawn`/`idle_add_local` üzerinden gitmesi.

---

## Dosya Yapısı

### Yeni dosyalar

- `assets/style.css` — `main.rs` içindeki mevcut base CSS'in birebir taşındığı yer.
- `src/startup.rs` — `connect_activate` gövdesi, ikon teması, CSS provider, güncelleme/changelog akışı.
- `src/desktop.rs` — masaüstü kısayolu, AppImage sabit kopya, bağımlılık kontrolleri.
- `src/diagnostics.rs` — `--check-icons`, `--check-shaders`.
- `src/icons.rs` — `prepend_icon_paths`.
- `src/app/playback.rs` — oynatma state machine'i.
- `src/app/pages/mod.rs` — sayfa modülleri.
- `src/app/pages/episodes.rs` — bölüm sayfası ve filtreleri.
- `src/local_playback.rs` — indirilen bölüm arama/öncelik saf mantığı.
- `src/memory.rs` — playback sonrası RAM trim.
- `src/stats.rs` — istatistik agregasyonu.
- `src/ui/stats_view.rs` — istatistik sayfası GTK/Cairo görünümü.
- `assets/hicolor/scalable/actions/office-chart-bar-symbolic.svg` — istatistik sidebar ikonu.

### Değişecek dosyalar

- `src/main.rs` — sadece modül bildirimleri, argüman ayrıştırma, uygulama başlatma.
- `src/app.rs` — `Page`, `Msg`, `App`, `clone_ref`, `handle_msg`, router ve fetch metotları.
- `src/ui/mod.rs` — `stats_view` modülü.
- `src/api.rs` — `Watched` alanları, `save_watched`, `apply_watched_list`, transient cache trim.
- `src/covers.rs` — `trim_budget`.
- `src/auth.rs`, `src/aria.rs`, `src/ui/components.rs`, `src/ui/views.rs`, `src/ui/welcome.rs`, `src/ui/play_quality_dialog.rs`, `src/ui/operation_stack.rs`, `src/bin/probe.rs`, `tests/next_episode.rs` — warning temizliği.
- `src/theme.rs` — `accent_rgb`.
- `src/app/pages/episodes.rs` — indirildi ikonu, sağ tık indirme, watched bar.
- `src/app/playback.rs` — local-first, `Msg::PlayLocal`, `PlayEvent::Closed`.

---

## Faz 1 — Warning Temizliği ve Yorum Disiplini

### Task 1: Derleyici uyarılarını sıfırla

**Files:**
- Modify: `src/app.rs:2`, `src/aria.rs:12`, `src/auth.rs:14-18`, `src/ui/components.rs:3`, `src/ui/views.rs:4`, `src/ui/welcome.rs:4`, `src/ui/play_quality_dialog.rs:5`
- Modify: `src/api.rs:5152-5155`
- Modify: `tests/next_episode.rs:23`
- Modify: `src/bin/probe.rs:1-3,78` (yerel/ignore; commit dışı)
- Modify: `src/api.rs:3784-3792`, `src/ui/operation_stack.rs:168`
- Test: `cargo check --release --bin animecix --message-format=short`
- Test: `cargo check --release --bin animecix --tests --message-format=short`

**Interfaces:**
- Consumes: mevcut modül yapısı.
- Produces: uyarısız derleme; sonraki refactor commit'leri için temiz baseline.

- [ ] **Step 1: Kullanılmayan import'ları kaldır**

`src/app.rs` içinden `use gtk::prelude::*;`, `src/aria.rs` içinden `DownloadEvent, DownloadHandle`, `src/auth.rs` içinden `std::path::PathBuf` ve `BASE`, UI dosyalarından kullanılmayan `gtk::prelude::*` satırlarını kaldır.

- [ ] **Step 2: Gereksiz test özniteliklerini ve `mut`u düzelt**

`src/api.rs` içindeki `#[test]` tekrarını tek bir `#[test]`e indir. `tests/next_episode.rs` içindeki `let mut playing = true;` satırını `let playing = true;` yap.

- [ ] **Step 3: Probe gürültüsünü sustur**

`src/bin/probe.rs` başına `#![allow(dead_code)]` ekle; `Err(e)` içindeki kullanılmayan değişkeni `_e` yap.

- [ ] **Step 4: Ölü kod kararını uygula**

- `Client::is_tools_tip_seen` ve `set_tools_tip_seen` metotlarını ve `tools_tip_seen_survives_migrate` testini sil. `State.tools_tip_seen` alanı geriye dönük uyumluluk için kalır.
- `OperationStack::remove_prefix` ve testini sil.
- `src/auth.rs` başına şu yorumla modül seviyesinde allow ekle: `#![allow(dead_code)] // gecici-yerel: uzak mod protokol yardımcıları testlerle korunuyor.`
- `LastEpisode`/`ref_title`, `Client::pick_strict`, `Client::is_watched`, `download::free_space` için kısa gerekçeli `#[allow(dead_code)]` ekle.

- [ ] **Step 5: Uyarıları doğrula**

Run: `cargo check --release --bin animecix --message-format=short`
Expected: `warning:` satırı yok.

Run: `cargo check --release --bin animecix --tests --message-format=short`
Expected: `warning:` satırı yok.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/aria.rs src/auth.rs src/api.rs src/ui/components.rs src/ui/play_quality_dialog.rs src/ui/operation_stack.rs tests/next_episode.rs
git add -p src/ui/views.rs src/ui/welcome.rs
# probe.rs ignore edildiği için commit edilmez.
git commit -m "Temizlik: derleyici uyarılarını sıfırla"
```

### Task 2: Yorum disiplinini uygula

**Files:**
- Modify: temiz `src/**/*.rs` dosyaları (yalnızca yorum satırları)
- Hariç: `src/ui/views.rs`, `src/ui/welcome.rs`, `src/main.rs` CSS bloğu, `src/bin/probe.rs`
- Test: `cargo check --release --bin animecix`
- Test: `cargo fmt --all -- --check`

**Interfaces:**
- Consumes: Task 1'in temiz derlemesi.
- Produces: davranışı aynı, daha az gürültülü yorum seti; doc comment'ler korunmuş.

- [ ] **Step 1: Aday yorumları listele**

Run: `rg -n '^\s*//' src --glob '*.rs' > /tmp/animecix-comments-before.txt`
Run: `rg -n '^\s*///|^\s*//!' src --glob '*.rs' | wc -l`

Beklenen: ~1.096 toplam yorum satırı; doc comment'ler ayrı sayılır.

- [ ] **Step 2: Yalnızca tekrar eden “ne yapıyor” yorumlarını sil**

Şunları koru:
- `///`, `//!` doc comment'leri.
- Güvenlik, yarış durumu, GTK/mpv tuhaflığı, SELinux, önbellek sınırı, state machine edge-case açıklamaları.
- “Neden böyle” anlatan kısa notlar.

Şunları sil:
- Bir sonraki satırdaki kodu Türkçeye çeviren yorumlar.
- Güncelliğini yitirmiş TODO/FIXME'ler.
- Yorum satırına alınmış eski kod (varsa).

- [ ] **Step 3: Yorum sayısını karşılaştır**

Run: `rg -n '^\s*//' src --glob '*.rs' | wc -l`

Beklenen: 1.096'dan anlamlı biçimde düşük; doc comment sayısı korunmuş. Katı bir sayı hedefi yok.

- [ ] **Step 4: Derleme ve format doğrula**

Run: `cargo fmt --all -- --check`
Run: `cargo check --release --bin animecix`
Expected: ikisi de başarılı; yeni warning yok.

- [ ] **Step 5: Commit**

```bash
# Yalnızca bu görevde değiştirilen temiz dosyaları ekle.
# Kirli dosyalar (main.rs/views.rs/welcome.rs) bu commit'e girmez.
git add <değiştirilen-temiz-dosyalar>
git commit -m "Temizlik: yorum disiplini"
```

---

## Faz 2 — `main.rs` Refactor

### Task 3: Base CSS'i asset dosyasına taşı

**Files:**
- Create: `assets/style.css`
- Modify: `src/main.rs:139-703`
- Test: `cargo check --release --bin animecix`

**Interfaces:**
- Consumes: mevcut `css.load_from_data(r#"..."#)` bloğu.
- Produces: `include_str!("../assets/style.css")` ile derlenen aynı CSS.

- [ ] **Step 1: CSS içeriğini birebir taşı**

`src/main.rs` içindeki `r#"` ile `"#` arasındaki tüm CSS'i `assets/style.css` dosyasına kopyala. CSS içeriğini değiştirme.

- [ ] **Step 2: `include_str!` kullan**

`css.load_from_data(r#"..."#);` çağrısını şu hale getir:

```rust
css.load_from_data(include_str!("../assets/style.css"));
```

- [ ] **Step 3: Doğrula**

Run: `cargo check --release --bin animecix`
Expected: PASS.

- [ ] **Step 4: Commit (kullanıcı hunk'ını koru)**

```bash
git add -N assets/style.css
# Yalnızca bizim taşıdığımız CSS satırlarını sahnele;
# kullanıcıya ait .history-time hunk'ı unstaged kalsın.
git add -p assets/style.css
git add -p src/main.rs
git commit -m "Refactor: base CSS'i asset dosyasına taşı"
```

### Task 4: Masaüstü/AppImage kodunu `desktop.rs`e taşı

**Files:**
- Create: `src/desktop.rs`
- Modify: `src/main.rs:810-1170`
- Test: `cargo test --release --bin animecix desktop_exec`
- Test: `cargo test --release --bin animecix appimage_integration`

**Interfaces:**
- Consumes: `src/main.rs` içindeki desktop/AppImage fonksiyonları ve testleri.
- Produces: `desktop::install_desktop_entry()`, `desktop::uninstall_application()`, `desktop::stable_appimage_path(&str)`, `desktop::system_installation_present()`, `desktop::desktop_exec_target(&str)`, `desktop::cleanup_stale_appimage_integration(&str, &str)`.

- [ ] **Step 1: Fonksiyonları taşı**

`DepStatus`, `check_all_dependencies`, `check_desktop_entry_installed`, `check_and_auto_update_installation`, `STABLE_APPIMAGE_NAME`, `stable_appimage_path`, `SYSTEM_BIN_PATH`, `system_installation_present`, `appimage_integration_blocked`, `appimage_integration_disabled`, `desktop_exec_target`, `desktop_exec_target_for`, `parse_desktop_exec`, `cleanup_stale_appimage_integration`, `install_desktop_entry`, `uninstall_application`, `check_command`, `detect_package_manager_cmd`, `detect_distro_info` ve ilgili `mod tests` içeriğini taşı.

- [ ] **Step 2: Görünürlükleri ayarla**

`src/main.rs` dışından çağrılan fonksiyonları `pub(crate)` yap. Testler `desktop::tests` altında kalsın.

- [ ] **Step 3: `main.rs` çağrılarını güncelle**

`use crate::desktop;` ekle; `check_desktop_entry_installed()` gibi çağrıları `desktop::...` yap.

- [ ] **Step 4: Testleri çalıştır**

Run: `cargo test --release --bin animecix desktop_exec -- --test-threads=1`
Run: `cargo test --release --bin animecix appimage_integration -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/desktop.rs
git add -p src/main.rs
git commit -m "Refactor: masaüstü ve AppImage kodunu ayır"
```

### Task 5: Tanılama ve ikon yardımcılarını ayır

**Files:**
- Create: `src/diagnostics.rs`
- Create: `src/icons.rs`
- Modify: `src/main.rs:1327-1512`
- Test: `./target/release/animecix --check-icons`
- Test: `./target/release/animecix --check-shaders`

**Interfaces:**
- Consumes: `prepend_icon_paths`, `cmd_check_icons`, `cmd_check_shaders`.
- Produces: `icons::prepend_icon_paths(&gtk::IconTheme, &[PathBuf])`, `diagnostics::cmd_check_icons()`, `diagnostics::cmd_check_shaders()`.

- [ ] **Step 1: `prepend_icon_paths`i taşı**

`src/icons.rs` oluştur; fonksiyonu `pub(crate)` yap. `main.rs` içindeki iki çağrıyı `icons::prepend_icon_paths` olarak güncelle.

- [ ] **Step 2: Tanılama komutlarını taşı**

`cmd_check_icons` ve `cmd_check_shaders` fonksiyonlarını `src/diagnostics.rs`e taşı; `pub(crate)` yap. `main.rs` argüman dallarını `diagnostics::...` olarak güncelle.

- [ ] **Step 3: Doğrula**

Run: `cargo build --release --bin animecix`
Run: `./target/release/animecix --check-icons`
Expected: ikon kontrolü başarılı; eksik ikon yok.

Run: `./target/release/animecix --check-shaders`
Expected: shader kontrolü başarılı.

- [ ] **Step 4: Commit**

```bash
git add src/diagnostics.rs src/icons.rs
git add -p src/main.rs
git commit -m "Refactor: tanılama ve ikon yardımcılarını ayır"
```

### Task 6: Startup akışını `startup.rs`e taşı

**Files:**
- Create: `src/startup.rs`
- Modify: `src/main.rs:50-808`
- Test: `cargo check --release --bin animecix`
- Test: izole Xvfb smoke

**Interfaces:**
- Consumes: `app.connect_activate(|app| { ... })` gövdesi.
- Produces: `pub(crate) fn activate(app: &adw::Application)`.

- [ ] **Step 1: Activate gövdesini taşı**

İkon teması kurulumu, CSS provider, AppImage entegrasyon kontrolü, `App::new`, kapak süpürme, güncelleme/changelog popup'ları ve `window.present()` kodunu `startup::activate` içine taşı.

- [ ] **Step 2: `main.rs`i sadeleştir**

`app.connect_activate(startup::activate);` kullan. Argüman ayrıştırma ve `app.run_with_args` `main` içinde kalsın.

- [ ] **Step 3: Doğrula**

Run: `cargo check --release --bin animecix`
Expected: PASS.

Run (izole):
```bash
tmp=$(mktemp -d /tmp/opencode/animecix-startup.XXXXXX)
mkdir -p "$tmp/run" "$tmp/cache" "$tmp/config" "$tmp/data"
chmod 700 "$tmp/run"
dbus-run-session -- env APPIMAGE_EXTRACT_AND_RUN=1 HOME="$tmp" XDG_RUNTIME_DIR="$tmp/run" XDG_CACHE_HOME="$tmp/cache" XDG_CONFIG_HOME="$tmp/config" XDG_DATA_HOME="$tmp/data" timeout 6s xvfb-run -a ./target/release/animecix
```
Expected: exit 124 (timeout), çökme yok.

- [ ] **Step 4: Commit**

```bash
git add src/startup.rs
git add -p src/main.rs
git commit -m "Refactor: startup akışını ayır"
```

---

## Faz 3 — `app.rs` Bölüm Sayfası Refactor

### Task 7: Bölüm sayfasını `app/pages/episodes.rs`e taşı

**Files:**
- Create: `src/app/pages/mod.rs`
- Create: `src/app/pages/episodes.rs`
- Modify: `src/app.rs:3009-4014`, `src/app.rs:1327-1615`
- Test: `cargo test --release --bin animecix season_parts_show_five_then_overflow`
- Test: `cargo test --release --bin animecix episode_search_ignores_season_filter`

**Interfaces:**
- Consumes: `App` alanları ve `build_episodes_view(&self, title: &Title, eps: &[Episode]) -> gtk::Overlay`.
- Produces: `impl App { pub(crate) fn build_episodes_view(...) -> gtk::Overlay }` aynı imzayla; `split_seasons`, `episode_filter_matches` ve testleri yeni modülde.

- [ ] **Step 1: Modül iskeletini kur**

`src/app/pages/mod.rs` içine `pub(crate) mod episodes;` ekle. `src/app.rs` içine `mod pages;` ekle.

- [ ] **Step 2: Bölüm sayfası metodunu taşı**

`build_episodes_view` gövdesini ve yalnızca bu sayfaya hizmet eden yardımcı closure'ları `episodes.rs` içine `impl App` bloğuna taşı. Metot görünürlüğünü `pub(crate)` yap.

- [ ] **Step 3: Saf filtre yardımcılarını taşı**

`split_seasons`, `episode_filter_matches`, `set_ep_row_visible` ve ilgili testleri `episodes.rs`e taşı. `MAX_VISIBLE_SEASONS` sabitini de yanına al.

- [ ] **Step 4: Router'ı güncelle**

`show_page` içindeki `Page::Episodes { title, eps } | Page::Movie { title, eps }` dalı aynı `self.build_episodes_view(title, eps)` çağrısını korusun.

- [ ] **Step 5: Testleri çalıştır**

Run: `cargo test --release --bin animecix season_parts_show_five_then_overflow -- --test-threads=1`
Run: `cargo test --release --bin animecix episode_search_ignores_season_filter -- --test-threads=1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/app/pages
git commit -m "Refactor: bölüm sayfasını ayrı modüle taşı"
```

---

## Faz 4 — `app.rs` Oynatma Motoru Refactor

### Task 8: Playback state machine'ini `app/playback.rs`e taşı

**Files:**
- Create: `src/app/playback.rs`
- Modify: `src/app.rs:93-121`, `src/app.rs:4281-5566`, `src/app.rs:4037-4280`
- Test: `cargo test --release --bin animecix decide_retry -- --test-threads=1`
- Test: `cargo test --release --bin animecix play_terminal -- --test-threads=1`
- Test: `cargo test --release --bin animecix socket_timeout -- --test-threads=1`

**Interfaces:**
- Consumes: `play`, `play_with_fansub`, `play_with_fansub_inner`, `play_resolved`, `play_local`, `download_title_id`, `play_candidates`, `request_neighbor`, `decide_retry`, `source_is_dead`, `socket_timeout_hit`.
- Produces: aynı metot imzaları `impl App` içinde; `PlaySources`, `PlayEvent`, `PlayTerminal`, `opening_finish_delay`, `play_terminal` yeni modülde.

- [ ] **Step 1: Modülü ve tipleri taşı**

`src/app/playback.rs` oluştur; `PlaySources`, `PlayEvent`, `PlayTerminal`, `opening_finish_delay`, `play_terminal`, `decide_retry`, `source_is_dead`, `socket_timeout_hit` ve ilgili testleri taşı. `src/app.rs` içine `pub(crate) mod playback;` ekle.

- [ ] **Step 2: Oynatma metotlarını taşı**

`play`, `play_with_fansub`, `play_with_fansub_inner`, `play_resolved`, `play_local`, `play_local_record` (varsa), `download_title_id`, `play_candidates`, `request_neighbor` metotlarını `impl App` bloğuna taşı. Gerekli yardımcıları `pub(crate)` yap.

- [ ] **Step 3: `Msg` referanslarını güncelle**

`Msg::Play` içindeki `PlaySources` tipini `crate::app::playback::PlaySources` olarak kullan. `Msg::PlayLocal` bu fazda henüz eklenmez; Faz 5'te eklenecek.

- [ ] **Step 4: Testleri çalıştır**

Run: `cargo test --release --bin animecix decide_retry -- --test-threads=1`
Run: `cargo test --release --bin animecix play_terminal -- --test-threads=1`
Run: `cargo test --release --bin animecix socket_timeout -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Tam test ve manuel smoke**

Run: `xvfb-run -a cargo test --release --bin animecix -- --test-threads=1 --skip download::tests::parallel_three_small_files_all_done --skip download::tests::tarpit_two_stall_one_fast`
Expected: mevcut test sayısı kadar PASS.

Manuel: bir bölüm aç, mpv kapanınca `n` ve `p` geçişlerini dene; çift mpv süreci kalmadığını `pgrep -a mpv` ile kontrol et.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/app/playback.rs
git commit -m "Refactor: oynatma motorunu ayrı modüle taşı"
```

---

## Faz 5 — Özellikler

### Task 9: `local_playback` saf arama modülü

**Files:**
- Create: `src/local_playback.rs`
- Modify: `src/main.rs` (modül kaydı)
- Test: `src/local_playback.rs` içindeki `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `crate::download::{DownloadRecord, DownloadStatus, sanitize_filename}`.
- Produces:

```rust
pub fn find_for_episode(
    records: &[DownloadRecord],
    title_id: u64,
    title_name: &str,
    season: u64,
    episode: u64,
) -> Option<DownloadRecord>;
```

- [ ] **Step 1: Başarısız testleri yaz**

Testler:
- `done_only_matches`: `Done` olmayan kayıt eşleşmez.
- `missing_file_is_skipped`: dosya yoksa sonraki aday denenir.
- `old_record_matches_by_sanitized_name`: `title_id=0` kayıt `sanitize_filename(title_name)` ile eşleşir.
- `first_done_record_wins`: aynı bölümde iki Done varsa kuyruktaki ilk kayıt döner.
- `wrong_season_or_episode_does_not_match`.

Test yardımcısı olarak dosya varlığını enjekte edilebilir yap:

```rust
fn find_matching<F>(
    records: &[DownloadRecord],
    title_id: u64,
    title_name: &str,
    season: u64,
    episode: u64,
    file_exists: F,
) -> Option<DownloadRecord>
where
    F: Fn(&std::path::Path) -> bool;
```

`find_for_episode` bu fonksiyonu `|p| p.is_file()` ile çağırır.

- [ ] **Step 2: Testlerin başarısız olduğunu doğrula**

Run: `cargo test --release --bin animecix local_playback -- --test-threads=1`
Expected: FAIL (fonksiyon yok).

- [ ] **Step 3: Minimal implementasyonu yaz**

Eşleşme sırası:
1. `rec.status == Done`
2. `rec.season == season && rec.episode == episode`
3. Başlık: `title_id != 0 && rec.title_id == title_id`; aksi halde `rec.title_id == 0 && (rec.title == title_name || rec.title == sanitize_filename(title_name))`
4. `file_exists(&rec.dest)`

İlk eşleşen kaydı klonlayarak döndür.

- [ ] **Step 4: Testleri geçir**

Run: `cargo test --release --bin animecix local_playback -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/local_playback.rs
git add -p src/main.rs
git commit -m "Özellik: indirilen bölüm arama modülü"
```

### Task 10: Yerel öncelik, indirildi ikonu ve yerel skip

**Files:**
- Modify: `src/app/playback.rs` (`play`, `play_local`, yeni `play_local_record`)
- Modify: `src/app/pages/episodes.rs` (indirildi ikonu)
- Modify: `src/app.rs` (`Msg::PlayLocal`)
- Test: `cargo test --release --bin animecix local_playback -- --test-threads=1`
- Manuel: indirilmiş bölümü aç; skip planı varsa intro atla.

**Interfaces:**
- Consumes: `local_playback::find_for_episode`, `crate::skip::fetch_plan_for_embeds`, `crate::download::DownloadRecord`.
- Produces:

```rust
enum Msg {
    PlayLocal {
        title: Title,
        ep: Episode,
        path: String,
        plan: Option<crate::skip::SkipPlan>,
    },
}
```

- [ ] **Step 1: `App::play` başına yerel kontrol ekle**

`play()` içinde `is_movie` kontrolünden önce:

```rust
if let Some(rec) = crate::local_playback::find_for_episode(
    &self.dl_manager.snapshot(),
    title.id,
    &title.name,
    ep.season,
    ep.episode,
) {
    self.play_local_record(title, ep, &rec);
    return;
}
```

- [ ] **Step 2: `play_local_record` ekle**

İmza:

```rust
fn play_local_record(
    &self,
    title: &Title,
    ep: &Episode,
    rec: &crate::download::DownloadRecord,
);
```

Davranış:
- `rec.dest.is_file()` değilse hata göster ve dön.
- `title.id == 0` ise `play_candidates(..., None)` ile hemen oynat.
- Aksi halde `busy(true)` + `spawn` ile worker'da `fetch_plan_for_embeds(client, title.id, ep.season, ep.episode, &[], &[], &manual_secret)` çağır.
- Worker sonucu `Msg::PlayLocal { title, ep, path, plan }` olarak döndür.

- [ ] **Step 3: `Msg::PlayLocal` işle**

`handle_msg` içine ekle:

```rust
Msg::PlayLocal { title, ep, path, plan } => {
    self.play_candidates(&title, &ep, std::slice::from_ref(&path), &[], &[], plan);
}
```

- [ ] **Step 4: `play_local`u yeni yola bağla**

İndirilenler sayfasındaki `play_local(&rec)` mevcut `Title`/`Episode` kurulumunu yapsın ve `play_local_record` çağırsın. `download_title_id` fallback'i korunur.

- [ ] **Step 5: Bölüm satırına indirildi ikonu ekle**

`build_episodes_view` içinde `local_records` snapshot'ını bir kez al. Her satır için:

```rust
let downloaded = crate::local_playback::find_for_episode(
    &local_records, title.id, &title.name, e.season, e.episode,
).is_some();
```

`downloaded` ise başlık etiketinin yanına `gtk::Image::from_icon_name("folder-download-symbolic")` ekle; `pixel_size(14)`, `add_css_class("dim-label")`, tooltip “İndirildi — yerel dosyadan oynatılır”.

- [ ] **Step 6: Doğrula**

Run: `cargo test --release --bin animecix local_playback -- --test-threads=1`
Run: `cargo check --release --bin animecix`
Manuel: indirilmiş bir bölümü aç; `pgrep -a mpv` tek süreç; intro varsa `s` ile atla.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/app/playback.rs src/app/pages/episodes.rs
git commit -m "Özellik: yerel indirilen bölüm önceliği ve skip"
```

### Task 11: Sağ tık indirme

**Files:**
- Modify: `src/app/pages/episodes.rs`
- Modify: `src/app.rs` (gerekirse `download_episode` yardımcısı)
- Test: `cargo check --release --bin animecix`
- Manuel: sağ tık → İndir → kalite seç → kuyruk.

**Interfaces:**
- Consumes: `ask_download_quality`, `start_download_prefetch`.
- Produces: `App::download_episode(&self, title: Title, ep: Episode)`.

- [ ] **Step 1: Tekil indirme yardımcısını çıkar**

Bölüm satırındaki indirme butonunun gövdesini `download_episode` metoduna taşı. Buton bu metodu çağırsın.

- [ ] **Step 2: Sağ tık menüsüne ekle**

`RowMenu::build` listesinin başına `("⬇ İndir".to_string(), on_download)` ekle. `on_download`, `this_ctx.download_episode(title_ctx.clone(), ep_ctx.clone())` çağırsın.

- [ ] **Step 3: Doğrula**

Run: `cargo check --release --bin animecix`
Manuel: sağ tık menüsünde İndir görünür; kalite dialog'u açılır; indirme kuyruğa girer.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/app/pages/episodes.rs
git commit -m "Özellik: bölüm sağ tık menüsüne indirme ekle"
```

### Task 12: İzlendi progress bar'ını anında full yap

**Files:**
- Modify: `src/app.rs:164-215` (`EpisodeBar` alanı)
- Modify: `src/app/pages/episodes.rs`
- Modify: `src/app/playback.rs:5134-5202`
- Test: `cargo test --release --bin animecix episode_bar -- --test-threads=1`

**Interfaces:**
- Consumes: `Rc<RefCell<HashMap<String, EpisodeBar>>>`.
- Produces:

```rust
pub(crate) struct EpisodeBar {
    pub bar: gtk::ProgressBar,
    pub label: gtk::Label,
    pub watched: Rc<RefCell<bool>>,
}

fn episode_bar_display(watched: bool, pos: f64, dur: f64) -> (f64, Option<String>);
```

- [ ] **Step 1: Saf görünüm yardımcısını test et**

Test:
- `watched_true_is_full`: `(1.0, Some("İzlendi"))`
- `unwatched_with_duration_uses_fraction`
- `unwatched_without_duration_hides`

Run: `cargo test --release --bin animecix episode_bar -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 2: `EpisodeBar` yapısını ekle ve haritayı değiştir**

`App::progress_bars` tipini `Rc<RefCell<HashMap<String, EpisodeBar>>>` yap. Tüm insert/update/remove noktalarını güncelle.

- [ ] **Step 3: İlk kurulumda full yap**

Bölüm satırı kurulurken `is_watched` hesaplandıktan sonra:

```rust
if *is_watched.borrow() {
    progress_bar.set_fraction(1.0);
    progress_bar.set_visible(true);
    time_lbl.set_text("İzlendi");
    time_lbl.set_visible(true);
}
```

- [ ] **Step 4: Toggle davranışını güncelle**

“İzlendi” seçilince bar 1.0, label “İzlendi”. “İzlenmedi” seçilince `progress` haritasındaki kayıtlı `(pos, dur)` ile eski görünüme dön; yoksa barı gizle.

- [ ] **Step 5: Oynatıcı tick'ini güncelle**

`progress_bars` güncellemesinde `watched` bayrağı true ise barı 1.0 ve “İzlendi” olarak bırak; false ise mevcut pos/dur gösterimini uygula.

Otomatik `%90` işaretinden sonra ilgili `EpisodeBar.watched` true yap ve barı full'le.

- [ ] **Step 6: Testleri geçir**

Run: `cargo test --release --bin animecix episode_bar -- --test-threads=1`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/app/pages/episodes.rs src/app/playback.rs
git commit -m "Özellik: izlendi bölümün progress bar'ını full göster"
```

### Task 13: MPV sonrası RAM trim

**Files:**
- Create: `src/memory.rs`
- Modify: `src/main.rs` (modül kaydı)
- Modify: `src/covers.rs` (`trim_budget`)
- Modify: `src/api.rs` (`trim_transient_caches`)
- Modify: `src/app/playback.rs` (`PlayEvent::Closed`)
- Modify: `src/app.rs` (pump)
- Test: `cargo test --release --bin animecix memory -- --test-threads=1`

**Interfaces:**
- Consumes: `CoverManager`, `Arc<Client>`, `libc`.
- Produces:

```rust
pub fn trim_after_playback(
    covers: &crate::covers::CoverManager,
    client: &std::sync::Arc<crate::api::Client>,
);

pub fn trim_allocator() -> bool;
```

- [ ] **Step 1: `CoverManager::trim_budget` ekle**

```rust
pub fn trim_budget(&self, max_bytes: usize);
```

LRU `order` kuyruğundan en eski girdileri, toplam bayt `max_bytes` altına inene kadar çıkar. Görüntülenen `Picture`'lar kendi texture referanslarını tuttuğu için ekrandaki kapaklar kaybolmaz.

- [ ] **Step 2: `Client::trim_transient_caches` ekle**

`cache` ve `skip_plans` mutex'lerini temizle. Kalıcı disk önbelleğine dokunma.

- [ ] **Step 3: `memory.rs` yaz**

```rust
pub fn trim_allocator() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        return libc::malloc_trim(0) != 0;
    }
    #[allow(unreachable_code)]
    false
}

pub fn trim_after_playback(covers: &CoverManager, client: &Arc<Client>) {
    let covers = covers.clone_ref();
    let client = client.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
        covers.trim_budget(8 * 1024 * 1024);
        client.trim_transient_caches();
        let _ = trim_allocator();
    });
}
```

- [ ] **Step 4: `PlayEvent::Closed` ekle ve gönder**

Supervisor'ın sonunda, yalnızca güncel `play_gen` için:

```rust
let _ = play_tx_c.send(PlayEvent::Closed(gen));
```

Pump'ta `Closed` olayını yakala ve `memory::trim_after_playback(&covers_h, &client_h)` çağır. `covers_h`/`client_h` pump kurulurken klonlanmalı.

- [ ] **Step 5: Testleri yaz ve çalıştır**

Test:
- `trim_allocator_can_run_twice` paniklemez.
- `trim_transient_caches_clears_memory_maps` (Client cache'e bir değer koy, trim sonrası erişilemediğini doğrula; mevcut test yardımcılarıyla).

Run: `cargo test --release --bin animecix memory -- --test-threads=1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/memory.rs src/covers.rs src/api.rs src/app.rs src/app/playback.rs
git add -p src/main.rs
git commit -m "Özellik: mpv kapanınca RAM trim"
```

### Task 14: `Watched` şemasını istatistik için genişlet

**Files:**
- Modify: `src/api.rs:650-654`, `src/api.rs:3360-3385`, `src/api.rs:3492-3509`
- Modify: `src/api.rs` test literalleri (`Watched { ... }`)
- Test: `cargo test --release --bin animecix watched -- --test-threads=1`

**Interfaces:**
- Consumes: mevcut `Watched`.
- Produces:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Watched {
    pub title_id: u64,
    pub episode: u64,
    pub season: u64,
    #[serde(default)]
    pub title_name: Option<String>,
    #[serde(default)]
    pub watched_at: u64,
}
```

- [ ] **Step 1: Eski JSON uyumluluk testini yaz**

```rust
let w: Watched = serde_json::from_str(r#"{"title_id":1,"episode":2,"season":3}"#).unwrap();
assert_eq!(w.title_name, None);
assert_eq!(w.watched_at, 0);
```

Run: `cargo test --release --bin animecix watched_legacy_json -- --test-threads=1`
Expected: FAIL (alanlar yok).

- [ ] **Step 2: Alanları ekle ve `save_watched`ı güncelle**

- `title_name`: `name` boş değilse `Some(name.to_string())`; değilse mevcut `w.title_name`.
- `watched_at`: `w.watched_at == 0` ise `now_secs()`, değilse mevcut değer.
- Var olan kayıt varsa alanları güncelle, yenisini ekleme.

- [ ] **Step 3: Toplu işareti güncelle**

`apply_watched_list` mevcut imzasını korusun; içine `apply_watched_list_named(tid, "", eps)` çağrısı ekle. `mark_title_watched`, `apply_watched_list_named(t.id, &t.name, &eps)` kullansın.

- [ ] **Step 4: Testleri geçir**

Run: `cargo test --release --bin animecix watched -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/api.rs
git commit -m "Veri: Watched kaydına isim ve zaman ekle"
```

### Task 15: İstatistik agregasyonu

**Files:**
- Create: `src/stats.rs`
- Modify: `src/main.rs` (modül kaydı)
- Test: `src/stats.rs` içindeki `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `crate::api::State`, `crate::download::DownloadRecord`, `HashMap<u64, String>`.
- Produces:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatsSummary {
    pub anime_count: usize,
    pub episode_count: usize,
    pub watch_seconds: f64,
    pub watch_estimated: bool,
    pub downloaded_count: usize,
    pub downloaded_bytes: u64,
    pub top_anime: Vec<AnimeSlice>,
    pub seasons: Vec<SeasonSlice>,
    pub activity: Vec<ActivityBucket>,
}

pub struct AnimeSlice { pub title_id: u64, pub name: String, pub episodes: usize }
pub struct SeasonSlice { pub label: String, pub episodes: usize }
pub struct ActivityBucket { pub label: String, pub count: usize }

pub fn build_summary(
    state: &crate::api::State,
    downloads: &[crate::download::DownloadRecord],
    names: &std::collections::HashMap<u64, String>,
    now: u64,
) -> StatsSummary;

pub fn format_duration_human(seconds: f64) -> String;
```

- [ ] **Step 1: Testleri yaz**

Testler:
- `unique_anime_and_episode_count`
- `progress_only_90_percent_counts_as_watched`
- `watch_time_uses_duration_for_watched_and_position_for_partial`
- `missing_duration_sets_estimated`
- `top_anime_sorted_by_episode_count`
- `season_buckets_cover_s1_to_s6_plus`
- `activity_last_eight_weeks`
- `downloads_count_done_only`

Run: `cargo test --release --bin animecix stats -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 2: Agregasyonu yaz**

Kurallar:
- Bölüm anahtarı `(title_id, season, episode)`; `watched` + `pos/dur >= 0.9` progress birleşimi, tekrarsız.
- Anime sayısı: en az bir bölüm anahtarı olan benzersiz `title_id`.
- Süre:
  - progress varsa ve bölüm watched ise `dur` (yoksa `pos`);
  - progress yoksa medyan `dur`, yoksa `24 * 60`;
  - herhangi bir fallback kullanıldıysa `watch_estimated = true`.
- İndirilen: yalnızca `Done`; bayt `total.max(have)`.
- Top anime: bölüm sayısına göre azalan, isim artan.
- Sezon: S1..S5 ve “S6+”.
- Aktivite: `watched_at` + `history.ts`, aynı bölüm için en büyük ts; son 8 hafta.
- İsim: önce `Watched.title_name`, sonra verilen `names`, sonra `"Başlık #ID"`.

- [ ] **Step 3: Testleri geçir**

Run: `cargo test --release --bin animecix stats -- --test-threads=1`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/stats.rs
git add -p src/main.rs
git commit -m "Özellik: istatistik agregasyonu"
```

### Task 16: İstatistikler sayfası

**Files:**
- Create: `src/ui/stats_view.rs`
- Create: `assets/hicolor/scalable/actions/office-chart-bar-symbolic.svg`
- Modify: `src/ui/mod.rs`
- Modify: `src/app.rs` (`Page::Stats`, sidebar, `show_page`, `update_sidebar_selection`, `open_data_page`)
- Modify: `src/theme.rs` (`accent_rgb`)
- Test: `cargo check --release --bin animecix`
- Test: `cargo test --release --bin animecix accent_rgb -- --test-threads=1`
- Manuel: sidebar → İstatistikler; boş state ve dolu state.

**Interfaces:**
- Consumes: `crate::stats::StatsSummary`.
- Produces:

```rust
pub struct StatsView;
impl StatsView {
    pub fn build(stats: &crate::stats::StatsSummary, accent: (u8, u8, u8)) -> gtk::Box;
}
```

- [ ] **Step 1: `Page::Stats` ve sidebar**

`Page` enum'a `Stats` ekle. Sidebar listesine `(Page::Stats, "office-chart-bar-symbolic", "İstatistikler")` ekle (Geçmiş'ten sonra). `update_sidebar_selection` eşlemesine `Page::Stats => Some(Page::Stats)` ekle. `show_page` içinde:

```rust
Page::Stats => {
    self.title_label.set_text("İstatistikler");
    switch(&self.stack, "stats", gtk::StackTransitionType::Crossfade, self.build_stats_view());
}
```

- [ ] **Step 2: `build_stats_view` ekle**

`load_state`, `dl_manager.snapshot()` ve isim haritasını topla. İsim haritası: `watched.title_name`, `saved`, `history`, `marathon`, indirmeler, yüklü `cats`. `stats::build_summary` çağır. `theme::accent_rgb(&settings.theme)` ile accent al. `StatsView::build` sonucunu `ScrolledWindow` içine koy.

- [ ] **Step 3: `theme::accent_rgb` ekle**

`palette(theme_id).accent` hex değerini `(u8,u8,u8)`e çevir. Bilinmeyen tema koyu accent döner. Test: `accent_rgb("koyu") == (0x35,0x84,0xE4)`.

- [ ] **Step 4: İkonu ekle**

`assets/hicolor/scalable/actions/office-chart-bar-symbolic.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">
  <path fill="#2e3436" d="M2 13h2V7H2zm4 0h2V2H6zm4 0h2V9h-2zm4 0h2V4h-2z"/>
</svg>
```

Build script tüm `assets/hicolor/scalable/actions/*.svg` dosyalarını zaten kopyalıyor; ek kod gerekmez.

- [ ] **Step 5: `StatsView` görünümünü yaz**

Tasarım:
- `adw::Clamp` (max 1120) + geniş margin.
- Hero: “KÜTÜPHANE” eyebrow, büyük “İstatistikler”, alt açıklama.
- KPI `FlowBox`: Anime, Bölüm, Süre (`≈` gerekirse), İndirilen.
- Bento: en çok izlenen anime bar kartı + sezon donut kartı; altında son 8 hafta bar kartı.
- Çift katmanlı “double-bezel” kartlar: dış `stats-shell`, iç `stats-card`.
- CSS: hairline `alpha(currentColor, 0.08)`, yumuşak accent gölgesi, `border-radius: 24-28px`, `padding: 22-26px`.
- Grafikler `gtk::DrawingArea` + `gtk::cairo`; bar/donut çizimi `set_draw_func` ile.
- Donut merkezinde `gtk::Overlay` + büyük toplam etiketi.
- Bölümler `gtk::Revealer` ile 350ms fade/slide; hover geçişleri `cubic-bezier(0.32,0.72,0,1)`.
- Boş durum: “Henüz istatistik yok — ilk bölümü izlediğinde grafikler dolmaya başlar.”

- [ ] **Step 6: Doğrula**

Run: `cargo check --release --bin animecix`
Run: `cargo test --release --bin animecix accent_rgb -- --test-threads=1`
Manuel: boş state ve dolu state ile sayfayı aç; dar/geniş pencerede taşma kontrolü.

- [ ] **Step 7: Commit**

```bash
git add src/ui/stats_view.rs src/ui/mod.rs src/app.rs src/theme.rs assets/hicolor/scalable/actions/office-chart-bar-symbolic.svg
git commit -m "Özellik: istatistikler sayfası"
```

---

## Faz 6 — Opsiyonel: Kalan Sayfa Refactor'u

### Task 17 (opsiyonel): Home/library/tools sayfalarını ayır

**Files:**
- Create: `src/app/pages/home.rs`
- Create: `src/app/pages/library.rs`
- Create: `src/app/pages/tools.rs`
- Create: `src/app/search_popup.rs`
- Modify: `src/app.rs`

**Interfaces:**
- Consumes: mevcut `build_home_view`, `build_marathon_view`, `build_favs_view`, `build_history_view`, `build_news_view`, `build_calendar_view`, `build_kesfet_view`, `build_search_view`, `build_account_view`, `build_settings_view`, `open_search_popup_with`.
- Produces: aynı metot imzaları `impl App` içinde; `src/app.rs` ~1.500–2.000 satır.

- [ ] **Step 1: Home sayfasını taşı**

`create_title_card*`, `lazy_cover_picture`, `build_spotlight_*`, `build_continue_section`, `build_home_view` ve ilgili testleri `home.rs`e taşı.

- [ ] **Step 2: Library sayfalarını taşı**

Maraton, favoriler, geçmiş sayfalarını `library.rs`e taşı.

- [ ] **Step 3: Tools sayfalarını taşı**

Haber, takvim, keşfet, arama, hesap, ayarlar sarmalayıcılarını `tools.rs`e taşı.

- [ ] **Step 4: Arama popup'ını taşı**

`open_search_popup` / `open_search_popup_with` ve yardımcılarını `search_popup.rs`e taşı.

- [ ] **Step 5: Doğrula**

Run: `cargo check --release --bin animecix`
Run: `xvfb-run -a cargo test --release --bin animecix -- --test-threads=1 --skip download::tests::parallel_three_small_files_all_done --skip download::tests::tarpit_two_stall_one_fast`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/app/pages src/app/search_popup.rs
git commit -m "Refactor: kalan sayfaları modüllere ayır"
```

---

## Faz 7 — Final Doğrulama ve AppImage

### Task 18: Tam doğrulama

**Files:**
- No source change.
- Test: full suite + AppImage.

- [ ] **Step 1: Format ve warning kontrolü**

Run: `cargo fmt --all -- --check`
Run: `cargo check --release --bin animecix --message-format=short`
Run: `cargo check --release --bin animecix --tests --message-format=short`
Expected: 0 warning.

- [ ] **Step 2: Tam test paketi**

Run:
```bash
xvfb-run -a cargo test --release --bin animecix -- --test-threads=1 \
  --skip download::tests::parallel_three_small_files_all_done \
  --skip download::tests::tarpit_two_stall_one_fast
```
Expected: tüm testler PASS; bilinen 2 yavaş test hariç.

- [ ] **Step 3: Yavaş indirme testlerini ayrı çalıştır**

Run:
```bash
cargo test --release --bin animecix download::tests::parallel_three_small_files_all_done -- --test-threads=1
cargo test --release --bin animecix download::tests::tarpit_two_stall_one_fast -- --test-threads=1
```
Expected: PASS.

- [ ] **Step 4: AppImage üret**

Run:
```bash
GITHUB_TOKEN= VERSION_BUMP=none GITHUB_REPO=veilzon/animecix-linux bash build_appimage.sh
```
Expected: `AnimeciX-x86_64.AppImage` üretilir; sürüm `1.2.7`.

- [ ] **Step 5: Paket doğrulaması**

Run:
```bash
rm -rf /tmp/opencode/appimage-final && mkdir -p /tmp/opencode/appimage-final
cd /tmp/opencode/appimage-final
/home/veilzon/animecix-app/AnimeciX-x86_64.AppImage --appimage-extract >/dev/null
grep -n 'LD_LIBRARY_PATH' squashfs-root/AppRun || true
find squashfs-root/usr/lib -maxdepth 1 -type f \( -name 'libstdc++*' -o -name 'libgcc_s*' \) -printf '%f\n'
test -f squashfs-root/usr/share/icons/hicolor/scalable/actions/office-chart-bar-symbolic.svg
ldd squashfs-root/usr/bin/animecix | grep 'not found' || true
```
Expected:
- Gerçek `export LD_LIBRARY_PATH` yok (yalnızca yorum satırı olabilir).
- `libstdc++`/`libgcc_s` listesi boş.
- İstatistik ikonu pakette var.
- `ldd` eksik göstermiyor.

- [ ] **Step 6: İzole smoke testi**

Run:
```bash
tmp=$(mktemp -d /tmp/opencode/animecix-final-smoke.XXXXXX)
mkdir -p "$tmp/run" "$tmp/cache" "$tmp/config" "$tmp/data"
chmod 700 "$tmp/run"
set +e
dbus-run-session -- env APPIMAGE_EXTRACT_AND_RUN=1 HOME="$tmp" XDG_RUNTIME_DIR="$tmp/run" XDG_CACHE_HOME="$tmp/cache" XDG_CONFIG_HOME="$tmp/config" XDG_DATA_HOME="$tmp/data" timeout 6s xvfb-run -a ./AnimeciX-x86_64.AppImage >"$tmp/out.log" 2>&1
echo "EXIT=$?"
tail -n 8 "$tmp/out.log"
```
Expected: `EXIT=124`; çökme yok.

- [ ] **Step 7: Rapor**

Raporla:
- `sha256sum AnimeciX-x86_64.AppImage`
- boyut
- test sayıları
- değişen davranışlar
- bilinen sınırlamalar (çevrimdışı yerel skip yok; istatistik süresi tahmini).

- [ ] **Step 8: Son commit (varsa)**

```bash
git status --short
# Yalnızca bu plana ait değişiklikler varsa:
git add <dosyalar>
git commit -m "Doğrulama: v1.2.7 AppImage ve testler"
```

---

## Kabul Kriterleri

- `src/main.rs` ~150–200 satır.
- `src/app.rs` ~1.500–2.000 satır (opsiyonel Faz 6 dahil).
- `cargo check --release --bin animecix` ve `--tests` 0 warning.
- Mevcut testler + yeni testler geçiyor.
- İndirilmiş bölüm varsa normal açılışta ve `n`/`p` geçişlerinde yerel dosya oynuyor.
- Yerel oynatmada skip planı varsa intro/outro çalışıyor; plan yoksa oynatma bozulmuyor.
- Bölüm sağ tık menüsünde indirme var.
- İzlendi işaretli bölümün barı ilk açılışta ve toggle'da full.
- MPV kapandıktan sonra RAM trim çalışıyor; kapaklar kaybolmuyor.
- İstatistikler sayfası boş/dolu state'te açılıyor; grafikler çiziliyor; eski state bozulmuyor.
- AppImage SELinux/`LD_LIBRARY_PATH` açısından temiz; smoke testi geçiyor.
- Push/tag/release yok; kullanıcı verileri ve kirli dosyalar korunmuş.

## Bilinen Sınırlamalar

- Yerel skip planı oynatma anında ağdan çekilir; tamamen çevrimdışıysa skip olmayabilir.
- İzleme süresi eski kayıtlarda tahminidir; `≈` ile gösterilir.
- `probe.rs` bir tanılama binary'sidir; dead-code allow yalnızca bu crate'i sessizleştirir.
- `docs/` klasörü GitHub Pages sitesidir; bu plan `plans/` altında tutulur.
