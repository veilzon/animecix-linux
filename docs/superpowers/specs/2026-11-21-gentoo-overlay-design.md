# AnimeciX Gentoo Overlay Tasarımı

**Tarih:** 2026-11-21
**Durum:** Onaylandı

## Amaç

AnimeciX'i Gentoo kullanıcılarının kendi overlay'imiz üzerinden kaynaktan
kurabilmesi. Kullanıcı `eselect repository` ile overlay'i ekler, `emerge
media-video/animecix` ile uygulamayı derler.

## Kapsam

- Yeni repo: `veilzon/animecix-gentoo`
- Paket: `media-video/animecix`
- Kaynak ebuild: `v1.4.5` tag tarball'ı, `cargo.eclass`
- `animecix` ve `animecix-calendar` birlikte kurulur
- Desktop dosyası, metainfo, ikonlar ve `/usr/share/animecix/assets`
- `scripts/bump.sh` ile yeni sürüm ebuild'i ve `Manifest` üretimi
- CI: `pkgcheck` + `Manifest` doğrulaması; tam `emerge` testi manuel/tag

## Kapsam Dışı

- Resmî Gentoo ağacı veya GURU başvurusu (overlay oturduktan sonra)
- Binary ebuild / AppImage paketleme
- `animecix-calendar` için ayrı paket

## Kararlar

- Paket kategorisi `media-video`; uygulama medya oynatıcı.
- `KEYWORDS="~amd64"`; Gentoo'da test edilmeden stable işaretlenmez.
- Runtime bağımlılıkları: `gui-libs/gtk:4`, `gui-libs/libadwaita`,
  `media-video/mpv[libmpv]`, `net-misc/aria2`.
- Derleme bağımlılıkları: `virtual/rust`, `dev-build/cmake`,
  `llvm-core/clang`, `dev-lang/perl`, `dev-lang/nasm`, `sys-libs/zlib`.
- Desktop dosyası RPM ile aynı `tr.com.animecix` kimliğini kullanır.
- `Manifest`, Gentoo `ebuild ... manifest` ile üretilir; crate listesi
  `Cargo.lock`'tan otomatik çıkarılır.
- CI'da tam `emerge` yalnızca manuel/tag tetiklemesinde çalışır; her push'ta
  `pkgcheck` ve Manifest kontrolü yeterli.

## Doğrulama

- `pkgcheck scan --net` hatasız
- `ebuild animecix-1.4.5.ebuild manifest` başarılı
- Temiz `gentoo/stage3` container'ında `emerge media-video/animecix` başarılı
- Kurulum sonrası `animecix --check-icons`, `animecix --check-shaders`
