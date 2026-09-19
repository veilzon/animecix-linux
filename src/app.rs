use gtk::prelude::*;
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::api::{self, Client, Episode, Title};
use crate::covers::CoverManager;

use crate::ui::components;
use crate::ui::episodes_view;
use crate::ui::views;


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
    Episodes { title: Title, eps: Vec<Episode> },
    Movie { title: Title, eps: Vec<Episode> },
}

/// Çözüm paketi: oynatma adayları + video öncesi çözülmüş atlama planı.
pub struct PlaySources {
    pub candidates: Vec<String>,
    pub fast_embeds: Vec<String>,
    pub fallback_embeds: Vec<String>,
    pub plan: Option<crate::skip::SkipPlan>,
}

pub enum Msg {
    Cats(Result<Vec<api::Category>, String>),
    Search(Result<Vec<Title>, String>),
    Eps(Title, Result<Vec<Episode>, String>),
    Play(Title, Episode, Result<PlaySources, String>),
    FansubsLoaded {
        title: Title,
        ep: Episode,
        fansubs: Result<Vec<api::FansubInfo>, String>,
        default_template: Option<i64>,
    },
    FansubChosen {
        title: Title,
        ep: Episode,
        chosen: Option<api::FansubInfo>,
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

pub struct App {
    pub window: adw::ApplicationWindow,
    pub stack: gtk::Stack,
    pub back_btn: gtk::Button,
    pub tools_menu: Rc<RefCell<Option<crate::ui::tools_menu::ToolsMenu>>>,
    pub search_toggle_btn: gtk::Button,
    pub title_label: gtk::Label,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub loading: gtk::Box,
    pub toast: adw::ToastOverlay,
    pub client: Arc<Client>,
    pub covers: CoverManager,
    pub page_history: Rc<RefCell<Vec<Page>>>,
    pub cats: Rc<RefCell<Vec<api::Category>>>,
    pub search_results: Rc<RefCell<Vec<Title>>>,
    pub settings: Rc<RefCell<api::Settings>>,
    pub progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    pub progress_bars: Rc<RefCell<HashMap<String, (gtk::ProgressBar, gtk::Label)>>>,
    pub dl_rows: Rc<RefCell<HashMap<String, crate::ui::downloads_view::DlRow>>>,
    pub loading_toast: Rc<RefCell<Option<adw::Toast>>>,
    pub opening_toast: Rc<RefCell<Option<adw::Toast>>>,
    pub opening_toast_shown_at: Rc<RefCell<Option<std::time::Instant>>>,
    pub loading_gen: Rc<Cell<u32>>,
    pub home_acts: Rc<RefCell<Vec<Option<usize>>>>,
    pub dl_manager: crate::download::DownloadManager,
    /// Bölüm hızlı-arama tuşu: sayfa başına tek controller (birikmeyi önler).
    pub ep_search_controller: Rc<RefCell<Option<gtk::EventControllerKey>>>,
}

/// Geri-dönüş geçiş bayrağı (switch içinde tüketilir).
static BACK_ANIM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// İnternet durumu (60sn TTL önbellekli; geri-dönüşte senkron ağı engeller).
fn cached_internet_status() -> crate::api::InternetStatus {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    static CACHE: OnceLock<Mutex<(Option<Instant>, Option<crate::api::InternetStatus>)>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((None, None)));
    if let Ok(guard) = cache.lock() {
        if let (Some(at), Some(st)) = (&guard.0, &guard.1) {
            if at.elapsed() < Duration::from_secs(60) {
                return st.clone();
            }
        }
    }
    let st = crate::api::check_internet();
    if let Ok(mut guard) = cache.lock() {
        *guard = (Some(Instant::now()), Some(st.clone()));
    }
    st
}

pub(crate) fn resolve_upscale_shader(name: &str) -> Option<String> {    use std::sync::OnceLock;
    use std::sync::Mutex;

    // Embedded shader içeriği (binary'ye gömülü, AppImage extract'ten bağımsız).
    fn embedded(name: &str) -> Option<&'static str> {
        match name {
            "Anime4K_Upscale_CNN_x2_M.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_CNN_x2_M.glsl")),
            "Anime4K_Upscale_CNN_x2_UL.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_CNN_x2_UL.glsl")),
            "Anime4K_Upscale_DTD_x2.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_DTD_x2.glsl")),
            "Anime4K_Upscale_Original_x2.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_Original_x2.glsl")),
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
            candidates.push(std::path::Path::new(&ad).join("usr/share/animecix/assets/upscale").join(name));
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
    candidates.into_iter().find(|p| p.exists()).map(|p| p.to_string_lossy().into_owned())
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

impl App {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let client = Arc::new(Client::new());

        {
            let cl = client.clone();
            std::thread::spawn(move || {
                cl.warmup();
                let _ = cl.home_lists();
            });
        }
        let welcome_seen = client.is_welcome_seen();

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

        let search_toggle_btn = gtk::Button::from_icon_name("system-search-symbolic");
        search_toggle_btn.add_css_class("flat");
        search_toggle_btn.add_css_class("circular");
        search_toggle_btn.set_tooltip_text(Some("Arama Yap"));
        header.pack_end(&search_toggle_btn);
        // Sayfalar butonu App::new sonunda (goto kablosu Rc gerektirir) eklenir.

        let search_entry = gtk::SearchEntry::new();
        search_entry.set_placeholder_text(Some("Anime, dizi veya film ara…"));
        search_entry.set_hexpand(true);

        let search_bar = gtk::SearchBar::new();
        search_bar.set_child(Some(&search_entry));
        search_bar.connect_entry(&search_entry);
        search_bar.set_key_capture_widget(Some(&app.active_window().unwrap_or_default()));



        let main_stack = gtk::Stack::new();
        main_stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        main_stack.set_transition_duration(220);
        main_stack.set_vexpand(true);
        main_stack.set_hexpand(true);

        let loading = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        loading.add_css_class("card");
        loading.set_halign(gtk::Align::Center);
        loading.set_valign(gtk::Align::Start);
        loading.set_margin_top(12);
        loading.set_margin_bottom(12);
        loading.set_margin_start(16);
        loading.set_margin_end(16);

        let spin = gtk::Spinner::new();
        spin.start();
        let l_lbl = gtk::Label::new(Some("Yükleniyor…"));
        l_lbl.add_css_class("title-4");
        loading.append(&spin);
        loading.append(&l_lbl);
        loading.set_visible(false);

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&main_stack));
        overlay.add_overlay(&loading);
        overlay.set_vexpand(true);
        overlay.set_hexpand(true);

        let header_for_tools = header.clone();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&search_bar);
        content.append(&overlay);

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&content));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("AnimeciX")
            .default_width(980)
            .default_height(720)
            .resizable(false)
            .content(&toast)
            .build();

        let initial_page = if welcome_seen { Page::Home } else { Page::Welcome };

        let covers = CoverManager::new(client.clone());

        let (dl_tx, dl_rx) = std::sync::mpsc::channel::<crate::download::UiEvent>();
        let dl_manager =
            crate::download::DownloadManager::new(crate::download::queue_file_path(), dl_tx);

        let app_inst = Rc::new(Self {
            window,
            stack: main_stack,
            back_btn,
            tools_menu: Rc::new(RefCell::new(None)),
            search_toggle_btn,
            title_label,
            search_bar,
            search_entry,
            loading,
            toast,
            client: client.clone(),
            covers,
            page_history: Rc::new(RefCell::new(vec![initial_page.clone()])),
            cats: Rc::new(RefCell::new(Vec::new())),
            search_results: Rc::new(RefCell::new(Vec::new())),
            settings: Rc::new(RefCell::new(client.load_settings())),
            progress: Rc::new(RefCell::new(client.load_state().progress)),
            progress_bars: Rc::new(RefCell::new(HashMap::new())),
            dl_rows: Rc::new(RefCell::new(HashMap::new())),
            loading_toast: Rc::new(RefCell::new(None)),
            opening_toast: Rc::new(RefCell::new(None)),
            opening_toast_shown_at: Rc::new(RefCell::new(None)),
            loading_gen: Rc::new(Cell::new(0)),
            home_acts: Rc::new(RefCell::new(Vec::new())),
            dl_manager,
            ep_search_controller: Rc::new(RefCell::new(None)),
        });
        {
            // Sayfalar menüsü: goto kablosu Rc gerektirdiği için burada kurulur.
            let inst = app_inst.clone_ref();
            let goto: Rc<dyn Fn(usize)> = Rc::new(move |i| {
                let target = match i {
                    0 => Page::Home,
                    1 => Page::Favs,
                    2 => Page::Marathon,
                    3 => Page::History,
                    4 => Page::Downloads,
                    _ => Page::Settings,
                };
                let mut st = inst.page_history.borrow_mut();
                if st.last() != Some(&target) {
                    st.push(target.clone());
                }
                drop(st);
                inst.show_page(&target);
                if target == Page::Home {
                    inst.fetch_home();
                }
            });
            let settings_c = app_inst.settings.clone();
            let shortcut_label: Rc<dyn Fn() -> String> =
                Rc::new(move || settings_c.borrow().tools_shortcut.clone());
            let menu = crate::ui::tools_menu::ToolsMenu::build(
                goto,
                app_inst.toast.clone(),
                app_inst.client.clone(),
                shortcut_label,
            );
            header_for_tools.pack_end(&menu.button());
            *app_inst.tools_menu.borrow_mut() = Some(menu);
        }
        {
            // Aicix init deferred to Aşama 2
        }

        app_inst.chain_signals();
        app_inst.apply_ui_scale();
        crate::theme::apply_theme(&app_inst.window, &app_inst.settings.borrow().theme);
        crate::ui::tools_menu::ensure_tools_css();
        {
            // İndirme pompası: kuyruk olaylarını arayüze taşır.
            let pump = app_inst.clone_ref();
            let rx = std::sync::Arc::new(std::sync::Mutex::new(dl_rx));
            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let mut progress_dirty = false;
                let mut struct_dirty = false;
                let mut toasts: Vec<String> = Vec::new();
                loop {
                    let ev = rx.lock().unwrap().try_recv();
                    match ev {
                        Ok(crate::download::UiEvent::Tick) => progress_dirty = true,
                        Ok(crate::download::UiEvent::Changed) => struct_dirty = true,
                        Ok(crate::download::UiEvent::Toast(m)) => toasts.push(m),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                for m in toasts {
                    let t = adw::Toast::new(&m);
                    t.set_timeout(3);
                    pump.toast.add_toast(t);
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
        app_inst.show_page(&initial_page);
        if welcome_seen {
            app_inst.fetch_home();
        }
        app_inst.apply_goto_arg();
        app_inst
    }

    pub fn clone_ref(&self) -> Rc<Self> {
        Rc::new(Self {
            window: self.window.clone(),
            stack: self.stack.clone(),
            back_btn: self.back_btn.clone(),
            tools_menu: self.tools_menu.clone(),
            search_toggle_btn: self.search_toggle_btn.clone(),
            title_label: self.title_label.clone(),
            search_bar: self.search_bar.clone(),
            search_entry: self.search_entry.clone(),
            loading: self.loading.clone(),
            toast: self.toast.clone(),
            client: self.client.clone(),
            covers: self.covers.clone_ref(),
            page_history: self.page_history.clone(),
            cats: self.cats.clone(),
            search_results: self.search_results.clone(),
            settings: self.settings.clone(),
            progress: self.progress.clone(),
            progress_bars: self.progress_bars.clone(),
            dl_rows: self.dl_rows.clone(),
            loading_toast: self.loading_toast.clone(),
            opening_toast: self.opening_toast.clone(),
            opening_toast_shown_at: self.opening_toast_shown_at.clone(),
            loading_gen: self.loading_gen.clone(),
            home_acts: self.home_acts.clone(),
            dl_manager: self.dl_manager.clone(),
            ep_search_controller: self.ep_search_controller.clone(),
        })
    }

    fn chain_signals(&self) {
        let this = self.clone_ref();
        self.search_toggle_btn.connect_clicked(move |_| {
            let active = !this.search_bar.is_search_mode();
            this.search_bar.set_search_mode(active);
            if active {
                this.search_entry.grab_focus();
                if !this.client.is_search_tip_seen() {
                    this.client.set_search_tip_seen(true);
                    let sc = this.settings.borrow().search_shortcut.clone();
                    let toast = adw::Toast::new(&format!(
                        "💡 '{}' kısayolu ile arama çubuğunu hızlıca açabilirsiniz!",
                        glib::markup_escape_text(&sc)
                    ));
                    toast.set_timeout(4);
                    this.toast.add_toast(toast);
                }
            }
        });

        let this = self.clone_ref();
        self.back_btn.connect_clicked(move |_| {
            this.go_back();
        });

        let this = self.clone_ref();
        self.search_entry.connect_activate(move |e| {
            let q = e.text().to_string();
            if !q.trim().is_empty() {
                this.do_search(q);
            }
        });

        {
            let this = self.clone_ref();
            let search_bar = self.search_bar.clone();
            let search_entry = self.search_entry.clone();
            let settings = self.settings.clone();
            let window_c = self.window.clone();
            let tools_c = self.tools_menu.clone();
            let key_ctrl = gtk::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, keyval, _, state| {
                let sc = settings.borrow().search_shortcut.clone();
                let key_name = keyval.name().map(|s| s.to_string()).unwrap_or_default();
                let is_ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                let triggered = match sc.as_str() {
                    "Ctrl+K" => is_ctrl && (key_name == "k" || key_name == "K"),
                    "F2" => key_name == "F2",
                    "/" => key_name == "slash" || key_name == "kp_divide",
                    _ => is_ctrl && (key_name == "s" || key_name == "S"), // Ctrl+S
                };
                if triggered {
                    search_bar.set_search_mode(true);
                    search_entry.grab_focus();

                    if !this.client.is_search_tip_seen() {
                        this.client.set_search_tip_seen(true);
                    }
                    glib::Propagation::Stop
                } else {
                    // Sayfalar kısayolu (ayarlanabilir; çıplak T metin alanında yutulur).
                    let tsc = settings.borrow().tools_shortcut.clone();
                    let is_alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
                    let editable = gtk::prelude::GtkWindowExt::focus(&window_c)
                        .map(|f| {
                            f.is::<gtk::SearchEntry>()
                                || f.is::<gtk::Entry>()
                                || f.is::<gtk::Text>()
                                || f.is::<gtk::SpinButton>()
                                || f.is::<gtk::PasswordEntry>()
                        })
                        .unwrap_or(false);
                    if crate::ui::tools_menu::match_tools_shortcut(
                        &tsc, &key_name, is_ctrl, is_alt, editable,
                    ) {
                        if let Some(menu) = tools_c.borrow().as_ref() {
                            let toast_c = this.toast.clone();
                            let client_c = this.client.clone();
                            let settings_c2 = settings.clone();
                            let lbl: Rc<dyn Fn() -> String> = Rc::new(move || {
                                settings_c2.borrow().tools_shortcut.clone()
                            });
                            if menu.is_open() {
                                menu.close();
                            } else {
                                menu.open(&toast_c, &client_c, &lbl);
                            }
                        }
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
            });
            self.window.add_controller(key_ctrl);
        }
    }

    pub fn go_back(&self) {
        BACK_ANIM.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut st = self.page_history.borrow_mut();        if st.len() > 1 {
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
        let gen = self.loading_gen.get() + 1;
        self.loading_gen.set(gen);
        self.loading.set_visible(false);

        if on {
            if let Some(t) = self.loading_toast.borrow_mut().take() {
                t.dismiss();
            }
            let t = adw::Toast::new("Yükleniyor…");
            t.set_timeout(0);
            self.toast.add_toast(t.clone());
            *self.loading_toast.borrow_mut() = Some(t);
        } else if let Some(t) = self.loading_toast.borrow_mut().take() {
            t.dismiss();
        }
    }

    pub fn refresh_internet_status(&self) {
        // Ana sayfayı yeniden inşa eder; build_home_view yeniden kontrol eder
        let stack = self.stack.clone();
        let this = self.clone_ref();
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(50),
            move || {
                let widget = this.build_home_view();
                if let Some(prev) = stack.child_by_name("home") {
                    stack.remove(&prev);
                }
                stack.add_named(&widget, Some("home"));
                stack.set_visible_child_name("home");
            },
        );
    }

    fn apply_ui_scale(&self) {
        let s = self.settings.borrow().ui_scale;
        self.window.remove_css_class("ui-scale-125");
        self.window.remove_css_class("ui-scale-150");
        if (s - 1.25).abs() < 0.01 {
            self.window.add_css_class("ui-scale-125");
        } else if s >= 1.4 {
            self.window.add_css_class("ui-scale-150");
        }
    }

    fn apply_movie_tint(&self, target: &gtk::Box, poster: Option<&str>) {
        let Some(url) = poster.map(|s| s.to_string()) else { return };
        let client = self.client.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Option<[(u8, u8, u8); 3]>>();
        std::thread::spawn(move || {
            let _ = tx.send(client.cover_palette(&url));
        });
        let weak = target.downgrade();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok(pal) => {
                let Some(root) = weak.upgrade() else { return glib::ControlFlow::Break };
                let [c1, c2, c3] =
                    pal.unwrap_or([(122, 162, 247), (55, 70, 110), (140, 110, 190)]);
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
                let css_b = format!(
                    "#movie-tint-root {{ background-color: rgba({r2},{g2},{b2},0.35); \
                     background: radial-gradient(ellipse at 50% 100%, \
                     rgba({r3},{g3},{b3},0.30), rgba(0,0,0,0) 70%), \
                     linear-gradient(315deg, rgba({r3},{g3},{b3},0.30), \
                     rgba({r1},{g1},{b1},0.20) 55%, rgba({r2},{g2},{b2},0.30)); }}"
                );
                let prov_a = gtk::CssProvider::new();
                prov_a.load_from_data(&css_a);
                let prov_b = gtk::CssProvider::new();
                prov_b.load_from_data(&css_b);
                root.set_widget_name("movie-tint-root");
                let display = root.display();
                gtk::style_context_add_provider_for_display(
                    &display,
                    &prov_a,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                let weak2 = root.downgrade();
                let disp2 = display.clone();
                let (pa_c, pb_c) = (prov_a.clone(), prov_b.clone());
                let showing_a = Rc::new(Cell::new(true));
                glib::timeout_add_local(std::time::Duration::from_secs(5), move || {
                    if weak2.upgrade().is_none() {
                        gtk::style_context_remove_provider_for_display(&disp2, &pa_c);
                        gtk::style_context_remove_provider_for_display(&disp2, &pb_c);
                        return glib::ControlFlow::Break;
                    }
                    if showing_a.get() {
                        gtk::style_context_remove_provider_for_display(&display, &prov_a);
                        gtk::style_context_add_provider_for_display(
                            &display,
                            &prov_b,
                            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                        );
                    } else {
                        gtk::style_context_remove_provider_for_display(&display, &prov_b);
                        gtk::style_context_add_provider_for_display(
                            &display,
                            &prov_a,
                            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                        );
                    }
                    showing_a.set(!showing_a.get());
                    glib::ControlFlow::Continue
                });
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
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(400),
                move || {
                    let Some(visible) = stack_c.visible_child() else { return; };
                    let mut to_rm = vec![];
                    let mut cur = stack_c.first_child();
                    while let Some(child) = cur {
                        let next = child.next_sibling();
                        if child != visible {
                            to_rm.push(child);
                        }
                        cur = next;
                    }
                    for c in to_rm {
                        stack_c.remove(&c);
                    }
                },
            );
        }

        match page {
            Page::Welcome => {
                self.title_label.set_text("Hoş Geldiniz");
                switch(&self.stack, "welcome", gtk::StackTransitionType::Crossfade, self.build_welcome_view());
            }
            Page::Home => {
                self.title_label.set_text("AnimeciX");
                switch(&self.stack, "home", gtk::StackTransitionType::Crossfade, self.build_home_view());
            }
            Page::Favs => {
                self.title_label.set_text("Favorilerim");
                switch(&self.stack, "favs", gtk::StackTransitionType::Crossfade, self.build_favs_view());
            }
            Page::Marathon => {
                self.title_label.set_text("İzleme Maratonum 🏃‍♂️");
                switch(&self.stack, "marathon", gtk::StackTransitionType::Crossfade, self.build_marathon_view());
            }
            Page::History => {
                self.title_label.set_text("İzleme Geçmişi");
                switch(&self.stack, "history", gtk::StackTransitionType::Crossfade, self.build_history_view());
            }
            Page::Downloads => {
                self.title_label.set_text("İndirilenler");
                switch(&self.stack, "downloads", gtk::StackTransitionType::Crossfade, self.build_downloads_view());
            }
            Page::Settings => {
                self.title_label.set_text("Ayarlar");
                switch(&self.stack, "settings", gtk::StackTransitionType::Crossfade, self.build_settings_view());
            }
            Page::Search => {
                self.title_label.set_text("Arama Sonuçları");
                switch(&self.stack, "search", gtk::StackTransitionType::SlideLeft, self.build_search_view());
            }
            Page::Episodes { title, eps } | Page::Movie { title, eps } => {
                self.title_label.set_text(&title.name);
                let page_name = format!("eps_{}", title.id);
                switch(&self.stack, &page_name, gtk::StackTransitionType::SlideLeft, self.build_episodes_view(title, eps));
            }
        }

        // Odak iadesi: PgUp/PgDn/ok tuşları odak ister, hover yetmez.
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
                self.search_bar.set_search_mode(true);
                self.search_entry.set_text("Tokyo");
                let q = self.search_entry.text().to_string();
                self.do_search(q);
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
        crate::ui::welcome::WelcomeView::build(
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
                let t = adw::Toast::new(&msg);
                t.set_timeout(3);
                this_t.toast.add_toast(t);
            },
            move |theme_id| {
                crate::theme::apply_theme(&this_p.window, &theme_id);
            },
        )
    }

    /// Başlık kartı (kapak hemen yüklenir).
    fn create_title_card(&self, t: &Title) -> gtk::Box {
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 6);
        box_.add_css_class("title-btn");
        box_.set_size_request(140, -1);
        box_.set_hexpand(false);
        box_.set_vexpand(false);
        box_.set_halign(gtk::Align::Start);
        box_.set_valign(gtk::Align::Start);

        let pic = self.covers.cover_picture(t.poster.as_deref(), 140, 210);
        pic.set_size_request(140, 210);
        pic.set_can_shrink(false);
        pic.set_hexpand(false);
        pic.set_vexpand(false);
        pic.set_halign(gtk::Align::Start);

        let lbl = gtk::Label::new(Some(&t.name));
        lbl.add_css_class("card-title");
        lbl.set_wrap(true);
        lbl.set_justify(gtk::Justification::Center);
        lbl.set_xalign(0.5);
        lbl.set_max_width_chars(16);
        lbl.set_lines(2);
        lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

        box_.append(&pic);
        box_.append(&lbl);

        let gesture = gtk::GestureClick::new();
        let this = self.clone_ref();
        let title_clone = t.clone();
        gesture.connect_pressed(move |_, _, _, _| {
            this.open_episodes(title_clone.clone());
        });
        box_.add_controller(gesture);

        box_
    }

    fn build_home_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scroll.set_child(Some(&outer));

        // İnternet bağlantı uyarısı (offline ise; 60sn önbellekli, geri-dönüş donmaz).
        match cached_internet_status() {
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

        // Hızlı arama hapı: ortada, ilk rafın üstünde.
        {
            let pill = gtk::SearchEntry::new();
            pill.add_css_class("tools-search-pill");
            pill.set_placeholder_text(Some("Hızlı ara… (Enter)"));
            pill.set_halign(gtk::Align::Center);
            let this = self.clone_ref();
            pill.connect_activate(move |e| {
                let q = e.text().to_string();
                if !q.trim().is_empty() {
                    this.do_search(q);
                }
            });
            main_box.append(&pill);
        }

        for cat in cats.iter() {
            let shelf_title = gtk::Label::new(Some(&cat.name));
            shelf_title.add_css_class("shelf-title");
            shelf_title.set_xalign(0.0);
            shelf_title.set_margin_start(4);
            shelf_title.set_margin_bottom(4);
            main_box.append(&shelf_title);

            let flow = gtk::FlowBox::new();
            flow.set_halign(gtk::Align::Center);
            flow.set_valign(gtk::Align::Start);
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_activate_on_single_click(false);
            flow.set_column_spacing(16);
            flow.set_row_spacing(20);

            for t in &cat.items {
                let btn = self.create_title_card(t);
                flow.append(&btn);
            }
            main_box.append(&flow);
        }

        outer.append(&main_box);
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
                let item = this_toggle.client.get_marathon().into_iter().find(|m| m.title.id == id);
                let Some(item) = item else { return; };
                if item.completed {
                    this_toggle.client.mark_title_unwatched(id);
                    this_toggle.client.set_marathon_completed(id, false);
                    let toast = adw::Toast::new("⏳ Tüm bölümler izlenmedi olarak işaretlendi");
                    toast.set_timeout(2);
                    this_toggle.toast.add_toast(toast);
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
                                let toast = adw::Toast::new(&format!("🏁 {n} bölüm izlendi olarak işaretlendi!"));
                                toast.set_timeout(2);
                                this_async.toast.add_toast(toast);
                            }
                            _ => {
                                let toast = adw::Toast::new("❌ Bölüm listesi alınamadı (internete bağlı mısın?)");
                                toast.set_timeout(3);
                                this_async.toast.add_toast(toast);
                            }
                        }
                        this_async.show_page(&Page::Marathon);
                        glib::ControlFlow::Break
                    }
                });
            },
            move |id| {
                this_remove.client.remove_from_marathon(id);
                let toast = adw::Toast::new("Maratondan kaldırıldı");
                toast.set_timeout(2);
                this_remove.toast.add_toast(toast);
                this_remove.show_page(&Page::Marathon);
            },
            move || {
                this_clear.client.clear_marathon();
                let toast = adw::Toast::new("İzleme maratonu temizlendi");
                toast.set_timeout(2);
                this_clear.toast.add_toast(toast);
                this_clear.show_page(&Page::Marathon);
            },
            move |id, new_index| {
                this_reorder.client.reorder_marathon(id, new_index);
                this_reorder.show_page(&Page::Marathon);
            },
            move |poster, pic, w, h| {
                this_cover.covers.load_cover(poster, &pic, w, h);
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

        let scroll_t = scroll.clone();
        let timer_state = drag_pos.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
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
        let history = self.client.load_state().history;
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
        let (scroll, rows) = crate::ui::downloads_view::DownloadsView::build(
            &self.dl_manager,
            self.effective_download_dir(),
        );
        *self.dl_rows.borrow_mut() = rows;
        scroll
    }

    fn build_settings_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let settings = self.settings.borrow();
        let this_save = self.clone_ref();
        let this_wipe = self.clone_ref();

        let last_save: Rc<RefCell<std::time::Instant>> = Rc::new(RefCell::new(std::time::Instant::now()));
        let last_save_c = last_save.clone();
        let view = views::SettingsView::build(
            &settings,
            move |new_s| {
                *this_save.settings.borrow_mut() = new_s.clone();
                this_save.client.save_settings(&new_s);
                this_save.client.set_cf_clearance(&new_s.cf_clearance);
                this_save.apply_ui_scale();
                crate::theme::apply_theme(&this_save.window, &new_s.theme);
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(*last_save_c.borrow()).as_millis();
                *last_save_c.borrow_mut() = now;
                if elapsed >= 400 {
                    let toast = adw::Toast::new("Ayarlar kaydedildi");
                    toast.set_timeout(2);
                    this_save.toast.add_toast(toast);
                }
            },
            move |remove_app| {
                this_wipe.client.wipe_all_data();
                if remove_app {
                    crate::uninstall_application();
                    std::process::exit(0);
                } else {
                    let toast = adw::Toast::new("Tüm veriler temizlendi ve sıfırlandı!");
                    toast.set_timeout(3);
                    this_wipe.toast.add_toast(toast);
                    this_wipe.page_history.borrow_mut().clear();
                    this_wipe.page_history.borrow_mut().push(Page::Welcome);
                    this_wipe.show_page(&Page::Welcome);
                }
            },
        );

        scroll.set_child(Some(&view));
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

    fn build_episodes_view(&self, title: &Title, eps: &[Episode]) -> gtk::Overlay {
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");

        let is_movie = title.title_type.as_deref() == Some("movie")
            || (eps.len() <= 1 && eps.first().map(|e| e.name.contains("Filmi")).unwrap_or(false));

        if is_movie {
            let header_poster = self.covers.cover_picture(title.poster.as_deref(), 220, 330);
            let bookmark_btn = components::bookmark_button(&self.client, title);
            let this_bm = self.clone_ref();
            let t_clone = title.clone();
            bookmark_btn.connect_clicked(move |b| {
                let saved = this_bm.client.toggle_saved(&t_clone);
                b.set_icon_name(if saved { "starred-symbolic" } else { "non-starred-symbolic" });
                b.set_tooltip_text(Some(if saved { "Favorilerden Çıkar" } else { "Favorilere Ekle" }));
            });

            let marathon_btn = components::marathon_button(&self.client, title);
            let this_mar = self.clone_ref();
            let t_clone_mar = title.clone();
            marathon_btn.connect_clicked(move |b| {
                let added = this_mar.client.toggle_marathon(&t_clone_mar);
                b.set_icon_name(if added { "media-playlist-repeat-symbolic" } else { "flag-symbolic" });
                b.set_tooltip_text(Some(if added { "Maratondan Çıkar" } else { "İzleme Maratonuna Ekle" }));
                let msg = if added { "🏆 İzleme Maratonuna eklendi!" } else { "İzleme Maratonundan çıkarıldı" };
                let toast = adw::Toast::new(msg);
                toast.set_timeout(2);
                this_mar.toast.add_toast(toast);
            });

            let this_play = self.clone_ref();
            let title_c = title.clone();
            let ep_c = eps.first().cloned().unwrap_or(Episode {
                episode: 1,
                season: 1,
                name: title.name.clone(),
            });
            let movie_progress = self.client.get_progress(title.id, 1, 1);
            let (movie_view, movie_pb, movie_lbl) = episodes_view::create_movie_detail_view(
                title,
                &header_poster,
                &bookmark_btn,
                &marathon_btn,
                movie_progress,
                move || {
                    this_play.play(&title_c, &ep_c);
                },
            );
            let prog_key = format!("{}:1:1", title.id);
            self.progress_bars.borrow_mut().insert(prog_key, (movie_pb, movie_lbl));
            movie_view.add_css_class("movie-tint");
            self.apply_movie_tint(&movie_view, title.poster.as_deref());

            let dl_film = gtk::Button::from_icon_name("folder-download-symbolic");
            dl_film.add_css_class("circular");
            dl_film.set_halign(gtk::Align::End);
            dl_film.set_valign(gtk::Align::Start);
            dl_film.set_margin_top(16);
            dl_film.set_margin_end(16);
            dl_film.set_tooltip_text(Some("Filmi indir"));
            {
                let this_dl = self.clone_ref();
                let title_dl = title.clone();
                dl_film.connect_clicked(move |_| {
                    let this_q = this_dl.clone_ref();
                    let this2 = this_dl.clone_ref();
                    let title2 = title_dl.clone();
                    this_q.ask_download_quality(move |q| {
                        let Some(quality) = q else { return };
                        let title3 = title2.clone();
                        let dir = this2.effective_download_dir();
                        this2.busy(true);
                        this2.spawn(move |c| {
                            let res = c.resolve_movie(title3.id).map(|url| {
                                let series = crate::download::sanitize_filename(&title3.name);
                                let dest = dir.join(&series).join(format!(
                                    "{} [{}].mp4",
                                    series,
                                    crate::download::sanitize_filename(&quality)
                                ));
                                crate::download::DownloadRecord {
                                    id: format!("film:{}:{quality}", title3.id),
                                    title: series,
                                    season: 1,
                                    episode: 1,
                                    ep_name: title3.name.clone(),
                                    fansub: String::new(),
                                    quality: quality.clone(),
                                    url,
                                    referer: None,
                                    dest,
                                    total: 0,
                                    have: 0,
                                    status: crate::download::DownloadStatus::Queued,
                                }
                            });
                            move || match res {
                                Ok(rec) => Msg::DlBatchResolved(vec![rec], Vec::new(), true),
                                Err(e) => {
                                    eprintln!("[DL] film çözülemedi: {e}");
                                    Msg::DlBatchResolved(Vec::new(), vec![e], true)
                                }
                            }
                        });
                    });
                });
            }
            scroll.set_child(Some(&movie_view));
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&scroll));
            overlay.add_overlay(&dl_film);
            return overlay;
        }

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let header_poster = self.covers.cover_picture(title.poster.as_deref(), 120, 180);
        let bookmark_btn = components::bookmark_button(&self.client, title);
        let this_bm = self.clone_ref();
        let t_clone = title.clone();
        bookmark_btn.connect_clicked(move |b| {
            let saved = this_bm.client.toggle_saved(&t_clone);
            b.set_icon_name(if saved { "starred-symbolic" } else { "non-starred-symbolic" });
            b.set_tooltip_text(Some(if saved { "Favorilerden Çıkar" } else { "Favorilere Ekle" }));
        });

        let marathon_btn = components::marathon_button(&self.client, title);
        let this_mar = self.clone_ref();
        let t_clone_mar = title.clone();
        marathon_btn.connect_clicked(move |b| {
            let added = this_mar.client.toggle_marathon(&t_clone_mar);
            b.set_icon_name(if added { "media-playlist-repeat-symbolic" } else { "flag-symbolic" });
            b.set_tooltip_text(Some(if added { "Maratondan Çıkar" } else { "İzleme Maratonuna Ekle" }));
            let msg = if added { "🏆 İzleme Maratonuna eklendi!" } else { "İzleme Maratonundan çıkarıldı" };
            let toast = adw::Toast::new(msg);
            toast.set_timeout(2);
            this_mar.toast.add_toast(toast);
        });

        // Toplu indirme modu durumu.
        let dl_mode = Rc::new(Cell::new(false));
        let dl_checks: Rc<RefCell<Vec<(Episode, gtk::CheckButton)>>> =
            Rc::new(RefCell::new(Vec::new()));

        let dl_mode_btn = gtk::Button::from_icon_name("folder-download-symbolic");
        dl_mode_btn.add_css_class("flat");
        dl_mode_btn.add_css_class("circular");
        dl_mode_btn.add_css_class("lg-icon");
        dl_mode_btn.set_valign(gtk::Align::Center);
        dl_mode_btn.set_tooltip_text(Some("Toplu İndirme Modu"));

        let dl_float = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        dl_float.add_css_class("dl-float-pill");
        dl_float.set_margin_bottom(20);
        dl_float.set_margin_start(12);
        dl_float.set_margin_end(12);
        let dl_go = gtk::Button::with_label("⬇ İndir (0)");
        dl_go.add_css_class("suggested-action");
        dl_go.add_css_class("pill");
        dl_go.set_sensitive(false);
        let dl_cancel = gtk::Button::with_label("Vazgeç");
        dl_cancel.add_css_class("flat");
        dl_cancel.add_css_class("pill");
        dl_float.append(&dl_go);
        dl_float.append(&dl_cancel);

        // Toast gibi altta ortada beliren animasyonlu hap.
        let dl_reveal = gtk::Revealer::new();
        dl_reveal.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        dl_reveal.set_transition_duration(250);
        dl_reveal.set_halign(gtk::Align::Center);
        dl_reveal.set_valign(gtk::Align::End);
        dl_reveal.set_child(Some(&dl_float));
        dl_reveal.set_visible(false);

        let float_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
        let set_floating = {
            let dl_reveal = dl_reveal.clone();
            let float_gen = float_gen.clone();
            Rc::new(move |show: bool| {
                let g = float_gen.get() + 1;
                float_gen.set(g);
                if show {
                    dl_reveal.set_visible(true);
                    dl_reveal.set_reveal_child(true);
                } else {
                    dl_reveal.set_reveal_child(false);
                    let dl_reveal_c = dl_reveal.clone();
                    let gen_c = float_gen.clone();
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(260),
                        move || {
                            if gen_c.get() == g {
                                dl_reveal_c.set_visible(false);
                            }
                        },
                    );
                }
            })
        };

        let exit_dl_mode = {
            let dl_mode = dl_mode.clone();
            let dl_checks = dl_checks.clone();
            let hide = set_floating.clone();
            let dl_mode_btn = dl_mode_btn.clone();
            Rc::new(move || {
                dl_mode.set(false);
                for (_, c) in dl_checks.borrow().iter() {
                    c.set_active(false);
                    c.set_visible(false);
                }
                hide(false);
                dl_mode_btn.remove_css_class("suggested-action");
            })
        };

        let refresh_dl_bar = {
            let dl_mode = dl_mode.clone();
            let dl_checks = dl_checks.clone();
            let dl_go = dl_go.clone();
            let show = set_floating.clone();
            Rc::new(move || {
                let n = dl_checks.borrow().iter().filter(|(_, c)| c.is_active()).count();
                dl_go.set_label(&format!("⬇ İndir ({n})"));
                dl_go.set_sensitive(n > 0);
                show(dl_mode.get() && n > 0);
            })
        };

        {
            let dl_mode = dl_mode.clone();
            let dl_checks = dl_checks.clone();
            let btn_c = dl_mode_btn.clone();
            let btn_c2 = dl_mode_btn.clone();
            let refresh = refresh_dl_bar.clone();
            let exit = exit_dl_mode.clone();
            btn_c.connect_clicked(move |_| {
                if dl_mode.get() {
                    exit();
                } else {
                    dl_mode.set(true);
                    for (_, c) in dl_checks.borrow().iter() {
                        c.set_visible(true);
                    }
                    btn_c2.add_css_class("suggested-action");
                }
                refresh();
            });
        }
        {
            let exit = exit_dl_mode.clone();
            dl_cancel.connect_clicked(move |_| exit());
        }
        {
            let this_go = self.clone_ref();
            let title_go = title.clone();
            let dl_checks_go = dl_checks.clone();
            let exit = exit_dl_mode.clone();
            dl_go.connect_clicked(move |_| {
                let eps: Vec<Episode> = dl_checks_go
                    .borrow()
                    .iter()
                    .filter(|(_, c)| c.is_active())
                    .map(|(e, _)| e.clone())
                    .collect();
                if eps.is_empty() {
                    return;
                }
                exit();
                let this_q = this_go.clone_ref();
                let this2 = this_go.clone_ref();
                let title2 = title_go.clone();
                this_q.ask_download_quality(move |q| {
                    if let Some(quality) = q {
                        this2.start_download_prefetch(title2.clone(), eps.clone(), quality, false);
                    }
                });
            });
        }

        let detail_header = episodes_view::create_title_detail_header(title, &header_poster, &bookmark_btn, &marathon_btn, &dl_mode_btn);
        root.append(&detail_header);

        let settings = self.settings.borrow();

        if settings.quick_search_enabled && !self.client.is_quick_search_tip_seen() {
            let this_tip = self.clone_ref();
            let tip_banner = episodes_view::create_quick_search_tip_banner(
                &settings.quick_search_shortcut,
                move || {
                    this_tip.client.set_quick_search_tip_seen(true);
                },
            );
            root.append(&tip_banner);
        }

        if !self.client.is_right_click_tip_seen() {
            let this_tip2 = self.clone_ref();
            let right_click_tip = episodes_view::create_right_click_tip_banner(move || {
                this_tip2.client.set_right_click_tip_seen(true);
            });
            root.append(&right_click_tip);
        }

        let ep_search_entry = gtk::SearchEntry::new();
        if settings.quick_search_enabled {
            let search_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            search_box.set_margin_start(12);
            search_box.set_margin_end(12);
            search_box.set_margin_bottom(8);

            ep_search_entry.set_placeholder_text(Some(&format!(
                "Bölüm numarası veya adı ara… ({})",
                settings.quick_search_shortcut
            )));
            ep_search_entry.set_hexpand(true);
            search_box.append(&ep_search_entry);
            root.append(&search_box);

            let shortcut_key = settings.quick_search_shortcut.clone();
            let ep_entry_clone = ep_search_entry.clone();
            let key_controller = gtk::EventControllerKey::new();
            key_controller.connect_key_pressed(move |_, keyval, _, state| {
                let key_name = keyval.name().map(|s| s.to_string()).unwrap_or_default();
                let is_ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);

                let triggered = match shortcut_key.as_str() {
                    "Ctrl+F" => is_ctrl && (key_name == "f" || key_name == "F"),
                    "Ctrl+K" => is_ctrl && (key_name == "k" || key_name == "K"),
                    "F3" => key_name == "F3",
                    _ => key_name == "slash" || key_name == "kp_divide",
                };

                if triggered {
                    ep_entry_clone.grab_focus();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            // Önceki sayfanın controller'ını kaldır (birikmeyi önle).
            if let Some(old) = self.ep_search_controller.borrow().as_ref() {
                self.window.remove_controller(old);
            }
            self.window.add_controller(key_controller.clone());
            *self.ep_search_controller.borrow_mut() = Some(key_controller);
        }
        drop(settings);

        let list_box = gtk::ListBox::new();
        list_box.add_css_class("content-list");
        list_box.set_margin_start(12);
        list_box.set_margin_end(12);
        list_box.set_margin_bottom(16);

        if eps.is_empty() {
            let sp = components::create_status_page(
                "Bölüm Bulunamadı",
                "Bu yapım için henüz bölüm listesi bulunmuyor.",
                "media-tape-symbolic",
            );
            root.append(&sp);
        } else {
            // Tek disk okuma: satır başına load_state() donmayı önler.
            let watched_all = self.client.load_state().watched;
            let rows: Vec<(Episode, gtk::Box)> = eps.iter().map(|e| {
                let key = format!("{}:{}:{}", title.id, e.season, e.episode);

                let name = gtk::Label::new(Some(&format!(
                    "S{:02} E{:02}   {}",
                    e.season, e.episode, e.name
                )));
                name.set_xalign(0.0);
                name.add_css_class("title-4");
                name.set_hexpand(true);

                let time_lbl = gtk::Label::new(None);
                time_lbl.set_xalign(1.0);
                time_lbl.add_css_class("dim-label");

                let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                header_box.append(&name);
                header_box.append(&time_lbl);

                let pic = self.covers.cover_picture(title.poster.as_deref(), 48, 72);
                pic.set_valign(gtk::Align::Center);

                let right_col = gtk::Box::new(gtk::Orientation::Vertical, 4);
                right_col.set_valign(gtk::Align::Center);
                right_col.append(&header_box);

                let (saved_pos, saved_dur) = self.progress.borrow()
                    .get(&key).copied()
                    .unwrap_or((0.0, 0.0));

                let progress_bar = gtk::ProgressBar::new();
                progress_bar.add_css_class("episode-progress");

                let fmt_time = |s: f64| -> String {
                    let s = s as u64;
                    if s >= 3600 { format!("{}:{:02}:{:02}", s/3600, (s%3600)/60, s%60) }
                    else { format!("{}:{:02}", s/60, s%60) }
                };

                if saved_dur > 0.0 && saved_pos > 1.0 {
                    progress_bar.set_fraction((saved_pos / saved_dur).clamp(0.0, 1.0));
                    progress_bar.set_visible(true);
                    time_lbl.set_text(&format!("{} / {}", fmt_time(saved_pos), fmt_time(saved_dur)));
                    time_lbl.set_visible(true);
                } else {
                    progress_bar.set_visible(false);
                    time_lbl.set_visible(false);
                }
                right_col.append(&progress_bar);

                self.progress_bars.borrow_mut().insert(key.clone(), (progress_bar.clone(), time_lbl.clone()));

                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.set_margin_top(6);
                row.set_margin_bottom(6);
                row.set_margin_start(14);
                row.set_margin_end(14);
                row.set_valign(gtk::Align::Center);

                let check = gtk::CheckButton::new();
                check.set_valign(gtk::Align::Center);
                check.set_visible(false);
                check.set_tooltip_text(Some("İndirme için seç"));
                {
                    let refresh_c = refresh_dl_bar.clone();
                    check.connect_toggled(move |_| refresh_c());
                }
                row.prepend(&check);
                dl_checks.borrow_mut().push((e.clone(), check.clone()));

                row.append(&pic);
                row.append(&right_col);

                let is_watched = Rc::new(RefCell::new(
                    watched_all
                        .get(&title.id.to_string())
                        .map(|l| l.iter().any(|x| x.episode == e.episode && x.season == e.season))
                        .unwrap_or(false)
                        || (saved_dur > 0.0 && saved_pos / saved_dur > 0.9)
                ));

                let done_icon = gtk::Image::from_icon_name("object-select-symbolic");
                done_icon.add_css_class("dim-label");
                done_icon.set_tooltip_text(Some("İzlendi"));
                done_icon.set_valign(gtk::Align::Center);
                done_icon.set_visible(*is_watched.borrow());
                row.append(&done_icon);

                let dl_one = gtk::Button::from_icon_name("folder-download-symbolic");
                dl_one.add_css_class("flat");
                dl_one.add_css_class("circular");
                dl_one.set_valign(gtk::Align::Center);
                dl_one.set_tooltip_text(Some("Bölümü indir"));
                {
                    let this_dl = self.clone_ref();
                    let title_dl = title.clone();
                    let ep_dl = e.clone();
                dl_one.connect_clicked(move |_| {
                    let this_q = this_dl.clone_ref();
                    let this2 = this_dl.clone_ref();
                    let title2 = title_dl.clone();
                    let ep2 = ep_dl.clone();
                    this_q.ask_download_quality(move |q| {
                            if let Some(quality) = q {
                                this2.start_download_prefetch(title2.clone(), vec![ep2.clone()], quality, true);
                            }
                        });
                    });
                }
                row.append(&dl_one);

                let this_play = self.clone_ref();
                let title_play = title.clone();
                let ep_play = e.clone();
                let row_c = row.clone();
                let check_c = check.clone();
                let dl_one_c = dl_one.clone();
                let click = gtk::GestureClick::new();
                click.set_button(1); // sadece sol tık
                click.connect_pressed(move |_, _, x, y| {
                    // Düğme/checkbox tıklaması satırı oynatmasın.
                    for w in [check_c.upcast_ref::<gtk::Widget>(), dl_one_c.upcast_ref::<gtk::Widget>()] {
                        if let Some(r) = w.compute_bounds(&row_c) {
                            let (bx, by, bw, bh) =
                                (r.x() as f64, r.y() as f64, r.width() as f64, r.height() as f64);
                            if x >= bx && x <= bx + bw && y >= by && y <= by + bh {
                                return;
                            }
                        }
                    }
                    this_play.play(&title_play, &ep_play);
                });
                row.add_controller(click);

                let this_ctx = self.clone_ref();
                let title_ctx = title.clone();
                let ep_ctx = e.clone();
                let is_watched_ctx = is_watched.clone();
                let done_icon_ctx = done_icon.clone();
                let row_ctx = row.clone();
                let bar_ctx = progress_bar.clone();
                let lbl_ctx = time_lbl.clone();
                let key_ctx = key.clone();
                let progress_ctx = self.progress.clone();
                let bars_ctx = self.progress_bars.clone();

                let right_click = gtk::GestureClick::new();
                right_click.set_button(3);
                right_click.connect_pressed(move |gesture, _, x, y| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);

                    let currently_watched = *is_watched_ctx.borrow();

                    let toggle_label = if currently_watched {
                        "✖ İzlenmedi Olarak İşaretle"
                    } else {
                        "✅ İzlendi Olarak İşaretle"
                    }
                    .to_string();

                    let client_c = this_ctx.client.clone();
                    let title_c = title_ctx.clone();
                    let ep_c = ep_ctx.clone();
                    let is_watched_c = is_watched_ctx.clone();
                    let done_icon_c = done_icon_ctx.clone();
                    let this_refresh = this_ctx.clone_ref();

                    let on_toggle: Rc<dyn Fn()> = Rc::new(move || {
                        let was_watched = *is_watched_c.borrow();
                        if was_watched {
                            client_c.remove_watched(title_c.id, ep_c.season, ep_c.episode);
                            *is_watched_c.borrow_mut() = false;
                            done_icon_c.set_visible(false);
                        } else {
                            let w = api::Watched {
                                title_id: title_c.id,
                                episode: ep_c.episode,
                                season: ep_c.season,
                            };
                            client_c.save_watched(&w, &title_c.name);
                            client_c.add_history(&title_c, &ep_c);
                            *is_watched_c.borrow_mut() = true;
                            done_icon_c.set_visible(true);
                        }
                        let msg = if was_watched {
                            "✖ İzlenmedi olarak işaretlendi"
                        } else {
                            "✅ İzlendi olarak işaretlendi"
                        };
                        let toast = adw::Toast::new(msg);
                        toast.set_timeout(2);
                        this_refresh.toast.add_toast(toast);
                    });
                    let client_d = this_ctx.client.clone();
                    let title_d = title_ctx.clone();
                    let ep_d = ep_ctx.clone();
                    let is_watched_d = is_watched_ctx.clone();
                    let done_icon_d = done_icon_ctx.clone();
                    let bar_d = bar_ctx.clone();
                    let lbl_d = lbl_ctx.clone();
                    let key_d = key_ctx.clone();
                    let progress_d = progress_ctx.clone();
                    let bars_d = bars_ctx.clone();
                    let this_refresh_d = this_ctx.clone_ref();
                    let on_clear: Rc<dyn Fn()> = Rc::new(move || {
                        client_d.clear_episode(title_d.id, ep_d.season, ep_d.episode);
                        progress_d.borrow_mut().remove(&key_d);
                        bars_d.borrow_mut().remove(&key_d);
                        *is_watched_d.borrow_mut() = false;
                        done_icon_d.set_visible(false);
                        bar_d.set_fraction(0.0);
                        bar_d.set_visible(false);
                        lbl_d.set_text("");
                        lbl_d.set_visible(false);
                        let toast = adw::Toast::new("🧹 Bölüm temizlendi");
                        toast.set_timeout(2);
                        this_refresh_d.toast.add_toast(toast);
                    });

                    crate::ui::row_menu::RowMenu::build(vec![
                        (toggle_label, on_toggle),
                        ("🧹 Temizle".to_string(), on_clear),
                    ])
                    .popup_at(&row_ctx, x, y);
                });
                row.add_controller(right_click);

                (e.clone(), row)
            }).collect();

            for (_, row_widget) in &rows {
                list_box.append(row_widget);
            }

            let rows_rc = Rc::new(rows);
            ep_search_entry.connect_search_changed(move |e| {
                let query = e.text().trim().to_lowercase();
                for (ep_data, row_widget) in rows_rc.iter() {
                    if query.is_empty() {
                        row_widget.set_visible(true);
                    } else {
                        let name_match = ep_data.name.to_lowercase().contains(&query);
                        let ep_num_match = ep_data.episode.to_string() == query
                            || format!("e{}", ep_data.episode) == query
                            || format!("s{:02}e{:02}", ep_data.season, ep_data.episode) == query;
                        row_widget.set_visible(name_match || ep_num_match);
                    }
                }
            });

            root.append(&list_box);
        }

        scroll.set_child(Some(&root));
        let page_overlay = gtk::Overlay::new();
        page_overlay.set_child(Some(&scroll));
        page_overlay.add_overlay(&dl_reveal);
        page_overlay
    }

    fn spawn<F, R>(&self, f: F)
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
                    if self.page_history.borrow().last() == Some(&Page::Home) {
                        self.show_page(&Page::Home);
                    }
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Search(res) => match res {
                Ok(results) => {
                    *self.search_results.borrow_mut() = results;
                    let mut st = self.page_history.borrow_mut();
                    if st.last() != Some(&Page::Search) {
                        st.push(Page::Search);
                    }
                    drop(st);
                    self.show_page(&Page::Search);
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Eps(title, res) => match res {
                Ok(eps) => {
                    let page = if eps.is_empty() {
                        Page::Movie { title, eps }
                    } else {
                        match title.title_type.as_deref() {
                            Some("movie") => Page::Movie { title, eps },
                            _ => Page::Episodes { title, eps },
                        }
                    };
                    self.page_history.borrow_mut().push(page.clone());
                    self.show_page(&page);
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Play(title, ep, res) => match res {
                Ok(src) => {
                    self.play_candidates(&title, &ep, &src.candidates, &src.fast_embeds, &src.fallback_embeds, src.plan)
                }
                Err(e) => self.show_error(&e),
            },
            Msg::FansubsLoaded { title, ep, fansubs, default_template } => match fansubs {
                Ok(list) => self.after_fansubs_loaded(title, ep, list, default_template),
                Err(e) => self.show_error(&e),
            },
            Msg::FansubChosen { title, ep, chosen } => {
                if let Some(fs) = chosen {
                    self.play_with_fansub(&title, &ep, &fs, Vec::new());
                } else {
                    self.play_resolved(&title, &ep, None);
                }
            }
            Msg::PlayQualities { title, ep, fs, rest, quals } => {
                self.busy(false);
                match quals {
                    Err(_) => self.play_with_fansub_inner(&title, &ep, &fs, rest, None),
                    Ok(list) => {
                        let labels: Vec<String> =
                            list.iter().map(|q| q.label.clone()).collect();
                        let title_c = title.clone();
                        let ep_c = ep.clone();
                        let this_c = self.clone_ref();
                        crate::ui::play_quality_dialog::show_play_quality_dialog(
                            &self.window,
                            &title.name,
                            &labels,
                            move |choice| match choice {
                                crate::ui::play_quality_dialog::PlayChoice::Cancelled => {}
                                crate::ui::play_quality_dialog::PlayChoice::Best => {
                                    this_c.play_with_fansub_inner(
                                        &title_c, &ep_c, &fs, rest.clone(), None,
                                    );
                                }
                                crate::ui::play_quality_dialog::PlayChoice::Quality(
                                    label,
                                ) => {
                                    let forced = list
                                        .iter()
                                        .find(|q| q.label == label)
                                        .cloned();
                                    this_c.play_with_fansub_inner(
                                        &title_c, &ep_c, &fs, rest.clone(), forced,
                                    );
                                }
                            },
                        );
                    }
                }
            }
            Msg::DlLists { title, quality, items, is_single } => {
                let with_subs: Vec<(Episode, Vec<api::FansubInfo>)> = items
                    .into_iter()
                    .filter(|(_, l)| !l.is_empty())
                    .collect();
                if with_subs.is_empty() {
                    let t = adw::Toast::new("⚠️ Seçili bölümlerde çeviri bulunamadı");
                    t.set_timeout(3);
                    self.toast.add_toast(t);
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
                                &c, &dir, &series, ep, fs, &quality_c,
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
                        this_c.spawn(move |c| {
                            let mut recs = Vec::new();
                            let mut skipped = Vec::new();
                            for (ep, fs) in &done {
                                match crate::download::resolve_for_download(
                                    &c, &dir_c, &series_c, ep, fs, &quality_cc,
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
                    let t = adw::Toast::new(&format!("{n} bölüm kuyruğa eklendi"));
                    t.set_timeout(3);
                    self.toast.add_toast(t);
                }
                if !skipped.is_empty() {
                    let shown: Vec<&str> =
                        skipped.iter().take(3).map(|s| s.as_str()).collect();
                    let more = if skipped.len() > 3 {
                        format!(" +{}", skipped.len() - 3)
                    } else {
                        String::new()
                    };
                    let t = adw::Toast::new(&format!(
                        "Atlanan: {}{more}",
                        shown.join(", ")
                    ));
                    t.set_timeout(5);
                    self.toast.add_toast(t);
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

    fn play(&self, title: &Title, ep: &Episode) {
        let title = title.clone();
        let ep = ep.clone();
        eprintln!("[PLAY] çağrıldı: {} S{:02}E{:02}", title.name, ep.season, ep.episode);
        let is_movie = title.title_type.as_deref() == Some("movie");
        if is_movie {
            self.play_resolved(&title, &ep, None);
            return;
        }
        let default_template = self.settings.borrow().default_fansub_template;
        let manual_s = self.settings.borrow().official_skip_secret.clone();
        self.busy(true);
        let title_s = title.clone();
        let ep_s = ep.clone();
        self.spawn(move |c| {
            let mut res = c.list_fansubs(title_s.id, ep_s.episode, ep_s.season);
            if matches!(&res, Err(e) if api::is_server_error(e)) {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                res = c.list_fansubs(title_s.id, ep_s.episode, ep_s.season);
            }
            // Liste hâlâ sunucu-korumalıysa best-video yedeğine düş (çeviri seçimsiz oynat).
            let fallback_url = match &res {
                Err(e) if api::is_server_error(e) => {
                    c.resolve_best_video(title_s.id, ep_s.episode, ep_s.season).ok()
                }
                _ => None,
            };
            move || match fallback_url {
                Some(url) => {
                    let plan = crate::skip::fetch_plan_for_embeds(
                        &c, title_s.id, ep_s.season, ep_s.episode, &[], &[], &manual_s,
                    );
                    Msg::Play(title_s, ep_s, Ok(PlaySources {
                        candidates: vec![url],
                        fast_embeds: Vec::new(),
                        fallback_embeds: Vec::new(),
                        plan,
                    }))
                }
                None => Msg::FansubsLoaded {
                    title: title_s,
                    ep: ep_s,
                    fansubs: res.map_err(|e| api::friendly_play_error(&e)),
                    default_template,
                },
            }
        });
    }

    fn after_fansubs_loaded(&self, title: Title, ep: Episode, fansubs: Vec<api::FansubInfo>, default_template: Option<i64>) {
        self.busy(false);

        if fansubs.is_empty() {
            self.play_resolved(&title, &ep, None);
            return;
        }

        if let Some(tpl) = default_template {
            if let Some(fs) = fansubs.iter().find(|f| f.template_id == tpl) {
                let rest: Vec<api::FansubInfo> = fansubs
                    .iter()
                    .filter(|f| f.template_id != tpl)
                    .cloned()
                    .collect();
                self.play_with_fansub(&title, &ep, fs, rest);
                return;
            }
        }

        if fansubs.len() == 1 {
            self.play_with_fansub(&title, &ep, &fansubs[0], Vec::new());
            return;
        }

        let ask = self.settings.borrow().fansub_ask_each_time;
        if !ask {
            if let Some(best) = fansubs.first() {
                eprintln!("[FS] otomatik seçim: {} ({:.2}★)", best.name, best.rating);
                let rest: Vec<api::FansubInfo> = fansubs[1..].to_vec();
                self.play_with_fansub(&title, &ep, best, rest);
                return;
            }
        }

        let title_s = title.clone();
        let ep_s = ep.clone();
        let app_rc = self.clone_ref();
        let fansubs_for_rest = fansubs.clone();
        crate::ui::fansub_dialog::show_fansub_dialog(
            &self.window,
            &format!("{} — S{:02}E{:02}", title.name, ep.season, ep.episode),
            fansubs,
            move |chosen: api::FansubInfo| {
                let rest: Vec<api::FansubInfo> = fansubs_for_rest
                    .iter()
                    .filter(|f| f.template_id != chosen.template_id)
                    .cloned()
                    .collect();
                app_rc.play_with_fansub(&title_s, &ep_s, &chosen, rest);
            },
        );
    }

    /// Kalite sorusu (tekli: her indirmede; toplu: grup başı bir kez).
    fn ask_download_quality(&self, cb: impl Fn(Option<String>) + 'static) {
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
    fn start_download_prefetch(&self, title: Title, eps: Vec<Episode>, quality: String, is_single: bool) {
        self.busy(true);
        let title_c = title.clone();
        self.spawn(move |c| {
            let mut items = Vec::new();
            for ep in &eps {
                let fansubs = c.list_fansubs(title_c.id, ep.episode, ep.season).unwrap_or_default();
                items.push((ep.clone(), fansubs));
            }
            move || Msg::DlLists { title: title_c, quality, items, is_single }
        });
    }

    fn play_with_fansub(
        &self,
        title: &Title,
        ep: &Episode,
        fs: &api::FansubInfo,
        rest: Vec<api::FansubInfo>,
    ) {
        // Oynatma kalite sorusu AÇIKSA önce kaliteler çözülür, sonra dialog.
        if self.settings.borrow().play_ask_quality {
            let title_c = title.clone();
            let ep_c = ep.clone();
            let fs_c = fs.clone();
            self.busy(true);
            self.spawn(move |c| {
                let quals =
                    crate::play_quality::available_play_qualities(&c, &fs_c.mirrors);
                move || Msg::PlayQualities {
                    title: title_c,
                    ep: ep_c,
                    fs: fs_c,
                    rest,
                    quals,
                }
            });
            return;
        }
        self.play_with_fansub_inner(title, ep, fs, rest, None);
    }

    fn play_with_fansub_inner(
        &self,
        title: &Title,
        ep: &Episode,
        fs: &api::FansubInfo,
        rest: Vec<api::FansubInfo>,
        forced: Option<crate::play_quality::PlayQuality>,
    ) {
        let toast = adw::Toast::new(&format!(
            "🎬 {} hazırlanıyor ({} · {:.1}★)…",
            glib::markup_escape_text(&title.name),
            glib::markup_escape_text(&fs.name),
            fs.rating
        ));
        toast.set_timeout(2);
        self.toast.add_toast(toast);
        eprintln!(
            "[PLAY-FS] {} S{:02}E{:02} → {} ({:.2}★, {} mirror)",
            title.name, ep.season, ep.episode, fs.name, fs.rating, fs.mirror_count
        );
        let queue = api::fansub_fallback_order(fs, &rest);
        let title_c = title.clone();
        let ep_c = ep.clone();
        let client = self.client.clone();
        let manual_c = self.settings.borrow().official_skip_secret.clone();
        self.busy(true);
        self.spawn(move |_| {
            let mut tried: Vec<String> = Vec::new();
            let mut last_err = String::new();
            let mut won: Option<(Vec<String>, Vec<String>, Vec<String>)> = None;
            for f in &queue {
                let mirror_urls: Vec<String> = f.mirrors.iter().map(|m| m.url.clone()).collect();
                let res = client
                    .resolve_urls(&mirror_urls, mirror_urls.len().max(1))
                    .and_then(|fast_pairs| {
                        let mut fb = client
                            .episode_candidates(title_c.id, ep_c.episode, ep_c.season)
                            .unwrap_or_default();
                        let tried_n = 3.min(fb.len());
                        fb.drain(..tried_n);
                        let fast: Vec<String> = fast_pairs.iter().map(|(m, _)| m.clone()).collect();
                        let fast_emb: Vec<String> = fast_pairs.iter().map(|(_, e)| e.clone()).collect();
                        Ok((fast, fast_emb, fb))
                    });
                match res {
                    Ok(ok) => {
                        if !tried.is_empty() {
                            eprintln!(
                                "[PLAY-FS] {} öldü, sıradaki {} açıldı",
                                tried.join(", "),
                                f.name
                            );
                        }
                        won = Some(ok);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[PLAY-FS] {} mirror'ları çözülemedi: {}", f.name, e);
                        tried.push(f.name.clone());
                        last_err = e;
                    }
                }
            }
            let res = match won {
                Some((mut fast, fast_emb, fb)) => {
                    // Seçili kalite adayların başına konur (yedekler korunur).
                    if let Some(pq) = &forced {
                        if !fast.iter().any(|u| u == &pq.url) {
                            fast.insert(0, pq.url.clone());
                        }
                    }
                    let plan = crate::skip::fetch_plan_for_embeds(
                        &client, title_c.id, ep_c.season, ep_c.episode, &fast_emb, &fb, &manual_c,
                    );
                    Ok(PlaySources { candidates: fast, fast_embeds: fast_emb, fallback_embeds: fb, plan })
                }
                None if tried.len() <= 1 => Err(format!(
                    "{} çevirisi oynatılamadı ({}). Başka bir çeviri seçin.",
                    tried.first().map(String::as_str).unwrap_or("?"),
                    last_err
                )),
                None => Err(format!(
                    "{} çevirisi denendi ({}) ama hiçbiri açılamadı ({}).",
                    tried.len(),
                    tried.join(", "),
                    last_err
                )),
            };
            move || Msg::Play(title_c, ep_c, res)
        });
    }

    fn play_resolved(&self, title: &Title, ep: &Episode, _fansub_template: Option<i64>) {
        let toast = adw::Toast::new(&format!(
            "🎬 {} hazırlanıyor…",
            glib::markup_escape_text(&title.name)
        ));
        toast.set_timeout(2);
        self.toast.add_toast(toast);
        let title = title.clone();
        let ep = ep.clone();
        let manual_r = self.settings.borrow().official_skip_secret.clone();
        self.busy(true);
        self.spawn(move |c| {
            let pref = c.get_preferred_host(title.id);
            let res = if title.title_type.as_deref() == Some("movie") {
                // Filmde atlama planı yok (bölüm eşlemesi belirsiz).
                c.resolve_movie(title.id).map(|u| PlaySources {
                    candidates: vec![u],
                    fast_embeds: Vec::new(),
                    fallback_embeds: Vec::new(),
                    plan: None,
                })
            } else {
                c.resolve_top(title.id, ep.episode, ep.season, 3, pref.as_deref())
                    .and_then(|fast_pairs| {
                        let mut fb = c.episode_candidates(title.id, ep.episode, ep.season)?;
                        let tried = 3.min(fb.len());
                        fb.drain(..tried);
                        if let Some(p) = &pref {
                            fb.sort_by_key(|u| {
                                if api::Client::source_host_hint(u) == p.as_str() { 0 } else { 1 }
                            });
                        }
                        let fast: Vec<String> = fast_pairs.iter().map(|(m, _)| m.clone()).collect();
                        let fast_emb: Vec<String> = fast_pairs.iter().map(|(_, e)| e.clone()).collect();
                        let plan = crate::skip::fetch_plan_for_embeds(
                            &c, title.id, ep.season, ep.episode, &fast_emb, &fb, &manual_r,
                        );
                        Ok(PlaySources { candidates: fast, fast_embeds: fast_emb, fallback_embeds: fb, plan })
                    })
            };
            move || Msg::Play(title, ep, res)
        });
    }

    fn decide_retry(
        exited: bool,
        success: bool,
        playing: bool,
    ) -> (bool, bool) {
        if !exited {
            return (false, playing);
        }
        if playing && !success {
            return (true, false);
        }
        (false, playing)
    }

    const SOCKET_TIMEOUT_SECS: u64 = 25;

    fn source_is_dead(elapsed_secs: u64, core_idle: bool, duration: f64, media_loaded: bool, threshold_secs: u64) -> bool {
        !media_loaded && core_idle && duration <= 0.0 && elapsed_secs >= threshold_secs
    }

    fn socket_timeout_hit(elapsed_secs: u64, socket_seen: bool) -> bool {
        !socket_seen && elapsed_secs >= Self::SOCKET_TIMEOUT_SECS
    }

    fn play_candidates(&self, title: &Title, ep: &Episode, candidates: &[String], fast_embeds: &[String], fallback_embeds: &[String], plan: Option<crate::skip::SkipPlan>) {
        let w = api::Watched {
            title_id: title.id,
            episode: ep.episode,
            season: ep.season,
        };
        self.client.set_current(&w);
        self.client.add_history(&title, &ep);
        eprintln!(
            "[PLAY-CAND] yeni mpv başlatılıyor: {} S{:02}E{:02} (kaynak sayısı={}, plan={})",
            title.name, ep.season, ep.episode, candidates.len(),
            plan.as_ref().map(|p| p.source.as_str()).unwrap_or("yok"),
        );

        let media_title = format!("{} | S{:02}E{:02}", title.name, ep.season, ep.episode);
        let tid = title.id;
        let season = ep.season;
        let episode = ep.episode;
        let prog_key = format!("{tid}:{season}:{episode}");

        let saved_pos = self.client.get_progress(tid, season, episode)
            .filter(|(pos, dur)| *pos > 5.0 && *dur > 0.0 && *pos / *dur < 0.95)
            .map(|(pos, _)| pos);

        let sock_path = format!("/tmp/animecix-mpv-{tid}-{season}-{episode}.sock");
        let _ = std::fs::remove_file(&sock_path);

        let auto_fullscreen = self.settings.borrow().auto_fullscreen;
        let upscale = self.settings.borrow().upscale.clone();
        let show_intro_hint = self.settings.borrow().show_intro_hint;
        let show_music_hint = self.settings.borrow().show_music_hint;
        let skip_times = plan.as_ref().map(|p| p.times.clone()).unwrap_or_default();
        let skip_shared = std::sync::Arc::new(std::sync::Mutex::new(skip_times));

        // Conf video açılmadan hazır yazılır (gerçek tuşlar veya dürüst durum).
        let input_conf_path = format!("/tmp/animecix-input-{tid}-{season}-{episode}.conf");
        let mut input_conf_content = match &plan {
            Some(p) => crate::skip::input_conf(&p.times, &p.source),
            None => crate::skip::input_conf(&api::SkipTimes::default(), ""),
        };
        let song_url = plan.as_ref().and_then(|p| p.song_url.clone());
        if let Some(url) = &song_url {
            input_conf_content.push_str(&crate::music::music_keybind_line(url));
        }
        let _ = std::fs::write(&input_conf_path, input_conf_content);

        // Kalıcı sağ-üst şarkı katmanı (ASS). Şarkı yoksa dosya yazılmaz.
        let ass_path = match &plan {
            Some(p) => {
                let op = match (p.times.op_start, p.times.op_end, &p.music_op) {
                    (Some(f), Some(t), Some(line)) => Some((f, t, line.as_str())),
                    _ => None,
                };
                let ed = match (p.times.ed_start, p.times.ed_end, &p.music_ed) {
                    (Some(f), Some(t), Some(line)) => Some((f, t, line.as_str())),
                    _ => None,
                };
                let show_hint = show_music_hint && song_url.is_some();
                let ass = crate::music::music_ass(op, ed, crate::font::FONT_STYLE, show_hint);
                if ass.is_empty() {
                    None
                } else {
                    let path = format!("/tmp/animecix-music-{tid}-{season}-{episode}.ass");
                    match std::fs::write(&path, ass) {
                        Ok(()) => Some(path),
                        Err(e) => {
                            eprintln!("[PLAY-CAND] ass yazılamadı: {e}");
                            None
                        }
                    }
                }
            }
            None => None,
        };

        let progress = self.progress.clone();
        let progress_bars = self.progress_bars.clone();
        let client = self.client.clone();
        let toast = self.toast.clone();

        if let Some(old) = self.opening_toast.borrow_mut().take() {
            old.dismiss();
        }
        let t = adw::Toast::new(&format!(
            "▶ {} açılıyor…{}",
            glib::markup_escape_text(&media_title),
            saved_pos.map(|p| {
                let s = p as u64;
                if s >= 3600 { format!(" ({}:{:02}:{:02}'den)", s/3600, (s%3600)/60, s%60) }
                else { format!(" ({}:{:02}'den)", s/60, s%60) }
            }).unwrap_or_default()
        ));
        t.set_timeout(0);
        self.opening_toast.borrow_mut().replace(t.clone());
        self.opening_toast_shown_at.borrow_mut().replace(std::time::Instant::now());
        self.toast.add_toast(t);

        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let current_shared = std::sync::Arc::new(std::sync::Mutex::new((episode, season)));

        let mpv_child = std::sync::Arc::new(std::sync::Mutex::new(None::<std::process::Child>));

        let (toast_tx, toast_rx) = std::sync::mpsc::channel::<String>();
        const DISMISS_OPENING: &str = "__animecix_dismiss_opening__";
        {
            let toast_rx = std::sync::Arc::new(std::sync::Mutex::new(toast_rx));
            let alive_toast = alive.clone();
            let toast_h = toast.clone();
            let opening_toast_h = self.opening_toast.clone();
            let opening_toast_shown_at_h = self.opening_toast_shown_at.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                let rx = toast_rx.lock().unwrap();
                let mut msg: Option<String> = None;
                loop {
                    match rx.try_recv() {
                        Ok(m) => msg = Some(m),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                drop(rx);
                if let Some(m) = msg {
                    if m == DISMISS_OPENING {
                        if let Some(t) = opening_toast_h.borrow_mut().take() {
                            let elapsed = opening_toast_shown_at_h
                                .borrow()
                                .map(|i| i.elapsed())
                                .unwrap_or_default();
                            if elapsed < std::time::Duration::from_secs(10) {
                                let remain = std::time::Duration::from_secs(10) - elapsed;
                                let t2 = t.clone();
                                glib::timeout_add_local(remain, move || {
                                    t2.dismiss();
                                    glib::ControlFlow::Break
                                });
                            } else {
                                t.dismiss();
                            }
                        }
                    } else {
                        let tt = adw::Toast::new(&m);
                        tt.set_timeout(3);
                        toast_h.add_toast(tt);
                    }
                }
                if !alive_toast.load(std::sync::atomic::Ordering::Relaxed) {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        }

        // Plan bildirimleri (GTK): hazır + şarkılar, ya da dürüst yokluk.
        match &plan {
            Some(p) => {
                let t = adw::Toast::new(&format!(
                    "⏩ Atlama hazır ({} · 's' intro, 'e' outro)",
                    glib::markup_escape_text(&p.source)
                ));
                t.set_timeout(3);
                self.toast.add_toast(t);
                for m in [&p.music_op, &p.music_ed].into_iter().flatten() {
                    let tm = adw::Toast::new(&glib::markup_escape_text(m));
                    tm.set_timeout(4);
                    self.toast.add_toast(tm);
                }
            }
            None => {
                let t = adw::Toast::new("⚠️ İntro/outro zamanları bulunamadı");
                t.set_timeout(3);
                self.toast.add_toast(t);
            }
        }

        {
            let alive = alive.clone();
            let sock_poll = sock_path.clone();
            let sock_c = sock_path.clone();
            let skip_c = skip_shared.clone();
            let show_intro_hint_c = show_intro_hint;
            let client_c = client.clone();
            let current_shared_c = current_shared.clone();
            let (sender, receiver) = std::sync::mpsc::channel::<(f64, f64)>();
            std::thread::spawn(move || {
                let mut op_prompted = false;
                let mut ed_prompted = false;

                while alive.load(std::sync::atomic::Ordering::Relaxed)
                    && !std::path::Path::new(&sock_poll).exists()
                {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                if !alive.load(std::sync::atomic::Ordering::Relaxed) { return; }

                loop {
                    if std::path::Path::new(&sock_c).exists() {
                        let snap = skip_c.lock().unwrap().clone();
                        let (pos, dur) = crate::player::query_mpv_position(&sock_c).unwrap_or((0.0, 0.0));
                        if show_intro_hint_c {
                            if let Some(st) = snap.op_start {
                                if !op_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                                op_prompted = true;
                                let msg = crate::skip::prompt_op(None);
                                for cmd in crate::skip::skip_osd_cmds(&msg, 4000) {
                                    crate::player::send_mpv_cmd_retry(&sock_c, &cmd, 2);
                                }
                                }
                            }
                            if let Some(st) = snap.ed_start {
                                if !ed_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                                ed_prompted = true;
                                let msg = crate::skip::prompt_ed(None);
                                for cmd in crate::skip::skip_osd_cmds(&msg, 4000) {
                                    crate::player::send_mpv_cmd_retry(&sock_c, &cmd, 2);
                                }
                                }
                            }
                        }
                        if sender.send((pos, dur)).is_err() { break; }
                    } else {
                        if !alive.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });

            let progress2 = progress.clone();
            let progress_bars2 = progress_bars.clone();
            let client_prog = client.clone();
            let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
            let current_shared_t = current_shared.clone();
            let mut marked_ep: Option<(u64, u64)> = None;
            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let rx = receiver.lock().unwrap();
                let mut latest: Option<(f64, f64)> = None;
                let mut disconnected = false;
                loop {
                    match rx.try_recv() {
                        Ok(v) => { latest = Some(v); }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => { disconnected = true; break; }
                    }
                }
                drop(rx);
                let cur = *current_shared_t.lock().unwrap();
                let (cur_ep, cur_season) = cur;
                let pk_cur = format!("{tid}:{cur_season}:{cur_ep}");

                if disconnected {
                    let last = progress2.borrow().get(&pk_cur).copied();
                    if let Some((pos, dur)) = last {
                        client_prog.save_progress(tid, cur.1, cur.0, pos, dur);
                    }
                    return glib::ControlFlow::Break;
                }

                if let Some((pos, dur)) = latest {
                    if pos < 1.0 { return glib::ControlFlow::Continue; }
                    progress2.borrow_mut().insert(pk_cur.clone(), (pos, dur));
                    if let Some((pb, lbl)) = progress_bars2.borrow().get(&pk_cur) {
                        if dur > 0.0 {
                            pb.set_fraction((pos / dur).clamp(0.0, 1.0));
                            let fmt = |s: f64| -> String {
                                let s = s as u64;
                                if s >= 3600 { format!("{}:{:02}:{:02}", s/3600, (s%3600)/60, s%60) }
                                else { format!("{}:{:02}", s/60, s%60) }
                            };
                            lbl.set_text(&format!("{} / {}", fmt(pos), fmt(dur)));
                            lbl.set_visible(true);
                            pb.set_visible(true);
                        }
                    }
                    client_prog.save_progress(tid, cur.1, cur.0, pos, dur);
                    if api::Client::played_enough(pos, dur) && marked_ep != Some(cur) {
                        client_prog.save_watched(&api::Watched { title_id: tid, episode: cur.0, season: cur.1 }, "");
                        marked_ep = Some(cur);
                    }
                }
                glib::ControlFlow::Continue
            });
        }

        {
            let alive = alive.clone();
            let sock_path_c = sock_path.clone();
            let input_conf_path_c = input_conf_path.clone();
            let media_title_c = media_title.clone();
            let toast_tx_c = toast_tx.clone();
            let saved_pos_c = saved_pos;
            let auto_fullscreen_c = auto_fullscreen;
            let upscale_c = upscale;
            let ass_path_c = ass_path.clone();
            let mpv_child_c = mpv_child.clone();
            let mut candidates: Vec<String> = candidates.to_vec();
            let fallback_embeds_c = fallback_embeds.to_vec();
            let fast_embeds_c = fast_embeds.to_vec();
            let candidates_len_c = candidates.len();
            let client_fb = self.client.clone();
            let tid_c = title.id;
            let patience_c = self.client.load_settings().source_patience_secs.max(10);
            let total = candidates.len() + fallback_embeds_c.len();
            std::thread::spawn(move || {
                'supervisor: for i in 0..total {
                    let url: String = if i < candidates.len() {
                        candidates[i].clone()
                    } else {
                        let emb = &fallback_embeds_c[i - candidates.len()];
                        eprintln!("[SUP] JIT yedek çözümleniyor");
                        match client_fb.resolve_single(emb) {
                            Ok(u) => u,
                            Err(e) => {
                                eprintln!("[SUP] JIT yedek çözülemedi, geçiliyor: {e}");
                                continue;
                            }
                        }
                    };
                    let _ = std::fs::remove_file(&sock_path_c);
                    let mut cmd = std::process::Command::new("mpv");
                    cmd.arg("--user-agent=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .arg(format!("--force-media-title={media_title_c}"))
                        .arg("--keep-open=yes")
                        .arg(format!("--input-ipc-server={sock_path_c}"));
                    if auto_fullscreen_c { cmd.arg("--fullscreen"); }
                    // Atlama OSD'si alt-solda (şarkı ASS'i sağ-üstte; üst-üste binmez).
                    cmd.arg("--osd-align-x=left")
                        .arg("--osd-align-y=bottom")
                        .arg("--osd-margin-x=30")
                        .arg("--osd-margin-y=30");
                    cmd.arg(format!("--input-conf={input_conf_path_c}"));
                    if let Some(ass) = ass_path_c.as_deref() {
                        cmd.arg(format!("--sub-file={ass}"));
                    }
                    if let Some(fdir) = crate::font::ensure_fonts() {
                        cmd.args(crate::font::mpv_font_args(&fdir));
                    }
                    cmd.args(saved_pos_c.map(|p| format!("--start={p:.1}")).as_slice())
                        .arg("--cache=yes")
                        .arg("--demuxer-max-bytes=128MiB")
                        .arg("--demuxer-max-back-bytes=32MiB")
                        .arg("--demuxer-readahead-secs=120")
                        .arg("--cache-pause=yes")
                        .arg("--cache-pause-wait=3")
                        .arg("--cache-secs=120")
                        .arg("--stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_delay_max=5")
                        .arg("--network-timeout=10")
                        .arg("--hwdec=auto-safe")
                        .arg("--ytdl-format=bestvideo[height<=1080]+bestaudio/best")
                        .args(crate::api::upscale_mpv_args(&upscale_c, match upscale_c.as_str() {
                            "hafif" => resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
                            "ultra" => resolve_upscale_shader("Anime4K_Upscale_CNN_x2_UL.glsl"),
                            "hafif_keskin" => resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
                            _ => None,
                        }.as_deref(), None))
                        .arg(url.as_str());
                    if url.contains("video.sibnet.ru/v/") {
                        let vid = url
                            .split("/v/")
                            .nth(1)
                            .and_then(|s| s.split('/').nth(1))
                            .map(|s| s.trim_end_matches(".mp4"))
                            .unwrap_or("");
                        let referer = if vid.is_empty() {
                            "https://video.sibnet.ru/".to_string()
                        } else {
                            format!("https://video.sibnet.ru/shell.php?videoid={}", vid)
                        };
                        cmd.arg(format!(
                            "--http-header-fields=Referer: {}\nAccept: */*",
                            referer
                        ));
                    }
                    eprintln!("[SUP] mpv spawn deneniyor (ep={}, kaynak={}, url={:.80})", episode, i, url);
                    let child = match cmd.spawn() {
                        Ok(c) => c,
                        Err(e) => { eprintln!("[SUP] HATA mpv başlatılamadı (ep={}, kaynak={}): {}", episode, i, e); continue; }
                    };
                    eprintln!("[SUP] mpv spawn edildi (ep={}, kaynak={})", episode, i);
                    *mpv_child_c.lock().unwrap() = Some(child);

                    let start = std::time::Instant::now();
                    let mut playing = false;
                    let mut media_loaded = false;
                    loop {
                        let (exited, success) = {
                            let mut g = mpv_child_c.lock().unwrap();
                            match g.as_mut().unwrap().try_wait() {
                                Ok(Some(status)) => (true, status.success()),
                                Ok(None) => (false, false),
                                Err(_) => (true, false),
                            }
                        };
                        if exited {
                            let retry = Self::decide_retry(
                                true,
                                success,
                                playing,
                            ).0;
                            if retry {
                                eprintln!("[SUP] kaynak hatalı çıktı, sonraki kaynağa geçiliyor (ep={}, kaynak={})", episode, i);
                                playing = false;
                            }
                            break;
                        }
                        if std::path::Path::new(&sock_path_c).exists() {
                            if !playing {
                                eprintln!("[SUP] socket belirdi, oynatma başladı (ep={}, kaynak={})", episode, i);
                                let _ = toast_tx_c.send(DISMISS_OPENING.to_string());
                            }
                            playing = true;
                        }
                        if playing {
                            let idle = crate::player::query_mpv_prop(&sock_path_c, "core-idle").unwrap_or(0.0);
                            let dur = crate::player::query_mpv_prop(&sock_path_c, "duration").unwrap_or(0.0);
                            if dur > 0.0 {
                                media_loaded = true;
                            }
                            let elapsed_secs = start.elapsed().as_secs();
                            if Self::source_is_dead(elapsed_secs, idle >= 1.0, dur, media_loaded, patience_c) {
                                eprintln!("[SUP] kaynak hiç yüklemedi (idle+duration=0, {}sn), sonraki kaynağa geçiliyor (ep={}, kaynak={})", elapsed_secs, episode, i);
                                let _ = toast_tx_c.send("Kaynak açıldı ama oynatamadı, diğer kaynağa geçiliyor…".to_string());
                                if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                                    let _ = c.kill();
                                }
                                playing = false;
                                break;
                            }
                        }
                        if Self::socket_timeout_hit(start.elapsed().as_secs(), playing) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }

                    eprintln!("[SUP] döngü bitti (ep={}, kaynak={}, playing={})", episode, i, playing);

                    if playing {
                        let src_hint = if i < fast_embeds_c.len() {
                            api::Client::source_host_hint(&fast_embeds_c[i])
                        } else if i >= candidates_len_c {
                            api::Client::source_host_hint(&fallback_embeds_c[i - candidates_len_c])
                        } else {
                            ""
                        };
                        if !src_hint.is_empty() {
                            client_fb.set_preferred_host(tid_c, src_hint);
                        }
                        if let Some(c) = mpv_child_c.lock().unwrap().as_mut() { let _ = c.wait(); }
                        break 'supervisor;
                    } else {
                        if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                            let _ = c.kill();
                            let _ = c.wait();
                        }
                        if i + 1 < total {
                            let _ = toast_tx_c.send("Kaynak açılamadı, diğer kaynağa geçiliyor…".to_string());
                            continue;
                        } else {
                            let _ = toast_tx_c.send("Bölüm hiçbir kaynakta açılamadı.".to_string());
                            break;
                        }
                    }
                }
                alive.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = std::fs::remove_file(&sock_path_c);
            });
        }
    }

    fn do_search(&self, q: String) {
        let q = q.trim().to_string();
        if q.is_empty() { return; }
        self.busy(true);
        self.spawn(move |c| {
            let res = c.search(&q);
            move || Msg::Search(res)
        });
    }

    fn fetch_home(&self) {
        self.busy(true);
        self.spawn(move |c| {
            let res = c.home_lists();
            move || Msg::Cats(res)
        });
    }

    fn show_error(&self, msg: &str) {
        eprintln!("animecix hatası: {msg}");
        let t = adw::Toast::new(&format!("Hata: {}", glib::markup_escape_text(msg)));
        t.set_timeout(4);
        self.toast.add_toast(t);
    }
}

#[cfg(test)]
mod decide_retry_tests {
    use super::App;

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
    fn toast_text_escapes_markup_breakers() {
        // Rose&Night vakası: ham & bildirimi sessizce öldürüyordu.
        let esc = glib::markup_escape_text("Rose&Night Subs");
        assert!(esc.contains("&amp;"), "ham & kaçırılmalı: {esc}");
        assert!(!esc.contains(" & "), "çıplak & kalmamalı");
    }

    #[test]
    fn slow_source_within_window_is_not_killed() {
        assert!(!App::source_is_dead(5, true, 0.0, false, 20), "5sn'de öldürülmemeli");
        assert!(!App::source_is_dead(19, true, 0.0, false, 20), "19sn'de hâlâ sabırlı olunmalı");
    }

    #[test]
    fn never_loaded_idle_source_is_dead_after_threshold() {
        assert!(App::source_is_dead(20, true, 0.0, false, 20), "20sn+idle+dur=0 -> ölü");
        assert!(App::source_is_dead(120, true, 0.0, false, 20));
    }

    #[test]
    fn threshold_is_user_configurable() {
        assert!(!App::source_is_dead(45, true, 0.0, false, 90), "90sn sabırda 45sn ölü sayılmamalı");
        assert!(App::source_is_dead(90, true, 0.0, false, 90), "90sn sabırda eşikte ölü");
    }

    #[test]
    fn loaded_source_is_never_killed_by_dead_check() {
        assert!(!App::source_is_dead(600, true, 1435.0, true, 20));
        assert!(!App::source_is_dead(600, true, 0.0, true, 20), "yüklendiyse duration anlık 0 okunsa bile");
    }

    #[test]
    fn playing_source_or_unknown_duration_not_dead() {
        assert!(!App::source_is_dead(600, false, 0.0, false, 20));
        assert!(!App::source_is_dead(600, true, 12.0, false, 20));
    }

    #[test]
    fn socket_timeout_only_when_socket_never_seen() {
        assert!(App::socket_timeout_hit(26, false), "soket hiç gelmedi + 25sn doldu -> vazgeç");
        assert!(!App::socket_timeout_hit(24, false), "henüz süre dolmadı");
        assert!(!App::socket_timeout_hit(600, true), "soket varken zaman aşımı uygulanmaz");
    }

    #[test]
    fn anime4k_normal_maps_to_bundled_cnn_shader() {
        let p = super::resolve_upscale_shader("Anime4K_Upscale_CNN_x2_M.glsl");
        assert!(p.is_some(), "normal modu için CNN_x2_M shader'ı bundle edilmiş olmalı");
    }
}
