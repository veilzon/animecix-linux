# AnimeciX Linux

[![Web](https://img.shields.io/badge/Web-veilzon.github.io%2Fanimecix--linux-0969da?style=flat-square)](https://veilzon.github.io/animecix-linux/)
[![Releases](https://img.shields.io/github/v/release/veilzon/animecix-linux?style=flat-square)](https://github.com/veilzon/animecix-linux/releases/latest)
[![License: MIT](https://img.shields.io/github/license/veilzon/animecix-linux?style=flat-square)](LICENSE)

<img src="assets/hicolor/256x256/apps/tr.com.animecix.png" align="right" width="96" height="96" alt="AnimeciX">

**Linux için GTK4/libadwaita ile yazılmış anime, dizi ve film izleme masaüstü istemcisi.**

**Flatpak** (önerilen), **.deb/.rpm** paketleri veya tek dosyalık taşınabilir
**AppImage** olarak dağıtılır. Flatpak kendi depomuzdan kurulur; güncellemeler uygulama içinden
denetlenir ve `flatpak update` ile de yapılabilir; AppImage/deb/rpm sistemdeki GTK4, libadwaita ve mpv'yi kullanır.
Sürüm notları için [CHANGELOG.md](CHANGELOG.md).

> ⚠️ Bu uygulama **gayriresmî** ve eğitim/kişisel kullanım amaçlıdır; herkese açık bir API
> kullanır. Hizmet sağlayıcıya zarar vermeden, kendi sorumluluğunuzda kullanın.

---

## Özellikler

- **Yan menü**: Ana Sayfa, Keşfet, Favoriler, Maraton, Geçmiş, İstatistikler, Takvim, Haberler, İndirilenler, Ayarlar ve Kısayollar tek tıkla; daraltınca sadece simgeler kalır
- **Ana sayfa**: tam genişlikte öne çıkan (hero) öneri, iskelet yükleme, kaldığın bölümler (Devam rafı), sayfalı raflar
- **Keşfet / Takvim / Haberler**: tür-sıralama filtreli katalog, yayın takvimi, uygulama içi haberler (yazı boyu ayarlanabilir)
- **5 koyu tema**: Koyu, Bordo, Orman, Lacivert, Mor; karşılama ekranında canlı önizleme
- **Hızlı arama**: ana sayfadaki öne çıkan alanın üstündeki arama çubuğu Enter ile sonuç penceresi açar; bölüm ekranında hızlı bölüm arama
- **Oynatıcı**: MPV ile oynatma, otomatik tam ekran, resmi intro/outro atlama (`s`/`e`), çalan şarkı bilgisi ve Shift+M ile tarayıcıda açma, `n`/`p` ile önceki/sonraki bölüm (otomatik geçiş ayarı da var), isteğe bağlı oynatma kalite seçici; indirilmiş bölümlerde de intro/outro atlama çalışır
- **Takip**: Favoriler, sürükle-bırak sıralamalı Maraton, izleme Geçmişi, kaldığın yerden devam; izlendi işaretlenen bölümün ilerleme çubuğu anında dolar
- **İstatistikler**: anime/bölüm/süre özeti, en çok izlenenler, sezon dağılımı ve son 8 haftanın izleme ritmi; filmler bölüm sayılmaz, ayrı film sayısı gösterilir
- **Discord Rich Presence**: Discord'da izlediğin anime/film ve bölüm bilgisi görünür; varsayılan açık, Ayarlar → Discord'dan kapatılabilir veya socket yolu değiştirilebilir
- **İndirme yöneticisi**: bölüm ve film indirme, kaldığı yerden devam, toplu indirme; indirilen bölümler listede işaretlenir, sağ tıkla indirilebilir ve normal oynatmada yerel dosyadan öncelikli açılır
- **Bellek toparlama**: MPV kapandıktan sonra kapak ve geçici önbellek belleği toparlanır
- **Hesap**: giriş/çıkış, profil; favori, konum ve geçmiş siteyle çift-yön eşitlenir
- **Akıllı kaynak seçimi**: kaynaklar paralel çözülür, en kalitelisi seçilir; ölü kaynak elenir, açılmayan kaynakta sonrakine geçilir. manuel kaynak seçimi mevcuttur.
- **Flatpak**: kendi Flatpak depomuzdan tek komutla kurulum; güncellemeler uygulama içinden denetlenir
- **Otomatik güncelleme**: AppImage, .deb, .rpm ve Flatpak kurulumları başlatmada yeni sürümü denetler, tek tıkla günceller (yetki istenir); GitHub indirmeleri paralel parçalarla hızlandırılır
- **Kurulum sihirbazı**: bağımlılık kontrolü ve masaüstü başlatıcı kurulumu
- **Arayüz ölçeği**: %100–%125 arası serbest değer, anında uygulanır
- **Ayarlar**: tema, kısayollar, haber yazı boyu, indirme klasörü, Discord RPC, masaüstü başlatıcı, veri sıfırlama
- **Hızlı yükleme**: HTTP/2 multiplexing ve bağlantı havuzu (keep-alive);
  kapak görselleri paralel (12 worker) indirilir

---

## Ekran Görüntüleri

_ekran görüntüleri her zaman güncel olmayabilir._

<p align="center">

<div align="center">

| | |
|:---:|:---:|
| <img src="screenshots/anasayfa.png" width="100%" alt="Ana Sayfa"> | <img src="screenshots/bölümler.png" width="100%" alt="Bölümler"> |
| **Ana Sayfa** | **Bölümler** |
| <img src="screenshots/keşfet.png" width="100%" alt="Keşfet"> | <img src="screenshots/film.png" width="100%" alt="Film"> |
| **Keşfet** | **Film** |
| <img src="screenshots/favoriler.png" width="100%" alt="Favoriler"> | <img src="screenshots/maraton.png" width="100%" alt="İzleme Maratonu"> |
| **Favoriler** | **İzleme Maratonu** |
| <img src="screenshots/geçmiş.png" width="100%" alt="Geçmiş"> | <img src="screenshots/istatistikler.png" width="100%" alt="İstatistikler"> |
| **Geçmiş** | **İstatistikler** |
| <img src="screenshots/takvim.png" width="100%" alt="Yayın Takvimi"> | <img src="screenshots/haberler.png" width="100%" alt="Haberler"> |
| **Yayın Takvimi** | **Haberler** |
| <img src="screenshots/indirilenler.png" width="100%" alt="İndirilenler"> | <img src="screenshots/ayarlar.png" width="100%" alt="Ayarlar"> |
| **İndirilenler** | **Ayarlar** |
| <img src="screenshots/kısayollar.png" width="100%" alt="Kısayollar"> | |
| **Kısayollar (F1)** | |

</div>

---

## Kurulum

### Flatpak (önerilen)

Flatpak kuruluysa tek komutla kur:

```bash
flatpak install --from https://veilzon.github.io/animecix-flatpak/animecix.flatpakref
```

Flatpak uygulamayı kendi depomuzdan kurar; yalnızca GNOME runtime'ı Flathub'dan
gelir. Uygulama açılışta güncellemeyi denetler; manuel güncelleme:

```bash
flatpak update io.github.veilzon.AnimeciX
```

Kaldırma:

```bash
flatpak uninstall io.github.veilzon.AnimeciX
```

### AppImage

1. [Releases](https://github.com/veilzon/animecix-linux/releases) sayfasından `AnimeciX-x86_64.AppImage` dosyasını indirin.
2. Çalıştırılabilir yapın ve açın:

   ```bash
   chmod +x AnimeciX-x86_64.AppImage
   ./AnimeciX-x86_64.AppImage
   ```

AppImage tek dosyadır; sistemde GTK4, libadwaita ve mpv varsa ek kurulum gerektirmez.
Minimal aria2 paket içinde gelir; ayrıca `aria2` kurmana gerek yoktur. İstersen masaüstü
başlatıcısını **Ayarlar → Masaüstü Başlatıcısını Sistemime Kur** ile ekleyebilirsin.

### Debian / Ubuntu (.deb)

[Releases](https://github.com/veilzon/animecix-linux/releases/latest) sayfasından
`animecix_1.3.4_amd64.deb` dosyasını indir veya doğrudan:

[**animecix_1.3.4_amd64.deb**](https://github.com/veilzon/animecix-linux/releases/download/v1.3.4/animecix_1.3.4_amd64.deb)

```bash
sudo apt install ./animecix_1.3.4_amd64.deb
```

Debian 12+ ve Ubuntu 24.04+ içindir. `mpv`, `libgtk-4-1`, `libadwaita-1-0` ve `aria2`
bağımlılıkları apt tarafından kurulur. Güncellemeler uygulama içinden veya
`sudo apt upgrade` ile yapılır.

### Fedora / RPM

#### COPR (önerilen)

```bash
sudo dnf copr enable veilzon/animecix
sudo dnf install animecix
```

Güncelleme:

```bash
sudo dnf upgrade animecix
```

> `mpv` RPM Fusion deposunda bulunur. Kurulu değilse önce RPM Fusion'ı ekleyin:
> ```bash
> sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm
> ```

#### Doğrudan RPM

[Releases](https://github.com/veilzon/animecix-linux/releases/latest) sayfasından
`animecix-1.3.4-1.fc40.x86_64.rpm` dosyasını indir veya doğrudan:

[**animecix-1.3.4-1.fc40.x86_64.rpm**](https://github.com/veilzon/animecix-linux/releases/download/v1.3.4/animecix-1.3.4-1.fc40.x86_64.rpm)

```bash
sudo dnf install ./animecix-1.3.4-1.fc40.x86_64.rpm
```

Fedora 40+ içindir. `mpv`, `gtk4`, `libadwaita` ve `aria2` bağımlılıkları dnf tarafından
kurulur. Güncellemeler uygulama içinden veya `sudo dnf upgrade` ile yapılır.

### Sistem gereksinimleri

- **Ortak:** x86_64 Linux (Wayland veya X11) ve internet bağlantısı.
- **Flatpak:** Flatpak kurulu olmalı; GNOME runtime'ı Flathub'dan otomatik gelir.
- **AppImage:** glibc ≥ 2.36, GTK ≥ 4.8, libadwaita ≥ 1.2 ve `mpv` (`PATH`'te).
  `libfuse.so.2` yoksa `./AnimeciX-x86_64.AppImage --appimage-extract-and-run` kullan.
- **.deb:** Debian 12+ / Ubuntu 24.04+; bağımlılıklar apt tarafından kurulur.
- **.rpm:** Fedora 40+; bağımlılıklar dnf tarafından kurulur.

AppImage için dağıtım paketleri:

```bash
# Debian/Ubuntu (libfuse2 adı sürüme göre libfuse2t64 olabilir)
sudo apt install libgtk-4-1 libadwaita-1-0 mpv libfuse2

# Fedora
sudo dnf install gtk4 libadwaita mpv fuse-libs

# Arch
sudo pacman -S gtk4 libadwaita mpv fuse2
```

> Tüm sürümler ve eski paketler: [Releases](https://github.com/veilzon/animecix-linux/releases).

### Kaynaktan derleme

Gerekli sistem bağımlılıkları:

- **Debian/Ubuntu:** `sudo apt install libgtk-4-dev libadwaita-1-dev mpv pkg-config build-essential cmake libclang-dev`
- **Fedora:** `sudo dnf install gtk4-devel libadwaita-devel mpv pkgconf-pkg-config gcc-c++ make cmake clang-devel`
- **Arch:** `sudo pacman -S gtk4 libadwaita mpv pkgconf base-devel cmake clang`

Güncel stable Rust kurulu olmalı:

```bash
git clone https://github.com/veilzon/animecix-linux.git
cd animecix-linux
cargo build --release
./target/release/animecix
```

---

## Güncelleme

Uygulama bir **AppImage** olarak çalışıyorsa başlangıçta yeni sürümü kontrol eder:

- **Otomatik:** *Ayarlar → Güncelleme → Otomatik Güncelleme* açıkken yeni sürüm bulunursa
  onay kutusu çıkar; “Güncelle ve Yeniden Başlat” deyince indirir, kurar ve uygulamayı yeniden başlatır.
- **Elle:** *Ayarlar → Şimdi Güncelle* ile istediğin an kontrol edebilirsin.
- **Paketler:** .deb/.rpm güncellemeleri GitHub'dan indirilip sistem paket
  yöneticisiyle (`apt-get`/`dnf`/`zypper`) kurulur; kurulum sonrası sürüm
  doğrulanır. deb/rpm kurulumlarında masaüstü kısayolunu sistem paketi yönetir.
- **Paralel indirme:** Güncelleme dosyaları birden çok bağlantıyla indirilir;
  sunucu desteklemezse tek bağlantıya düşülür.
Kaynaktan derlenen sürümde otomatik güncelleme devre dışıdır.

---

## Geliştiriciler için derleme & yayın

Sürüm hazırlığı:

1. `Cargo.toml`, `CHANGELOG.md` ve uygulama içi `changelog_items` metnini güncelle.
2. Testleri çalıştır:

   ```bash
   xvfb-run -a cargo test --release --bin animecix
   python3 tests/skip_harness.py
   ```

3. Tag’i oluştur ve gönder:

   ```bash
   git tag -a vX.Y.Z -m "AnimeciX vX.Y.Z"
   git push origin main vX.Y.Z
   ```

4. AppImage artık yerelde derlenmez:
   - `.github/workflows/appimage.yml` her `main` push’unda Debian 12 tabanlı
     (glibc 2.36) ve minimal aria2’li AppImage’i üretip artifact olarak yükler.
   - `.github/workflows/release.yml` tag push’unda AppImage’i üretir ve taslak
     release’e ekler.

5. Taslak release’i kontrol edip yayınla:

   ```bash
   gh release edit vX.Y.Z --draft=false --latest
   ```

   `.github/workflows/package.yml` yayınlanan release’e deb/rpm paketlerini ekler.

---

## Klavye Kısayolları

| Kısayol | İşlev |
|---|---|
| `F1` | Kısayollar penceresini aç |
| `F11` | Tam ekran |
| `/` | Bölüm ekranında hızlı bölüm arama |
| `Ctrl+S` | Ana ekranda arama çubuğuna odaklan |
| `s` | Oynatıcıda intro sonuna atla |
| `e` | Oynatıcıda outro sonuna atla |
| `n` / `p` | Oynatıcıda sonraki / önceki bölüm |
| `Ctrl+RIGHT` | Oynatıcıda sonraki bölüm |
| `Shift+M` (`M`) | Çalan şarkıyı tarayıcıda aç (şarkı bilgisi varsa) |
| `Esc` | Geri / aramayı kapat |

Kısayollar *Ayarlar* ekranından değiştirilebilir.

---

## Yapılandırma & Veri

Tüm veriler şurada tutulur:

```
~/state.json                 # izleme geçmişi, favoriler, maraton, ilerleme
~/.local/share/animecix/     # settings.json
~/.cache/animecix/           # API ve kapak önbelleği
```

*Ayarlardan* “Tüm Verileri Sıfırla ve Temizle” ile sıfırlanabilir.

---

## Performans

Uygulama, ağ gecikmesini azaltmak için aşağıdaki teknikleri kullanır:

- **HTTP/2 multiplexing**: Tek TCP/TLS bağlantısı üzerinden çoklu eşzamanlı akış; özellikle
  ana sayfadaki onlarca kapak görselini sıraya sokmadan paralel getirir.
- **Bağlantı havuzu & keep-alive**: Boşta bağlantılar 60sn boyunca sıcak tutulur
  (`pool_idle_timeout`), böylece her istekte tekrar TLS el sıkışması yapılmaz.
- **Brotli sıkıştırma**: JSON yanıtları `br` ile sıkıştırılarak aktarılır.
- **Paralel kapak indirme**: 12 worker ile kapaklar eşzamanlı çekilir (diskte 7 gün önbellekli).
- **Stale-while-revalidate API önbelleği**: Süresi dolmuş veri anında gösterilir, arka planda
  tazelenir; çevrimdışıyken bile eski veri kullanılır.

> Geliştiriciler: `ANIMECIX_BENCH=1 ./target/release/animecix` ile her isteğin süresini
> stderr'a loglayabilir (davranışı etkilemez).

---

## Resmi İntro/Outro Verisi

AnimeciX, intro ve outro süreleri ile açılış/kapanış şarkı bilgilerini
AnimeciX'in resmi video altyapısından alır.
Video açılmadan önce çözülür; sonuç 6 saat önbelleğe alınır.

**Davranış**:
- `s` / `e` tuşları intro/outro sonuna atlar; indirilmiş bölümlerde de çalışır ve atlayınca MPV OSD'de bildirim çıkar
- Şarkı varsa bölüm boyunca sağ üstte görünür; `Shift+M` tuşu şarkıyı tarayıcıda açar
- İntro/outro bildirimleri ve `Shift+M` ipucu Ayarlar'dan kapatılabilir (tuşlar çalışmaya devam eder)

---

## Lisans

[MIT](LICENSE) © 2026 veilzon
