# AnimeciX Veri Yönetimi — Dışa/İçe Aktarma ve Veriyi Koruyarak Kaldırma

Tarih: 2026-11-20  
Durum: öneri (onay bekliyor)

## Amaç

Kullanıcıların izleme verilerini yedekleyip başka bir kuruluma taşıyabilmesi;
uygulamayı kaldırırken kişisel verilerin korunması. Veri işlemleri ayrı bir
`src/data_management.rs` modülünde toplanacak; UI yalnızca dosya seçimi ve
onay akışını yönetecek.

## Kapsam

Yedeğe girenler:

- İzleme durumu: geçmiş, favoriler, maraton, izlenen bölümler, ilerleme,
  tercih edilen kaynak.
- Ayarlar: tema, oynatıcı, indirme, Discord, takip vb.
- Yayın takibi listesi (`follows.json` içindeki `items`).

Yedeğe girmeyenler:

- Kapak/API önbelleği, indirilen video dosyaları.
- İndirme kuyruğu (`downloads.json`): cihaza özel yollar ve URL'ler içerir.
- Gizli/cihaza özel ayarlar: `cf_clearance`, `official_skip_secret`,
  `download_dir`, `discord_rpc_socket`.
- Takip bildirim defteri (`ledger.json`): yeni cihazda bildirimleri
  bastırmaması için taşınmaz.

## Yedek formatı

Tek JSON dosyası; uzantı `.json`, önerilen ad
`animecix-yedek-YYYY-AA-GG.json`.

```json
{
  "format": "animecix-backup",
  "format_version": 1,
  "app_version": "1.4.4",
  "exported_at": 1795112220,
  "state": { ... },
  "settings": { ... },
  "follows": [ ... ]
}
```

- Dosya `0600` izinleriyle atomik yazılır.
- Boyut sınırı: 10 MB. Geçersiz/bozuk/yanlış format reddedilir.
- Gelecekteki alanlar `serde` varsayılanlarıyla uyumlu okunur.

## Dışa aktarma

1. Kullanıcı `Verileri Dışa Aktar` düğmesine basar.
2. Kaydetme dosya seçici açılır; varsayılan ad önerilir.
3. `data_management::export_bundle(path)` yedeği yazar.
4. Başarı mesajında dosya yolu gösterilir.

## İçe aktarma

1. Kullanıcı `Verileri İçe Aktar` düğmesine basar ve dosyayı seçer.
2. Dosya doğrulanır; özet gösterilir: yedek tarihi, uygulama sürümü,
   favori/izlenen/geçmiş/maraton/takip sayıları.
3. Kullanıcı iki davranıştan birini seçer:
   - **Tam geri yükle:** mevcut veri ve ayarların yerine geçer.
   - **Birleştir:** izleme verileri ve takip listesi birleştirilir; mevcut
     ayarlar korunur.
4. Her iki modda da önce mevcut durum
   `~/.local/share/animecix/backups/` altına otomatik yedeklenir; son 5
   otomatik yedek tutulur.
5. İçe aktarma sonrası takip arka plan süreci durdurulur, autostart yeni
   ayarlara göre düzenlenir ve uygulama yeniden başlatılır.

## Önceki yedeklere dönüş

`Veri Yönetimi` içinde **Önceki Yedekler** bölümü bulunur:

- `~/.local/share/animecix/backups/` altındaki en yeni 5 otomatik yedek
  listelenir (tarih, boyut ve yedek türü: içe aktarma öncesi / sıfırlama
  öncesi).
- Her satırda `Geri Yükle` düğmesi vardır. Geri yükleme de normal içe
  aktarma gibi önce özet ve **Tam geri yükle / Birleştir** seçimini gösterir.
- Geri yüklemeden önce mevcut durum tekrar otomatik yedeklenir; böylece
  yanlış geri yükleme de geri alınabilir.
- **Tüm Verileri Sıfırla** işlemi, canlı veriyi ve önbelleği temizler ama
  otomatik yedekleri **silmez**. Sıfırlama öncesi ayrıca bir güvenlik yedeği
  oluşturulur.
- Yedekleri tamamen temizlemek isteyen kullanıcı için listebaşlığında
  `Tüm Yedekleri Sil` eylemi bulunur.

### Birleştirme kuralları

- Favoriler: `id` bazında birleşim; yerel sıra korunur.
- Geçmiş: `title.id` bazında en yeni `ts` kazanır; en fazla 30 kayıt.
- İzlenenler: `(title, season, episode)` bazında yeni `watched_at` kazanır.
- İlerleme: çakışmada yerel kayıt korunur; yedekte olup yerelde olmayan
  anahtarlar eklenir.
- Maraton: `title.id` bazında yerel kayıt korunur; eksikler eklenir.
- Tercih edilen kaynak: çakışmada yerel kazanır.
- Takip listesi: `title_id` bazında birleşim; çakışmada yerel kayıt korunur.
- İpucu/görülme bayrakları: mantıksal VEYA.

### Ayar geri yükleme

Tam geri yüklemede yedekteki ayarlar uygulanır; ancak mevcut gizli/cihaza
özel alanlar korunur: `cf_clearance`, `official_skip_secret`,
`download_dir`, `discord_rpc_socket`. Birleştirme modunda ayarlar hiç
değiştirilmez.

## Veriyi koruyarak kaldırma

Veri Yönetimi bölümü iki ayrı eylem sunar:

- **Uygulamayı Kaldır (Verileri Koru):** paketi/uygulamayı kaldırır;
  `state.json`, `settings.json`, `follows.json`, yedekler ve önbellek
  korunur. Flatpak'ta uygulama kendi kendini kaldıramadığı için terminal
  komutu gösterilir.
- **Tüm Verileri Sıfırla:** yalnız AnimeciX verilerini siler; uygulama
  kurulu kalır.

Mevcut "sıfırla + kaldır" birleşik akışı kaldırılır. Bu, yanlışlıkla veri
silmeyi önler. Paketlerin `postrm`/`post_remove` betikleri zaten kullanıcı
verisine dokunmuyor; yalnızca uygulama içi çağrıdaki `wipe_all_data`
kaldırma yolundan çıkarılır.

`Tüm Verileri Sıfırla` ayrıca `downloads.json` dosyasını temizler; otomatik
yedekleri korur. Yedekler yalnızca `Tüm Yedekleri Sil` ile kaldırılır.

## UI

`Ayarlar → Veri Yönetimi` grubu:

- `Verileri Dışa Aktar` + `Dışa Aktar` düğmesi.
- `Verileri İçe Aktar` + `İçe Aktar` düğmesi.
- `Önceki Yedekler`: son 5 otomatik yedek, `Geri Yükle` düğmeleri ve
  `Tüm Yedekleri Sil`.
- `Uygulamayı Kaldır (Verileri Koru)` + `Kaldır` düğmesi.
- `Tüm Verileri Sıfırla` + kırmızı `Sıfırla` düğmesi.

## Güvenlik

- Yedeklerde oturum bileti ve resmi API sırrı yok.
- Yedek dosyaları `0600`.
- İçe aktarma öncesi otomatik yedek; başarısız içe aktarmada mevcut veri
  değişmez (önce doğrula, sonra yaz).
- Hata mesajlarında dosya içeriği yazılmaz.

## Testler

- Yedek serileştirme/geri okuma ve sürüm/format doğrulama.
- Gizli ve cihaza özel ayarların dışa aktarılmadığı; tam geri yüklemede
  mevcut değerlerin korunduğu.
- Birleştirme: favori, geçmiş, izlenen, ilerleme, maraton, takip ve bayrak
  kuralları.
- Otomatik yedek dosyası, listeleme ve son 5 yedek sınırı.
- Önceki yedeği geri yükleme ve geri yükleme öncesi yeni yedek oluşturma.
- `Tüm Verileri Sıfırla`nın indirme kuyruğunu temizleyip yedekleri
  koruduğu; `Tüm Yedekleri Sil`in yedekleri kaldırdığı.
- Mevcut testlerin tamamı.

## Yapılmayanlar

- Kapak/video/önbellek taşıma.
- İndirme kuyruğunu taşıma.
- Bulut yedekleme veya zamanlanmış (tarihli) yedekleme. Yalnızca içe
  aktarma/sıfırlama öncesi otomatik yedekler ve manuel dışa aktarma vardır.
