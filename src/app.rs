use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::api::{self, Client, Episode, Title};
use crate::covers::CoverManager;

use crate::ui::components;
use crate::ui::episodes_view;
use crate::ui::operation_stack::{OperationStack, OperationState};
use crate::ui::views;

mod pages;
mod playback;

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Welcome,
    Home,
    Favs,
    Marathon,
    History,
    Settings,
    Search,
    Downloads,
    Kesfet,
    Calendar,
    News,
    Episodes {
        title: Title,
        eps: Vec<Episode>,
    },
    Movie {
        title: Title,
        eps: Vec<Episode>,
    },
    /// Hesap sayfası (giriş formu / profil).
    Account,
    /// Gerçek sayfa değil: sidebar satırı direkt kısayol penceresini açar.
    Shortcuts,
}

/// Gezinme geçmişi üst sınırı: uzun oturumlarda bölüm listelerinin
/// sınırsız klonlanmasını önler.
const MAX_PAGE_HISTORY: usize = 30;
fn push_page_history(st: &mut Vec<Page>, page: Page) {
    if st.last() == Some(&page) {
        return;
    }
    st.push(page);
    if st.len() > MAX_PAGE_HISTORY {
        st.remove(0);
    }
}

/// Çözüm paketi: oynatma adayları + video öncesi çözülmüş atlama planı.
pub struct PlaySources {
    pub candidates: Vec<String>,
    pub fast_embeds: Vec<String>,
    pub fallback_embeds: Vec<String>,
    pub plan: Option<crate::skip::SkipPlan>,
}

/// Oynatma açılış satırına gönderilen olaylar (yalnızca ana iş parçacığı işler).
pub(crate) enum PlayEvent {
    DismissOpening(u64),
    Info(u64, String),
    Failed(u64, String),
}

/// Oynatma sonunda açılış satırının terminal durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlayTerminal {
    Done,
    Failed,
}

pub(crate) fn play_terminal(ever_media: bool) -> PlayTerminal {
    if ever_media {
        PlayTerminal::Done
    } else {
        PlayTerminal::Failed
    }
}

/// 10 saniyelik açılış minimumu için kalan süre; süre dolduysa `None`.
pub(crate) fn opening_finish_delay(elapsed: std::time::Duration) -> Option<std::time::Duration> {
    let minimum = std::time::Duration::from_secs(10);
    if elapsed < minimum {
        Some(minimum - elapsed)
    } else {
        None
    }
}

pub enum Msg {
    Cats(Result<Vec<api::Category>, String>),
    Search(Result<Vec<Title>, String>),
    News(u32, Result<(Vec<api::NewsItem>, usize, u32), String>),
    Calendar(Result<Vec<api::CalendarDay>, String>),
    Discover(Result<(Vec<Title>, usize, u32), String>),
    Eps(Title, Result<Vec<Episode>, String>),
    Play(Title, Episode, Result<PlaySources, String>),
    PlayLocal {
        title: Title,
        ep: Episode,
        path: String,
        plan: Option<crate::skip::SkipPlan>,
    },
    NextEp(Title, Result<Episode, String>),
    FansubsLoaded {
        title: Title,
        ep: Episode,
        fansubs: Result<Vec<api::FansubInfo>, String>,
        default_template: Option<i64>,
    },
    DlLists {
        title: Title,
        quality: String,
        items: Vec<(Episode, Vec<api::FansubInfo>)>,
        is_single: bool,
    },
    DlBatchResolved(Vec<crate::download::DownloadRecord>, Vec<String>, bool),
    PlayQualities {
        title: Title,
        ep: Episode,
        fs: api::FansubInfo,
        rest: Vec<api::FansubInfo>,
        quals: Result<Vec<crate::play_quality::PlayQuality>, String>,
    },
}

pub(crate) struct EpisodeBar {
    pub bar: gtk::ProgressBar,
    pub label: gtk::Label,
    pub watched: Rc<RefCell<bool>>,
}

pub(crate) fn fmt_ep_time(s: f64) -> String {
    let s = s as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub(crate) fn episode_bar_display(watched: bool, pos: f64, dur: f64) -> (f64, Option<String>) {
    if watched {
        return (1.0, Some("İzlendi".to_string()));
    }
    if dur > 0.0 && pos > 1.0 {
        (
            (pos / dur).clamp(0.0, 1.0),
            Some(format!("{} / {}", fmt_ep_time(pos), fmt_ep_time(dur))),
        )
    } else {
        (0.0, None)
    }
}

pub struct App {
    pub window: adw::ApplicationWindow,
    pub stack: gtk::Stack,
    pub back_btn: gtk::Button,
    pub title_label: gtk::Label,
    pub ops: OperationStack,
    /// Tek film tonu provider'ı; her sayfada yeni provider eklemeyi önler.
    pub movie_tint_css: gtk::CssProvider,
    pub client: Arc<Client>,
    pub covers: CoverManager,
    pub page_history: Rc<RefCell<Vec<Page>>>,
    pub cats: Rc<RefCell<Vec<api::Category>>>,
    pub search_results: Rc<RefCell<Vec<Title>>>,
    pub settings: Rc<RefCell<api::Settings>>,
    pub progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    pub progress_bars: Rc<RefCell<HashMap<String, EpisodeBar>>>,
    pub dl_rows: Rc<RefCell<HashMap<String, crate::ui::downloads_view::DlRow>>>,
    pub opening_op_id: Rc<RefCell<Option<String>>>,
    pub opening_op_shown_at: Rc<RefCell<Option<std::time::Instant>>>,
    pub play_gen: Arc<std::sync::atomic::AtomicU64>,
    /// Son başlatılan mpv sürecinin PID'i (0 = yok). Yeni oynatmada eskisi
    /// sonlandırılır; süreçler üst üste birikmez.
    pub current_mpv_pid: Arc<std::sync::atomic::AtomicU64>,
    pub home_acts: Rc<RefCell<Vec<Option<usize>>>>,
    pub dl_manager: crate::download::DownloadManager,
    /// Bölüm hızlı-arama tuşu: sayfa başına tek controller (birikmeyi önler).
    pub ep_search_controller: Rc<RefCell<Option<gtk::EventControllerKey>>>,
    /// Yan ray: (sayfa, satır, etiket, tam ad) + daraltma durumu
    /// (Lowell137/animecix-linux SideItem birebir).
    pub sidebar_rows: Rc<RefCell<Vec<(Page, gtk::Button, gtk::Label, String)>>>,
    pub sidebar_collapsed: Rc<Cell<bool>>,
    pub sidebar_box: gtk::Box,
    /// Spotlight yüksekliği (pencereye göre; Lowell hero_h_for_window).
    pub hero_h: Rc<Cell<i32>>,
    /// Ev görünümü kirlendi mi? (view-cache geçersizleme bayrağı)
    pub home_dirty: Rc<Cell<bool>>,
    /// Raf kart boyu + sütun sayısı (kuantum responsive).
    pub card_w: Rc<Cell<i32>>,
    pub grid_cols: Rc<Cell<u32>>,
    /// Haberler önbelleği: (öğeler, toplam, son_sayfa) + istenen sayfa.
    pub news: Rc<RefCell<Option<(Vec<api::NewsItem>, usize, u32)>>>,
    pub news_page: Rc<Cell<u32>>,
    /// Takvim önbelleği.
    pub calendar: Rc<RefCell<Option<Vec<api::CalendarDay>>>>,
    /// Keşfet filtresi + sonuç önbelleği: (öğeler, toplam, son_sayfa).
    pub discover_filter: Rc<RefCell<api::DiscoverFilter>>,
    pub discover: Rc<RefCell<Option<(Vec<Title>, usize, u32)>>>,
    /// Sunucu geçmişi (sessiz senkronla dolar; devam/geçmiş birleşir).
    pub server_history: Rc<RefCell<Vec<crate::api::ServerEntry>>>,
    /// Arayüz ölçeği CSS sağlayıcısı (sürekli 100-125 değeri; kalıcı).
    pub scale_css: gtk::CssProvider,
}

/// Spotlight yüksekliği: pencere boyunun %52'si (20px kuantum) ile
/// sütun tabanının büyüğü, 360–560 kelepçeli (Lowell hero_h_for_window birebir).
fn hero_h_for_window(win_h: i32, cols: u32) -> i32 {
    let from_h = (win_h as f32 * 0.52) as i32 / 20 * 20;
    let from_c = 260 + cols.max(3).min(8) as i32 * 20;
    from_h.max(from_c).clamp(360, 560)
}

/// Kart kuantumu: genişlikten sütun (3-8) + kart boyu (140-220, 10px
/// kuantum). Pencere büyüyünce kartlar da büyür (sabit 140 dönemi kapandı).
fn card_quantum(win_w: i32) -> (u32, i32) {
    let cols = ((win_w / 180).max(3).min(8)) as u32;
    let raw = (win_w - 24 - (cols as i32 - 1) * 16) / cols as i32;
    let w = (raw / 10 * 10).clamp(140, 220);
    (cols, w)
}

/// Spotlight seçimi (saf): havuzdan slot_count başlık. Şimdilik karıştır +
/// ilk N; algoritma değişince sadece bu fonksiyonun içi değişir.
fn pick_spotlight(pool: &[Title], slot_count: usize, seed: u64) -> Vec<Title> {
    let mut idx: Vec<usize> = (0..pool.len()).collect();
    // Basit LCG shuffle (harici crate yok).
    let mut s = seed | 1;
    for i in (1..idx.len()).rev() {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (s >> 33) as usize % (i + 1);
        idx.swap(i, j);
    }
    idx.into_iter()
        .take(slot_count)
        .map(|i| pool[i].clone())
        .collect()
}

/// Oturum tohumu bir kez: her açılışta farklı sıra, oturum boyu sabit
/// (yeniden kurulumlarda hero zıplamaz).
fn spot_seed() -> u64 {
    static SPOT_SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *SPOT_SEED.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b9)
    })
}
/// Hero havuzu: tüm kategorilerden birleşik + id-dedup + görselli.
/// Sıralama: rating ≥7 bonus, son 15 yıl öne (eskiler elenmez, arkaya).
/// Bölüm filtresi (yumuşak): kesin-boş anime (`episode_count == Some(0)`)
/// elenir; bilinmeyen (None) arkaya itilir, filmler muaf tutulur.
/// Karıştırma `pick_spotlight`'ta; havuz darsa bile boş dönmez.
/// Gerçek oynatılabilirlik arka-plan `episodes()` ön-kontrolüyle netleşir
/// (`build_spotlight_async`); burası yalnız ucuz eleme yapar.
fn hero_pool(cats: &[api::Category]) -> Vec<Title> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut scored: Vec<(i64, Title)> = Vec::new();
    for cat in cats {
        for t in &cat.items {
            if !seen.insert(t.id) {
                continue;
            }
            if t.backdrop.is_none() && t.poster.is_none() {
                continue;
            }
            let is_movie = t.title_type.as_deref() == Some("movie");
            if !is_movie && t.episode_count == Some(0) {
                continue;
            }
            let mut score = 0i64;
            if t.rating.unwrap_or(0.0) >= 7.0 {
                score += 100;
            }
            match t.year {
                Some(y) if y >= 2011 => score += 50,
                None => score -= 10,
                _ => {}
            }
            if !is_movie && t.episode_count.is_none() {
                score -= 30;
            }
            scored.push((score, t.clone()));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, t)| t).collect()
}

/// Geri-dönüş geçiş bayrağı (switch içinde tüketilir).
static BACK_ANIM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// İnternet durumu önbelleği (60sn TTL). İlk boyama asla bloklamaz:
/// soğukken iyimser (çevrimiçi) çizilir, gerçek kontrol `App::new`
/// sonundaki warmer thread ile yapılıp rozet tazelenir.
fn internet_cache() -> &'static std::sync::Mutex<(
    Option<std::time::Instant>,
    Option<crate::api::InternetStatus>,
)> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<
        Mutex<(
            Option<std::time::Instant>,
            Option<crate::api::InternetStatus>,
        )>,
    > = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new((None, None)))
}

/// Taze önbellek varsa klonu (yoksa None; ağa dokunmaz).
fn internet_cached() -> Option<crate::api::InternetStatus> {
    let guard = internet_cache().lock().ok()?;
    let (at, st) = (&guard.0, &guard.1);
    match (at, st) {
        (Some(at), Some(st)) if at.elapsed() < std::time::Duration::from_secs(60) => {
            Some(st.clone())
        }
        _ => None,
    }
}

fn store_internet_status(st: crate::api::InternetStatus) {
    if let Ok(mut guard) = internet_cache().lock() {
        *guard = (Some(std::time::Instant::now()), Some(st));
    }
}

/// Önbellek taze mi? (ilk boyama kararı; ağa dokunmaz).
fn internet_cache_fresh() -> bool {
    internet_cached().is_some()
}

pub(crate) fn resolve_upscale_shader(name: &str) -> Option<String> {
    use std::sync::Mutex;
    use std::sync::OnceLock;

    // Embedded shader içeriği (binary'ye gömülü, AppImage extract'ten bağımsız).
    fn embedded(name: &str) -> Option<&'static str> {
        match name {
            "Anime4K_Upscale_CNN_x2_M.glsl" => Some(include_str!(
                "../assets/upscale/Anime4K_Upscale_CNN_x2_M.glsl"
            )),
            "Anime4K_Upscale_CNN_x2_UL.glsl" => Some(include_str!(
                "../assets/upscale/Anime4K_Upscale_CNN_x2_UL.glsl"
            )),
            "Anime4K_Upscale_DTD_x2.glsl" => Some(include_str!(
                "../assets/upscale/Anime4K_Upscale_DTD_x2.glsl"
            )),
            "Anime4K_Upscale_Original_x2.glsl" => Some(include_str!(
                "../assets/upscale/Anime4K_Upscale_Original_x2.glsl"
            )),
            _ => None,
        }
    }

    // Her isim için sadece bir kez temp'e yaz.
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(p) = cache.lock().unwrap().get(name).cloned() {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }

    // Gömülü içeriği temp'e yaz.
    if let Some(src) = embedded(name) {
        let dir = std::env::temp_dir().join("animecix-upscale");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        if std::fs::write(&path, src).is_ok() {
            let p = path.to_string_lossy().into_owned();
            cache.lock().unwrap().insert(name.to_string(), p.clone());
            return Some(p);
        }
    }

    // Fallback: disk üzerinde ara (dev/Flatpak/sistem kurulumları için).
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(ad) = std::env::var("APPDIR") {
        if !ad.is_empty() {
            candidates.push(
                std::path::Path::new(&ad)
                    .join("usr/share/animecix/assets/upscale")
                    .join(name),
            );
        }
    }
    if std::path::Path::new("/app").exists() {
        candidates.push(std::path::PathBuf::from("/app/share/animecix/assets/upscale").join(name));
    }
    candidates.push(std::path::PathBuf::from("/usr/share/animecix/assets/upscale").join(name));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("usr/share/animecix/assets/upscale").join(name));
            candidates.push(parent.join("assets/upscale").join(name));
            candidates.push(parent.join("../../assets/upscale").join(name));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

/// İndirilenler kaydırma konumunu geri yükler. Yerleşim (allocate) henüz
/// bitmemişse (üst-sınır 0) en fazla 3 idle denemesi yapar.
fn restore_downloads_scroll(stack: gtk::Stack, value: f64, attempt: u8) {
    glib::idle_add_local_once(move || {
        let mut retry = false;
        if let Some(w) = stack.child_by_name("downloads") {
            if let Ok(s) = w.downcast::<gtk::ScrolledWindow>() {
                let adj = s.vadjustment();
                if adj.upper() <= 0.0 && attempt < 3 {
                    retry = true;
                } else {
                    let max = (adj.upper() - adj.page_size()).max(0.0);
                    adj.set_value(value.min(max));
                }
            }
        }
        if retry {
            restore_downloads_scroll(stack.clone(), value, attempt + 1);
        }
    });
}

/// Raf başlığı + pager (ev/devam ortak; Lowell pager_head uyarlaması).
/// Dönen `wire` FlowBox'a bağlanır: görünürlük penceresi + sayaç + oklar.
fn build_shelf_pager(
    title: &str,
    total: usize,
    per: usize,
) -> (gtk::Box, Rc<dyn Fn(&gtk::FlowBox)>) {
    let per = per.max(1);
    let pages = total.div_ceil(per).max(1);
    let page = Rc::new(Cell::new(0usize));
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let shelf_title = gtk::Label::new(Some(title));
    shelf_title.add_css_class("shelf-title");
    shelf_title.set_xalign(0.0);
    shelf_title.set_hexpand(true);
    shelf_title.set_margin_start(4);
    head.append(&shelf_title);
    let counter = gtk::Label::new(None);
    counter.add_css_class("dim-label");
    counter.set_valign(gtk::Align::Center);
    head.append(&counter);
    let prev_btn = gtk::Button::from_icon_name("go-previous-symbolic");
    prev_btn.add_css_class("flat");
    prev_btn.add_css_class("circular");
    let next_btn = gtk::Button::from_icon_name("go-next-symbolic");
    next_btn.add_css_class("flat");
    next_btn.add_css_class("circular");
    head.append(&prev_btn);
    head.append(&next_btn);
    let slot: Rc<RefCell<Option<gtk::FlowBox>>> = Rc::new(RefCell::new(None));
    let apply = {
        let counter = counter.clone();
        let prev_btn = prev_btn.clone();
        let next_btn = next_btn.clone();
        let page = page.clone();
        let slot = slot.clone();
        Rc::new(move || {
            let Some(flow) = slot.borrow().clone() else {
                return;
            };
            let p = page.get().min(pages.saturating_sub(1));
            page.set(p);
            for i in 0..total {
                if let Some(ch) = flow.child_at_index(i as i32) {
                    ch.set_visible(i >= p * per && i < p * per + per);
                }
            }
            let a = if total == 0 { 0 } else { p * per + 1 };
            let b = ((p + 1) * per).min(total);
            counter.set_text(&format!("{a}–{b} / {total}"));
            prev_btn.set_sensitive(p > 0);
            next_btn.set_sensitive(p + 1 < pages);
        })
    };
    {
        let apply_p = apply.clone();
        let page_p = page.clone();
        prev_btn.connect_clicked(move |_| {
            page_p.set(page_p.get().saturating_sub(1));
            apply_p();
        });
    }
    {
        let apply_n = apply.clone();
        let page_p = page.clone();
        next_btn.connect_clicked(move |_| {
            page_p.set(page_p.get() + 1);
            apply_n();
        });
    }
    // Tek sayfada oklar gizlenir.
    if pages <= 1 {
        prev_btn.set_visible(false);
        next_btn.set_visible(false);
        counter.set_visible(false);
    }
    let wire = {
        let slot = slot.clone();
        let apply = apply.clone();
        Rc::new(move |flow: &gtk::FlowBox| {
            *slot.borrow_mut() = Some(flow.clone());
            apply();
        })
    };
    (head, wire)
}

impl App {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let client = Arc::new(Client::new());

        // Açılışta state TEK kez okunur (hydrate pahalı olabilir; iki ayrı
        // load_state çağrısı maliyeti ikiye katlardı).
        let init_state = client.load_state();
        let _welcome_seen = init_state.welcome_seen;
        let init_progress = init_state.progress;

        {
            let cl = client.clone();
            std::thread::spawn(move || {
                cl.warmup();
                let _ = cl.home_lists();
            });
        }
        let welcome_seen = init_state.welcome_seen;

        let header = adw::HeaderBar::new();
        let title_label = gtk::Label::new(Some("AnimeciX"));
        title_label.add_css_class("title-2");
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.set_max_width_chars(28);
        header.set_title_widget(Some(&title_label));

        let back_btn = gtk::Button::with_label("‹ Geri");
        back_btn.add_css_class("flat");
        back_btn.set_tooltip_text(Some("Geri"));
        back_btn.set_visible(false);
        header.pack_start(&back_btn);
        // Sayfalar butonu App::new sonunda (goto kablosu Rc gerektirir) eklenir.

        let main_stack = gtk::Stack::new();
        main_stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        main_stack.set_transition_duration(220);
        main_stack.set_vexpand(true);
        main_stack.set_hexpand(true);

        // Yan ray kabuğu (satırlar App::new sonunda dolar; Lowell uyarlaması).
        let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        sidebar_box.set_margin_top(8);
        sidebar_box.set_margin_bottom(8);
        sidebar_box.set_margin_start(8);
        sidebar_box.set_margin_end(4);
        sidebar_box.set_valign(gtk::Align::Fill);
        sidebar_box.set_vexpand(true);
        sidebar_box.set_halign(gtk::Align::Start);
        sidebar_box.set_hexpand(false);
        sidebar_box.set_width_request(164);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_hexpand(true);
        content.append(&header);
        content.append(&main_stack);
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&sidebar_box);
        let sep = gtk::Separator::new(gtk::Orientation::Vertical);
        body.append(&sep);
        body.append(&content);

        let ops = OperationStack::new();

        let root_overlay = gtk::Overlay::new();
        root_overlay.set_child(Some(&body));
        root_overlay.add_overlay(ops.widget());

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("AnimeciX")
            .default_width(1280)
            .default_height(800)
            .content(&root_overlay)
            .build();

        let initial_page = if welcome_seen {
            Page::Home
        } else {
            Page::Welcome
        };

        let covers = CoverManager::new(client.clone());

        let movie_tint_css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gtk::prelude::WidgetExt::display(&window),
            &movie_tint_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let (dl_tx, dl_rx) = std::sync::mpsc::channel::<crate::download::UiEvent>();
        let dl_manager =
            crate::download::DownloadManager::new(crate::download::queue_file_path(), dl_tx);
        // Kayıtlı paralellik açılışta motora işlenir (eski dosya → 1).
        dl_manager.set_max_parallel(client.load_settings().max_parallel_downloads as usize);

        let app_inst = Rc::new(Self {
            window,
            stack: main_stack,
            back_btn,
            title_label,
            ops,
            movie_tint_css,
            client: client.clone(),
            covers,
            page_history: Rc::new(RefCell::new(vec![initial_page.clone()])),
            cats: Rc::new(RefCell::new(Vec::new())),
            search_results: Rc::new(RefCell::new(Vec::new())),
            settings: Rc::new(RefCell::new(client.load_settings())),
            progress: Rc::new(RefCell::new(init_progress)),
            progress_bars: Rc::new(RefCell::new(HashMap::new())),
            dl_rows: Rc::new(RefCell::new(HashMap::new())),
            opening_op_id: Rc::new(RefCell::new(None)),
            opening_op_shown_at: Rc::new(RefCell::new(None)),
            play_gen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            current_mpv_pid: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            home_acts: Rc::new(RefCell::new(Vec::new())),
            dl_manager,
            ep_search_controller: Rc::new(RefCell::new(None)),
            sidebar_rows: Rc::new(RefCell::new(Vec::new())),
            sidebar_collapsed: Rc::new(Cell::new(false)),
            sidebar_box,
            hero_h: Rc::new(Cell::new(hero_h_for_window(800, 5))),
            home_dirty: Rc::new(Cell::new(true)),
            card_w: Rc::new(Cell::new(card_quantum(1280).1)),
            grid_cols: Rc::new(Cell::new(card_quantum(1280).0)),
            news: Rc::new(RefCell::new(None)),
            news_page: Rc::new(Cell::new(1)),
            calendar: Rc::new(RefCell::new(None)),
            discover_filter: Rc::new(RefCell::new(api::DiscoverFilter {
                page: 1,
                ..Default::default()
            })),
            discover: Rc::new(RefCell::new(None)),
            server_history: Rc::new(RefCell::new(Vec::new())),
            scale_css: Self::make_scale_css(),
        });
        {
            // Yan ray satırları (Lowell sidebar uyarlaması).
            let pages: [(Page, &str, &str); 10] = [
                (Page::Home, "go-home-symbolic", "Ana Sayfa"),
                (Page::Kesfet, "view-grid-symbolic", "Keşfet"),
                (Page::Favs, "starred-symbolic", "Favoriler"),
                (
                    Page::Marathon,
                    "media-playlist-consecutive-symbolic",
                    "Maraton",
                ),
                (Page::History, "document-open-recent-symbolic", "Geçmiş"),
                (Page::Calendar, "x-office-calendar-symbolic", "Takvim"),
                (Page::News, "view-list-symbolic", "Haberler"),
                (Page::Downloads, "folder-download-symbolic", "İndirilenler"),
                (Page::Settings, "emblem-system-symbolic", "Ayarlar"),
                (Page::Shortcuts, "input-keyboard-symbolic", "Kısayollar"),
            ];
            // Satır kurucu: Hesap satırı da buradan geçer (Daralt üstü).
            let add_row = |p: Page, icon: &str, name: &str| {
                // Satır yapısı Lowell side_row birebir (marjlar iç kutuda).
                let btn = gtk::Button::new();
                btn.add_css_class("flat");
                btn.add_css_class("side-row");
                btn.set_tooltip_text(Some(name));
                btn.set_halign(gtk::Align::Fill);
                let inner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                inner.set_margin_top(6);
                inner.set_margin_bottom(6);
                inner.set_margin_start(10);
                inner.set_margin_end(10);
                let img = gtk::Image::from_icon_name(icon);
                img.set_valign(gtk::Align::Center);
                inner.append(&img);
                let lbl = gtk::Label::new(Some(name));
                lbl.set_xalign(0.0);
                lbl.set_hexpand(true);
                inner.append(&lbl);
                btn.set_child(Some(&inner));
                let inst = app_inst.clone_ref();
                let pp = p.clone();
                if pp == Page::Shortcuts {
                    // Sayfa değil: direkt kısayol penceresi (tarihçeye yazılmaz).
                    btn.connect_clicked(move |_| {
                        let s = inst.settings.borrow();
                        crate::ui::shortcuts::present(
                            &inst.window,
                            &s.search_shortcut,
                            &s.quick_search_shortcut,
                            s.quick_search_enabled,
                        );
                    });
                } else {
                    btn.connect_clicked(move |_| {
                        inst.open_data_page(&pp);
                    });
                }
                app_inst.sidebar_box.append(&btn);
                app_inst
                    .sidebar_rows
                    .borrow_mut()
                    .push((p, btn, lbl, name.to_string()));
            };
            for (p, icon, name) in pages {
                add_row(p, icon, name);
            }
            let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
            spacer.set_vexpand(true);
            spacer.set_hexpand(false);
            app_inst.sidebar_box.append(&spacer);
            // Hesap satırı bu derlemede yok (yerel mod).
            // Daraltma satırı: diğer satırlarla aynı yapıda (hizalama otomatik).
            let collapse_btn = gtk::Button::new();
            collapse_btn.add_css_class("flat");
            collapse_btn.add_css_class("side-row");
            collapse_btn.set_tooltip_text(Some("Yan menüyü daralt/genişlet"));
            collapse_btn.set_halign(gtk::Align::Fill);
            let collapse_inner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            collapse_inner.set_margin_top(6);
            collapse_inner.set_margin_bottom(6);
            collapse_inner.set_margin_start(10);
            collapse_inner.set_margin_end(10);
            let collapse_img = gtk::Image::from_icon_name("go-previous-symbolic");
            collapse_img.set_valign(gtk::Align::Center);
            collapse_inner.append(&collapse_img);
            let collapse_lbl = gtk::Label::new(Some("Daralt"));
            collapse_lbl.set_xalign(0.0);
            collapse_lbl.set_hexpand(true);
            collapse_inner.append(&collapse_lbl);
            collapse_btn.set_child(Some(&collapse_inner));
            {
                let inst = app_inst.clone_ref();
                let collapse_lbl_c = collapse_lbl.clone();
                let collapse_img_c = collapse_img.clone();
                let collapse_btn_c = collapse_btn.clone();
                let collapse_inner_c = collapse_inner.clone();
                collapse_btn.connect_clicked(move |_| {
                    let now = !inst.sidebar_collapsed.get();
                    inst.sidebar_collapsed.set(now);
                    inst.apply_sidebar_collapsed();
                    // Daralt satırı da aynı dock disiplinine girer.
                    if now {
                        collapse_btn_c.add_css_class("side-dock");
                    } else {
                        collapse_btn_c.remove_css_class("side-dock");
                    }
                    collapse_lbl_c.set_visible(!now);
                    collapse_inner_c.set_spacing(if now { 0 } else { 10 });
                    // Geniş modda Fill'e dönülür (koşulsuz Center, Daralt
                    // satırını ortada takılı bırakıyordu).
                    collapse_inner_c.set_halign(if now {
                        gtk::Align::Center
                    } else {
                        gtk::Align::Fill
                    });
                    collapse_inner_c.set_margin_start(if now { 0 } else { 10 });
                    collapse_inner_c.set_margin_end(if now { 0 } else { 10 });
                    collapse_inner_c.set_margin_top(6);
                    collapse_inner_c.set_margin_bottom(6);
                    collapse_img_c.set_pixel_size(if now { 18 } else { -1 });
                    collapse_img_c.set_halign(if now {
                        gtk::Align::Center
                    } else {
                        gtk::Align::Fill
                    });
                    if !now {
                        collapse_btn_c.set_tooltip_text(Some("Yan menüyü daralt/genişlet"));
                    }
                    collapse_lbl_c.set_text(if now { "Genişlet" } else { "Daralt" });
                    collapse_img_c.set_icon_name(if now {
                        Some("go-next-symbolic")
                    } else {
                        Some("go-previous-symbolic")
                    });
                });
            }
            app_inst.sidebar_box.append(&collapse_btn);
            app_inst.apply_sidebar_collapsed();
            app_inst.update_sidebar_account();
        }
        {
            // Aicix init deferred to Aşama 2
        }

        app_inst.chain_signals();
        app_inst.apply_ui_scale();
        crate::theme::apply_theme(&app_inst.window, &app_inst.settings.borrow().theme);
        {
            // İndirme pompası: kuyruk olaylarını arayüze taşır.
            let pump = app_inst.clone_ref();
            let pump_rm = pump.remove_hint_cb();
            let pump_play: Rc<dyn Fn(crate::download::DownloadRecord)> = {
                let pump = pump.clone_ref();
                Rc::new(move |rec| pump.play_local(&rec))
            };
            let rx = std::sync::Arc::new(std::sync::Mutex::new(dl_rx));
            glib::timeout_add_local(std::time::Duration::from_millis(1500), move || {
                pump.ops.tick();
                pump.ops.sync_downloads(&pump.dl_manager.snapshot());
                let mut progress_dirty = false;
                let mut struct_dirty = false;
                loop {
                    let ev = rx.lock().unwrap().try_recv();
                    match ev {
                        Ok(crate::download::UiEvent::Tick) => progress_dirty = true,
                        Ok(crate::download::UiEvent::Changed) => struct_dirty = true,
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                if pump.stack.visible_child_name().as_deref() == Some("downloads") {
                    if struct_dirty {
                        // Kayıt ID seti değişmediyse (duraklat/devam/hata/bitiş)
                        // tam rebuild yok: satırlar yerinde tazelenir, kaydırma yaşar.
                        let items = pump.dl_manager.snapshot();
                        let same_ids = {
                            let rows = pump.dl_rows.borrow();
                            rows.len() == items.len()
                                && items.iter().all(|r| rows.contains_key(&r.id))
                        };
                        if same_ids {
                            let rows = pump.dl_rows.borrow();
                            for rec in &items {
                                if let Some(row) = rows.get(&rec.id) {
                                    crate::ui::downloads_view::DownloadsView::refresh_row(
                                        row,
                                        rec,
                                        &pump.dl_manager,
                                        &pump_rm,
                                        &pump_play,
                                    );
                                }
                            }
                        } else {
                            // Ekle/kaldır var: kaydırma konumunu koruyarak yeniden kur.
                            let saved = pump
                                .stack
                                .child_by_name("downloads")
                                .and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok())
                                .map(|s| s.vadjustment().value());
                            pump.show_page(&Page::Downloads);
                            if let Some(v) = saved {
                                restore_downloads_scroll(pump.stack.clone(), v, 0);
                            }
                        }
                    } else if progress_dirty {
                        // Yerinde güncelle: yeniden kurulum yok, kaydırma oynamaz.
                        let items = pump.dl_manager.snapshot();
                        let rows = pump.dl_rows.borrow();
                        for rec in &items {
                            if let Some(row) = rows.get(&rec.id) {
                                let (f, txt, stxt) =
                                    crate::ui::downloads_view::DownloadsView::row_state(rec);
                                row.bar.set_fraction(f);
                                row.bar.set_text(Some(&txt));
                                row.status.set_text(&stxt);
                            }
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        // Hero + kart kuantumu pencereye göre (Lowell birebir): layout
        // değişimini izle. İlk ölçüm ANINDA uygulanır (açılışta 1280
        // tahmini yerine gerçek pencere; boyut zıplaması olmaz), sonraki
        // değişimler 300ms debounce'ludur. Değişim evi kirli işaretler:
        // görünürdeyse hemen yeniden kurulur, değilse dönüşte kurulur
        // (bayat kapak boyutu kalması düzeltmesi).
        {
            let inst = app_inst.clone_ref();
            let gen = Rc::new(Cell::new(0u32));
            let first = Rc::new(Cell::new(true));
            let last_h = Rc::new(Cell::new(inst.hero_h.get()));
            let last_q = Rc::new(Cell::new((inst.grid_cols.get(), inst.card_w.get())));
            app_inst.window.connect_map(move |w| {
                let Some(surface) = w.surface() else {
                    return;
                };
                let inst_c = inst.clone_ref();
                let gen_c = gen.clone();
                let first_c = first.clone();
                let last_h_c = last_h.clone();
                let last_q_c = last_q.clone();
                surface.connect_layout(move |_, ww, h| {
                    let my = gen_c.get() + 1;
                    gen_c.set(my);
                    if first_c.get() {
                        first_c.set(false);
                        inst_c.apply_window_metrics(ww, h, &last_h_c, &last_q_c);
                        return;
                    }
                    let gen_c2 = gen_c.clone();
                    let inst_c2 = inst_c.clone_ref();
                    let last_h_c2 = last_h_c.clone();
                    let last_q_c2 = last_q_c.clone();
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(300),
                        move || {
                            if gen_c2.get() != my {
                                return;
                            }
                            inst_c2.apply_window_metrics(ww, h, &last_h_c2, &last_q_c2);
                        },
                    );
                });
            });
        }
        app_inst.show_page(&initial_page);
        if welcome_seen {
            app_inst.fetch_home();
        }
        // Sessiz hesap senkronu (toast/busy yok): favori + sunucu geçmişi.
        // 401'de oturum sessizce düşer, sidebar tazelenir.
        app_inst.silent_sync();
        // İnternet rozeti ilk boyamada bekletmesin: cache soğuksa iyimser
        // çiz (çevrimiçi varsay); gerçek kontrol arka planda yapılıp
        // bitince ana sayfa tazelenir.
        if !internet_cache_fresh() {
            app_inst.refresh_internet_status();
        }
        app_inst.apply_goto_arg();
        app_inst
    }

    pub fn clone_ref(&self) -> Rc<Self> {
        Rc::new(Self {
            window: self.window.clone(),
            stack: self.stack.clone(),
            back_btn: self.back_btn.clone(),
            title_label: self.title_label.clone(),
            ops: self.ops.clone(),
            movie_tint_css: self.movie_tint_css.clone(),
            client: self.client.clone(),
            covers: self.covers.clone_ref(),
            page_history: self.page_history.clone(),
            cats: self.cats.clone(),
            search_results: self.search_results.clone(),
            settings: self.settings.clone(),
            progress: self.progress.clone(),
            progress_bars: self.progress_bars.clone(),
            dl_rows: self.dl_rows.clone(),
            opening_op_id: self.opening_op_id.clone(),
            opening_op_shown_at: self.opening_op_shown_at.clone(),
            play_gen: self.play_gen.clone(),
            current_mpv_pid: self.current_mpv_pid.clone(),
            home_acts: self.home_acts.clone(),
            dl_manager: self.dl_manager.clone(),
            ep_search_controller: self.ep_search_controller.clone(),
            sidebar_rows: self.sidebar_rows.clone(),
            sidebar_collapsed: self.sidebar_collapsed.clone(),
            sidebar_box: self.sidebar_box.clone(),
            hero_h: self.hero_h.clone(),
            home_dirty: self.home_dirty.clone(),
            card_w: self.card_w.clone(),
            grid_cols: self.grid_cols.clone(),
            news: self.news.clone(),
            news_page: self.news_page.clone(),
            calendar: self.calendar.clone(),
            discover_filter: self.discover_filter.clone(),
            discover: self.discover.clone(),
            server_history: self.server_history.clone(),
            scale_css: self.scale_css.clone(),
        })
    }

    /// Ölçek sağlayıcısı: display'e bir kez takılır, değeri değişir.
    fn make_scale_css() -> gtk::CssProvider {
        let p = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &p,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        p
    }

    fn chain_signals(&self) {
        let this = self.clone_ref();
        self.back_btn.connect_clicked(move |_| {
            this.go_back();
        });

        {
            let this = self.clone_ref();
            let settings = self.settings.clone();
            let window_c = self.window.clone();
            let key_ctrl = gtk::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, keyval, _, state| {
                let key_name = keyval.name().map(|s| s.to_string()).unwrap_or_default();
                let is_ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                // Tam ekran (Lowell137/animecix-linux uyarlaması): F11 aç/kapat,
                // Esc tam ekrandan çıkar (fonksiyon tuşu; metin alanına yazılmaz).
                if key_name == "F11" {
                    if gtk::prelude::GtkWindowExt::is_fullscreen(&window_c) {
                        window_c.unfullscreen();
                    } else {
                        window_c.fullscreen();
                    }
                    return glib::Propagation::Stop;
                }
                if key_name == "Escape" && gtk::prelude::GtkWindowExt::is_fullscreen(&window_c) {
                    window_c.unfullscreen();
                    return glib::Propagation::Stop;
                }
                // F1: kısayol yardım penceresi (ayar değerlerini yansıtır).
                if key_name == "F1" {
                    let s = settings.borrow();
                    crate::ui::shortcuts::present(
                        &window_c,
                        &s.search_shortcut,
                        &s.quick_search_shortcut,
                        s.quick_search_enabled,
                    );
                    return glib::Propagation::Stop;
                }
                let sc = settings.borrow().search_shortcut.clone();
                let triggered = match sc.as_str() {
                    "F2" => key_name == "F2",
                    "/" => key_name == "slash" || key_name == "kp_divide",
                    _ => is_ctrl && (key_name == "s" || key_name == "S"), // Ctrl+S
                };
                if triggered {
                    this.open_search_popup();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            self.window.add_controller(key_ctrl);
        }
    }

    /// Yan ray gezinmesi: farklı sayfaysa history'e ekle + göster.
    /// (Lowell sidebar uyarlaması.)
    pub fn navigate_to(&self, p: &Page) {
        {
            let mut st = self.page_history.borrow_mut();
            push_page_history(&mut st, p.clone());
        }
        self.show_page(p);
        if matches!(p, Page::Home) {
            self.fetch_home();
        }
    }

    /// Pencere ölçüsü değişimini uygular (layout: ilk ölçüm anında,
    /// sonrakiler debounce'lu). Hero yüksekliği + kart kuantumu güncellenir;
    /// değişim varsa ev kirlenir ve görünürdeyse yeniden kurulur. Rebuild
    /// bayraksız çağrılamaz: `show_page(Home)` kirli değilse cache'i korur
    /// ve boyutlar pencereden bağımsız kalırdı (kapak boyu takılması).
    fn apply_window_metrics(
        &self,
        ww: i32,
        h: i32,
        last_h: &Rc<Cell<i32>>,
        last_q: &Rc<Cell<(u32, i32)>>,
    ) {
        let nh = hero_h_for_window(h, 5);
        let nq = card_quantum(ww);
        let mut dirty = false;
        if nh != last_h.get() {
            last_h.set(nh);
            self.hero_h.set(nh);
            dirty = true;
        }
        if nq != last_q.get() {
            last_q.set(nq);
            self.grid_cols.set(nq.0);
            self.card_w.set(nq.1);
            dirty = true;
        }
        if !dirty {
            return;
        }
        self.home_dirty.set(true);
        if self.page_history.borrow().last() == Some(&Page::Home) {
            self.show_page(&Page::Home);
        }
    }

    /// Sidebar hesap satırı: girişliyken kullanıcı adı, değilse "Giriş Yap".
    pub fn update_sidebar_account(&self) {
        let logged = self.client.is_logged_in();
        let expired = self.client.auth_expired();
        let label = if expired {
            "Tekrar Giriş".to_string()
        } else {
            self.client
                .session_user()
                .map(|u| u.name)
                .unwrap_or_else(|| "Giriş Yap".to_string())
        };
        for (p, btn, lbl, full) in self.sidebar_rows.borrow_mut().iter_mut() {
            if *p == Page::Account {
                *full = label.clone();
                btn.set_tooltip_text(Some(if expired {
                    "Oturum süresi doldu — tekrar giriş yap"
                } else if logged {
                    "Hesap"
                } else {
                    "Hesaba giriş yap"
                }));
                if expired {
                    btn.add_css_class("side-expired");
                } else {
                    btn.remove_css_class("side-expired");
                }
                if lbl.is_visible() {
                    lbl.set_text(&label);
                }
            }
        }
    }

    /// Yan ray seçim vurgusu (Lowell birebir: detay → Ana Sayfa vurgusu).
    pub fn update_sidebar_selection(&self, page: &Page) {
        let sel: Option<Page> = match page {
            Page::Home | Page::Episodes { .. } | Page::Movie { .. } => Some(Page::Home),
            Page::Favs => Some(Page::Favs),
            Page::Marathon => Some(Page::Marathon),
            Page::History => Some(Page::History),
            Page::Downloads => Some(Page::Downloads),
            Page::Settings => Some(Page::Settings),
            Page::Kesfet => Some(Page::Kesfet),
            Page::Calendar => Some(Page::Calendar),
            Page::News => Some(Page::News),
            Page::Account => Some(Page::Account),
            _ => None,
        };
        for (p, btn, _, _) in self.sidebar_rows.borrow().iter() {
            if Some(p) == sel.as_ref() {
                btn.add_css_class("side-selected");
            } else {
                btn.remove_css_class("side-selected");
            }
        }
    }

    /// Daralt/genişlet (Lowell apply_sidebar birebir: kutu genişliği +
    /// marjlar + satır içi hizalama; iconsuz/login yok bizde).
    pub fn apply_sidebar_collapsed(&self) {
        let collapsed = self.sidebar_collapsed.get();
        self.sidebar_box
            .set_size_request(if collapsed { 58 } else { 164 }, -1);
        self.sidebar_box
            .set_margin_start(if collapsed { 3 } else { 8 });
        self.sidebar_box
            .set_margin_end(if collapsed { 3 } else { 4 });
        for (_, btn, lbl, full) in self.sidebar_rows.borrow().iter() {
            let inner = btn.child().and_downcast::<gtk::Box>();
            if collapsed {
                if !btn.has_css_class("side-dock") {
                    btn.add_css_class("side-dock");
                }
                // Dock modunda yalnızca ikon göster; metin tooltip'te kalır.
                lbl.set_visible(false);
                lbl.set_text("");
                if let Some(img) = inner
                    .as_ref()
                    .and_then(|b| b.first_child())
                    .and_then(|w| w.downcast::<gtk::Image>().ok())
                {
                    // Dock disiplini (Lowell137/animecix-linux birebir): 18px.
                    img.set_pixel_size(18);
                    img.set_halign(gtk::Align::Center);
                }
                if let Some(ref inner) = inner {
                    inner.set_spacing(0);
                    inner.set_halign(gtk::Align::Center);
                    inner.set_margin_start(0);
                    inner.set_margin_end(0);
                    inner.set_margin_top(6);
                    inner.set_margin_bottom(6);
                }
            } else {
                btn.remove_css_class("side-dock");
                if let Some(img) = inner
                    .as_ref()
                    .and_then(|b| b.first_child())
                    .and_then(|w| w.downcast::<gtk::Image>().ok())
                {
                    // Normal mod: tema varsayılanı (Lowell `-1` birebir, ~16px).
                    img.set_pixel_size(-1);
                    img.set_halign(gtk::Align::Fill);
                }
                lbl.set_visible(true);
                lbl.set_xalign(0.0);
                lbl.set_text(full);
                if let Some(inner) = inner {
                    inner.set_spacing(10);
                    inner.set_halign(gtk::Align::Fill);
                    inner.set_margin_start(10);
                    inner.set_margin_end(10);
                    inner.set_margin_top(6);
                    inner.set_margin_bottom(6);
                }
            }
        }
    }

    pub fn go_back(&self) {
        BACK_ANIM.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut st = self.page_history.borrow_mut();
        if st.len() > 1 {
            st.pop();
            while st.len() > 1 && st.last() == st.get(st.len() - 2) {
                st.pop();
            }
        }
        let top = st.last().cloned().unwrap_or(Page::Home);
        drop(st);
        self.show_page(&top);
    }

    pub fn busy(&self, on: bool) {
        if on {
            self.ops.begin("loading", "Yükleniyor…", "");
        } else {
            self.ops.finish(
                "loading",
                OperationState::Done,
                std::time::Duration::from_millis(400),
            );
        }
    }

    pub fn refresh_internet_status(&self) {
        // Taze kontrol arka planda yapılır; bitince ana sayfa yeniden
        // inşa edilir (iş bitmeden UI bloklanmaz).
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let st = crate::api::check_internet();
            store_internet_status(st);
            let _ = tx.send(());
        });
        let stack = self.stack.clone();
        let this = self.clone_ref();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok(()) => {
                let widget = this.build_home_view();
                if let Some(prev) = stack.child_by_name("home") {
                    stack.remove(&prev);
                }
                stack.add_named(&widget, Some("home"));
                stack.set_visible_child_name("home");
                glib::ControlFlow::Break
            }
            Err(_) => glib::ControlFlow::Continue,
        });
    }

    fn apply_ui_scale(&self) {
        let s = self.settings.borrow().ui_scale.clamp(1.0, 1.25);
        self.window.remove_css_class("ui-scale-125");
        self.window.remove_css_class("ui-scale-150");
        // Sürekli değer: kalıcı provider'a pencere taban fontu yazılır
        // (16px taban; eski kademeli sınıflar kalktı, %150 migrate edilir).
        let px = (16.0 * s).round() as i32;
        self.scale_css
            .load_from_data(&format!("window {{ font-size: {px}px; }}"));
    }

    pub(crate) fn apply_movie_tint(&self, target: &gtk::Box, poster: Option<&str>) {
        let Some(url) = poster.map(|s| s.to_string()) else {
            return;
        };
        let client = self.client.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Option<[(u8, u8, u8); 3]>>();
        std::thread::spawn(move || {
            let _ = tx.send(client.cover_palette(&url));
        });
        let weak = target.downgrade();
        let movie_tint_css = self.movie_tint_css.clone();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok(pal) => {
                let Some(root) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let [c1, c2, c3] = pal.unwrap_or([(122, 162, 247), (55, 70, 110), (140, 110, 190)]);
                let (r1, g1, b1) = c1;
                let (r2, g2, b2) = c2;
                let (r3, g3, b3) = c3;
                let css_a = format!(
                    "#movie-tint-root {{ background-color: rgba({r2},{g2},{b2},0.35); \
                     background: radial-gradient(ellipse at 50% 0%, \
                     rgba({r1},{g1},{b1},0.32), rgba(0,0,0,0) 70%), \
                     linear-gradient(135deg, rgba({r1},{g1},{b1},0.30), \
                     rgba({r2},{g2},{b2},0.20) 55%, rgba({r3},{g3},{b3},0.30)); }}"
                );
                movie_tint_css.load_from_data(&css_a);
                root.set_widget_name("movie-tint-root");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        });
    }

    pub fn show_page(&self, page: &Page) {
        use gtk::prelude::IsA;
        self.progress_bars.borrow_mut().clear();
        self.back_btn
            .set_visible(self.page_history.borrow().len() > 1);

        fn switch<T: IsA<gtk::Widget>>(
            stack: &gtk::Stack,
            name: &str,
            transition: gtk::StackTransitionType,
            widget: T,
        ) {
            if let Some(old) = stack.child_by_name(name) {
                stack.remove(&old);
            }
            stack.set_transition_type(transition);
            stack.add_named(&widget, Some(name));
            // Geri-dönüş bayrağı: hafif SlideRight, yoksa ileri varsayılanı.
            if BACK_ANIM.swap(false, std::sync::atomic::Ordering::SeqCst) {
                stack.set_transition_type(gtk::StackTransitionType::SlideRight);
                stack.set_transition_duration(120);
            } else {
                stack.set_transition_duration(220);
            }
            stack.set_visible_child_name(name);

            let stack_c = stack.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                let Some(visible) = stack_c.visible_child() else {
                    return;
                };
                let mut to_rm = vec![];
                let mut cur = stack_c.first_child();
                while let Some(child) = cur {
                    let next = child.next_sibling();
                    // Ev view-cache'lenir: kirlenince zaten yeniden kurulur.
                    let keep =
                        child == visible || stack_c.child_by_name("home").as_ref() == Some(&child);
                    if !keep {
                        to_rm.push(child);
                    }
                    cur = next;
                }
                for c in to_rm {
                    stack_c.remove(&c);
                }
            });
        }

        match page {
            Page::Welcome => {
                self.title_label.set_text("Hoş Geldiniz");
                switch(
                    &self.stack,
                    "welcome",
                    gtk::StackTransitionType::Crossfade,
                    self.build_welcome_view(),
                );
            }
            Page::Home => {
                self.title_label.set_text("AnimeciX");
                // View-cache: ev zaten kurulu ve kirlenmediyse rebuild yok
                // (dönüşlerde kapaklar anında gelir).
                if self.stack.child_by_name("home").is_some() && !self.home_dirty.get() {
                    self.stack.set_visible_child_name("home");
                } else {
                    self.home_dirty.set(false);
                    switch(
                        &self.stack,
                        "home",
                        gtk::StackTransitionType::Crossfade,
                        self.build_home_view(),
                    );
                }
            }
            Page::Favs => {
                self.title_label.set_text("Favorilerim");
                switch(
                    &self.stack,
                    "favs",
                    gtk::StackTransitionType::Crossfade,
                    self.build_favs_view(),
                );
            }
            Page::Marathon => {
                self.title_label.set_text("İzleme Maratonum 🏃‍♂️");
                switch(
                    &self.stack,
                    "marathon",
                    gtk::StackTransitionType::Crossfade,
                    self.build_marathon_view(),
                );
            }
            Page::History => {
                self.title_label.set_text("İzleme Geçmişi");
                switch(
                    &self.stack,
                    "history",
                    gtk::StackTransitionType::Crossfade,
                    self.build_history_view(),
                );
            }
            Page::Downloads => {
                self.title_label.set_text("İndirilenler");
                switch(
                    &self.stack,
                    "downloads",
                    gtk::StackTransitionType::Crossfade,
                    self.build_downloads_view(),
                );
            }
            Page::Kesfet => {
                self.title_label.set_text("Keşfet");
                switch(
                    &self.stack,
                    "kesfet",
                    gtk::StackTransitionType::Crossfade,
                    self.build_kesfet_view(),
                );
            }
            Page::Calendar => {
                self.title_label.set_text("Yayın Takvimi");
                switch(
                    &self.stack,
                    "calendar",
                    gtk::StackTransitionType::Crossfade,
                    self.build_calendar_view(),
                );
            }
            Page::News => {
                self.title_label.set_text("Haberler");
                switch(
                    &self.stack,
                    "news",
                    gtk::StackTransitionType::Crossfade,
                    self.build_news_view(),
                );
            }
            Page::Settings => {
                self.title_label.set_text("Ayarlar");
                switch(
                    &self.stack,
                    "settings",
                    gtk::StackTransitionType::Crossfade,
                    self.build_settings_view(),
                );
            }
            Page::Account => {
                self.title_label.set_text("Hesap");
                switch(
                    &self.stack,
                    "account",
                    gtk::StackTransitionType::Crossfade,
                    self.build_account_view(),
                );
            }
            // Gerçek sayfa değil (sidebar direkt pencere açar); güvenlik dalı.
            Page::Shortcuts => {
                let s = self.settings.borrow();
                crate::ui::shortcuts::present(
                    &self.window,
                    &s.search_shortcut,
                    &s.quick_search_shortcut,
                    s.quick_search_enabled,
                );
            }
            Page::Search => {
                self.title_label.set_text("Arama Sonuçları");
                switch(
                    &self.stack,
                    "search",
                    gtk::StackTransitionType::SlideLeft,
                    self.build_search_view(),
                );
            }
            Page::Episodes { title, eps } | Page::Movie { title, eps } => {
                self.title_label.set_text(&title.name);
                let page_name = format!("eps_{}", title.id);
                switch(
                    &self.stack,
                    &page_name,
                    gtk::StackTransitionType::SlideLeft,
                    self.build_episodes_view(title, eps),
                );
            }
        }

        // Odak iadesi: PgUp/PgDn/ok tuşları odak ister, hover yetmez.
        // Yan ray vurgusunu da tazele.
        self.update_sidebar_selection(page);
        // Geçiş sonrası odak ölü widget/header'da kalırsa tuşlar boşa düşer.
        let stack_c = self.stack.clone();
        glib::idle_add_local_once(move || {
            if let Some(visible) = stack_c.visible_child() {
                Self::focus_visible_scroll(&visible);
            }
        });
    }

    /// Görünür sayfadaki ilk kaydırma alanına odak verir.
    /// Overlay sarmalayan sayfalarda (karşılama/bölüm/film) içe yürünür.
    fn focus_visible_scroll(widget: &gtk::Widget) -> bool {
        if let Some(sw) = widget.downcast_ref::<gtk::ScrolledWindow>() {
            sw.set_can_focus(true);
            sw.set_focusable(true);
            sw.grab_focus();
            return true;
        }
        let mut cur = widget.first_child();
        while let Some(child) = cur {
            if Self::focus_visible_scroll(&child) {
                return true;
            }
            cur = child.next_sibling();
        }
        false
    }

    fn apply_goto_arg(&self) {
        let args: Vec<String> = std::env::args().collect();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--goto" {
                if let Some(val) = it.next() {
                    self.goto_page(val);
                }
            } else if let Some(val) = a.strip_prefix("--goto=") {
                // main.rs her iki formu da GTK'dan gizler; burada ikisi de anlaşılır.
                self.goto_page(val);
            }
        }
    }

    fn goto_page(&self, val: &str) {
        self.page_history.borrow_mut().clear();
        match val {
            "welcome" => {
                self.page_history.borrow_mut().push(Page::Welcome);
                self.show_page(&Page::Welcome);
            }
            "home" => {
                self.page_history.borrow_mut().push(Page::Home);
                self.show_page(&Page::Home);
                self.fetch_home();
            }
            "favorites" => {
                self.page_history.borrow_mut().push(Page::Favs);
                self.show_page(&Page::Favs);
            }
            "marathon" => {
                self.page_history.borrow_mut().push(Page::Marathon);
                self.show_page(&Page::Marathon);
            }
            "history" => {
                self.page_history.borrow_mut().push(Page::History);
                self.show_page(&Page::History);
            }
            "downloads" => {
                self.page_history.borrow_mut().push(Page::Downloads);
                self.show_page(&Page::Downloads);
            }
            "settings" => {
                self.page_history.borrow_mut().push(Page::Settings);
                self.show_page(&Page::Settings);
            }
            "search" => {
                self.page_history.borrow_mut().push(Page::Home);
                self.show_page(&Page::Home);
                self.fetch_home();
                self.do_search("Tokyo".to_string());
            }
            "episodes" => {
                self.page_history.borrow_mut().push(Page::Home);
                self.show_page(&Page::Home);
                self.fetch_home();
                let this = self.clone_ref();
                glib::timeout_add_local_once(std::time::Duration::from_millis(1800), move || {
                    let cats = this.cats.borrow();
                    let first = cats.iter().flat_map(|c| c.items.iter()).next().cloned();
                    drop(cats);
                    if let Some(t) = first {
                        this.open_episodes(t);
                    }
                });
            }
            _ => {}
        }
    }

    fn build_welcome_view(&self) -> gtk::Overlay {
        let settings = self.settings.borrow().clone();
        let this = self.clone_ref();
        let this_t = self.clone_ref();
        let this_p = self.clone_ref();
        let view = crate::ui::welcome::WelcomeView::build(
            &settings,
            move |new_s| {
                this.client.save_settings(&new_s);
                *this.settings.borrow_mut() = new_s.clone();
                crate::theme::apply_theme(&this.window, &new_s.theme);
                this.client.set_welcome_seen(true);
                this.page_history.borrow_mut().clear();
                this.page_history.borrow_mut().push(Page::Home);
                this.show_page(&Page::Home);
                this.fetch_home();
            },
            move |msg| {
                this_t.ops.info(&msg, "", std::time::Duration::from_secs(4));
            },
            move |theme_id| {
                crate::theme::apply_theme(&this_p.window, &theme_id);
            },
        );
        // Döngü kırıcı: görünüm kurulduysa bir daha zorla gösterme.
        // (Buton/Atla/Esc sadece Home'a geçirir.)
        self.client.set_welcome_seen(true);
        view
    }

    /// Başlık kartı (kapak hemen yüklenir).
    fn create_title_card(&self, t: &Title) -> gtk::Box {
        self.create_title_card_sized(t, 140)
    }

    /// Boyut parametreli kart (ev/devam kuantumu; arama/keşfet 140 sabit).
    /// Kart disiplini (Lowell poster_card birebir): sabit boy, başlık tek
    /// satır, alt yazı boşsa hayalet " ".
    fn create_title_card_sized(&self, t: &Title, w: i32) -> gtk::Box {
        self.create_title_card_sub(t, w, None)
    }

    /// Kapak yalnızca kart görünür olduğunda istenir. İlk aşamada unmap'te
    /// doku bırakılmaz; böylece pager'da gizli kartlar kapak yüklemez ama
    /// bir kez yüklenen kapak asla kaybolmaz.
    fn lazy_cover_picture(&self, url: Option<&str>, w: i32, h: i32) -> gtk::Picture {
        let pic = crate::covers::new_sized_picture(w, h);
        let Some(url) = url.map(str::to_string) else {
            return pic;
        };
        let covers = self.covers.clone_ref();
        let loaded = Rc::new(Cell::new(false));
        let load: Rc<dyn Fn()> = {
            let covers = covers.clone_ref();
            let pic_c = pic.clone();
            let loaded = loaded.clone();
            let url = url.clone();
            Rc::new(move || {
                if !loaded.replace(true) {
                    covers.load_cover(Some(&url), &pic_c, w, h);
                }
            })
        };
        {
            let load = load.clone();
            pic.connect_map(move |_| load());
        }
        // Nadir de olsa sinyal bağlanmadan önce mapped olmuşsa kaçırma.
        if pic.is_mapped() {
            load();
        }
        pic
    }

    /// Alt yazısı ezilebilir kart: devam rafı SxxExx rozetini buradan verir,
    /// geometri normal kartla birebir aynı kalır (hiza kayması olmaz).
    fn create_title_card_sub(&self, t: &Title, w: i32, sub_override: Option<String>) -> gtk::Box {
        let h = w * 3 / 2;
        let mwc = (w * 16 / 140).max(16);
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 4);
        box_.add_css_class("title-btn");
        box_.set_size_request(w, h + 60);
        box_.set_hexpand(false);
        box_.set_vexpand(false);
        box_.set_halign(gtk::Align::Start);
        box_.set_valign(gtk::Align::Start);

        let pic = self.lazy_cover_picture(t.poster.as_deref(), w, h);
        pic.set_size_request(w, h);
        pic.set_can_shrink(false);
        pic.set_hexpand(false);
        pic.set_vexpand(false);
        pic.set_halign(gtk::Align::Center);
        // Poster-lift (Lowell137/animecix-linux uyarlaması): hoverda kart
        // yerinden oynamaz, yalnız poster 3px kalkar.
        pic.add_css_class("poster-lift");
        let motion = gtk::EventControllerMotion::new();
        let pic_enter = pic.clone();
        motion.connect_enter(move |_, _, _| {
            pic_enter.add_css_class("lifted");
        });
        let pic_leave = pic.clone();
        motion.connect_leave(move |_| {
            pic_leave.remove_css_class("lifted");
        });
        pic.add_controller(motion);

        let lbl = gtk::Label::new(Some(&t.name));
        lbl.add_css_class("card-title");
        lbl.set_wrap(false);
        lbl.set_single_line_mode(true);
        lbl.set_lines(1);
        lbl.set_max_width_chars(mwc);
        lbl.set_xalign(0.5);
        lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

        box_.append(&pic);
        box_.append(&lbl);
        // Alt yazı: ezme varsa o (devam rafı SxxExx), yoksa anlamlı
        // tür/yıl/tip (yükseklik sabit kalır).
        let sub = gtk::Label::new(Some(&sub_override.unwrap_or_else(|| t.card_subtitle())));
        sub.add_css_class("dim-label");
        sub.set_wrap(false);
        sub.set_single_line_mode(true);
        sub.set_lines(1);
        sub.set_max_width_chars(mwc + 2);
        sub.set_xalign(0.5);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        box_.append(&sub);

        let gesture = gtk::GestureClick::new();
        let this = self.clone_ref();
        let title_clone = t.clone();
        gesture.connect_pressed(move |_, _, _, _| {
            this.open_episodes(title_clone.clone());
        });
        box_.add_controller(gesture);

        box_
    }

    /// Seçilmiş listeden hero kurar (skeleton takası da bu gövdeyi kullanır).
    fn build_spotlight_picks(&self, picks: &[Title]) -> Option<gtk::Box> {
        let h = self.hero_h.get();
        let carousel = adw::Carousel::new();
        carousel.set_allow_mouse_drag(true);
        carousel.set_allow_scroll_wheel(false);
        carousel.set_hexpand(true);
        let mut count = 0u32;
        for t in picks.iter().take(5) {
            let Some(url) = t.backdrop.clone().or_else(|| t.poster.clone()) else {
                continue;
            };
            let pic = gtk::Picture::new();
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_hexpand(true);
            pic.set_width_request(-1);
            pic.set_height_request(h);
            self.covers.load_cover(Some(&url), &pic, 1280, h);

            let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
            shade.add_css_class("hero-shade");
            shade.set_hexpand(true);
            shade.set_vexpand(true);

            let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
            content.set_valign(gtk::Align::End);
            content.set_margin_start(20);
            content.set_margin_end(20);
            content.set_margin_bottom(18);
            if let Some(g0) = t.display_genre() {
                let badge = gtk::Label::new(Some(&g0));
                badge.add_css_class("detail-badge");
                badge.set_xalign(0.0);
                content.append(&badge);
            }
            let title = gtk::Label::new(Some(&t.display_name()));
            title.add_css_class("title-1");
            title.add_css_class("hero-text");
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            content.append(&title);
            let genre_year = match (t.display_genre(), t.year) {
                (Some(g), Some(y)) => {
                    format!("{g} · {y}")
                }
                (Some(g), None) => g,
                (None, Some(y)) => y.to_string(),
                (None, None) => String::new(),
            };
            if !genre_year.is_empty() {
                let gl = gtk::Label::new(Some(&genre_year));
                gl.add_css_class("hero-genre");
                gl.set_xalign(0.0);
                gl.set_single_line_mode(true);
                gl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                content.append(&gl);
            }
            if let Some(d) = t
                .description
                .as_ref()
                .map(|d| d.trim())
                .filter(|d| !d.is_empty())
            {
                let cut: String = d.chars().take(90).collect();
                let dl = gtk::Label::new(Some(&cut));
                dl.add_css_class("hero-text");
                dl.add_css_class("dim-label");
                dl.set_xalign(0.0);
                dl.set_wrap(true);
                dl.set_lines(2);
                dl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                content.append(&dl);
            }
            let watch = gtk::Button::with_label("▶ İzle");
            watch.add_css_class("pill");
            watch.add_css_class("suggested-action");
            watch.set_halign(gtk::Align::Start);
            let this = self.clone_ref();
            let tt = t.clone();
            watch.connect_clicked(move |_| {
                this.open_episodes(tt.clone());
            });
            content.append(&watch);

            let overlay = gtk::Overlay::new();
            overlay.add_css_class("hero-clip");
            // Sayfa viewportu doldurur (komşu kart taşması kapanır;
            // tam ekranda otomatik genişler).
            overlay.set_hexpand(true);
            overlay.set_halign(gtk::Align::Fill);
            overlay.set_size_request(-1, h);
            overlay.set_child(Some(&pic));
            overlay.add_overlay(&shade);
            overlay.add_overlay(&content);
            carousel.append(&overlay);
            count += 1;
        }
        if count == 0 {
            return None;
        }
        // 6sn oto-dönüş; sayfa değişince (ebeveyn yok) timer ölür.
        let car_c = carousel.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(6), move || {
            if car_c.parent().is_none() {
                return glib::ControlFlow::Break;
            }
            let n = car_c.n_pages();
            if n > 1 {
                let next = car_c.position() as u32 + 1;
                let page = car_c.nth_page(if next >= n { 0 } else { next });
                car_c.scroll_to(&page, true);
            }
            glib::ControlFlow::Continue
        });
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        root.append(&carousel);
        let dots = adw::CarouselIndicatorDots::new();
        dots.set_carousel(Some(&carousel));
        dots.set_halign(gtk::Align::Center);
        root.append(&dots);
        Some(root)
    }

    /// Skeleton + arka-plan bölüm ön-kontrolü: hero hemen nabız iskeletle
    /// belirir; `episodes()` boş dönen aday sessizce düşer (hepsi düşerse
    /// ham liste gösterilir, hero asla boş kalmaz). Eski görünüm yıkıldıysa
    /// takas sessizce atlanır.
    fn build_spotlight_async(&self, parent: &gtk::Box, items: &[Title]) {
        let h = self.hero_h.get();
        let skel = gtk::Box::new(gtk::Orientation::Vertical, 6);
        skel.set_hexpand(true);
        skel.set_halign(gtk::Align::Fill);
        let ph = gtk::Box::new(gtk::Orientation::Vertical, 0);
        ph.add_css_class("hero-skeleton");
        ph.set_size_request(-1, h);
        ph.set_hexpand(true);
        ph.set_halign(gtk::Align::Fill);
        skel.append(&ph);
        parent.append(&skel);

        let picks = pick_spotlight(items, 8, spot_seed());
        if picks.is_empty() {
            parent.remove(&skel);
            return;
        }
        let raw = picks.clone();
        let skel_w = skel.downgrade();
        let client = self.client.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<Title>>();
        std::thread::spawn(move || {
            // Aday başına bir işçi: enrich + episodes, boşu ele.
            // (Client paylaşımı Arc ile; scope/Sync gerekmez.)
            let mut handles = Vec::new();
            for t in picks {
                let c = client.clone();
                handles.push(std::thread::spawn(move || {
                    let enriched = c.enrich_title(&t);
                    match c.episodes(&enriched) {
                        Ok(eps) if !eps.is_empty() => Some(t),
                        _ => None,
                    }
                }));
            }
            let mut ok: Vec<Title> = Vec::new();
            for hdl in handles {
                if let Ok(Some(t)) = hdl.join() {
                    ok.push(t);
                }
            }
            let final_picks = if ok.is_empty() { raw } else { ok };
            let _ = tx.send(final_picks);
        });
        // Takas UI tarafında: görünüm yıkıldıysa sessizce atlanır.
        let this = self.clone_ref();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok(final_picks) => {
                if let Some(skel) = skel_w.upgrade() {
                    if let Some(parent) = skel.parent().and_downcast::<gtk::Box>() {
                        let prev = skel.prev_sibling();
                        parent.remove(&skel);
                        if let Some(view) = this.build_spotlight_picks(&final_picks) {
                            parent.insert_child_after(&view, prev.as_ref());
                        }
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Devam rafı: son izlenenler (yerel + sunucu birleşik, en yeni önce;
    /// tıklama detaya gider, alt yazı SxxExx rozetidir).
    /// (Lowell137/animecix-linux `continue_card` uyarlaması.)
    fn build_continue_section(&self) -> Option<gtk::Box> {
        let st = self.client.load_state();
        let merged = crate::auth::merged_history(&st.history, &self.server_history.borrow());
        let mut seen = std::collections::HashSet::new();
        let mut items: Vec<(Title, crate::api::Episode)> = Vec::new();
        for h in merged.iter() {
            if seen.insert(h.title.id) {
                items.push((h.title.clone(), h.episode.clone()));
            }
            if items.len() >= 6 {
                break;
            }
        }
        if items.is_empty() {
            return None;
        }
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let cw = self.card_w.get();
        let (head, wire) =
            build_shelf_pager("▶ Devam Et", items.len(), self.grid_cols.get() as usize);
        root.append(&head);

        let flow = gtk::FlowBox::new();
        flow.set_halign(gtk::Align::Center);
        flow.set_valign(gtk::Align::Start);
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_activate_on_single_click(false);
        flow.set_column_spacing(16);
        flow.set_row_spacing(20);
        flow.set_max_children_per_line(self.grid_cols.get());
        flow.set_min_children_per_line(1);
        // Normal kart şablonu + SxxExx alt yazısı: hover-play overlay
        // kalktı (tıklama zaten detaya gider), geometri raflarla eşit.
        for (t, ep) in items {
            let sub = components::episode_label_compact(ep.season, ep.episode, &ep.name);
            flow.append(&self.create_title_card_sub(&t, cw, Some(sub)));
        }
        wire(&flow);
        root.append(&flow);
        Some(root)
    }

    fn build_home_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scroll.set_child(Some(&outer));

        // İnternet bağlantı uyarısı (önbellek soğuksa iyimser geçilir;
        // gerçek durum warmer thread ile gelip görünümü tazeler).
        match internet_cached().unwrap_or(crate::api::InternetStatus::Online) {
            crate::api::InternetStatus::Online => {}
            crate::api::InternetStatus::Offline { reason: _ } => {
                let banner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                banner.add_css_class("tip-banner");
                let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
                icon.set_icon_size(gtk::IconSize::Normal);
                icon.set_valign(gtk::Align::Center);
                let text = gtk::Label::new(Some("İnternet bağlantısı yok"));
                text.add_css_class("tip-banner-text");
                text.set_xalign(0.0);
                text.set_wrap(true);
                text.set_hexpand(true);
                text.set_valign(gtk::Align::Center);
                let retry_btn = gtk::Button::with_label("Yeniden Kontrol Et");
                retry_btn.add_css_class("flat");
                retry_btn.add_css_class("pill");
                retry_btn.set_valign(gtk::Align::Center);
                let this = self.clone_ref();
                retry_btn.connect_clicked(move |_| {
                    this.refresh_internet_status();
                });
                banner.append(&icon);
                banner.append(&text);
                banner.append(&retry_btn);
                outer.append(&banner);
            }
        }

        let cats = self.cats.borrow();

        if cats.is_empty() {
            let spinner_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
            spinner_box.set_valign(gtk::Align::Center);
            spinner_box.set_halign(gtk::Align::Center);
            spinner_box.set_vexpand(true);
            let spinner = gtk::Spinner::new();
            spinner.set_size_request(48, 48);
            spinner.start();
            let lbl = gtk::Label::new(Some("İçerikler yükleniyor…"));
            lbl.add_css_class("dim-label");
            spinner_box.append(&spinner);
            spinner_box.append(&lbl);
            scroll.set_child(Some(&spinner_box));
            return scroll;
        }

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
        main_box.set_margin_top(12);
        main_box.set_margin_bottom(18);
        main_box.set_margin_start(12);
        main_box.set_margin_end(12);

        // Spotlight hero: iskelet hemen, doğrulanmış 8'li sonra
        // (bölümsüz aday arka-planda elenir).
        if !cats.is_empty() {
            let pool = hero_pool(&cats);
            self.build_spotlight_async(&main_box, &pool);
        }

        if let Some(cont) = self.build_continue_section() {
            main_box.append(&cont);
        }

        // Raflar kuantum boyda: PER = sütun x 2 (her genişlikte 2 satır).
        let per_home = (self.grid_cols.get() * 2).max(2) as usize;
        let card_w = self.card_w.get();
        for cat in cats.iter() {
            let (head, wire) = build_shelf_pager(&cat.name, cat.items.len(), per_home);
            main_box.append(&head);
            let flow = gtk::FlowBox::new();
            flow.set_halign(gtk::Align::Center);
            flow.set_valign(gtk::Align::Start);
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_activate_on_single_click(false);
            flow.set_column_spacing(16);
            flow.set_row_spacing(20);
            flow.set_max_children_per_line(self.grid_cols.get());
            flow.set_min_children_per_line(1);
            for t in &cat.items {
                flow.append(&self.create_title_card_sized(t, card_w));
            }
            wire(&flow);
            main_box.append(&flow);
        }

        // Hero üstü yüzen hızlı arama hapı (Lowell düzeni): Clamp'lı içerik
        // + üstte ortalı Entry. Enter metni popup'a taşıyıp hapı boşaltır;
        // açılışta odak alınmaz (kısayollar/carousel bozulmasın).
        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(1400);
        clamp.set_child(Some(&main_box));
        let home_overlay = gtk::Overlay::new();
        home_overlay.set_child(Some(&clamp));
        let quick = gtk::Entry::new();
        quick.set_placeholder_text(Some("Hızlı ara… (Enter)"));
        quick.add_css_class("quick-search");
        quick.set_size_request(560, -1);
        quick.set_halign(gtk::Align::Center);
        quick.set_valign(gtk::Align::Start);
        quick.set_margin_top(10);
        quick.set_tooltip_text(Some("Anime veya dizi ara"));
        {
            let this = self.clone_ref();
            quick.connect_activate(move |e| {
                let q = e.text().to_string();
                e.set_text("");
                this.open_search_popup_with(&q);
            });
        }
        home_overlay.add_overlay(&quick);
        outer.append(&home_overlay);
        scroll
    }

    fn build_marathon_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        let this_click = self.clone_ref();
        let this_toggle = self.clone_ref();
        let this_remove = self.clone_ref();
        let this_clear = self.clone_ref();
        let this_cover = self.clone_ref();
        let this_reorder = self.clone_ref();

        let view = views::MarathonView::build(
            self.client.clone(),
            move |title| {
                this_click.open_episodes(title);
            },
            move |id| {
                let item = this_toggle
                    .client
                    .get_marathon()
                    .into_iter()
                    .find(|m| m.title.id == id);
                let Some(item) = item else {
                    return;
                };
                if item.completed {
                    this_toggle.client.mark_title_unwatched(id);
                    this_toggle.client.set_marathon_completed(id, false);
                    this_toggle.ops.info(
                        "⏳ Tüm bölümler izlenmedi olarak işaretlendi",
                        "",
                        std::time::Duration::from_secs(2),
                    );
                    this_toggle.show_page(&Page::Marathon);
                    return;
                }
                let title = item.title.clone();
                let client = this_toggle.client.clone();
                let (tx, rx) = std::sync::mpsc::channel::<Result<usize, String>>();
                std::thread::spawn(move || {
                    let _ = tx.send(client.mark_title_watched(&title));
                });
                let this_async = this_toggle.clone();
                glib::idle_add_local(move || match rx.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    msg => {
                        match msg {
                            Ok(Ok(n)) => {
                                this_async.client.set_marathon_completed(id, true);
                                this_async.ops.info(
                                    &format!("🏁 {n} bölüm izlendi olarak işaretlendi!"),
                                    "",
                                    std::time::Duration::from_secs(2),
                                );
                            }
                            _ => {
                                this_async.ops.info(
                                    "❌ Bölüm listesi alınamadı (internete bağlı mısın?)",
                                    "",
                                    std::time::Duration::from_secs(3),
                                );
                            }
                        }
                        this_async.show_page(&Page::Marathon);
                        glib::ControlFlow::Break
                    }
                });
            },
            move |id| {
                this_remove.client.remove_from_marathon(id);
                this_remove.ops.info(
                    "Maratondan kaldırıldı",
                    "",
                    std::time::Duration::from_secs(2),
                );
                this_remove.show_page(&Page::Marathon);
            },
            move || {
                this_clear.client.clear_marathon();
                this_clear.ops.info(
                    "İzleme maratonu temizlendi",
                    "",
                    std::time::Duration::from_secs(2),
                );
                this_clear.show_page(&Page::Marathon);
            },
            move |id, new_index| {
                this_reorder.client.reorder_marathon(id, new_index);
                this_reorder.show_page(&Page::Marathon);
            },
            move |poster, pic, w, h| {
                this_cover.covers.load_cover(poster, pic, w, h);
            },
        );
        scroll.set_child(Some(&view));

        let motion = gtk::DropControllerMotion::new();
        let drag_pos: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));
        let motion_state = drag_pos.clone();
        let scroll_m = scroll.clone();
        motion.connect_motion(move |_, _x, y| {
            let h = scroll_m.height() as f64;
            *motion_state.borrow_mut() = Some((y, h));
        });
        let leave_state = drag_pos.clone();
        motion.connect_leave(move |_| {
            *leave_state.borrow_mut() = None;
        });
        scroll.add_controller(motion);

        let scroll_w = scroll.downgrade();
        let timer_state = drag_pos.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            let Some(scroll_t) = scroll_w.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            if let Some((y, h)) = *timer_state.borrow() {
                let margin = 50.0;
                let adj = scroll_t.vadjustment();
                let max = (adj.upper() - adj.page_size()).max(0.0);
                let cur = adj.value();
                let new = if y < margin {
                    (cur - ((margin - y) * 0.6 + 6.0)).clamp(0.0, max)
                } else if y > h - margin {
                    (cur + ((y - (h - margin)) * 0.6 + 6.0)).clamp(0.0, max)
                } else {
                    cur
                };
                adj.set_value(new);
            }
            gtk::glib::ControlFlow::Continue
        });

        scroll
    }

    fn build_favs_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let saved = self.client.load_state().saved;
        if saved.is_empty() {
            let sp = components::create_status_page(
                "Henüz Favori Eklenmedi",
                "Beğendiğiniz anime, dizileri ve filmleri yıldız ikonuna tıklayarak favorilerinize ekleyin.",
                "starred-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }

        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 5);
        list_box.set_margin_top(6);
        list_box.set_margin_bottom(6);
        list_box.set_margin_start(10);
        list_box.set_margin_end(10);
        list_box.set_vexpand(false);

        // Nereye kaydedildiği belli olsun: sitedeki İzleme Listesi.
        let where_lbl = gtk::Label::new(Some("(AnimeciX'te İzleme Listesi)"));
        where_lbl.add_css_class("dim-label");
        where_lbl.set_xalign(0.0);
        where_lbl.set_margin_start(4);
        where_lbl.set_margin_bottom(4);
        list_box.append(&where_lbl);

        for t in saved {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.add_css_class("fav-item-card");

            let pic = gtk::Picture::new();
            pic.set_width_request(48);
            pic.set_height_request(72);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_css_classes(&["cover", "cover-thumb"]);
            pic.set_valign(gtk::Align::Center);
            self.covers.load_cover(t.poster.as_deref(), &pic, 48, 72);

            let info_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
            info_box.set_valign(gtk::Align::Center);
            info_box.set_hexpand(true);

            let name = gtk::Label::new(Some(&t.name));
            name.add_css_class("title-3");
            name.set_xalign(0.0);
            name.set_wrap(false);
            name.set_single_line_mode(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);

            info_box.append(&name);
            episodes_view::append_title_submeta(&info_box, &t);

            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            actions.set_valign(gtk::Align::Center);

            let play_btn = gtk::Button::with_label("▶ İzle");
            play_btn.add_css_class("suggested-action");
            play_btn.add_css_class("pill");
            let this_play = self.clone_ref();
            let t_play = t.clone();
            play_btn.connect_clicked(move |_| this_play.open_episodes(t_play.clone()));

            let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
            del_btn.add_css_class("flat");
            del_btn.add_css_class("circular");
            del_btn.add_css_class("destructive-action");
            del_btn.set_tooltip_text(Some("Favorilerden Çıkar"));
            let this_del = self.clone_ref();
            let t_del = t.clone();
            del_btn.connect_clicked(move |_| {
                this_del.client.toggle_saved(&t_del);
                crate::auth::sync_fav_remote(&this_del.client, t_del.id, false);
                this_del.show_page(&Page::Favs);
            });

            actions.append(&play_btn);
            actions.append(&del_btn);

            row.append(&pic);
            row.append(&info_box);
            row.append(&actions);

            list_box.append(&row);
        }

        scroll.set_child(Some(&list_box));
        scroll
    }

    fn build_history_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        // Yerel + sunucu birleşik (sunucu boşsa yerel aynen).
        let history = crate::auth::merged_history(
            &self.client.load_state().history,
            &self.server_history.borrow(),
        );
        if history.is_empty() {
            let sp = components::create_status_page(
                "İzleme Geçmişi Boş",
                "İzlediğiniz bölümler burada görünecek.",
                "avatar-default-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }

        let this_del = self.clone_ref();
        let this_clr = self.clone_ref();
        let this_open = self.clone_ref();
        let this_cov = self.clone_ref();
        let view = views::HistoryView::build(
            &self.client,
            &history,
            move |ids| {
                this_del.client.remove_history_items(&ids);
                this_del.show_page(&Page::History);
            },
            move || {
                this_clr.client.clear_history();
                this_clr.show_page(&Page::History);
            },
            move |h| {
                this_open.open_episodes(h.title.clone());
            },
            move |url, pic, w, h| {
                this_cov.covers.load_cover(url, pic, w, h);
            },
        );

        scroll.set_child(Some(&view));
        scroll
    }

    /// Etkin indirme klasörü (ayar boşsa varsayılan; oluşturulur).
    fn effective_download_dir(&self) -> std::path::PathBuf {
        let d = self
            .settings
            .borrow()
            .download_dir
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(crate::download::default_download_dir);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn build_downloads_view(&self) -> gtk::ScrolledWindow {
        let on_play: Rc<dyn Fn(crate::download::DownloadRecord)> = {
            let this = self.clone_ref();
            Rc::new(move |rec| this.play_local(&rec))
        };
        let (scroll, rows) = crate::ui::downloads_view::DownloadsView::build(
            &self.dl_manager,
            self.effective_download_dir(),
            self.remove_hint_cb(),
            on_play,
        );
        *self.dl_rows.borrow_mut() = rows;
        scroll
    }

    /// Listeden silme ipucu (bir kez, 10sn): bitmiş video silinmez.
    fn remove_hint_cb(&self) -> std::rc::Rc<dyn Fn()> {
        let this = self.clone_ref();
        std::rc::Rc::new(move || {
            if this.settings.borrow().seen_remove_hint {
                return;
            }
            this.ops.info(
                "Listeden silindi — tamamlanan video dosyası silinmez.",
                "",
                std::time::Duration::from_secs(10),
            );
            this.settings.borrow_mut().seen_remove_hint = true;
            let s = this.settings.borrow().clone();
            this.client.save_settings(&s);
        })
    }

    fn build_settings_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let settings = self.settings.borrow();
        let this_save = self.clone_ref();
        let this_wipe = self.clone_ref();

        let last_save: Rc<RefCell<std::time::Instant>> =
            Rc::new(RefCell::new(std::time::Instant::now()));
        let last_save_c = last_save.clone();
        let view = views::SettingsView::build(
            &settings,
            move |new_s| {
                let old_s = this_save.settings.borrow().clone();
                *this_save.settings.borrow_mut() = new_s.clone();
                this_save.client.save_settings(&new_s);
                this_save.client.set_cf_clearance(&new_s.cf_clearance);
                if new_s.max_parallel_downloads != old_s.max_parallel_downloads {
                    // Artırım bekleyen işleri hemen başlatır; azaltım
                    // devam edenleri bölmez, yeni alımı kısar.
                    this_save
                        .dl_manager
                        .set_max_parallel(new_s.max_parallel_downloads as usize);
                }
                this_save.apply_ui_scale();
                crate::theme::apply_theme(&this_save.window, &new_s.theme);
                if new_s.cover_quality != old_s.cover_quality {
                    // Kalite değişimi restart ister: dialog → bayrak + bellek
                    // temizliği → restart; silme yeni proseste olur (yarış yok).
                    let dlg_app = this_save.clone_ref();
                    let dialog = adw::MessageDialog::builder()
                        .heading("Kapak Kalitesi Değişti")
                        .body("Diskteki kapaklar silinip uygulama yeniden başlatılsın mı? Yeni kalitedeki kapaklar açılışta indirilir. Devam eden indirmeler kaldığı yerden devam eder.")
                        .close_response("cancel")
                        .default_response("cancel")
                        .build();
                    dialog.set_transient_for(Some(&this_save.window));
                    dialog.add_response("cancel", "Vazgeç");
                    dialog.add_response("restart", "Sil ve Yeniden Başlat");
                    dialog.set_response_appearance("restart", adw::ResponseAppearance::Destructive);
                    dialog.connect_response(None, move |_, resp| {
                        if resp == "restart" {
                            dlg_app.client.request_covers_wipe();
                            dlg_app.covers.reset();
                            crate::restart_app();
                        } else {
                            // Vazgeç: eski ayarı geri yaz, sayfayı tazele.
                            *dlg_app.settings.borrow_mut() = old_s.clone();
                            dlg_app.client.save_settings(&old_s);
                            dlg_app.show_page(&Page::Settings);
                        }
                    });
                    dialog.present();
                }
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(*last_save_c.borrow()).as_millis();
                *last_save_c.borrow_mut() = now;
                if elapsed >= 400 {
                    this_save
                        .ops
                        .info("Ayarlar kaydedildi", "", std::time::Duration::from_secs(2));
                }
            },
            move |remove_app| {
                this_wipe.client.wipe_all_data();
                if remove_app {
                    crate::uninstall_application();
                    std::process::exit(0);
                } else {
                    this_wipe.ops.info(
                        "Tüm veriler temizlendi ve sıfırlandı!",
                        "",
                        std::time::Duration::from_secs(3),
                    );
                    this_wipe.page_history.borrow_mut().clear();
                    this_wipe.page_history.borrow_mut().push(Page::Welcome);
                    this_wipe.show_page(&Page::Welcome);
                }
            },
        );

        scroll.set_child(Some(&view));
        scroll
    }

    /// Haberler: kart listesi + sayfalayıcı (Lowell uyarlaması).
    fn build_news_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        let cached = self.news.borrow().clone();
        let Some((items, _total, last)) = cached else {
            let sp = components::create_status_page(
                "Yükleniyor…",
                "Haberler getiriliyor.",
                "view-list-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        };
        if items.is_empty() {
            let sp = components::create_status_page(
                "Haber Yok",
                "Şu anda gösterilecek haber bulunamadı.",
                "view-list-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.set_margin_top(12);
        root.set_margin_bottom(18);
        root.set_margin_start(12);
        root.set_margin_end(12);
        for n in &items {
            let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            card.add_css_class("card");
            card.add_css_class("history-item-card");
            card.set_margin_top(3);
            card.set_margin_bottom(3);
            if let Some(img_url) = n.image.clone().or_else(|| n.backdrop.clone()) {
                let pic = self.covers.cover_picture(Some(&img_url), 120, 68);
                pic.set_valign(gtk::Align::Center);
                card.append(&pic);
            }
            let vb = gtk::Box::new(gtk::Orientation::Vertical, 4);
            vb.set_valign(gtk::Align::Center);
            vb.set_hexpand(true);
            let title = gtk::Label::new(Some(&n.title));
            title.add_css_class("title-4");
            title.set_xalign(0.0);
            title.set_wrap(true);
            title.set_lines(2);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            vb.append(&title);
            let date_txt = match crate::api::split_iso(&n.created_at) {
                Some((y, m, d, hh, mm)) => {
                    format!("{d:02}.{m:02}.{y} {hh:02}:{mm:02}")
                }
                None => n.created_at.clone(),
            };
            let date = gtk::Label::new(Some(&date_txt));
            date.add_css_class("dim-label");
            date.set_xalign(0.0);
            vb.append(&date);
            let more = gtk::Label::new(Some("Devamını oku →"));
            more.add_css_class("dim-label");
            more.set_xalign(0.0);
            vb.append(&more);
            card.append(&vb);
            // Dokun → tam metin dialogu (tek tık: çoklu basış hem diyalog
            // açıp hem gövdede select-all tetiklemesin).
            let this = self.clone_ref();
            let nn = n.clone();
            let gesture = gtk::GestureClick::new();
            gesture.connect_pressed(move |_, n_press, _, _| {
                if n_press != 1 {
                    return;
                }
                this.open_news_dialog(&nn);
            });
            card.add_controller(gesture);
            root.append(&card);
        }
        let cur = self.news_page.get();
        if last > 1 {
            let pager = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            pager.set_halign(gtk::Align::Center);
            pager.set_margin_top(8);
            let prev_btn = gtk::Button::from_icon_name("go-previous-symbolic");
            prev_btn.add_css_class("flat");
            prev_btn.add_css_class("circular");
            prev_btn.set_sensitive(cur > 1);
            let next_btn = gtk::Button::from_icon_name("go-next-symbolic");
            next_btn.add_css_class("flat");
            next_btn.add_css_class("circular");
            next_btn.set_sensitive(cur < last);
            let lbl = gtk::Label::new(Some(&format!("{cur} / {last}")));
            lbl.add_css_class("dim-label");
            lbl.set_valign(gtk::Align::Center);
            {
                let this = self.clone_ref();
                prev_btn.connect_clicked(move |_| {
                    this.fetch_news(cur.saturating_sub(1).max(1));
                });
            }
            {
                let this = self.clone_ref();
                next_btn.connect_clicked(move |_| {
                    this.fetch_news(cur + 1);
                });
            }
            pager.append(&prev_btn);
            pager.append(&lbl);
            pager.append(&next_btn);
            root.append(&pager);
        }
        scroll.set_child(Some(&root));
        scroll
    }

    /// Haber tam metin dialogu (satıra dokununca açılır).
    pub fn open_news_dialog(&self, n: &api::NewsItem) {
        let dlg = adw::Window::new();
        dlg.set_title(Some("Haber"));
        dlg.set_modal(true);
        dlg.set_transient_for(Some(&self.window));
        dlg.set_default_size(560, 600);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let titlebar = adw::HeaderBar::new();
        let title = adw::WindowTitle::new("Haber", "");
        titlebar.set_title_widget(Some(&title));
        root.append(&titlebar);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let body_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        body_box.set_margin_top(16);
        body_box.set_margin_bottom(18);
        body_box.set_margin_start(18);
        body_box.set_margin_end(18);
        if let Some(img_url) = n.image.clone().or_else(|| n.backdrop.clone()) {
            let pic = self.covers.cover_picture(Some(&img_url), 480, 270);
            pic.set_halign(gtk::Align::Center);
            body_box.append(&pic);
        }
        let title_lbl = gtk::Label::new(Some(&n.title));
        title_lbl.add_css_class("title-2");
        title_lbl.set_xalign(0.0);
        title_lbl.set_wrap(true);
        body_box.append(&title_lbl);
        let date_txt = match crate::api::split_iso(&n.created_at) {
            Some((y, m, d, hh, mm)) => format!("{d:02}.{m:02}.{y} {hh:02}:{mm:02}"),
            None => n.created_at.clone(),
        };
        let date = gtk::Label::new(Some(&date_txt));
        date.add_css_class("dim-label");
        date.set_xalign(0.0);
        body_box.append(&date);
        let body = gtk::Label::new(Some(n.body.trim()));
        body.add_css_class(&format!(
            "news-font-{}",
            self.settings.borrow().news_font_size
        ));
        body.set_xalign(0.0);
        body.set_wrap(true);
        body.set_selectable(true);
        body_box.append(&body);
        scroll.set_child(Some(&body_box));
        root.append(&scroll);
        dlg.set_content(Some(&root));
        dlg.present();
        // Açılışta seçim sıfırla (kopyalamaya izin ver, select-all gösterme).
        let body_c = body.clone();
        glib::idle_add_local_once(move || {
            body_c.select_region(0, 0);
        });
    }

    /// Profil başlığı: banner + avatar + isim (taze veriyle kurulur).
    /// Hesap sayfası (gecici-yerel): giriş özellikleri derlenmedi.
    fn build_account_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&components::create_status_page(
            "Hesap Devre Dışı",
            "Bu derlemede giriş özellikleri yok (yerel mod).",
            "avatar-default-symbolic",
        )));
        scroll
    }
    fn build_calendar_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        let cached = self.calendar.borrow().clone();
        let Some(days) = cached else {
            let sp = components::create_status_page(
                "Yükleniyor…",
                "Yayın takvimi getiriliyor.",
                "x-office-calendar-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        };
        let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
        root.set_margin_top(12);
        root.set_margin_bottom(18);
        root.set_margin_start(12);
        root.set_margin_end(12);
        let mut any = false;
        for day in &days {
            if day.episodes.is_empty() {
                continue;
            }
            any = true;
            let head_txt = match crate::api::split_iso(&day.date) {
                Some((y, m, d, _, _)) => format!("{d:02}.{m:02}.{y}"),
                None => day.date.clone(),
            };
            let head = gtk::Label::new(Some(&head_txt));
            head.add_css_class("shelf-title");
            head.set_xalign(0.0);
            head.set_margin_start(4);
            root.append(&head);
            for ce in &day.episodes {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.add_css_class("history-item-card");
                row.set_margin_top(3);
                row.set_margin_bottom(3);
                row.set_margin_start(4);
                row.set_margin_end(4);
                let poster = ce.poster.clone().or_else(|| ce.title.poster.clone());
                let pic = self.covers.cover_picture(poster.as_deref(), 96, 54);
                pic.set_valign(gtk::Align::Center);
                row.append(&pic);
                let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
                vb.set_valign(gtk::Align::Center);
                vb.set_hexpand(true);
                let name = gtk::Label::new(Some(&format!(
                    "{} S{:02}E{:02} · {}",
                    ce.title.name, ce.season, ce.episode, ce.name
                )));
                name.add_css_class("title-4");
                name.set_xalign(0.0);
                name.set_single_line_mode(true);
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                vb.append(&name);
                let time_txt = match crate::api::split_iso(&ce.release_date) {
                    Some((_, _, _, hh, mm)) => format!("{hh:02}:{mm:02}"),
                    None => String::new(),
                };
                if !time_txt.is_empty() {
                    let tm = gtk::Label::new(Some(&time_txt));
                    tm.add_css_class("dim-label");
                    tm.set_xalign(0.0);
                    vb.append(&tm);
                }
                row.append(&vb);
                let t = ce.title.clone();
                let this = self.clone_ref();
                let gesture = gtk::GestureClick::new();
                gesture.connect_pressed(move |_, _, _, _| {
                    this.open_episodes(t.clone());
                });
                row.add_controller(gesture);
                root.append(&row);
            }
        }
        if !any {
            let sp = components::create_status_page(
                "Takvim Boş",
                "Bu hafta yayınlanan bölüm bulunamadı.",
                "x-office-calendar-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }
        scroll.set_child(Some(&root));
        scroll
    }

    /// Keşfet: tip/sıra filtresi + ızgara + sayfalayıcı (Lowell uyarlaması).
    fn build_kesfet_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.set_margin_top(12);
        root.set_margin_bottom(18);
        root.set_margin_start(12);
        root.set_margin_end(12);

        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.set_valign(gtk::Align::Center);
        let type_list = gtk::StringList::new(&["Tümü", "Anime", "Film"]);
        let type_drop = gtk::DropDown::new(Some(type_list), None::<gtk::Expression>);
        type_drop.set_tooltip_text(Some("Tür"));
        type_drop.set_valign(gtk::Align::Center);
        let order_list = gtk::StringList::new(
            &crate::api::DISCOVER_ORDERS
                .iter()
                .map(|(l, _)| *l)
                .collect::<Vec<_>>(),
        );
        let order_drop = gtk::DropDown::new(Some(order_list), None::<gtk::Expression>);
        order_drop.set_tooltip_text(Some("Sıralama"));
        order_drop.set_valign(gtk::Align::Center);
        let stream_check = gtk::CheckButton::with_label("Sadece izlenebilenler");
        stream_check.set_valign(gtk::Align::Center);
        let apply_btn = gtk::Button::with_label("Filtrele");
        apply_btn.add_css_class("suggested-action");
        apply_btn.set_valign(gtk::Align::Center);
        // Mevcut filtreyi kontrollere yansıt.
        {
            let f = self.discover_filter.borrow();
            type_drop.set_selected(match f.title_type.as_deref() {
                Some("anime") => 1,
                Some("movie") => 2,
                _ => 0,
            });
            let oi = crate::api::DISCOVER_ORDERS
                .iter()
                .position(|(_, o)| f.order.as_deref() == *o)
                .unwrap_or(0) as u32;
            order_drop.set_selected(oi);
            stream_check.set_active(f.only_streamable);
        }
        bar.append(&type_drop);
        bar.append(&order_drop);
        bar.append(&stream_check);
        bar.append(&apply_btn);
        root.append(&bar);
        {
            let this = self.clone_ref();
            let type_drop_c = type_drop.clone();
            let order_drop_c = order_drop.clone();
            let stream_check_c = stream_check.clone();
            apply_btn.connect_clicked(move |_| {
                {
                    let mut f = this.discover_filter.borrow_mut();
                    f.title_type = match type_drop_c.selected() {
                        1 => Some("anime".to_string()),
                        2 => Some("movie".to_string()),
                        _ => None,
                    };
                    f.order = crate::api::DISCOVER_ORDERS
                        .get(order_drop_c.selected() as usize)
                        .and_then(|(_, o)| *o)
                        .map(|s| s.to_string());
                    f.only_streamable = stream_check_c.is_active();
                    f.page = 1;
                }
                *this.discover.borrow_mut() = None;
                this.fetch_discover();
            });
        }

        let cached = self.discover.borrow().clone();
        let Some((items, total, last)) = cached else {
            let sp = components::create_status_page(
                "Yükleniyor…",
                "Keşfet sonuçları getiriliyor.",
                "view-grid-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        };
        let counter = gtk::Label::new(Some(&format!("{total} başlık")));
        counter.add_css_class("dim-label");
        counter.set_xalign(1.0);
        counter.set_margin_end(4);
        root.append(&counter);

        let flow = gtk::FlowBox::new();
        flow.set_halign(gtk::Align::Center);
        flow.set_valign(gtk::Align::Start);
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_activate_on_single_click(false);
        flow.set_column_spacing(16);
        flow.set_row_spacing(20);
        // Ev kuantumu: büyük kapaklar + sütun disiplini (arama 140 kalır).
        flow.set_max_children_per_line(self.grid_cols.get());
        flow.set_min_children_per_line(1);
        let cw = self.card_w.get();
        for t in &items {
            flow.append(&self.create_title_card_sized(t, cw));
        }
        root.append(&flow);

        let cur = self.discover_filter.borrow().page.max(1);
        if last > 1 {
            let pager = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            pager.set_halign(gtk::Align::Center);
            pager.set_margin_top(8);
            let prev_btn = gtk::Button::from_icon_name("go-previous-symbolic");
            prev_btn.add_css_class("flat");
            prev_btn.add_css_class("circular");
            prev_btn.set_sensitive(cur > 1);
            let next_btn = gtk::Button::from_icon_name("go-next-symbolic");
            next_btn.add_css_class("flat");
            next_btn.add_css_class("circular");
            next_btn.set_sensitive(cur < last);
            let lbl = gtk::Label::new(Some(&format!("{cur} / {last}")));
            lbl.add_css_class("dim-label");
            lbl.set_valign(gtk::Align::Center);
            {
                let this = self.clone_ref();
                prev_btn.connect_clicked(move |_| {
                    {
                        let mut f = this.discover_filter.borrow_mut();
                        f.page = f.page.saturating_sub(1).max(1);
                    }
                    *this.discover.borrow_mut() = None;
                    this.fetch_discover();
                });
            }
            {
                let this = self.clone_ref();
                next_btn.connect_clicked(move |_| {
                    {
                        let mut f = this.discover_filter.borrow_mut();
                        f.page += 1;
                    }
                    *this.discover.borrow_mut() = None;
                    this.fetch_discover();
                });
            }
            pager.append(&prev_btn);
            pager.append(&lbl);
            pager.append(&next_btn);
            root.append(&pager);
        }
        scroll.set_child(Some(&root));
        scroll
    }

    fn build_search_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        let results = self.search_results.borrow();

        if results.is_empty() {
            let sp = components::create_status_page(
                "Sonuç Bulunamadı",
                "Arama sorgunuza uygun anime, dizi veya film bulunamadı.",
                "system-search-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }

        let flow = gtk::FlowBox::new();
        flow.set_margin_top(12);
        flow.set_margin_bottom(18);
        flow.set_margin_start(12);
        flow.set_margin_end(12);
        flow.set_halign(gtk::Align::Center);
        flow.set_valign(gtk::Align::Start);
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_activate_on_single_click(false);
        flow.set_column_spacing(16);
        flow.set_row_spacing(20);

        for t in results.iter() {
            let btn = self.create_title_card(t);
            flow.append(&btn);
        }

        scroll.set_child(Some(&flow));
        scroll
    }

    pub(crate) fn spawn<F, R>(&self, f: F)
    where
        F: FnOnce(Arc<Client>) -> R + Send + 'static,
        R: FnOnce() -> Msg + Send + 'static,
    {
        let c = self.client.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        std::thread::spawn(move || {
            let res_fn = f(c);
            let _ = tx.send(res_fn());
        });
        let this = self.clone_ref();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok(msg) => {
                this.busy(false);
                this.handle_msg(msg);
                glib::ControlFlow::Break
            }
            Err(_) => glib::ControlFlow::Continue,
        });
    }

    fn handle_msg(&self, msg: Msg) {
        match msg {
            Msg::Cats(res) => match res {
                Ok(cats) => {
                    *self.cats.borrow_mut() = cats;
                    self.home_dirty.set(true);
                    if self.page_history.borrow().last() == Some(&Page::Home) {
                        self.show_page(&Page::Home);
                    }
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Search(res) => match res {
                Ok(results) => self.show_search_results(results),
                Err(e) => self.show_error(&e),
            },
            Msg::News(page, res) => match res {
                Ok((items, total, last)) => {
                    *self.news.borrow_mut() = Some((items, total, last));
                    self.news_page.set(page);
                    if self.page_history.borrow().last() == Some(&Page::News) {
                        self.show_page(&Page::News);
                    }
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Calendar(res) => match res {
                Ok(days) => {
                    *self.calendar.borrow_mut() = Some(days);
                    if self.page_history.borrow().last() == Some(&Page::Calendar) {
                        self.show_page(&Page::Calendar);
                    }
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Discover(res) => match res {
                Ok((items, total, last)) => {
                    *self.discover.borrow_mut() = Some((items, total, last));
                    if self.page_history.borrow().last() == Some(&Page::Kesfet) {
                        self.show_page(&Page::Kesfet);
                    }
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Eps(title, res) => match res {
                Ok(eps) => self.show_title_eps(title, eps),
                Err(e) => self.show_error(&e),
            },
            Msg::Play(title, ep, res) => match res {
                Ok(src) => self.play_candidates(
                    &title,
                    &ep,
                    &src.candidates,
                    &src.fast_embeds,
                    &src.fallback_embeds,
                    src.plan,
                ),
                Err(e) => self.show_error(&e),
            },
            Msg::PlayLocal {
                title,
                ep,
                path,
                plan,
            } => {
                self.play_candidates(&title, &ep, std::slice::from_ref(&path), &[], &[], plan);
            }
            Msg::NextEp(title, res) => match res {
                Ok(ep) => self.play(&title, &ep),
                Err(e) => {
                    self.ops.error(&e, "", std::time::Duration::from_secs(3));
                }
            },
            Msg::FansubsLoaded {
                title,
                ep,
                fansubs,
                default_template,
            } => match fansubs {
                Ok(list) => self.after_fansubs_loaded(title, ep, list, default_template),
                Err(e) => self.show_error(&e),
            },
            Msg::PlayQualities {
                title,
                ep,
                fs,
                rest,
                quals,
            } => {
                self.busy(false);
                match quals {
                    Err(_) => self.play_with_fansub_inner(&title, &ep, &fs, rest, None),
                    Ok(list) => {
                        let labels: Vec<String> = list.iter().map(|q| q.label.clone()).collect();
                        let title_c = title.clone();
                        let ep_c = ep.clone();
                        let this_c = self.clone_ref();
                        crate::ui::play_quality_dialog::show_play_quality_dialog(
                            &self.window,
                            &format!(
                                "{} — {}",
                                title.name,
                                components::episode_label_compact(ep.season, ep.episode, &ep.name)
                            ),
                            &labels,
                            move |choice| match choice {
                                crate::ui::play_quality_dialog::PlayChoice::Cancelled => {}
                                crate::ui::play_quality_dialog::PlayChoice::Best => {
                                    this_c.play_with_fansub_inner(
                                        &title_c,
                                        &ep_c,
                                        &fs,
                                        rest.clone(),
                                        None,
                                    );
                                }
                                crate::ui::play_quality_dialog::PlayChoice::Quality(label) => {
                                    let forced = list.iter().find(|q| q.label == label).cloned();
                                    this_c.play_with_fansub_inner(
                                        &title_c,
                                        &ep_c,
                                        &fs,
                                        rest.clone(),
                                        forced,
                                    );
                                }
                            },
                        );
                    }
                }
            }
            Msg::DlLists {
                title,
                quality,
                items,
                is_single,
            } => {
                let with_subs: Vec<(Episode, Vec<api::FansubInfo>)> =
                    items.into_iter().filter(|(_, l)| !l.is_empty()).collect();
                if with_subs.is_empty() {
                    self.ops.error(
                        "Uyarı",
                        "⚠️ Seçili bölümlerde çeviri bulunamadı",
                        std::time::Duration::from_secs(6),
                    );
                    return;
                }
                // "Her bölümde sor" kapalıysa ilk çevirmenle sessizce devam.
                if !self.settings.borrow().fansub_ask_each_time {
                    let auto: Vec<(Episode, api::FansubInfo)> = with_subs
                        .into_iter()
                        .map(|(ep, mut l)| (ep, l.remove(0)))
                        .collect();
                    let title_c = title.clone();
                    let quality_c = quality.clone();
                    let this_c = self.clone_ref();
                    let dir = this_c.effective_download_dir();
                    let series = crate::download::sanitize_filename(&title_c.name);
                    self.spawn(move |c| {
                        let mut recs = Vec::new();
                        let mut skipped = Vec::new();
                        for (ep, fs) in &auto {
                            match crate::download::resolve_for_download(
                                &c, &dir, title_c.id, &series, ep, fs, &quality_c,
                            ) {
                                Ok(rec) => recs.push(rec),
                                Err(e) => {
                                    eprintln!("[DL] çözümleme atlandı: {e}");
                                    skipped
                                        .push(format!("S{:02}E{:02}: {e}", ep.season, ep.episode));
                                }
                            }
                        }
                        move || Msg::DlBatchResolved(recs, skipped, is_single)
                    });
                    return;
                }
                let quality_c = quality.clone();
                let this_c = self.clone_ref();
                let dir = this_c.effective_download_dir();
                let series = crate::download::sanitize_filename(&title.name);
                crate::ui::flashcard::show_flashcard_wizard(
                    &self.window,
                    &title,
                    with_subs,
                    move |done| {
                        if done.is_empty() {
                            return;
                        }
                        let dir_c = dir.clone();
                        let series_c = series.clone();
                        let quality_cc = quality_c.clone();
                        let title_id_c = title.id;
                        this_c.spawn(move |c| {
                            let mut recs = Vec::new();
                            let mut skipped = Vec::new();
                            for (ep, fs) in &done {
                                match crate::download::resolve_for_download(
                                    &c,
                                    &dir_c,
                                    title_id_c,
                                    &series_c,
                                    ep,
                                    fs,
                                    &quality_cc,
                                ) {
                                    Ok(rec) => recs.push(rec),
                                    Err(e) => {
                                        eprintln!("[DL] çözümleme atlandı: {e}");
                                        skipped.push(format!(
                                            "S{:02}E{:02}: {e}",
                                            ep.season, ep.episode
                                        ));
                                    }
                                }
                            }
                            move || Msg::DlBatchResolved(recs, skipped, is_single)
                        });
                    },
                );
            }
            Msg::DlBatchResolved(recs, skipped, is_single) => {
                let n = recs.len();
                let quiet = !is_single;
                for rec in recs {
                    self.dl_manager.enqueue(rec, quiet);
                }
                // Sayaç yalnızca topluda; tekilde pompa bildirimi yeter.
                if !is_single && n > 0 {
                    self.ops.info(
                        &format!("{n} bölüm kuyruğa eklendi"),
                        "",
                        std::time::Duration::from_secs(3),
                    );
                }
                if !skipped.is_empty() {
                    let shown: Vec<&str> = skipped.iter().take(3).map(|s| s.as_str()).collect();
                    let more = if skipped.len() > 3 {
                        format!(" +{}", skipped.len() - 3)
                    } else {
                        String::new()
                    };
                    self.ops.info(
                        &format!("Atlanan: {}{more}", shown.join(", ")),
                        "",
                        std::time::Duration::from_secs(5),
                    );
                }
            }
        }
    }

    pub fn open_episodes(&self, title: Title) {
        self.busy(true);
        self.spawn(move |c| {
            let enriched = c.enrich_title(&title);
            let res = c.episodes(&enriched);
            move || Msg::Eps(enriched.clone(), res)
        });
    }

    /// Tek bölüm indirme akışı: kalite sor + çevirmenleri önden çek.
    pub(crate) fn download_episode(&self, title: Title, ep: Episode) {
        let this_q = self.clone_ref();
        let this = self.clone_ref();
        this_q.ask_download_quality(move |quality| {
            if let Some(quality) = quality {
                this.start_download_prefetch(title.clone(), vec![ep.clone()], quality, true);
            }
        });
    }

    pub(crate) fn ask_download_quality(&self, cb: impl Fn(Option<String>) + 'static) {
        let dialog = adw::MessageDialog::builder()
            .heading("İndirme Kalitesi")
            .body("Bu indirme için hangi kalite kullanılsın?")
            .close_response("cancel")
            .default_response("best")
            .build();
        dialog.set_transient_for(Some(&self.window));
        dialog.add_response("cancel", "İptal");
        dialog.add_response("best", "En iyi");
        dialog.add_response("1080p", "1080p");
        dialog.add_response("720p", "720p");
        dialog.add_response("480p", "480p");
        dialog.set_response_appearance("best", adw::ResponseAppearance::Suggested);
        dialog.connect_response(None, move |_, resp| match resp {
            "best" | "1080p" | "720p" | "480p" => cb(Some(resp.to_string())),
            _ => cb(None),
        });
        dialog.present();
    }

    /// Bölüm listesinin çevirmenlerini worker'da önden çeker.
    fn start_download_prefetch(
        &self,
        title: Title,
        eps: Vec<Episode>,
        quality: String,
        is_single: bool,
    ) {
        self.busy(true);
        let title_c = title.clone();
        self.spawn(move |c| {
            let mut items = Vec::new();
            for ep in &eps {
                let fansubs = c
                    .list_fansubs(title_c.id, ep.episode, ep.season)
                    .unwrap_or_default();
                items.push((ep.clone(), fansubs));
            }
            move || Msg::DlLists {
                title: title_c,
                quality,
                items,
                is_single,
            }
        });
    }

    pub fn open_search_popup(&self) {
        self.open_search_popup_with("");
    }

    /// Headbar hapından devredilen metinle açılır (boşsa boş açılır;
    /// doluysa arama hemen başlar).
    pub fn open_search_popup_with(&self, initial: &str) {
        let dlg = adw::Window::new();
        dlg.set_title(Some("Ara"));
        dlg.set_modal(true);
        dlg.set_transient_for(Some(&self.window));
        dlg.set_default_size(520, 560);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let titlebar = adw::HeaderBar::new();
        let title = adw::WindowTitle::new("Ara", "");
        titlebar.set_title_widget(Some(&title));
        // Kapatma: HeaderBar'ın native pencere kontrolleri yeterli
        // (çift X olmaması için özel buton yok).
        root.append(&titlebar);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.set_margin_top(0);
        header.set_margin_bottom(12);
        header.set_margin_start(12);
        header.set_margin_end(12);
        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Anime veya dizi ara…"));
        entry.set_hexpand(true);
        header.append(&entry);
        root.append(&header);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let results = gtk::Box::new(gtk::Orientation::Vertical, 6);
        results.set_margin_top(10);
        results.set_margin_bottom(12);
        results.set_margin_start(12);
        results.set_margin_end(12);
        scroll.set_child(Some(&results));
        root.append(&scroll);
        dlg.set_content(Some(&root));

        fn clear(box_: &gtk::Box) {
            let mut cur = box_.first_child();
            while let Some(child) = cur {
                let next = child.next_sibling();
                box_.remove(&child);
                cur = next;
            }
        }

        fn show_empty(box_: &gtk::Box) {
            clear(box_);
            let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
            col.set_halign(gtk::Align::Center);
            col.set_valign(gtk::Align::Center);
            col.set_vexpand(true);
            col.set_margin_top(64);
            let img = gtk::Image::from_icon_name("system-search-symbolic");
            img.set_pixel_size(48);
            img.set_opacity(0.35);
            let lbl = gtk::Label::new(Some("Aramak istediğiniz anime veya diziyi yazın"));
            lbl.add_css_class("dim-label");
            lbl.set_opacity(0.65);
            col.append(&img);
            col.append(&lbl);
            box_.append(&col);
        }

        fn spinner(box_: &gtk::Box) {
            clear(box_);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_halign(gtk::Align::Center);
            row.set_margin_top(48);
            let sp = gtk::Spinner::new();
            sp.start();
            let lbl = gtk::Label::new(Some("Aranıyor…"));
            lbl.add_css_class("dim-label");
            row.append(&sp);
            row.append(&lbl);
            box_.append(&row);
        }

        show_empty(&results);

        let dlg_w = dlg.downgrade();
        let gen = Rc::new(Cell::new(0u32));
        let first_hit: Rc<RefCell<Option<Title>>> = Rc::new(RefCell::new(None));
        {
            let results_w = results.downgrade();
            let dlg_w_rows = dlg_w.clone();
            let gen = gen.clone();
            let first_hit = first_hit.clone();
            let client = self.client.clone();
            let this = self.clone_ref();
            entry.connect_search_changed(move |e| {
                let q = e.text().to_string();
                let my = gen.get() + 1;
                gen.set(my);
                if q.trim().is_empty() {
                    if let Some(b) = results_w.upgrade() {
                        show_empty(&b);
                    }
                    *first_hit.borrow_mut() = None;
                    return;
                }
                if let Some(b) = results_w.upgrade() {
                    spinner(&b);
                }
                let results_w2 = results_w.clone();
                let dlg_w2 = dlg_w_rows.clone();
                let gen2 = gen.clone();
                let first_hit2 = first_hit.clone();
                let client2 = client.clone();
                let this2 = this.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
                    if gen2.get() != my {
                        return;
                    }
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(client2.search(&q));
                    });
                    glib::idle_add_local(move || match rx.try_recv() {
                        Ok(res) => {
                            if gen2.get() != my {
                                return glib::ControlFlow::Break;
                            }
                            let Some(box_) = results_w2.upgrade() else {
                                return glib::ControlFlow::Break;
                            };
                            clear(&box_);
                            match res {
                                Ok(list) => {
                                    let mut first: Option<Title> = None;
                                    for t in list.iter().take(15) {
                                        if first.is_none() {
                                            first = Some(t.clone());
                                        }
                                        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                                        row.add_css_class("history-item-card");
                                        row.set_margin_top(3);
                                        row.set_margin_bottom(3);
                                        row.set_margin_start(4);
                                        row.set_margin_end(4);
                                        let pic =
                                            this2.covers.cover_picture(t.poster.as_deref(), 48, 72);
                                        pic.set_valign(gtk::Align::Center);
                                        row.append(&pic);
                                        let vb = gtk::Box::new(gtk::Orientation::Vertical, 4);
                                        vb.set_valign(gtk::Align::Center);
                                        vb.set_hexpand(true);
                                        let name = gtk::Label::new(Some(&t.name));
                                        name.add_css_class("title-4");
                                        name.set_xalign(0.0);
                                        name.set_single_line_mode(true);
                                        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                                        vb.append(&name);
                                        let meta = gtk::Label::new(Some(&t.meta_line()));
                                        meta.add_css_class("dim-label");
                                        meta.set_xalign(0.0);
                                        vb.append(&meta);
                                        row.append(&vb);
                                        let tc = t.clone();
                                        let this3 = this2.clone_ref();
                                        let dlg_w3 = dlg_w2.clone();
                                        let gesture = gtk::GestureClick::new();
                                        gesture.connect_pressed(move |_, _, _, _| {
                                            if let Some(d) = dlg_w3.upgrade() {
                                                d.close();
                                            }
                                            this3.open_episodes(tc.clone());
                                        });
                                        row.add_controller(gesture);
                                        box_.append(&row);
                                    }
                                    *first_hit2.borrow_mut() = first;
                                    if box_.first_child().is_none() {
                                        let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
                                        col.set_halign(gtk::Align::Center);
                                        col.set_margin_top(48);
                                        let img =
                                            gtk::Image::from_icon_name("system-search-symbolic");
                                        img.set_pixel_size(48);
                                        img.set_opacity(0.35);
                                        let title = gtk::Label::new(Some("Sonuç bulunamadı"));
                                        title.add_css_class("title-4");
                                        let sub = gtk::Label::new(Some(
                                            "Farklı bir arama terimi deneyin",
                                        ));
                                        sub.add_css_class("dim-label");
                                        col.append(&img);
                                        col.append(&title);
                                        col.append(&sub);
                                        box_.append(&col);
                                    }
                                }
                                Err(err) => {
                                    let lbl =
                                        gtk::Label::new(Some(&format!("Arama başarısız: {err}")));
                                    lbl.add_css_class("dim-label");
                                    lbl.set_wrap(true);
                                    lbl.set_halign(gtk::Align::Center);
                                    lbl.set_margin_top(36);
                                    box_.append(&lbl);
                                }
                            }
                            glib::ControlFlow::Break
                        }
                        Err(_) => glib::ControlFlow::Continue,
                    });
                });
            });
        }
        {
            let dlg_w_act = dlg_w.clone();
            let first_hit = first_hit.clone();
            let this = self.clone_ref();
            entry.connect_activate(move |_| {
                if let Some(t) = first_hit.borrow().clone() {
                    if let Some(d) = dlg_w_act.upgrade() {
                        d.close();
                    }
                    this.open_episodes(t);
                }
            });
        }
        dlg.present();
        if !initial.trim().is_empty() {
            set_search_text_caret_end(&entry, initial);
        }
        entry.grab_focus();
        entry.set_position(-1);
    }

    fn do_search(&self, q: String) {
        let q = q.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.busy(true);
        self.spawn(move |c| {
            let res = c.search(&q);
            move || Msg::Search(res)
        });
    }

    /// Arama sonuçlarını Search sayfasında gösterir (do_search + palet ortak).
    fn show_search_results(&self, results: Vec<Title>) {
        *self.search_results.borrow_mut() = results;
        let mut st = self.page_history.borrow_mut();
        push_page_history(&mut st, Page::Search);
        drop(st);
        self.show_page(&Page::Search);
    }

    /// Başlık + bölümleri Episodes/Movie sayfasında gösterir (Eps + palet ortak).
    fn show_title_eps(&self, title: Title, eps: Vec<Episode>) {
        let page = if eps.is_empty() {
            Page::Movie { title, eps }
        } else {
            match title.title_type.as_deref() {
                Some("movie") => Page::Movie { title, eps },
                _ => Page::Episodes { title, eps },
            }
        };
        {
            let mut st = self.page_history.borrow_mut();
            push_page_history(&mut st, page.clone());
        }
        self.show_page(&page);
    }

    /// Sürüm notları: bilinen sürüm → kaydırılabilir yenilik penceresi.
    /// Her sürümde bir kez. CHANGELOG.md'deki Öne çıkanlar + Eklenenler
    /// başlıksız tek liste olarak konur (okunması kolay, kaydırmalı).
    pub(crate) fn maybe_show_changelog(&self) {
        let cur = crate::update::CURRENT_VERSION;
        if self.settings.borrow().seen_changelog == cur {
            return;
        }
        {
            let mut s = self.settings.borrow_mut();
            s.seen_changelog = cur.to_string();
            self.client.save_settings(&s);
        }
        let Some((heading, items)) = changelog_items(cur) else {
            return;
        };
        let dlg = adw::Window::new();
        dlg.set_title(Some(heading));
        dlg.set_modal(true);
        dlg.set_transient_for(Some(&self.window));
        dlg.set_default_size(520, 560);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let titlebar = adw::HeaderBar::new();
        let title = adw::WindowTitle::new(heading, "");
        titlebar.set_title_widget(Some(&title));
        root.append(&titlebar);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
        list.set_margin_top(16);
        list.set_margin_bottom(18);
        list.set_margin_start(18);
        list.set_margin_end(18);
        for it in items {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let dot = gtk::Label::new(Some("•"));
            dot.set_valign(gtk::Align::Start);
            dot.add_css_class("dim-label");
            row.append(&dot);
            let lbl = gtk::Label::new(Some(it));
            lbl.set_xalign(0.0);
            lbl.set_wrap(true);
            lbl.set_hexpand(true);
            row.append(&lbl);
            list.append(&row);
        }
        scroll.set_child(Some(&list));
        root.append(&scroll);
        dlg.set_content(Some(&root));
        dlg.present();
    }

    /// Açılış/giriş sessiz senkronu: favori birleşir + sunucu geçmişi
    /// state'e alınır. Toast/busy yok; 401'de oturum sessizce düşer.
    pub fn silent_sync(&self) {
        if !self.client.is_logged_in() {
            return;
        }
        let client = self.client.clone();
        let (tx, rx) = std::sync::mpsc::channel::<(usize, Vec<crate::api::ServerEntry>)>();
        std::thread::spawn(move || {
            let merged = client.merge_server_favs();
            // Lowell birebir: açılış/giriş SADECE çeker (union birleşir),
            // asla yazmaz (yerel modda itiş hiçbir yerde yok).
            let entries = client.server_top(3).unwrap_or_default().0;
            let _ = tx.send((merged, entries));
        });
        // UI güncellemesi ana döngüde (Rc<App> işçiye taşınmaz).
        let this = self.clone_ref();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok((merged, entries)) => {
                let has_new = !entries.is_empty() || merged > 0;
                *this.server_history.borrow_mut() = entries;
                this.update_sidebar_account();
                // Yeni veri varsa görünen sayfayı tazele (yoksa rebuild yok).
                if has_new {
                    let cur = this
                        .page_history
                        .borrow()
                        .last()
                        .cloned()
                        .unwrap_or(Page::Home);
                    if matches!(cur, Page::Home | Page::Account | Page::History) {
                        this.show_page(&cur);
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    fn fetch_home(&self) {
        self.busy(true);
        self.spawn(move |c| {
            let res = c.home_lists();
            move || Msg::Cats(res)
        });
    }

    /// Haber sayfası + önbellek doldurma (Lowell uyarlaması).
    fn fetch_news(&self, page: u32) {
        self.news_page.set(page.max(1));
        self.busy(true);
        let page = self.news_page.get();
        self.spawn(move |c| {
            let res = c.news(page);
            move || Msg::News(page, res)
        });
    }

    /// Takvim doldurma (Lowell uyarlaması).
    fn fetch_calendar(&self) {
        self.busy(true);
        self.spawn(move |c| {
            let res = c.calendar();
            move || Msg::Calendar(res)
        });
    }

    /// Keşfet: filtredeki sayfayla çalışır (Lowell uyarlaması).
    fn fetch_discover(&self) {
        let f = self.discover_filter.borrow().clone();
        self.busy(true);
        self.spawn(move |c| {
            let res = c.discover(&f);
            move || Msg::Discover(res)
        });
    }

    /// Sayfaya git + gerekiyorsa verisini çek (sidebar/kısayol ortak).
    pub fn open_data_page(&self, p: &Page) {
        match p {
            Page::News => {
                if self.news.borrow().is_none() {
                    self.fetch_news(self.news_page.get());
                }
            }
            Page::Calendar => {
                if self.calendar.borrow().is_none() {
                    self.fetch_calendar();
                }
            }
            Page::Kesfet if self.discover.borrow().is_none() => {
                self.fetch_discover();
            }
            _ => {}
        }
        self.navigate_to(p);
    }

    fn show_error(&self, msg: &str) {
        eprintln!("animecix hatası: {msg}");
        self.ops
            .error("Hata", msg, std::time::Duration::from_secs(6));
    }
}

/// Arama kutusuna metni yazar ve imleci metnin sonuna taşır.
/// `-1`, `GtkEditable` için son karakterden sonraki konumdur.
fn set_search_text_caret_end(entry: &gtk::SearchEntry, text: &str) {
    entry.set_text(text);
    entry.set_position(-1);
}

/// Sürüm yenilik maddeleri (bilinmeyen sürüm → None, sessiz geçilir).
/// Kaynak: CHANGELOG.md (sürüm başlığı altındaki düz liste).
fn changelog_items(v: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match v {
        "1.2.7" => Some((
            "Yenilikler (v1.2.7)",
            vec![
                "Ana sayfada arama çubuğu öne çıkan alanın üstüne taşındı; yazıp Enter'a basınca arama açılır.",
                "Oynatıcıda Ctrl + Sağ Ok ile sonraki bölüme geçilir.",
                "Yinelenen bölüm kayıtları teklenir; sonraki/önceki bölüme geçerken takılma olmaz.",
                "Kapak yüklemesi aksarsa sıra kendini toparlar, boş kapak kalmaz.",
            ],
        )),
        "1.2.6" => Some((
            "Yenilikler (v1.2.6)",
            vec![
                "Ana sayfa donması düzeltildi: kapaklar arka planda yükleniyor, sayfa açılırken takılma olmuyor.",
                "Hero eklendi: bölümü/filmi doğrulanmış animeler öneriliyor, tam genişlik.",
                "Haberler, Yayın Takvimi ve Keşfet eklendi: güncel haberleri ve yayın takvimini takip edebilir, Keşfet ile yeni animeler bulabilirsin.",
                "Arayüz ölçeği serbest oldu: %100–%125 arası istediğin değer.",
                "Arayüz Lowell'in katkılarıyla yenilendi: yan ray + daraltma, arama dialogu, headbar arama hapı, devam rafı, sayfalı raflar.",
                "Puan hapı, sezon ve izlendi filtreleri, detay sekmeleri eklendi.",
                "F1 kısayollar penceresi ve F11 tam ekran eklendi.",
                "Kuantum responsive kartlar: pencere büyüyünce kartlar da büyür; poster üzerine gelince hafifçe kalkar.",
                "Sürüm-notları popup'ı eklendi.",
                "Gömülü Adwaita ikonları eklendi (59 SVG).",
                "Kapak kalite değişiminde önbellek-temizleme uyarısı eklendi.",
            ],
        )),
        _ => None,
    }
}

#[cfg(test)]
mod decide_retry_tests {
    use super::App;
    use crate::api::Title;

    #[test]
    fn decide_retry_source_error_retries() {
        let (retry, playing) = App::decide_retry(true, false, true);
        assert!(retry, "kaynak hatası yeniden denenmeli");
        assert!(!playing);
    }

    #[test]
    fn decide_retry_user_close_no_retry() {
        let (retry, playing) = App::decide_retry(true, true, true);
        assert!(!retry);
        assert!(playing);
    }

    #[test]
    fn decide_retry_not_exited_no_retry() {
        let (retry, playing) = App::decide_retry(false, false, true);
        assert!(!retry);
        assert!(playing);
    }

    #[test]
    fn decide_retry_never_opened_no_retry_flag() {
        let (retry, playing) = App::decide_retry(true, false, false);
        assert!(!retry);
        assert!(!playing);
    }

    #[test]
    fn slow_source_within_window_is_not_killed() {
        assert!(
            !App::source_is_dead(5, true, 0.0, false, 20),
            "5sn'de öldürülmemeli"
        );
        assert!(
            !App::source_is_dead(19, true, 0.0, false, 20),
            "19sn'de hâlâ sabırlı olunmalı"
        );
    }

    #[test]
    fn never_loaded_idle_source_is_dead_after_threshold() {
        assert!(
            App::source_is_dead(20, true, 0.0, false, 20),
            "20sn+idle+dur=0 -> ölü"
        );
        assert!(App::source_is_dead(120, true, 0.0, false, 20));
    }

    #[test]
    fn threshold_is_user_configurable() {
        assert!(
            !App::source_is_dead(45, true, 0.0, false, 90),
            "90sn sabırda 45sn ölü sayılmamalı"
        );
        assert!(
            App::source_is_dead(90, true, 0.0, false, 90),
            "90sn sabırda eşikte ölü"
        );
    }

    #[test]
    fn loaded_source_is_never_killed_by_dead_check() {
        assert!(!App::source_is_dead(600, true, 1435.0, true, 20));
        assert!(
            !App::source_is_dead(600, true, 0.0, true, 20),
            "yüklendiyse duration anlık 0 okunsa bile"
        );
    }

    #[test]
    fn playing_source_or_unknown_duration_not_dead() {
        assert!(!App::source_is_dead(600, false, 0.0, false, 20));
        assert!(!App::source_is_dead(600, true, 12.0, false, 20));
    }

    #[test]
    fn hero_pool_orders_and_dedups() {
        let t = |id: u64, rating: Option<f64>, year: Option<i64>, genres: &[&str]| Title {
            id,
            name: format!("t{id}"),
            rating,
            year,
            poster: Some("p".to_string()),
            genres: Some(genres.iter().map(|s| s.to_string()).collect()),
            ..Default::default()
        };
        let cats = vec![
            crate::api::Category {
                name: "a".to_string(),
                items: vec![
                    t(1, Some(8.5), Some(2023), &["action"]),
                    t(2, None, Some(1999), &["action"]),
                    t(3, None, None, &["action"]),
                ],
            },
            crate::api::Category {
                name: "b".to_string(),
                items: vec![
                    t(1, Some(8.5), Some(2023), &["action"]), // tekrar
                    Title {
                        id: 4,
                        name: "t4".to_string(),
                        ..Default::default()
                    }, // görsel yok
                ],
            },
        ];
        // t(4)'ün posteri yok → elenir; t(1) bir kez.
        let pool = super::hero_pool(&cats);
        let ids: Vec<u64> = pool.iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![1, 2, 3], "puan+yıl öne, tekrar/görsel-elendi");
    }

    #[test]
    fn hero_pool_skips_certain_empty_anime_but_not_movies() {
        let t = |id: u64, eps: Option<i64>, tt: Option<&str>| Title {
            id,
            name: format!("t{id}"),
            poster: Some("p".to_string()),
            episode_count: eps,
            title_type: tt.map(|s| s.to_string()),
            ..Default::default()
        };
        let cats = vec![crate::api::Category {
            name: "a".to_string(),
            items: vec![
                t(1, Some(0), None),          // kesin boş anime → elenir
                t(2, Some(0), Some("movie")), // film muaf → kalır
                t(3, Some(12), None),         // bölümlü → kalır
                t(4, None, None),             // bilinmeyen → kalır (arkada)
            ],
        }];
        let pool = super::hero_pool(&cats);
        let ids: Vec<u64> = pool.iter().map(|x| x.id).collect();
        assert!(!ids.contains(&1), "bölümsüz anime elendi");
        assert!(ids.contains(&2), "film muaf");
        assert!(ids.contains(&3) && ids.contains(&4), "diğerleri durur");
        assert_eq!(*ids.last().unwrap(), 4, "bilinmeyen en arkada");
    }

    #[test]
    fn pick_spotlight_table() {
        let pool: Vec<Title> = (0..10)
            .map(|i| Title {
                id: i,
                name: format!("t{i}"),
                ..Default::default()
            })
            .collect();
        // Aynı tohum → aynı sıra.
        let a = super::pick_spotlight(&pool, 8, 42);
        let b = super::pick_spotlight(&pool, 8, 42);
        assert_eq!(
            a.iter().map(|t| t.id).collect::<Vec<_>>(),
            b.iter().map(|t| t.id).collect::<Vec<_>>()
        );
        // Slot sayısı respected.
        assert_eq!(super::pick_spotlight(&pool, 8, 42).len(), 8);
        assert_eq!(super::pick_spotlight(&pool, 99, 42).len(), 10);
        assert!(super::pick_spotlight(&[], 8, 42).is_empty());
        // Farklı tohum → farklı sıra (10 öğede pratikte kesin).
        let c = super::pick_spotlight(&pool, 8, 1337);
        assert_ne!(
            a.iter().map(|t| t.id).collect::<Vec<_>>(),
            c.iter().map(|t| t.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn card_quantum_table() {
        // 1280px: 7 sütun, 160px kart.
        assert_eq!(super::card_quantum(1280), (7, 160));
        // Dar: 3 sütun tabanı, 140px tabanı.
        assert_eq!(super::card_quantum(500), (3, 140));
        // Geniş: 8 sütun tavanı, 220px tavanı.
        let (c, w) = super::card_quantum(3000);
        assert_eq!(c, 8);
        assert_eq!(w, 220);
    }

    #[test]
    fn hero_h_table() {
        // Lowell hero_h_for_window vektörleri birebir.
        assert_eq!(super::hero_h_for_window(885, 8), 460);
        assert_eq!(super::hero_h_for_window(800, 5), 400);
        assert_eq!(super::hero_h_for_window(680, 3), 360);
        assert_eq!(super::hero_h_for_window(2000, 8), 560);
    }

    #[test]
    fn socket_timeout_only_when_socket_never_seen() {
        assert!(
            App::socket_timeout_hit(26, false),
            "soket hiç gelmedi + 25sn doldu -> vazgeç"
        );
        assert!(!App::socket_timeout_hit(24, false), "henüz süre dolmadı");
        assert!(
            !App::socket_timeout_hit(600, true),
            "soket varken zaman aşımı uygulanmaz"
        );
    }

    #[test]
    fn changelog_known_and_unknown() {
        assert!(super::changelog_items("1.2.6").is_some());
        assert!(super::changelog_items("9.9.9").is_none());
    }

    #[test]
    fn page_history_is_bounded() {
        let mut st = Vec::new();
        for i in 0..(super::MAX_PAGE_HISTORY + 10) {
            let page = if i % 2 == 0 {
                super::Page::Home
            } else {
                super::Page::Favs
            };
            super::push_page_history(&mut st, page);
        }
        assert_eq!(st.len(), super::MAX_PAGE_HISTORY);
        assert_eq!(st.last(), Some(&super::Page::Favs));
    }

    #[test]
    fn anime4k_normal_maps_to_bundled_cnn_shader() {
        let p = super::resolve_upscale_shader("Anime4K_Upscale_CNN_x2_M.glsl");
        assert!(
            p.is_some(),
            "normal modu için CNN_x2_M shader'ı bundle edilmiş olmalı"
        );
    }
}

#[cfg(test)]
mod search_caret_tests {
    use super::set_search_text_caret_end;
    use gtk::prelude::*;

    #[gtk::test]
    fn search_text_sets_caret_to_end() {
        let entry = gtk::SearchEntry::new();
        set_search_text_caret_end(&entry, "frieren");
        assert_eq!(entry.position(), 7);
    }

    #[gtk::test]
    fn search_text_empty_keeps_caret_at_zero() {
        let entry = gtk::SearchEntry::new();
        set_search_text_caret_end(&entry, "");
        assert_eq!(entry.position(), 0);
    }
}

#[cfg(test)]
mod episode_bar_tests {
    use super::episode_bar_display;

    #[test]
    fn watched_is_always_full() {
        assert_eq!(
            episode_bar_display(true, 10.0, 100.0),
            (1.0, Some("İzlendi".to_string()))
        );
        assert_eq!(
            episode_bar_display(true, 0.0, 0.0),
            (1.0, Some("İzlendi".to_string()))
        );
    }

    #[test]
    fn unwatched_uses_fraction() {
        let (fraction, text) = episode_bar_display(false, 30.0, 100.0);
        assert_eq!(fraction, 0.3);
        assert_eq!(text.as_deref(), Some("0:30 / 1:40"));
    }

    #[test]
    fn unwatched_without_duration_hides() {
        assert_eq!(episode_bar_display(false, 0.0, 0.0), (0.0, None));
    }
}

#[cfg(test)]
mod play_lifecycle_tests {
    use super::{opening_finish_delay, play_terminal, PlayTerminal};
    use std::time::Duration;

    #[test]
    fn play_terminal_media_is_done() {
        assert_eq!(play_terminal(true), PlayTerminal::Done);
    }

    #[test]
    fn play_terminal_no_media_is_failed() {
        assert_eq!(play_terminal(false), PlayTerminal::Failed);
    }

    #[test]
    fn opening_finish_delay_boundaries() {
        assert_eq!(
            opening_finish_delay(Duration::ZERO),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            opening_finish_delay(Duration::from_secs(5)),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            opening_finish_delay(Duration::from_millis(9_999)),
            Some(Duration::from_millis(1))
        );
        assert_eq!(opening_finish_delay(Duration::from_secs(10)), None);
        assert_eq!(opening_finish_delay(Duration::from_secs(11)), None);
    }
}
