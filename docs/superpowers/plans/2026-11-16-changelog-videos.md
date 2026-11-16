# Changelog Video Carousel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sürüm-notları popup'ına üç tanıtım videosu içeren sessiz bir carousel eklemek; videoları tüm paketlere gömmek ve F1 kısayol panelini yeni oynatıcı tuşlarıyla eşitlemek.

**Architecture:** `src/ui/changelog.rs` carousel'i, video verisini ve kaynak yolu çözücüsünü taşır. `src/app.rs` yalnızca sürüm kaydını ve popup'ı açar. Videolar `assets/changelog/` altında sıkıştırılmış H.264 dosyalarıdır. AppImage/Flatpak/deb/rpm/Arch aynı dizini paketler.

**Tech Stack:** Rust, GTK4 (`gtk::Video`/`GtkMediaFile`), libadwaita, ffmpeg (libx264), GitHub Actions.

**Spec:** `.superpowers/sdd/2026-11-16-changelog-videos-design.md`

## Global Constraints

- Release/push yok; `Cargo.toml` sürümü değişmez.
- Videolar: 1080×1080, H.264 High, CRF 20, preset veryslow, yuv420p, faststart, **ses yok**. Toplam hedef ≤ 2,5 MB.
- Dosya adları: `ekrangoruntusu.mp4`, `fansubsecimi.mp4`, `hizsecimi.mp4`.
- Popup'ta 5 madde; **paketleme ve odak düzeltmesi changelog'a girmez**.
- Geliştirme testi: `ANIMECIX_CHANGELOG_PREVIEW` ortam değişkeni (herhangi bir değer) popup'ı `seen_changelog` durumundan bağımsız açar; `seen_changelog` alanını değiştirmez.
- Yeni sürüm kaydı: `1.4.0`. Release hazırlığında `Cargo.toml` 1.4.0 olunca normal akışta görünür.
- Oynatıcı ikonları AppImage'da mevcut 60 SVG Adwaita setinden gelir; CI bunu doğrular.
- Videolar sessiz ve otomatik loop; aynı anda tek video oynar.
- Kullanıcı onayı olmadan commit dışı release/push yapılmaz.

## Review Focus

- Video dosyası yoksa/bozuksa popup çökmeden "Video yüklenemedi" göstermeli.
- AppImage/Flatpak/deb/rpm'de `assets/changelog` yolu bulunmalı; bulunamazsa liste yine çalışmalı.
- GStreamer decoder yoksa video gizlenip liste kalmalı.
- Oynatıcı açıldığında ilk tıklama olmadan klavye kısayolları çalışmalı.
- AppImage'da oynatıcı ikonları eksik olmamalı.
- Preview modu `seen_changelog`'u kirletmemeli.

---

### Task 1: Tanıtım videolarını sıkıştır ve varlıkları ekle

**Files:**
- Create: `scripts/encode-changelog-videos.sh`
- Create: `assets/changelog/ekrangoruntusu.mp4`
- Create: `assets/changelog/fansubsecimi.mp4`
- Create: `assets/changelog/hizsecimi.mp4`
- Modify: `.gitignore`

**Interfaces:**
- Produces: `assets/changelog/<name>.mp4` (H.264, sessiz, faststart)

- [ ] **Step 1: Betiği yaz**

`scripts/encode-changelog-videos.sh`, ffmpeg ile kök dizindeki üç kaydı `assets/changelog/` altına çevirir:

```bash
-vf "scale=1080:1080:flags=lanczos" \
-c:v libx264 -preset veryslow -crf 20 -profile:v high \
-pix_fmt yuv420p -movflags +faststart -map_metadata -1 -an
```

- [ ] **Step 2: Videoları üret**

Run: `bash scripts/encode-changelog-videos.sh`
Expected: üç dosya oluşur.

- [ ] **Step 3: Çıktıyı doğrula**

Run:
```bash
for f in assets/changelog/*.mp4; do ffprobe -v error -show_entries stream=codec_type,width,height -of compact "$f"; done
stat -c '%n %s' assets/changelog/*.mp4
```
Expected: her dosyada yalnız video akışı, 1080×1080; toplam ≤ 2,5 MB; ses akışı yok.

- [ ] **Step 4: Ham kayıtları git dışında tut**

`.gitignore` başına yalnız kök dizin kayıtlarını kapsayan satırı ekle:

```
/*.mp4
```

`assets/changelog/*.mp4` etkilenmemeli (`git check-ignore assets/changelog/hizsecimi.mp4` çıktı vermemeli).

- [ ] **Step 5: Commit**

```bash
git add scripts/encode-changelog-videos.sh assets/changelog .gitignore
git commit -m "assets(changelog): oynatıcı tanıtım videolarını ekle"
```

---

### Task 2: Carousel modülü, video verisi ve yol çözücü

**Files:**
- Create: `src/ui/changelog.rs`
- Modify: `src/ui/mod.rs`
- Modify: `assets/style.css`
- Test: `src/ui/changelog.rs` (test modülü)

**Interfaces:**
- Consumes: `assets/changelog/*.mp4` (Task 1).
- Produces:
  - `pub struct ChangelogVideo { pub file: &'static str, pub title: &'static str, pub caption: &'static str }`
  - `pub const PREVIEW_VERSION: &str = "1.4.0";`
  - `pub fn videos_for(version: &str) -> &'static [ChangelogVideo]`
  - `pub fn video_path(file: &str) -> Option<std::path::PathBuf>`
  - `fn video_candidates(appdir: Option<&std::path::Path>, exe: Option<&std::path::Path>, file: &str) -> Vec<std::path::PathBuf>`
  - `pub fn present(parent: &adw::ApplicationWindow, heading: &str, items: &[&str], videos: &[ChangelogVideo])`

- [ ] **Step 1: Testleri yaz**

`src/ui/changelog.rs` test modülüne:

```rust
#[test]
fn preview_has_three_videos_in_order() {
    let v = videos_for(PREVIEW_VERSION);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].file, "ekrangoruntusu.mp4");
    assert_eq!(v[1].file, "fansubsecimi.mp4");
    assert_eq!(v[2].file, "hizsecimi.mp4");
}

#[test]
fn bundled_videos_exist_and_are_not_empty() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/changelog");
    for v in videos_for(PREVIEW_VERSION) {
        let p = dir.join(v.file);
        let len = std::fs::metadata(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).len();
        assert!(len > 100_000, "{} çok küçük: {len}", p.display());
    }
}

#[test]
fn candidates_prefer_appdir_then_flatpak_then_fhs() {
    let c = video_candidates(Some(std::path::Path::new("/tmp/AppDir")), None, "x.mp4");
    assert!(c[0].ends_with("usr/share/animecix/assets/changelog/x.mp4"));
    assert!(c.iter().any(|p| p == std::path::Path::new("/app/share/animecix/assets/changelog/x.mp4")));
    assert!(c.iter().any(|p| p == std::path::Path::new("/usr/share/animecix/assets/changelog/x.mp4")));
}

#[test]
fn missing_video_returns_none() {
    assert!(video_path("boyle-bir-video-yok-12345.mp4").is_none());
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `nice -n 10 env CARGO_BUILD_JOBS=2 cargo test --release --bin animecix ui::changelog -- --test-threads=1`
Expected: FAIL (modül/fonksiyonlar yok).

- [ ] **Step 3: Veri ve yol çözücüyü uygula**

`ChangelogVideo` sabitleri ve `videos_for` yukarıdaki sırayla. `video_path` şu adayları sırayla dener:

1. `$APPDIR/usr/share/animecix/assets/changelog/<file>`
2. `/app/share/animecix/assets/changelog/<file>`
3. `/usr/share/animecix/assets/changelog/<file>`
4. `current_exe` ebeveyni + `assets/changelog/<file>` ve `../../assets/changelog/<file>`
5. `env!("CARGO_MANIFEST_DIR")/assets/changelog/<file>` (geliştirme/test)

- [ ] **Step 4: Carousel'i uygula**

`present` mevcut `app.rs` popup'ının yerini alacak şekilde:

- `adw::Window`, 560 genişlik; video varsa 680 yükseklik.
- Üstte 440×440 `gtk::Video`; `autoplay(true)`, `loop(true)`, `media_stream().set_muted(true)`.
- Video etrafında `overflow(hidden)` + `.changelog-video-wrap`; başlık/açıklama etiketleri.
- Üç nokta `gtk::Button` (`.changelog-dot`); tıklayınca `set_filename` + başlık/açıklama + `active` sınıfı.
- Dosya yoksa veya `MediaStream::error` gelirse videoyu gizle, "Video yüklenemedi" etiketi göster; liste çalışmaya devam eder.
- `connect_close_request` içinde `media_stream().set_playing(false)`.
- Videolar boşsa eski düz liste görünümü korunur.
- Madde listesi kaydırılabilir; carousel sabit kalır.

`assets/style.css` sonuna:

```css
.changelog-video-wrap { border-radius: 14px; background-color: #000; }
.changelog-video-title { font-weight: 700; font-size: 1.05em; }
.changelog-video-caption { color: alpha(currentColor, 0.72); }
.changelog-dot { min-width: 10px; min-height: 10px; padding: 0; border-radius: 999px; background-color: alpha(currentColor, 0.28); }
.changelog-dot.active { background-color: @accent_color; }
```

- [ ] **Step 5: Testleri doğrula**

Run: `nice -n 10 env CARGO_BUILD_JOBS=2 cargo test --release --bin animecix ui::changelog -- --test-threads=1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/changelog.rs src/ui/mod.rs assets/style.css
git commit -m "feat(changelog): video carousel bileşeni ve yol çözücü"
```

---

### Task 3: Sürüm-notları popup'ını yeni akışa bağla

**Files:**
- Modify: `src/app.rs:3789-3841` (`maybe_show_changelog`)
- Modify: `src/app.rs:3957+` (`changelog_items`)
- Test: `src/app.rs` (`decide_retry_tests` yanındaki changelog testleri)

**Interfaces:**
- Consumes: `ui::changelog::{present, videos_for, PREVIEW_VERSION}` ve `should_show_changelog`.
- Produces: `fn should_show_changelog(current: &str, seen: &str, preview: bool) -> bool`

- [ ] **Step 1: Testleri yaz**

```rust
#[test]
fn changelog_140_has_five_items_and_videos() {
    let (_, items) = super::changelog_items("1.4.0").expect("1.4.0 kaydı");
    assert_eq!(items.len(), 5);
    assert!(!items.iter().any(|i| i.to_lowercase().contains("paket")));
    assert_eq!(crate::ui::changelog::videos_for("1.4.0").len(), 3);
}

#[test]
fn preview_shows_without_touching_seen_state() {
    assert!(super::should_show_changelog("1.3.5", "1.3.5", true));
    assert!(!super::should_show_changelog("1.3.5", "1.3.5", false));
    assert!(super::should_show_changelog("1.3.5", "1.3.4", false));
}
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix changelog_140 -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: `1.4.0` maddelerini ekle**

`changelog_items("1.4.0")` beş madde döndürür:

1. Uygulama içi oynatıcı varsayılan; harici mpv Ayarlar'dan.
2. Fansub/kaynak seçimi; Mega.nz desteği.
3. Bölüm çekmecesi, önceki/sonraki, intro/outro, 10 sn kart gizleme.
4. Sağ tık menüsü: ekran görüntüsü, video bilgisi, oran, indirme.
5. Hız 0.5x–2.0x; F1 kısayol paneli güncellendi.

Paketleme ve odak maddesi yok.

- [ ] **Step 4: `maybe_show_changelog`'u değiştir**

- `preview = std::env::var_os("ANIMECIX_CHANGELOG_PREVIEW").is_some()`
- `should_show_changelog(cur, &seen, preview)` false ise çık.
- Preview değilse `seen_changelog = cur` kaydet; preview ise kaydetme.
- `version = if preview { PREVIEW_VERSION } else { cur }`.
- `changelog_items(version)` ve `ui::changelog::videos_for(version)` al; `ui::changelog::present(&self.window, heading, &items, videos)` çağır.
- Eski elle kurulan popup kodunu kaldır.

- [ ] **Step 5: Testleri doğrula**

Run: `cargo test --release --bin animecix changelog -- --test-threads=1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(changelog): v1.4.0 video carousel ve preview modu"
```

---

### Task 4: F1 kısayol panelini yeni oynatıcı tuşlarıyla eşitle

**Files:**
- Modify: `src/ui/shortcuts.rs`
- Test: `src/ui/shortcuts.rs`

**Interfaces:**
- Produces: `Gömülü Oynatıcı` grubunda Space, ←/→, ↑/↓, F, A, S/E, N/P, M, Shift+M.

- [ ] **Step 1: Testi genişlet**

`xml_includes_settings_rows` içine:

```rust
assert!(with.contains("10 sn geri"));
assert!(with.contains("10 sn ileri"));
assert!(with.contains("Sesi artır"));
assert!(with.contains("Sesi azalt"));
```

- [ ] **Step 2: Kırmızıyı doğrula**

Run: `cargo test --release --bin animecix ui::shortcuts -- --test-threads=1`
Expected: FAIL.

- [ ] **Step 3: Grubu tamamla**

Mevcut `Gömülü Oynatıcı` grubuna `Left`, `Right`, `Up`, `Down` öğelerini ekle; metinler yukarıdaki testlerle aynı. `M` (Shift) ve `Shift+M` ayrımını koru.

- [ ] **Step 4: Testleri doğrula**

Run: `cargo test --release --bin animecix ui::shortcuts -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/shortcuts.rs
git commit -m "feat(shortcuts): F1 panelinde gömülü oynatıcı tuşları"
```

---

### Task 5: Oynatıcı açılışında pencere ve klavye odağı

**Files:**
- Modify: `src/app/player/mod.rs`
- Modify: `src/app.rs:1531-1543` (`Page::Player` dalı)

**Interfaces:**
- Produces: `EmbeddedPlayer::focus(&self)`; map olduğunda `gl_area.grab_focus()`.

- [ ] **Step 1: `focus` metodunu ekle**

```rust
pub fn focus(&self) {
    self.state.borrow().gl_area.grab_focus();
}
```

- [ ] **Step 2: Map ve sayfa açılışında odağı ver**

- Player `build` içinde `gl_area.connect_map(|a| a.grab_focus())` bağla.
- `show_page` içindeki `Page::Player` dalında `switch` sonrası:
  - `self.window.present();`
  - `self.player.borrow().as_ref().map(|p| p.focus());`

- [ ] **Step 3: Derleme ve smoke**

Run:
```bash
nice -n 10 env CARGO_BUILD_JOBS=2 cargo check --release
```
Expected: PASS.

- [ ] **Step 4: Elle doğrula**

`ANIMECIX_LIBMPV_PATH=... ./target/release/animecix` ile bölüm aç; **hiç tıklamadan** Space/F/S/E çalışmalı. Çalışmazsa `window.present()` + `gl_area.grab_focus()` çağrılarını `glib::idle_add_local_once` ile bir sonraki döngüye taşı.

- [ ] **Step 5: Commit**

```bash
git add src/app/player/mod.rs src/app.rs
git commit -m "fix(player): açılışta pencere ve klavye odağı"
```

---

### Task 6: Paketleme ve CI ikon/video kontrolleri

**Files:**
- Modify: `packaging/flatpak/io.github.veilzon.AnimeciX.yml`
- Modify: `packaging/flathub-submission/io.github.veilzon.AnimeciX.yml`
- Modify: `.github/workflows/package.yml`
- Modify: `packaging/arch/PKGBUILD`
- Modify: `.github/workflows/appimage.yml`
- Test: `src/app/player/ui.rs` (ikon varlık testi) ve CI shell

**Interfaces:**
- Consumes: `assets/changelog` (Task 1) ve player ikon seti.
- Produces: tüm paketlerde `/usr/share/animecix/assets/changelog` ve `/app/share/animecix/assets/changelog`.

- [ ] **Step 1: İkon testi yaz**

`src/app/player/ui.rs` test modülüne:

```rust
#[test]
fn player_icons_are_bundled() {
    const ICONS: &[&str] = &[
        "go-previous-symbolic", "go-next-symbolic",
        "media-playback-start-symbolic", "media-playback-pause-symbolic",
        "media-playback-stop-symbolic", "media-skip-forward-symbolic",
        "audio-volume-high-symbolic", "audio-volume-low-symbolic",
        "audio-volume-medium-symbolic", "audio-volume-muted-symbolic",
        "view-fullscreen-symbolic", "view-restore-symbolic",
        "camera-photo-symbolic", "dialog-information-symbolic",
        "folder-download-symbolic",
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/hicolor/scalable/actions");
    for icon in ICONS {
        let p = dir.join(format!("{icon}.svg"));
        assert!(p.is_file(), "eksik ikon: {}", p.display());
    }
}
```

- [ ] **Step 2: Kırmızıyı doğrula (beklenen: PASS olabilir)**

Run: `cargo test --release --bin animecix player_icons_are_bundled -- --test-threads=1`
Expected: Mevcut sette PASS; eksik varsa dosyayı `assets/hicolor/scalable/actions` altına ekle.

- [ ] **Step 3: Paket manifestlerini güncelle**

- Flatpak (yerel): `cp -r assets/changelog /app/share/animecix/assets/` + `chmod -R a-w .../changelog`.
- Flathub submission: aynı satırlar.
- `package.yml` girdi hazırlığı: `cp -r assets/changelog girdi/usr/share/animecix/assets/`.
- Arch PKGBUILD: `cp -r assets/changelog "$pkgdir/usr/share/animecix/assets/"`.

- [ ] **Step 4: AppImage CI kontrolü ekle**

`appimage.yml` smoke adımında:
```bash
test -e "$APPDIR/usr/share/animecix/assets/changelog/ekrangoruntusu.mp4"
for icon in go-previous-symbolic go-next-symbolic media-playback-start-symbolic \
  media-playback-pause-symbolic media-playback-stop-symbolic media-skip-forward-symbolic \
  audio-volume-high-symbolic audio-volume-muted-symbolic view-fullscreen-symbolic \
  view-restore-symbolic camera-photo-symbolic dialog-information-symbolic folder-download-symbolic; do
  test -f "$APPDIR/usr/share/icons/Adwaita/symbolic/actions/$icon.svg"
done
```

- [ ] **Step 5: Sözdizimi ve test**

Run:
```bash
bash -n build_appimage.sh
/tmp/opencode/actionlint .github/workflows/*.yml
python3 - <<'PY'
import yaml, pathlib
for p in [pathlib.Path('packaging/flatpak/io.github.veilzon.AnimeciX.yml'), pathlib.Path('packaging/flathub-submission/io.github.veilzon.AnimeciX.yml')]:
    yaml.safe_load(p.read_text())
print('YAML OK')
PY
cargo test --release --bin animecix player_icons_are_bundled -- --test-threads=1
```
Expected: hepsi PASS.

- [ ] **Step 6: Commit**

```bash
git add packaging .github/workflows/package.yml .github/workflows/appimage.yml src/app/player/ui.rs
git commit -m "build(changelog): videoları ve oynatıcı ikonlarını paketle"
```

---

### Task 7: Uçtan uca doğrulama ve changelog metni

**Files:**
- Modify: `CHANGELOG.md` (yalnız kullanıcı onaylı 5 madde; paketleme/odak yok)
- Test: mevcut tüm testler + AppImage

- [ ] **Step 1: Tam test turu**

Run:
```bash
cargo fmt --all -- --check
xvfb-run -a env LIBGL_ALWAYS_SOFTWARE=1 CARGO_BUILD_JOBS=2 cargo test --release --bin animecix -- --test-threads=1
cargo test --release --test refcell_guard --test next_episode
```
Expected: PASS.

- [ ] **Step 2: Preview popup'ı elle doğrula**

```bash
ANIMECIX_CHANGELOG_PREVIEW=1 ANIMECIX_LIBMPV_PATH=/tmp/opencode/animecix-mpv-dev/usr/lib64/libmpv.so.2 \
  ./target/release/animecix
```
Expected: popup açılır; üç video sessiz/loop oynar; noktalar geçiş yapar; kapatınca ses/oynatma durur; alt maddeler görünür.

- [ ] **Step 3: AppImage doğrula**

`VERSION_BUMP=none ... bash build_appimage.sh` ile üret; `--check-libmpv`, `--player-smoke` ve paket içi video/ikon yollarını doğrula. Preview ile açıp videoları gözle kontrol et.

- [ ] **Step 4: CHANGELOG metnini kullanıcının son metniyle güncelle**

Yalnız onaylanan 5 madde; video tanıtım cümlesi kullanıcıdan gelince eklenir. Paketleme ve odak düzeltmesi eklenmez.

- [ ] **Step 5: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): v1.4.0 oynatıcı yenilikleri"
```

Release/push yok.
