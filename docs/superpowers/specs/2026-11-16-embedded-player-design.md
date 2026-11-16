# Uygulama içi mpv oynatıcı — tasarım

Tarih: 2026-11-16  
Durum: uygulama için hazır; kullanıcı **Yol 1** ve aşağıdaki görsel/kapsam yönünü onayladı.

## Amaç ve neden

AnimeciX'te oynatma varsayılan olarak ayrı mpv penceresi yerine ana uygulamanın içinde açılsın. Kullanıcı Lowell çatalının gömülü oynatıcı düzenini beğeniyor; ancak mevcut AnimeciX'in fansub, skip, Discord, indirme, kaynak seçimi ve ilerleme davranışları kaybolmamalı.

## Hedefler

- `Page::Player` ana pencerede video, yüzen kontroller ve sağdan açılan bölüm listesi sunar.
- Varsayılan oynatıcı gömülüdür; Ayarlar'daki seçimle mevcut harici mpv akışı korunur.
- Lowell'in `libmpv-sys` + `vo=libmpv` + `mpv_render_context` + `GtkGLArea` yaklaşımı Wayland ve X11'de kullanılır.
- Mevcut fansub sorgusu ve seçim diyaloğu değişmez. Oynatma isteği aynı `play_candidates` noktasında gömülü/harici dala ayrılır.
- Mevcut `SkipPlan`, op/ed şarkı adları, resmî/AniSkip zaman kaynakları, ipuçları ve modern **kart bildirimleri** gömülü oynatıcıya bağlanır. Otomatik atlama ve otomatik sonraki bölüm ayarları korunur.
- Kaynak çözme, ölü kaynak geçişi, manuel kaynak/kalite seçimi, kaldığın yerden devam, izlendi/ilerleme, geçmiş, tercih edilen host, Discord RPC, Anime4K, tampon ayarları, indirilen dosyadan oynatma ve OperationStack ile uyumlu çalışır.
- Tema ile tutarlı, videoyu kapatmayan kontroller; okunabilir kontrast, klavye ve fare kısayolları.

## Hedef olmayanlar

- Fansub listeleme/seçim akışını yeniden yazmak.
- Harici mpv akışını kaldırmak veya ayrı ikinci video penceresi açmak.
- Görev için ilgisiz sayfaları/tema sistemini değiştirmek.
- Celluloid/GStreamer/WebKit gibi başka bir oynatıcı motoru eklemek.
- Bu iş kapsamında sürüm yayınlamak, etiketlemek veya mevcut release'leri değiştirmek.

## Kabul edilen yaklaşım: Yol 1

Lowell'in render ve oyuncu iskeletini al; **doğrudan dosya kopyalayıp mevcut oynatma kodunu atlamak yerine**, mevcut `App`, `Page`, ayarlar, `play_candidates`, `SkipPlan` ve ilerleme/RPC veri modellerine uyarla. İki oynatma yolu da kalır.

Değerlendirilen diğer yollar:
- `mpv --wid`: yalnız X11; Wayland/Flatpak için uygun olmadığından reddedildi.
- Yalnız temel kontrollü küçük gömülü oynatıcı: tüm mevcut özellikleri içermediği için reddedildi.

## Uygulama parçaları

### 1. mpv ve sayfa yaşam döngüsü

- `src/embed_mpv.rs`: `libmpv-sys` FFI ve OpenGL render bağlamı.
- `src/app/player_window.rs`: gömülü widget/state; büyük Lowell dosyasını aynen almak yerine işlevsel parçalara ayır.
- `App.player` aktif gömülü oynatıcıyı tutar; `Page::Player` geçmişe eklenir.
- GL realize/render/unrealize ve `shutdown()` sırası korunur: callback ve çizim durur, render bağlamı GL current iken düşer, ardından mpv handle yok edilir.
- Bölüm değiştirme, geri gitme, başka sayfaya geçme ve pencereyi kapatma aktif oynatıcıyı temizler; eski timer/callback yeni bölümü etkileyemez.
- GL oluşturulamayan ortamda kullanıcıya anlaşılır hata ve harici mpv seçeneği gösterilir; çökme olmaz.

### 2. Ortak oynatma bağlantısı

- `play_candidates` öncesindeki fansub/kalite/çözümleme akışı aynı kalır.
- Başlangıçta ortak hazırlık: geçmiş/current, kayıtlı konum, Discord aktivitesi ve "Açılıyor" operasyonu. Aynı olayları iki yol da üretir; ikinci kez kayıt yazılmaz.
- Gömülü istek; aday URL'leri, fallback embed'leri, `SkipPlan`, tüm bölümler/komşular, yerel dosya, ayarlar ve geri/yeni bölüm callback'lerini taşır.
- Kaynak failover ve kalite geçişlerinde mevcut konum korunur. Video yüklenince başlangıç seek'i yalnız bir kere uygulanır.
- Manuel/otomatik önceki ve sonraki bölüm `App.play` üzerinden yeniden çözümlenir; mevcut geçiş üretme kuralları korunur.
- RPC oynatma/duraklatma/konum/bölüm durumunu günceller; oynatıcı kapandığında veya değiştiğinde eski aktivite temizlenir.
- İlerleme/izlendi/preferred-host bilgileri mevcut UI `ProgressBarEntry` yapısına yazılır. Bellek temizliği oynatma sonunda çalışır.

### 3. Görsel ve etkileşim sözleşmesi (uygulama ekranı, Operate modu, code-first)

**Thesis:** video ana yüzeydir; uygulamanın kalıcı navigasyonu oynatma sırasında kaybolur, kontroller yalnız ihtiyaç anında video üstünde belirir.

**İlk görünüm:**
```
┌ [<]                                     [tam ekran] ┐
│                                                     │
│                                                     │
│               [-10]   [oynat]   [+10]               │
│                                                     │
│   Anime adı / bölüm                  [hız] [kalite] │
│   [━━━━━━━━ seek ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━] │
│   [süre]                         [ses]   [top. süre]│
└─────────────────────────────────────────────────────┘
           (isteğe bağlı) sağda BÖLÜMLER çekmecesi
```

- Lowell'in üst geri/tam ekran, merkez `−10/oynat/+10`, alt başlık + hız + kalite + seek + zaman + ses düzeni korunur.
- Sağ kenardaki ince tutamaç veya ekran düğmesi bölüm çekmecesini açar; sezon sekmeleriyle bölüm seçilir. Film/tek bölümde gösterilmez.
- Sağ tık: oynat/duraklat, tam ekran, intro/outro atla, komşu bölümler, doldur/oran, görüntü al, video bilgisi ve gerektiğinde mevcut indirme yöneticisine bağlanan bölüm indirme.
- Kısayollar: Space, oklar, `n`/`p`, `s`/`e`, `m`, `f`, `a`; mevcut Shift+M müzik açma davranışı da korunur. Kısayollar sayfası güncellenir.
- İmleç hareketsizken kontroller ve imleç gizlenir; hareket, klavye veya açık popover ile geri gelir.
- Kontroller her temada siyah scrim/koyu kart ve açık metin kullanır; aksan rengi mevcut `@accent_color` temasından gelir. Açık/koyu tema değişse bile video okunabilirliği bozulmaz.
- Mini/masaüstü boyutlarında başlık elipsisle kısalır; kontroller taşmadan çalışır, uzun bölüm adları drawer'da en çok iki satır olur.

**Bildirim kartları:**
- Atlama zamanları ve şarkı bilgileri ilk hazır olduklarında video üzerinde küçük, okunabilir kartlarla gösterilir; video ortasını uzun süre kapatmazlar.
- Intro/outro zamanı yaklaşınca ilgili `İntroyu atla` / `Outro'yu atla` eylemi sağ altta görünür. `S`/`E` veya tıklama aynı seek/action yolunu kullanır.
- `SkipPlan.music_op/music_ed` ve `song_url` kullanıcıya mevcut metinlerle gösterilir; Shift+M aynı URL'yi açar. Bilgi yoksa eski dürüst "bulunamadı" durumu korunur.
- Kartlar gereksiz tekrar gösterilmez. Yeni bölümde yeni bölümün planı dışında eski timer/kart kalmaz.
- Otomatik atlama/focus veya auto-next gerçekleştiyse kart bunu belirtir; kullanıcının manuel kontrolü engellenmez.

### 4. libmpv ve dağıtım

- **Dinamik yükleme:** Uygulama açılışta `libmpv.so.2` → `libmpv.so.1` → `libmpv.so` sırasıyla `libloading` ile arar. Bulduğu kütüphanenin `mpv_client_api_version()` değerini derleme zamanındaki arayüz beklentisiyle karşılaştırır. Uyumlu sürüm yoksa gömülü oynatıcı başlatılmaz; kullanıcıya tek seferlik anlaşılır bildirim gösterilir ve oynatma mevcut harici mpv yoluna düşer. Bu, Debian/Fedora/Arch arasındaki FFmpeg soname farklarını uygulama başlamadan çökmeden yönetir.
- FFI bildirimleri ve sabitler yerel `src/embed_mpv/ffi.rs` içinde tutulur; `libmpv-sys` **üretim bağımlılığı olmaz**. Semboller (`mpv_create`, `mpv_initialize`, render API, property/command) yüklenen handle üzerinden çağrılır. LLVM/clang gerekmez. Testlerde gerçek `libmpv` varsa smoke test, yoksa sahte yükleyici ile sözleşme testi çalışır.
- Sistem paketlerinde çalışma zamanı bağımlılığı mevcut `mpv` paketidir (libmpv'yi sağlar). Debian `libmpv2`, Fedora `mpv-libs`, Arch `mpv`; eksikse harici fallback devreye girer. AppImage, mevcut `mpv`/FFmpeg bağımlılık taramasına ek olarak gerekli `libmpv` runtime'ını ve soname bağlantılarını taşır; paylaşılan kütüphaneleri iki kez kopyalamaz.
- Flatpak mpv modülünde `-Dlibmpv=true` olur. Mevcut LuaJIT/OSC ve GnuTLS yapılandırması; `b_ndebug` ile union patch'i dokunulmadan korunur.
- Debian 12/glibc 2.36 sınırı korunur. Önceki/karşılaştırma AppImage ve yeni AppImage'ın **sıkıştırılmış bayt boyutu** ölçülür; tahmin rapor olarak sunulmaz.
- `specs/`, `inf`, `packaging/post-install-cleanup.sh` gibi kullanıcıya ait mevcut değişiklikler alınmaz/commit edilmez.

### 5. Kapsam dışı bırakılan Lowell özellikleri

- Lowell'in **Focus Mod**'u, arka plan AniSkip fetch'i, otomatik intro/outro atlaması ve buna bağlı yeni ayarlar getirilmez. Bizim mevcut `SkipPlan` (resmî zamanlar/önbellek/şarkılar), manuel `S`/`E` atlama, `show_intro_hint` bildirimleri ve `auto_next` ayarı aynen kullanılır. Harici mpv davranışı da değişmez.

## Doğrulama kabul ölçütleri

- Ayar varsayılanı gömülü, eski ayar dosyaları migrate edilir; kapalı seçimde harici akış regresyon testlerinden geçer.
- Seek/volume/speed/kalite/kaynak ve klavye birimlerinde mpv çağrıları doğrudan test edilebilir bir yardımcı trait veya komut soyutlaması ile kontrol edilir.
- SkipPlan müzik metinleri, atlama pencereleri, tek-seferlik bildirim/seek, auto-next ve kaynak failover saf fonksiyon testleriyle korunur.
- HTTP aramaları mpv init/çizim için canlı API'ye bağlı UI testine dönüştürülmez.
- CI'da `xvfb` + OpenGL software renderer ile küçük yerel video oynatılır; GLArea ilk kareyi çizer, kontrol değişikliği ve tekrar aç/kapa sonrası süreç ayakta kalır. Gerçek FFmpeg video fixture'ı CI içinde üretilir.
- Her beş tema ve normal/kompakt pencere; kontrol, bölüm çekmecesi, skip müzik kartı ve hata durumları için UI görüntü/snapshot testi mümkün olan test altyapısıyla yapılır. Hazır pencere düzeni en az bir kez gerçek Xvfb ekran görüntüsüyle gözle kontrol edilir.
- Mevcut 287 test + yeni testler, tüm mevcut CI kontrolleri (`cargo fmt`, `clippy`, asset, metadata) geçer.
- Yerelde Arch container testi **çalıştırılmaz**; Arch ve diğer dağıtım doğrulamaları CI'da yapılır.

## Uygulayıcıya bırakılanlar

- Lowell kodundaki parçaların yeni modül sınırlarına bölünmesi ve gereksiz eski `DownloadDialog`/eski AniSkip fetch yolunun alınmaması.
- mpv FFI komutlarının mevcut modüllere en az bağımlılıkla bağlanması.
- Modern kartların tam piksel ölçüleri ve kısa geçişleri; mevcut GTK tema/operation-stack görsel diline uygun biçimde, global tasarımı değiştirmeden belirlenmesi.
- Sistem kurulu libmpv olmayan geliştirme makinesinde ürün kodu değişikliğinden önce uygun izole build yönteminin seçilmesi; global bağımlılık kurulmaması.
