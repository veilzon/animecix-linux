# AnimeciX Linux

[![Web](https://img.shields.io/badge/Web-veilzon.github.io%2Fanimecix--linux-0969da?style=flat-square)](https://veilzon.github.io/animecix-linux/)
[![Releases](https://img.shields.io/github/v/release/veilzon/animecix-linux?style=flat-square)](https://github.com/veilzon/animecix-linux/releases/latest)
[![License: MIT](https://img.shields.io/github/license/veilzon/animecix-linux?style=flat-square)](LICENSE)

<img src="assets/hicolor/256x256/apps/tr.com.animecix.png" align="right" width="96" height="96" alt="AnimeciX">

**Linux için GTK4/libadwaita ile yazılmış anime, dizi ve film izleme masaüstü istemcisi.**

Tek dosyalık taşınabilir **AppImage** olarak dağıtılır; uygulama başlatmada yeni sürümü
kontrol eder ve kendini otomatik güncelleyebilir. 

> ⚠️ Bu uygulama **gayriresmî** ve eğitim/kişisel kullanım amaçlıdır; herkese açık bir API
> kullanır. Hizmet sağlayıcıya zarar vermeden, kendi sorumluluğunuzda kullanın.

---

## Özellikler

- **Yan menü**: Ana Sayfa, Keşfet, Takvim, Haberler, Favoriler, Maraton, Geçmiş, İndirilenler ve Ayarlar tek tıkla; daraltınca sadece simgeler kalır
- **Ana sayfa**: tam genişlikte öne çıkan (hero) öneri, iskelet yükleme, kaldığın bölümler (Devam rafı), sayfalı raflar
- **Keşfet / Takvim / Haberler**: tür-sıralama filtreli katalog, yayın takvimi, uygulama içi haberler (yazı boyu ayarlanabilir)
- **5 koyu tema**: Koyu, Bordo, Orman, Lacivert, Mor; karşılama ekranında canlı önizleme
- **Hızlı arama**: ana sayfadaki öne çıkan alanın üstündeki arama çubuğu Enter ile sonuç penceresi açar; bölüm ekranında hızlı bölüm arama
- **Oynatıcı**: MPV ile oynatma, otomatik tam ekran, resmi intro/outro atlama (S/E), çalan şarkı bilgisi ve Shift+M ile tarayıcıda açma, `n`/`p` ile önceki/sonraki bölüm (otomatik geçiş ayarı da var), isteğe bağlı oynatma kalite seçici
- **Takip**: Favoriler, sürükle-bırak sıralamalı Maraton, izleme Geçmişi, kaldığın yerden devam
- **İndirme yöneticisi**: bölüm ve film indirme, kaldığı yerden devam, toplu indirme
- **Hesap**: giriş/çıkış, profil; favori, konum ve geçmiş siteyle çift-yön eşitlenir
- **Akıllı kaynak seçimi**: kaynaklar paralel çözülür, en kalitelisi seçilir; ölü kaynak elenir, açılmayan kaynakta sonrakine geçilir. manuel kaynak seçimi mevcuttur.
- **Otomatik güncelleme**: AppImage, .deb ve .rpm kurulumları başlatmada yeni sürümü denetler, tek tıkla günceller (paketlerde yetki istenir)
- **Kurulum sihirbazı**: bağımlılık kontrolü ve masaüstü başlatıcı kurulumu
- **Arayüz ölçeği**: %100–%125 arası serbest değer, anında uygulanır
- **Ayarlar**: tema, kısayollar, haber yazı boyu, indirme klasörü, masaüstü başlatıcı, veri sıfırlama
- **Hızlı yükleme**: HTTP/2 multiplexing, bağlantı havuzu (keep-alive) ve DNS önbelleği;
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
| <img src="screenshots/geçmiş.png" width="100%" alt="Geçmiş"> | <img src="screenshots/takvim.png" width="100%" alt="Yayın Takvimi"> |
| **Geçmiş** | **Yayın Takvimi** |
| <img src="screenshots/haberler.png" width="100%" alt="Haberler"> | <img src="screenshots/indirilenler.png" width="100%" alt="İndirilenler"> |
| **Haberler** | **İndirilenler** |
| <img src="screenshots/ayarlar.png" width="100%" alt="Ayarlar"> | <img src="screenshots/kısayollar.png" width="100%" alt="Kısayollar"> |
| **Ayarlar** | **Kısayollar (F1)** |

</div>

---

## Kurulum

### AppImage (önerilen)

1. [Releases](https://github.com/veilzon/animecix-linux/releases) sayfasından `AnimeciX-x86_64.AppImage` dosyasını indirin.
2. Çalıştırılabilir yapın ve açın:

   ```bash
   chmod +x AnimeciX-x86_64.AppImage
   ./AnimeciX-x86_64.AppImage
   ```

AppImage tek dosyadır; taşınabilir, kurulum gerektirmez. İstersen masaüstü başlatıcısını
uygulama içindeki **Ayarlar → Masaüstü Başlatıcısını Sistemime Kur** ile ekleyebilirsin.

### Kaynaktan derleme

Gerekli sistem bağımlılıkları:

- **Debian/Ubuntu:** `sudo apt install libgtk-4-dev libadwaita-1-dev mpv pkg-config`
- **Fedora:** `sudo dnf install gtk4-devel libadwaita-devel mpv pkgconf-pkg-config`
- **Arch:** `sudo pacman -S gtk4 libadwaita mpv pkgconf`

Rust (1.74+) kurulu olmalı:

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
Kaynaktan derlenen sürümde otomatik güncelleme devre dışıdır.

---

## Geliştiriciler için derleme & yayın

AppImage üretmek ve GitHub Release oluşturmak için:

```bash
# GITHUB_TOKEN (repo için içerik/yayın yetkisi olan PAT) tanımlıysa
# betik sürümü otomatik artırır, derler ve release olarak yayınlar:
export GITHUB_TOKEN=ghp_xxxxxxxx
bash build_appimage.sh
```

`build_appimage.sh` varsayılan olarak `Cargo.toml` sürümünü otomatik artırır
(`VERSION_BUMP=none` ile atlanır), `cargo build --release`
çalıştırır ve `GITHUB_TOKEN` tanımlıysa `AnimeciX-x86_64.AppImage` dosyasını
`v<sürüm>` etiketli bir GitHub Release olarak yükler.

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

Tüm veriler (geçmiş, ayarlar, kapak önbelleği) şurada tutulur:

```
~/.local/share/animecix/
~/.cache/animecix/
```

*Ayarlardan* “Tüm Verileri Sıfırla ve Temizle” ile sıfırlanabilir.

---

## Performans

Uygulama, ağ gecikmesini azaltmak için aşağıdaki teknikleri kullanır:

- **HTTP/2 multiplexing**: Tek TCP/TLS bağlantısı üzerinden çoklu eşzamanlı akış; özellikle
  ana sayfadaki onlarca kapak görselini sıraya sokmadan paralel getirir.
- **Bağlantı havuzu & keep-alive**: Boşta bağlantılar 60sn boyunca sıcak tutulur
  (`pool_idle_timeout`), böylece her istekte tekrar TLS/DNS el sıkışması yapılmaz.
- **DNS önbelleği** (`hickory-dns`): Çözümlenen adresler saklanır, tekrarlı `getaddrinfo`
  engellenir.
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
- `s` / `e` tuşları intro/outro sonuna atlar; atlayınca MPV OSD'de bildirim çıkar
- Şarkı varsa bölüm boyunca sağ üstte görünür; `Shift+M` tuşu şarkıyı tarayıcıda açar
- İntro/outro bildirimleri ve `Shift+M` ipucu Ayarlar'dan kapatılabilir (tuşlar çalışmaya devam eder)

---

## Lisans

[MIT](LICENSE) © 2026 veilzon
