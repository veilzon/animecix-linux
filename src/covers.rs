use crate::api::Client;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

fn ensure_size_provider(w: i32, h: i32) {
    static CACHE: Mutex<Option<HashMap<(i32, i32), ()>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if map.contains_key(&(w, h)) {
        return;
    }

    let css = gtk::CssProvider::new();
    css.load_from_data(&format!(
        ".cover-fixed-{w}-{h} {{ \
            min-width:{w}px; max-width:{w}px; \
            min-height:{h}px; max-height:{h}px; \
        }}"
    ));

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    map.insert((w, h), ());
}

pub fn new_sized_picture(w: i32, h: i32) -> gtk::Picture {
    let pic = gtk::Picture::new();
    pic.set_width_request(w);
    pic.set_height_request(h);
    pic.set_hexpand(false);
    pic.set_vexpand(false);
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk::ContentFit::Cover);

    let size_class = match w {
        0..=60 => "cover-thumb",
        61..=130 => "cover-header",
        131..=150 => "cover-shelf",
        _ => "cover-movie-header",
    };

    ensure_size_provider(w, h);
    let fixed_class = format!("cover-fixed-{w}-{h}");
    pic.set_css_classes(&["cover", size_class, &fixed_class]);
    pic
}

pub struct CoverManager {
    client: Arc<Client>,
    cache: Rc<RefCell<HashMap<String, Option<gtk::gdk::Texture>>>>,
    /// Anahtar → yaklaşık doku baytı (RGBA); LRU bayt bütçesi için.
    bytes: Rc<RefCell<HashMap<String, usize>>>,
    waiters: Rc<RefCell<HashMap<String, Vec<gtk::Picture>>>>,
    queue: Arc<Mutex<VecDeque<String>>>,
    active: Rc<Cell<usize>>,
    /// L1 LRU sırası (önde eski). Negatif (None) girdiler de dahildir.
    order: Rc<RefCell<VecDeque<String>>>,
}

/// Çözümlü kapak üst sınırı (adet).
const MAX_L1_COVERS: usize = 250;
/// Çözümlü kapak dokularının toplam bellek bütçesi.
const MAX_L1_BYTES: usize = 32 * 1024 * 1024;

fn texture_bytes_for(w: i32, h: i32) -> usize {
    (w.max(0) as usize)
        .saturating_mul(h.max(0) as usize)
        .saturating_mul(4)
}

fn texture_bytes(t: &gtk::gdk::Texture) -> usize {
    texture_bytes_for(t.width(), t.height())
}

/// Aynı anda çalışan kapak işçisi (donma testi 1'di; 3 paralel + UI-dışı
/// decode ile hem hızlı hem donmasız).
const MAX_COVER_WORKERS: usize = 3;

/// UI-dışı işçide üretilen ham doku: boyutlar + RGBA/RGB pikseller.
/// `Pixbuf`/`Texture` `Send` olmadığından ham bayt taşınır; UI'da
/// `from_bytes` ile sarılır (yalnızca memcpy + doku sargısı).
struct RawCover {
    w: i32,
    h: i32,
    stride: i32,
    has_alpha: bool,
    pixels: Vec<u8>,
}

impl CoverManager {
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            cache: Rc::new(RefCell::new(HashMap::new())),
            bytes: Rc::new(RefCell::new(HashMap::new())),
            waiters: Rc::new(RefCell::new(HashMap::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            active: Rc::new(Cell::new(0)),
            order: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    pub fn clone_ref(&self) -> Self {
        Self {
            client: self.client.clone(),
            cache: self.cache.clone(),
            bytes: self.bytes.clone(),
            waiters: self.waiters.clone(),
            queue: self.queue.clone(),
            active: self.active.clone(),
            order: self.order.clone(),
        }
    }

    /// Bellek önbelleğini sıfırlar (kalite değişimi / wipe öncesi).
    /// Bekleyen kuyruk da düşer; havadaki indirme bitince zararsız
    /// şekilde yeniden kuyruğa girer (anahtarlar kaliteye bağlı).
    pub fn reset(&self) {
        self.cache.borrow_mut().clear();
        self.bytes.borrow_mut().clear();
        self.order.borrow_mut().clear();
        self.waiters.borrow_mut().clear();
        self.queue.lock().unwrap().clear();
        self.active.set(0);
    }

    fn cache_bytes(&self) -> usize {
        self.bytes.borrow().values().copied().sum()
    }

    /// LRU önbelleğini verilen bayt bütçesine çeker. Görüntülenen
    /// `Picture`'lar kendi texture referanslarını tuttuğu için ekrandaki
    /// kapaklar bu işlemden etkilenmez.
    pub fn trim_budget(&self, max_bytes: usize) {
        let mut order = self.order.borrow_mut();
        while self.cache_bytes() > max_bytes {
            let Some(old) = order.pop_front() else {
                break;
            };
            self.cache.borrow_mut().remove(&old);
            self.bytes.borrow_mut().remove(&old);
        }
    }

    /// LRU dokunuşu: anahtarı sona al; adet veya bayt bütçesi taşınca
    /// en eski girdileri at.
    fn lru_touch(&self, key: &str) {
        let mut order = self.order.borrow_mut();
        if let Some(pos) = order.iter().position(|k| k == key) {
            order.remove(pos);
        }
        order.push_back(key.to_string());
        while order.len() > MAX_L1_COVERS || self.cache_bytes() > MAX_L1_BYTES {
            if let Some(old) = order.pop_front() {
                self.cache.borrow_mut().remove(&old);
                self.bytes.borrow_mut().remove(&old);
            } else {
                break;
            }
        }
    }

    pub fn cover_picture(&self, url: Option<&str>, w: i32, h: i32) -> gtk::Picture {
        let pic = new_sized_picture(w, h);
        self.load_cover(url, &pic, w, h);
        pic
    }

    pub fn load_cover(&self, url: Option<&str>, pic: &gtk::Picture, w: i32, h: i32) {
        let Some(url) = url else { return };
        // Kalite ayarı uçta uygulanır: aynı poster farklı boyda ayrı önbelleklenir.
        let url = crate::api::tmdb_sized_url(url, &self.client.current_cover_quality());
        let key = format!("{url}@{w}x{h}");

        // NOT: borrow guard'ı lru_touch'tan ÖNCE düşmeli; lru_touch taşmada
        // aynı cache'e borrow_mut yapar (if-let koşul guard'ı gövde boyu yaşar).
        let hit = self.cache.borrow().get(&key).cloned();
        if let Some(Some(t)) = hit {
            self.lru_touch(&key);
            pic.set_paintable(Some(&t));
            return;
        }
        if let Some(None) = self.cache.borrow().get(&key) {
            return;
        }

        self.waiters
            .borrow_mut()
            .entry(key)
            .or_default()
            .push(pic.clone());
        let mut q = self.queue.lock().unwrap();
        if !q.iter().any(|u| u == &url) {
            q.push_back(url);
        }
        drop(q);
        self.pump_covers();
    }

    /// Baytları bir kez çözüp `Pixbuf` verir (aynı URL'nin her boyu
    /// ayrı decode etmez; bkz. `finish_cover_work`).
    fn decode_pixbuf(bytes: &[u8]) -> Option<gdk_pixbuf::Pixbuf> {
        let loader = gdk_pixbuf::PixbufLoader::new();
        loader.write(bytes).ok()?;
        loader.close().ok()?;
        loader.pixbuf()
    }

    /// Pixbuf'tan hedef boyda kırpılmış pixbuf üretir (oranı koruyup
    /// doldur-kırp). Saf pixbuf işlemi: UI-dışı işçide de çalışır.
    fn crop_to_size(src: &gdk_pixbuf::Pixbuf, w: i32, h: i32) -> Option<gdk_pixbuf::Pixbuf> {
        let (sw, sh) = (src.width() as f64, src.height() as f64);
        if sw <= 0.0 || sh <= 0.0 || w <= 0 || h <= 0 {
            return None;
        }
        let scale = (w as f64 / sw).max(h as f64 / sh);
        let (dw, dh) = ((sw * scale).round() as i32, (sh * scale).round() as i32);
        let pb = src.scale_simple(dw, dh, gdk_pixbuf::InterpType::Bilinear)?;
        let x = ((dw - w) / 2).max(0);
        let y = ((dh - h) / 2).max(0);
        Some(pb.new_subpixbuf(x, y, w.min(dw), h.min(dh)))
    }

    fn pump_covers(&self) {
        let max_workers = MAX_COVER_WORKERS;
        let mut active = self.active.get();

        while active < max_workers {
            let url = self.queue.lock().unwrap().pop_front();
            let Some(url) = url else { break };

            self.active.set(active + 1);
            active += 1;

            let client = self.client.clone();
            let url2 = url.clone();
            // UI tarafında boyut anlık görüntüsü: devirde eklenen
            // bekleyenler iş bitiminde yeniden kuyruğa girer.
            let sizes: Vec<(String, i32, i32)> = self
                .waiters
                .borrow()
                .keys()
                .filter_map(|key| {
                    let rest = key.strip_prefix(&url)?.strip_prefix('@')?;
                    let (ws, hs) = rest.split_once('x')?;
                    Some((key.clone(), ws.parse().ok()?, hs.parse().ok()?))
                })
                .collect();
            let (tx, rx) = std::sync::mpsc::channel::<(String, Vec<(String, Option<RawCover>)>)>();
            std::thread::spawn(move || {
                // Ağ + JPEG decode + Bilinear scale TAMAMI işçide;
                // UI'ya yalnız ham piksel taşınır (donma düzeltmesi).
                let bytes = client.get_bytes(&url2);
                let decoded = bytes.as_deref().and_then(Self::decode_pixbuf);
                let out: Vec<(String, Option<RawCover>)> = match decoded {
                    Some(src) => sizes
                        .into_iter()
                        .map(|(key, w, h)| {
                            let raw = Self::crop_to_size(&src, w, h).map(|pb| RawCover {
                                w: pb.width(),
                                h: pb.height(),
                                stride: pb.rowstride(),
                                has_alpha: pb.has_alpha(),
                                pixels: pb.read_pixel_bytes().to_vec(),
                            });
                            (key, raw)
                        })
                        .collect(),
                    None => sizes.into_iter().map(|(key, _, _)| (key, None)).collect(),
                };
                let _ = tx.send((url2, out));
            });

            let this = self.clone_ref();
            glib::idle_add_local(move || match rx.try_recv() {
                Ok((u, done)) => {
                    this.finish_cover_work(&u, done);
                    let curr = this.active.get();
                    if curr > 0 {
                        this.active.set(curr - 1);
                    }
                    this.pump_covers();
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // İşçi öldü, sonuç gelmeyecek: sayacı indirip kuyruğu
                    // ilerlet (yoksa kapak sırası takılı kalır).
                    let curr = this.active.get();
                    if curr > 0 {
                        this.active.set(curr - 1);
                    }
                    this.pump_covers();
                    glib::ControlFlow::Break
                }
            });
        }
    }

    /// İşçi çıktısını uygular: ham pikseli sar + bekleyen resimlere bas.
    /// UI'daki iş yalnız memcpy + doku sargısıdır. Anlık görüntü dışında
    /// kalıp sonradan eklenen bekleyenler yeniden kuyruğa girer.
    fn finish_cover_work(&self, url: &str, done: Vec<(String, Option<RawCover>)>) {
        let mut waiters = self.waiters.borrow_mut();
        let mut cache = self.cache.borrow_mut();
        let mut touched: Vec<String> = Vec::new();
        for (key, raw) in done {
            let Some(pics) = waiters.remove(&key) else {
                continue;
            };
            match raw {
                Some(r) => {
                    let bytes = glib::Bytes::from(r.pixels.as_slice());
                    let pb = gdk_pixbuf::Pixbuf::from_bytes(
                        &bytes,
                        gdk_pixbuf::Colorspace::Rgb,
                        r.has_alpha,
                        8,
                        r.w,
                        r.h,
                        r.stride,
                    );
                    let t = gtk::gdk::Texture::for_pixbuf(&pb);
                    for p in &pics {
                        p.set_paintable(Some(&t));
                    }
                    let b = texture_bytes(&t);
                    cache.insert(key.clone(), Some(t));
                    self.bytes.borrow_mut().insert(key.clone(), b);
                }
                None => {
                    cache.insert(key.clone(), None);
                    self.bytes.borrow_mut().remove(&key);
                }
            }
            touched.push(key);
        }
        drop(cache);
        for k in &touched {
            self.lru_touch(k);
        }
        // Devirde eklenen bekleyenler (anlık görüntü dışı) yeniden kuyruğa.
        let mut requeue = false;
        for key in waiters.keys() {
            if key
                .strip_prefix(url)
                .and_then(|r| r.strip_prefix('@'))
                .is_some()
            {
                requeue = true;
                break;
            }
        }
        drop(waiters);
        if requeue {
            self.queue.lock().unwrap().push_back(url.to_string());
            self.pump_covers();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_workers_are_sequential() {
        // Donma düzeltmesi 1 işçiydi; 3 paralel + UI-dışı yük ile denge.
        assert_eq!(
            MAX_COVER_WORKERS, 3,
            "işçi sayısı değişirse kuyruk hesabı gözden geçir"
        );
    }

    #[test]
    fn texture_bytes_uses_rgba_size() {
        assert_eq!(texture_bytes_for(10, 20), 800);
        assert_eq!(texture_bytes_for(0, 20), 0);
        assert_eq!(texture_bytes_for(-1, 20), 0);
    }

    #[test]
    fn reset_clears_mem_state() {
        let m = CoverManager::new(std::sync::Arc::new(Client::new()));
        m.cache.borrow_mut().insert("k".to_string(), None);
        m.bytes.borrow_mut().insert("k".to_string(), 123);
        m.order.borrow_mut().push_back("k".to_string());
        m.waiters.borrow_mut().insert("k".to_string(), Vec::new());
        m.queue.lock().unwrap().push_back("u".to_string());
        m.active.set(1);
        m.reset();
        assert!(m.cache.borrow().is_empty(), "cache boşalmalı");
        assert!(m.bytes.borrow().is_empty(), "bayt sayacı boşalmalı");
        assert!(m.order.borrow().is_empty(), "sıra boşalmalı");
        assert!(m.waiters.borrow().is_empty(), "bekleyenler boşalmalı");
        assert!(m.queue.lock().unwrap().is_empty(), "kuyruk boşalmalı");
        assert_eq!(m.active.get(), 0, "sayaç sıfırlanmalı");
    }
}
