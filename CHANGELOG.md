# Changelog — v1.4.0 (güncel sürüm notları)

- **Uygulama içi oynatıcı varsayılan oldu:** libmpv + GtkGLArea tabanlı gömülü
  oynatıcı; harici mpv Ayarlar'dan seçilebiliyor. Lowell benzeri krom, 10 sn
  sonra gizlenen şarkı/atla kartları ve tek ok geri düğmesi eklendi.
- **Kaynak ve çeviri seçimi:** Fansub, host ve kalite tek menüde; seçilen
  fansub yeniden çözülüyor. Mega.nz kaynakları yerel AES-CTR proxy ile
  oynatılıyor; başarısız kaynakta otomatik yedeğe geçiliyor.
- **Bölümler ve atlama:** Oynatıcı içi bölüm çekmecesi, önceki/sonraki bölüm,
  intro/outro atlama ve şarkı kartı eklendi. Otomatik sonraki bölüm açıkken
  outro'yu otomatik atlama ve seçili kaynakta plan yoksa diğer kaynaklardan
  plan çözme desteği var.
- **Oynatma deneyimi:** Oynatma çubuğunda önbellek doluluk göstergesi,
  oynatma butonunda yükleme dönengeci, seek sırasında geri bildirim,
  0.5x–2.0x hız ve sağ tık menüsünden ekran görüntüsü/video bilgisi/indirme.
- **Ayarlar ve ilk açılış:** Ayarlar Ortak / Gömülü Oynatıcı / Normal MPV
  olarak ayrıldı; modern welcome ekranı ve libmpv veya harici mpv kabul eden
  bağımlılık kontrolü eklendi.
- **Kısayollar:** F1 paneli gömülü oynatıcı tuşlarıyla güncellendi
  (Space, ←/→, ↑/↓, F, A, S/E, N/P, M, Shift+M).
- **Bölüm listeleri:** Birden fazla sezonu olan yapımlarda 2. sezon ve
  sonrası için gerçek bölüm adları gösteriliyor.
- **Tanıtım videoları:** Sürüm notlarına sağ tık menüsü, fansub/kalite seçimi
  ve oynatma hızını gösteren üç kısa video eklendi.
- **Paketleme:** AppImage minimal FFmpeg + mpv ile ~23 MB'a indirildi;
  sistem mpv'si gerekmiyor. CI'da ikon, shader, libmpv ve oynatıcı smoke
  testleri çalışıyor.

# Changelog — v1.3.5

- **Flatpak oynatma düzeltmesi:** mpv'nin ytdl desteği olmayan derlemelerinde
  `--ytdl-format` argümanı ölümcül hataya yol açıyordu; doğrudan video
  URL'leri için gereksiz olan argüman kaldırıldı.

# Changelog — v1.3.4

- **Güvenlik güncellemeleri:** TLS sertifika doğrulaması (rustls-webpki,
  webpki-roots), tokio ve zstd bağımlılıkları güncellendi; yeni RustSec
  duyuruları kapatıldı.
- **Flatpak mpv düzeltmesi:** mpv 0.36'daki `m_option_value` union
  başlatma hatası (GCC 15/glibc 2.42'de `mpv --version` çökmesi) upstream
  düzeltmesiyle giderildi; `mpv --version` ve kısa video oynatma CI'da test ediliyor.
- **Flatpak CI güvenilirliği:** Cache'ten gelen eski kurulum temizlenerek
  her zaman yeni build'in test edilmesi sağlandı.

# Changelog — v1.3.3

- **Flatpak'ta otomatik güncelleme:** Uygulama açılışta Flatpak deposunu
  denetler; yeni sürüm varsa tek tıkla güncelleme başlatılır, polkit izni
  istenir ve uygulama yeniden başlar.
- **Fedora COPR deposu:** `sudo dnf copr enable veilzon/animecix` ile kurulum
  ve `sudo dnf upgrade animecix` ile güncelleme.
- **Ortak güncelleme akışı:** Flatpak, deb ve rpm güncellemeleri aynı gömülü
  betik üzerinden yürütülür; yetki istemi korunur.

# Changelog — v1.3.2

- **Flatpak desteği:** AnimeciX artık kendi Flatpak depomuzdan tek komutla
  kuruluyor. `flatpak install --from https://veilzon.github.io/animecix-flatpak/animecix.flatpakref`
  ile kur, `flatpak update` ile güncelle. Flathub'a gönderim yok; uygulama
  bizim depomuzdan, yalnızca GNOME runtime'ı Flathub'dan geliyor.
- **Flatpak'ta Discord ve indirme:** Discord Rich Presence sandbox içinde
  çalışıyor; indirme klasörüne ve seçtiğin klasörlere erişim var.
- **Flatpak entegrasyonu:** Masaüstü kısayolunu ve güncellemeleri Flatpak
  yönetiyor; uygulama içi güncelleme ve sistemden kaldırma yerine
  `flatpak update` / `flatpak uninstall` yönlendirmesi gösteriliyor.

# Changelog — v1.3.1

- **Discord Rich Presence eklendi:** Discord'da izlediğin anime/film ve
  bölüm bilgisi görünüyor. Varsayılan olarak açık; normal, Flatpak, Snap ve
  Vesktop/Vencord kurulumlarını otomatik buluyor, Ayarlar → Discord'dan
  kapatılıp socket yolu değiştirilebiliyor. Süre/geri sayım gösterilmiyor.
- **Geçmişte göreli zaman:** izleme geçmişinde “Az önce”, “5 dk önce”,
  “2 gün önce” gibi etiketler görünüyor.
- **Sistem paketi entegrasyonu:** deb/rpm kurulumlarında masaüstü kısayolu
  paket tarafından yönetiliyor; gereksiz “Kur/Güncelle” düğmesi kaldırıldı.
  “Sistemden kaldır” artık paketi native paket yöneticisiyle kaldırıyor.
- **Hızlı güncelleme indirmesi:** GitHub'dan indirilen AppImage/deb/rpm
  güncellemeleri paralel parçalarla indiriliyor; sunucu desteklemezse tek
  bağlantıya düşülüyor.

# Changelog — v1.3.0

- **İstatistikler sayfası eklendi:** izlediğin anime ve bölüm sayısı,
  tahmini izleme süresi, en çok izlediğin animeler, sezon dağılımı ve
  son 8 haftanın izleme ritmi grafiklerle görülebiliyor. Filmler bölüm
  veya sezon sayılmıyor; anime sayısının yanında ayrı film sayısı
  gösteriliyor.
- **İndirilen bölümler öncelikli:** indirdiğin bölüm normal listeden
  açıldığında yerel dosyadan oynatılıyor; sonraki/önceki geçişlerde de
  öncelik veriliyor.
- **Bölüm listesinde indirildi işareti** ve **sağ tıkla indirme** eklendi.
- **Yerel bölümlerde intro/outro atlama:** indirilmiş bölümlerde de
  `s`/`e` ile atlama çalışıyor.
- **İzlendi işareti:** izlendi olarak işaretlenen bölümün ilerleme
  çubuğu anında doluyor; eski işaretler de aynı şekilde görünüyor.
- **Bellek toparlama:** MPV kapandıktan sonra uygulama kapak ve geçici
  önbellek belleğini toparlıyor.
- **Tam ekran düzeltmeleri:** tam ekrana geçince kapaklar artık anında
  büyüyor; F11/Esc ile tam ekrandan çıkış daha güvenilir; uygulama ikinci
  kez açıldığında yeni pencere oluşmuyor.
- **Kapak yerleşimi:** geniş pencerelerde kapaklar boşluğu dolduruyor,
  gereksiz küçük kalmıyor.

# Changelog — v1.2.7

- Ana sayfada arama çubuğu öne çıkan alanın üstüne taşındı; yazıp
  Enter'a basınca arama açılır.
- Oynatıcıda Ctrl + Sağ Ok ile sonraki bölüme geçilir.
- Yinelenen bölüm kayıtları teklenir; sonraki/önceki bölüme geçerken
  takılma olmaz.
- Kapak yüklemesi aksarsa sıra kendini toparlar, boş kapak kalmaz.

# Changelog — v1.2.6

## Öne çıkanlar

- **Hesap sistemi:** giriş/çıkış, profil sayfası; favori, konum ve geçmiş
  siteyle çift-yön eşitleniyor. Açılışta sessiz senkron, oturum düşünce
  uyarı.
- **Sonraki/önceki bölüm:** mpv içinde `n`/`p` ile geçme (her zaman açık).
  Ayarlardan otomatik sonraki bölüm açılabilir (varsayılan kapalı).
- **Müzik düzeltmesi:** şarkı bilgisi artık gömülü varsayılan sırla imzalı
  çekiliyor — müzikli kayıtlarda üstte şarkı katmanı, `Shift+M` ile
  tarayıcıda açma, mpv OSD'sinde şarkı adı.

## Eklenenler

- **Otomatik sonraki bölüm anahtarı** (Oynatıcı Ayarları).
- **CI kapıları:** fmt, clippy, ölü-kod, bağımlılık ve güvenlik denetimi
  + test kapsamı eşiği.
- `gio` bağımlılığı atıldı, AppImage inceldi.

## Kaldırılanlar

- Yok (kullanıcıya dönük kaldırma yok).

## Bilinen durum

- İntro/outro/müzik verisi tau-video most-sought küratör havuzundan gelir;
  AniSkip (api.aniskip.com) kullanılmıyor. Havuzda müzik satırı olmayan
  kayıtlarda (örn. Frieren S1) şarkı görünmez — sunucu verisi.
- Avatar/banner yükleme canlı doğrulandı ama bu ağaçta kapalı
  (yerel mod saplaması).

# Changelog — v1.2.5 → v1.2.6

## Öne çıkanlar

- **Ana sayfa donma düzeltmesi:** kapak JPEG decode + Bilinear scale artık arka-plan
  işçisinde yapılıyor; ana sayfa açılırken UI bloklanmıyor.
- **Ana sayfaya hero eklendi:** iskelet (skeleton) yükleme, bölümü/filmi doğrulanmış
  animeler öneriliyor, tam genişlik + 1280px kapak.
- **Haberler , Yayın Takvimi ve Keşfet sayfası eklendi:** AnimeciX API ile güncel haberleri ve yayın takvimini takip edebilirsiniz. Keşfet ile yeni animeler keşfediğ izleyebilirsiniz.
- **Arayüz ölçeği serbest oldu:** %100–%125 arası serbest değer (eski %150 tavana çekilir).

## Eklenenler
#### En Önemli Değişiklik: 
- Arayüz baştan aşağı Lowell'in katkıları sayesinde değiştirildi. Daha fazla bilgi için reposuna bakabilirsiniz: https://github.com/Lowell137/animecix-linux

- **Yan ray (sidebar) + daraltma:** sayfalar arası tek tıkla geçiş, daraltınca sadece ikonlar kalır.
- **Arama dialogu:** yazarken bekleyip arar, Enter ilk sonuca gider.
- **Headbar hızlı arama hapı:** üst çubuktan direkt arama, yazınca sonuç penceresi açılır.
- **Devam rafı:** kaldığın bölümler ana sayfada, tek tıkla detaya gidersin.
- **Sayfalı raflar:** uzun listeler sayfa sayfa gezilir, sayaç gösterir.
- **Puan hapı:** detayda TMDB puanı rozet olarak görünür.
- **Sezon ve izlendi filtreleri:** sezona ve izleme durumuna göre süzme.
- **Detay sekmeleri:** Bölümler canlı; Ekip / Benzerler / İncelemeler yakında.
- **Yayın Takvimi:** gün gün yeni bölümler.
- **Keşfet:** tür ve sıralama filtreleriyle tarama, büyük kapaklarla.
- **F1 kısayollar penceresi** ve **F11 tam ekran.**
- **Kuantum responsive kartlar:** pencere büyüyünce kartlar da büyür.
- **Poster-lift hover:** üzerine gelince poster hafifçe kalkar.
  
### DİĞER EKLENEN ŞEYLER
  
- Sürüm-notları popup'ı (changelog).
- Gömülü Adwaita ikonları (GitHub master, 59 SVG) + Adwaita tema kilidi
  + `--check-icons` tanı bayrağı.
- Kapak kalite ayarı etkisine zorunlu önbellek-temizleme popup'ı

## Kaldırılanlar

- Evdeki ortadaki hızlı-arama hapı (headbar hapı duruyor).
- Sayfalar menüsü (`tools_menu`) ve hover-play overlay'i.
- Kapak boru hattından zstd sıkıştırması (ham bayt). 
