use crate::api::{marathon_summary, Client, HistoryEntry, Settings, Title};
use crate::ui::episodes_view;
use adw::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct MarathonView;

impl MarathonView {
    pub fn build(
        client: std::sync::Arc<Client>,
        on_item_click: impl Fn(Title) + 'static,
        on_toggle_completed: impl Fn(u64) + 'static,
        on_remove_item: impl Fn(u64) + 'static,
        on_clear_all: impl Fn() + 'static,
        on_reorder: impl Fn(u64, usize) + 'static,
        cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    ) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(12);
        root.set_margin_bottom(12);
        root.set_margin_start(16);
        root.set_margin_end(16);

        let marathon_items = client.get_marathon();
        if marathon_items.is_empty() {
            let sp = crate::ui::components::create_status_page(
                "İzleme Maratonunuz Boş 🏃‍♂️",
                "Gelecekte izleyeceğiniz anime, dizi ve filmleri detay sayfasındaki maraton butonuna (🏁) tıklayarak ekleyebilirsiniz.",
                "media-playlist-repeat-symbolic",
            );
            root.append(&sp);
            return root;
        }

        let total_count = marathon_items.len();

        let summary_card = gtk::Box::new(gtk::Orientation::Vertical, 10);
        summary_card.add_css_class("marathon-summary-card");

        let top_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let header_lbl = gtk::Label::new(Some("🏃‍♂️ İzleme Maratonu İlerlemesi"));
        header_lbl.add_css_class("title-2");
        header_lbl.set_xalign(0.0);
        header_lbl.set_hexpand(true);

        let clear_btn = gtk::Button::with_label("Tümünü Temizle");
        clear_btn.add_css_class("destructive-action");
        clear_btn.add_css_class("pill");
        let on_clear_all_rc = Rc::new(on_clear_all);
        let on_clear_c = on_clear_all_rc.clone();
        clear_btn.connect_clicked(move |_| on_clear_c());

        top_row.append(&header_lbl);
        top_row.append(&clear_btn);

        let stats_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let stats_lbl = gtk::Label::new(Some(&format!("… / {} Anime Tamamlandı", total_count)));
        stats_lbl.add_css_class("title-4");
        stats_lbl.add_css_class("dim-label");
        stats_lbl.set_xalign(0.0);
        stats_lbl.set_hexpand(true);

        let percent_pill = gtk::Label::new(Some("%…"));
        percent_pill.add_css_class("marathon-percent-pill");

        stats_row.append(&stats_lbl);
        stats_row.append(&percent_pill);

        let pbar = gtk::ProgressBar::new();
        pbar.add_css_class("episode-progress");
        pbar.set_fraction(0.0);

        summary_card.append(&top_row);
        summary_card.append(&stats_row);
        summary_card.append(&pbar);

        root.append(&summary_card);

        let summary_titles: Vec<Title> = marathon_items.iter().map(|m| m.title.clone()).collect();
        let client_s = client.clone();
        let (stx, srx) = std::sync::mpsc::channel::<(usize, u32)>();
        std::thread::spawn(move || {
            let fracs: Vec<f64> = summary_titles
                .iter()
                .map(|t| client_s.title_progress_frac(t))
                .collect();
            let _ = stx.send(marathon_summary(&fracs));
        });
        glib::idle_add_local(move || match srx.try_recv() {
            Ok((done, percent)) => {
                stats_lbl.set_text(&format!("{} / {} Anime Tamamlandı", done, total_count));
                percent_pill.set_text(&format!("%{}", percent));
                pbar.set_fraction((percent as f64 / 100.0).clamp(0.0, 1.0));
                glib::ControlFlow::Break
            }
            Err(_) => glib::ControlFlow::Continue,
        });

        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 8);

        let on_item_click_rc = Rc::new(on_item_click);
        let on_toggle_rc = Rc::new(on_toggle_completed);
        let on_remove_rc = Rc::new(on_remove_item);
        let on_reorder_rc = Rc::new(on_reorder);
        let cover_loader_rc = Rc::new(cover_loader);

        for (idx, item) in marathon_items.iter().enumerate() {
            let card_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            card_box.add_css_class("marathon-item-card");

            let num = gtk::Label::new(Some(&format!("{}", idx + 1)));
            num.add_css_class("marathon-index");
            num.set_xalign(0.5);
            num.set_yalign(0.5);
            num.set_valign(gtk::Align::Center);
            card_box.append(&num);

            let drag = gtk::DragSource::new();
            drag.set_actions(gtk::gdk::DragAction::MOVE);
            let card_for_icon = card_box.clone();
            let src_id = item.title.id;
            drag.connect_prepare(move |drag, _, _| {
                let alloc = card_for_icon.allocation();
                let paintable = gtk::WidgetPaintable::new(Some(&card_for_icon));
                drag.set_icon(Some(&paintable), alloc.width() / 2, alloc.height() / 2);
                Some(gtk::gdk::ContentProvider::for_value(&glib::Value::from(
                    src_id.to_string(),
                )))
            });
            let card_dim = card_box.clone();
            drag.connect_drag_begin(move |_, _| {
                card_dim.set_opacity(0.35);
            });
            let card_restore = card_box.clone();
            drag.connect_drag_end(move |_, _, _| {
                card_restore.set_opacity(1.0);
            });
            card_box.add_controller(drag);

            let drop = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
            let self_id = item.title.id;
            let on_r = on_reorder_rc.clone();
            drop.connect_drop(move |_, value, _, _| {
                if let Ok(s) = value.get::<String>() {
                    if let Ok(src_id) = s.parse::<u64>() {
                        if src_id != self_id {
                            on_r(src_id, idx);
                        }
                    }
                }
                true
            });
            card_box.add_controller(drop);

            let chk = gtk::CheckButton::new();
            chk.set_active(item.completed);
            chk.set_valign(gtk::Align::Center);
            chk.set_tooltip_text(Some(if item.completed {
                "Tamamlandı olarak işaretli"
            } else {
                "Tamamlandı olarak işaretle"
            }));
            let tid = item.title.id;
            let on_t_c = on_toggle_rc.clone();
            let chk_guard = Rc::new(std::cell::Cell::new(false));
            let chk_guard_c = chk_guard.clone();
            chk.connect_toggled(move |_| {
                if chk_guard_c.get() {
                    return;
                }
                on_t_c(tid);
            });

            let pic = gtk::Picture::new();
            pic.set_width_request(48);
            pic.set_height_request(72);
            pic.set_hexpand(false);
            pic.set_vexpand(false);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_css_classes(&["cover", "cover-thumb"]);
            pic.set_valign(gtk::Align::Center);
            cover_loader_rc(item.title.poster.as_deref(), &pic, 48, 72);

            let info_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
            info_box.set_valign(gtk::Align::Center);
            info_box.set_hexpand(true);
            let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let name_lbl = gtk::Label::new(Some(&item.title.name));
            name_lbl.add_css_class("title-3");
            name_lbl.set_xalign(0.0);
            name_lbl.set_wrap(false);
            name_lbl.set_single_line_mode(true);
            name_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if item.completed {
                name_lbl.add_css_class("dim-label");
            }
            let status_badge = gtk::Label::new(Some(if item.completed {
                "🏁 Tamamlandı"
            } else {
                "⏳ Devam Ediyor"
            }));
            status_badge.add_css_class(if item.completed {
                "status-badge-completed"
            } else {
                "status-badge-progress"
            });
            title_row.append(&name_lbl);
            title_row.append(&status_badge);
            info_box.append(&title_row);
            episodes_view::append_title_submeta(&info_box, &item.title);

            let prog = gtk::ProgressBar::new();
            prog.add_css_class("episode-progress");
            prog.set_margin_top(4);
            prog.set_valign(gtk::Align::Center);
            info_box.append(&prog);
            let client_c = client.clone();
            let t_c = item.title.clone();
            let (tx, rx) = std::sync::mpsc::channel::<f64>();
            std::thread::spawn(move || {
                let _ = tx.send(client_c.title_progress_frac(&t_c));
            });
            let chk_u = chk.clone();
            let badge_u = status_badge.clone();
            let name_u = name_lbl.clone();
            let guard_u = chk_guard.clone();
            glib::idle_add_local(move || match rx.try_recv() {
                Ok(frac) => {
                    prog.set_fraction(frac);
                    let done = frac >= 0.999;
                    guard_u.set(true);
                    chk_u.set_active(done);
                    guard_u.set(false);
                    chk_u.set_tooltip_text(Some(if done {
                        "Tamamlandı olarak işaretli"
                    } else {
                        "Tamamlandı olarak işaretle"
                    }));
                    badge_u.set_text(if done {
                        "🏁 Tamamlandı"
                    } else {
                        "⏳ Devam Ediyor"
                    });
                    badge_u.remove_css_class("status-badge-completed");
                    badge_u.remove_css_class("status-badge-progress");
                    badge_u.add_css_class(if done {
                        "status-badge-completed"
                    } else {
                        "status-badge-progress"
                    });
                    if done {
                        name_u.add_css_class("dim-label");
                    } else {
                        name_u.remove_css_class("dim-label");
                    }
                    glib::ControlFlow::Break
                }
                Err(_) => glib::ControlFlow::Continue,
            });

            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            actions.set_valign(gtk::Align::Center);
            let play_btn = gtk::Button::with_label("▶ İzle");
            play_btn.add_css_class("suggested-action");
            play_btn.add_css_class("pill");
            let on_ic = on_item_click_rc.clone();
            let t_clone = item.title.clone();
            play_btn.connect_clicked(move |_| on_ic(t_clone.clone()));
            let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
            del_btn.add_css_class("flat");
            del_btn.add_css_class("circular");
            del_btn.add_css_class("destructive-action");
            del_btn.set_tooltip_text(Some("Maratondan Kaldır"));
            let on_rem = on_remove_rc.clone();
            del_btn.connect_clicked(move |_| on_rem(tid));
            actions.append(&play_btn);
            actions.append(&del_btn);

            card_box.append(&chk);
            card_box.append(&pic);
            card_box.append(&info_box);
            card_box.append(&actions);
            list_box.append(&card_box);
        }

        root.append(&list_box);
        root
    }
}

pub struct HistoryView;

impl HistoryView {
    pub fn build(
        _client: &Client,
        history: &[HistoryEntry],
        on_delete_selected: impl Fn(Vec<u64>) + 'static,
        on_clear_all: impl Fn() + 'static,
        on_item_click: impl Fn(HistoryEntry) + 'static,
        cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    ) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        root.set_margin_start(12);
        root.set_margin_end(12);

        if history.is_empty() {
            let sp = crate::ui::components::create_status_page(
                "İzleme Geçmişi Boş",
                "Henüz bir bölüm veya film izlemediniz.",
                "document-open-recent-symbolic",
            );
            root.append(&sp);
            return root;
        }

        let action_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        action_bar.set_margin_bottom(6);

        let select_all_chk = gtk::CheckButton::with_label("Tümünü Seç");
        select_all_chk.set_valign(gtk::Align::Center);

        let delete_sel_btn = gtk::Button::with_label("Seçilenleri Sil (0)");
        delete_sel_btn.add_css_class("destructive-action");
        delete_sel_btn.add_css_class("pill");
        delete_sel_btn.set_sensitive(false);
        delete_sel_btn.set_valign(gtk::Align::Center);

        let clear_all_btn = gtk::Button::with_label("Tümünü Temizle");
        clear_all_btn.add_css_class("flat");
        clear_all_btn.add_css_class("pill");
        clear_all_btn.set_valign(gtk::Align::Center);

        action_bar.append(&select_all_chk);
        action_bar.append(&delete_sel_btn);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        action_bar.append(&spacer);
        action_bar.append(&clear_all_btn);

        root.append(&action_bar);

        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 5);
        list_box.set_vexpand(false);

        let selected_ids = Rc::new(RefCell::new(Vec::<u64>::new()));
        let check_buttons = Rc::new(RefCell::new(Vec::<(u64, gtk::CheckButton)>::new()));
        let on_item_click_rc = Rc::new(on_item_click);

        let update_delete_btn = {
            let selected_ids = selected_ids.clone();
            let delete_sel_btn = delete_sel_btn.clone();
            move || {
                let count = selected_ids.borrow().len();
                delete_sel_btn.set_label(&format!("Seçilenleri Sil ({count})"));
                delete_sel_btn.set_sensitive(count > 0);
            }
        };

        for h in history {
            let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row_box.add_css_class("history-item-card");
            row_box.set_vexpand(false);
            row_box.set_height_request(84);

            let chk = gtk::CheckButton::new();
            chk.set_valign(gtk::Align::Center);

            let tid = h.title.id;
            let sel_clone = selected_ids.clone();
            let upd_clone = update_delete_btn.clone();
            chk.connect_toggled(move |b| {
                let mut ids = sel_clone.borrow_mut();
                if b.is_active() {
                    if !ids.contains(&tid) {
                        ids.push(tid);
                    }
                } else {
                    ids.retain(|&id| id != tid);
                }
                drop(ids);
                upd_clone();
            });
            check_buttons.borrow_mut().push((tid, chk.clone()));

            let pic = crate::covers::new_sized_picture(48, 72);
            pic.set_valign(gtk::Align::Center);
            cover_loader(h.title.poster.as_deref(), &pic, 48, 72);

            let text_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
            text_box.set_vexpand(true);
            text_box.set_valign(gtk::Align::Center);
            text_box.set_hexpand(true);

            let name = gtk::Label::new(Some(&h.title.name));
            name.set_xalign(0.0);
            name.set_wrap(false);
            name.set_single_line_mode(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.add_css_class("title-3");

            let sub_text = crate::ui::components::episode_label_spaced(
                h.episode.season,
                h.episode.episode,
                &h.episode.name,
            );
            let sub = gtk::Label::new(Some(&sub_text));
            sub.set_xalign(0.0);
            sub.set_wrap(false);
            sub.set_single_line_mode(true);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            sub.add_css_class("dim-label");

            text_box.append(&name);
            text_box.append(&sub);
            episodes_view::append_title_submeta(&text_box, &h.title);

            let click_btn = gtk::Button::with_label("▶ İzle");
            click_btn.add_css_class("suggested-action");
            click_btn.add_css_class("pill");
            click_btn.set_valign(gtk::Align::Center);

            let h_clone = h.clone();
            let on_ic = on_item_click_rc.clone();
            click_btn.connect_clicked(move |_| {
                on_ic(h_clone.clone());
            });

            row_box.append(&chk);
            row_box.append(&pic);
            row_box.append(&text_box);
            row_box.append(&click_btn);

            list_box.append(&row_box);
        }

        let check_buttons_clone = check_buttons.clone();
        select_all_chk.connect_toggled(move |b| {
            let active = b.is_active();
            for (_, chk) in check_buttons_clone.borrow().iter() {
                chk.set_active(active);
            }
        });

        let selected_ids_clone = selected_ids.clone();
        let on_del = Rc::new(on_delete_selected);
        delete_sel_btn.connect_clicked(move |_| {
            let ids = selected_ids_clone.borrow().clone();
            if !ids.is_empty() {
                on_del(ids);
            }
        });

        let on_ca = Rc::new(on_clear_all);
        clear_all_btn.connect_clicked(move |_| {
            on_ca();
        });

        root.append(&list_box);
        root
    }
}

pub struct SettingsView;

impl SettingsView {
    pub fn build(
        settings: &Settings,
        on_save: impl Fn(Settings) + 'static,
        on_wipe: impl Fn(bool) + 'static,
    ) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(12);
        root.set_margin_bottom(12);
        root.set_margin_start(16);
        root.set_margin_end(16);

        let settings_search = gtk::SearchEntry::new();
        settings_search.set_placeholder_text(Some("Ayarlarda ara…"));
        settings_search.set_hexpand(true);
        root.append(&settings_search);

        let ep_group = adw::PreferencesGroup::new();
        ep_group.set_title("Hızlı Bölüm Arama");

        let (search_toggle_row, search_toggle) = crate::ui::components::switch_row(
            "Aktif",
            "Bölüm ekranında klavye kısayolu ile hızlı bölüm arama çubuğunu aktif et",
            settings.quick_search_enabled,
        );

        let shortcut_row = adw::ComboRow::new();
        shortcut_row.set_title("Kısayol Tuşu");
        shortcut_row.set_subtitle("Bölüm sayfasında aramayı başlatacak klavye kısayolu");
        let ep_shortcuts = &["/", "Ctrl+F", "F3"];
        let ep_shortcut_model = gtk::StringList::new(ep_shortcuts);
        shortcut_row.set_model(Some(&ep_shortcut_model));
        let current_ep_sc = ep_shortcuts
            .iter()
            .position(|&s| s == settings.quick_search_shortcut)
            .unwrap_or(0);
        shortcut_row.set_selected(current_ep_sc as u32);
        shortcut_row.set_sensitive(settings.quick_search_enabled);

        ep_group.add(&search_toggle_row);
        ep_group.add(&shortcut_row);
        root.append(&ep_group);

        let search_group = adw::PreferencesGroup::new();
        search_group.set_title("Anime / Dizi Arama Kısayolu");

        let search_sc_row = adw::ComboRow::new();
        search_sc_row.set_title("Kısayol Tuşu");
        search_sc_row.set_subtitle("Ana ekranda arama çubuğunu açacak klavye kısayolu");
        let search_shortcuts = &["Ctrl+S", "F2", "/"];
        let search_sc_model = gtk::StringList::new(search_shortcuts);
        search_sc_row.set_model(Some(&search_sc_model));
        let current_sc = search_shortcuts
            .iter()
            .position(|&s| s == settings.search_shortcut)
            .unwrap_or(0);
        search_sc_row.set_selected(current_sc as u32);
        search_group.add(&search_sc_row);
        root.append(&search_group);

        let view_group = adw::PreferencesGroup::new();
        view_group.set_title("Görünüm");

        let scale_row = adw::ActionRow::new();
        scale_row.set_title("Arayüz Ölçeği (%)");
        scale_row.set_subtitle("100 ile 125 arasında, anında uygulanır");
        let scale_adj = gtk::Adjustment::new(
            (settings.ui_scale.clamp(1.0, 1.25) * 100.0) as f64,
            100.0,
            125.0,
            1.0,
            5.0,
            0.0,
        );
        let scale_spin = gtk::SpinButton::new(Some(&scale_adj), 1.0, 0);
        scale_spin.set_numeric(true);
        scale_spin.set_value((settings.ui_scale.clamp(1.0, 1.25) * 100.0) as f64);
        scale_row.add_suffix(&scale_spin);
        view_group.add(&scale_row);

        let news_font_row = adw::ComboRow::new();
        news_font_row.set_title("Haber Yazı Boyutu");
        news_font_row.set_subtitle("Haber okuma ekranındaki gövde metni");
        let news_font_names: Vec<&str> = crate::api::NEWS_FONT_OPTIONS
            .iter()
            .map(|(_, n)| *n)
            .collect();
        let news_font_model = gtk::StringList::new(&news_font_names);
        news_font_row.set_model(Some(&news_font_model));
        let current_news_font = crate::api::NEWS_FONT_OPTIONS
            .iter()
            .position(|(id, _)| *id == settings.news_font_size)
            .unwrap_or(1);
        news_font_row.set_selected(current_news_font as u32);
        view_group.add(&news_font_row);

        let theme_row = adw::ComboRow::new();
        theme_row.set_title("Tema");
        theme_row.set_subtitle("Koyu renklerde gradyan arka plan, anında uygulanır");
        let theme_names: Vec<&str> = crate::theme::THEMES.iter().map(|(_, n)| *n).collect();
        let theme_model = gtk::StringList::new(&theme_names);
        theme_row.set_model(Some(&theme_model));
        let current_theme = crate::theme::THEMES
            .iter()
            .position(|(id, _)| *id == settings.theme)
            .unwrap_or(0) as u32;
        theme_row.set_selected(current_theme);
        view_group.add(&theme_row);
        root.append(&view_group);

        let player_group = adw::PreferencesGroup::new();
        player_group.set_title("Oynatıcı Ayarları");

        let (fs_row, fs_sw) = crate::ui::components::switch_row(
            "MPV Otomatik Tam Ekran",
            "Video başladığında MPV'yi otomatik tam ekran modunda açar",
            settings.auto_fullscreen,
        );
        player_group.add(&fs_row);

        let (intro_hint_row, intro_hint_sw) = crate::ui::components::switch_row(
            "İntro/Outro Bildirimleri",
            "İntro ve outro başlayınca mpv'de bilgi gösterir ('s'/'e' tuşları hep çalışır)",
            settings.show_intro_hint,
        );
        player_group.add(&intro_hint_row);

        let (music_hint_row, music_hint_sw) = crate::ui::components::switch_row(
            "Şarkıda 'Shift+M' Tuşu İpucu",
            "Şarkı satırında şarkıyı tarayıcıda açan 'Shift+M' tuşunu hatırlatır",
            settings.show_music_hint,
        );
        player_group.add(&music_hint_row);

        let (play_q_row, play_q_sw) = crate::ui::components::switch_row(
            "Oynatırken Kalite Sor",
            "Bölüm açılırken kalite seçilsin (kapalıysa en iyi açılır)",
            settings.play_ask_quality,
        );
        player_group.add(&play_q_row);

        let (auto_next_row, auto_next_sw) = crate::ui::components::switch_row(
            "Otomatik Sonraki Bölüm",
            "Bölüm bitince sıradakini kendiliğinden açar (kapalıysa mpv'de 'n'/'p' ile geçilir)",
            settings.auto_next,
        );
        player_group.add(&auto_next_row);
        root.append(&player_group);

        let perf_group = adw::PreferencesGroup::new();
        perf_group.set_title("Performans");

        let (light_row, light_sw) = crate::ui::components::switch_row(
            "Hafif Mod (Düşük RAM)",
            "Arayüzü CPU ile çizer, bellek kullanımını ~%35 azaltır. Uygulamayı yeniden başlatınca geçerli olur.",
            settings.light_mode,
        );
        perf_group.add(&light_row);

        let patience_row = adw::ActionRow::new();
        patience_row.set_title("Kaynak Açılış Sabrı");
        patience_row.set_subtitle("Yavaş internet için artırın. Medya hiç açılmazsa ölü kaynakta bu kadar saniye (20-120) beklenir, sonra sıradakine geçilir.");
        let patience_adj = gtk::Adjustment::new(
            settings.source_patience_secs as f64,
            20.0,
            120.0,
            5.0,
            10.0,
            0.0,
        );
        let patience_spin = gtk::SpinButton::new(Some(&patience_adj), 1.0, 0);
        patience_spin.set_numeric(true);
        patience_spin.set_value(settings.source_patience_secs as f64);
        patience_row.add_suffix(&patience_spin);
        perf_group.add(&patience_row);
        root.append(&perf_group);

        // İndirilenler grubuna eklenecek satır (satır save_all'tan önce
        // yaratılmalı; gruba ekleme aşağıda dl_group bölümünde).
        let maxpar_row = adw::ActionRow::new();
        maxpar_row.set_title("Aynı Anda İndirilen Bölüm");
        maxpar_row.set_subtitle("Kaç bölüm eşzamanlı iner (1-32). Artırım yeni işlere uygulanır, devam edenler bölünmez.");
        let maxpar_adj = gtk::Adjustment::new(
            settings.max_parallel_downloads.clamp(1, 32) as f64,
            1.0,
            32.0,
            1.0,
            5.0,
            0.0,
        );
        let maxpar_spin = gtk::SpinButton::new(Some(&maxpar_adj), 1.0, 0);
        maxpar_spin.set_numeric(true);
        maxpar_spin.set_value(settings.max_parallel_downloads.clamp(1, 32) as f64);
        maxpar_row.add_suffix(&maxpar_spin);

        let img_group = adw::PreferencesGroup::new();
        img_group.set_title("Görüntü İyileştirme");
        let upscale_row = adw::ComboRow::new();
        upscale_row.set_title("Görüntü İyileştirme");
        upscale_row.set_subtitle("Düşük çözünürlüklü kaynağı yukarı ölçekler (1080p+ kaynaklarda sadece 'Keskinleştir' etkilidir)");
        let upscale_model = gtk::StringList::new(&[
            "Kapalı",
            "Keskinleştir",
            "Hafif",
            "Ultra",
            "Hafif + Keskinleştirme",
        ]);
        upscale_row.set_model(Some(&upscale_model));
        upscale_row.set_selected(match settings.upscale.as_str() {
            "hafif_keskin" => 4,
            "ultra" => 3,
            "hafif" => 2,
            "sharp" => 1,
            _ => 0,
        });
        img_group.add(&upscale_row);

        let cover_q_row = adw::ComboRow::new();
        cover_q_row.set_title("Kapak Kalitesi");
        cover_q_row.set_subtitle(
            "Liste ve film kapaklarının indirilen çözünürlüğü (değişim anında uygulanır)",
        );
        let cover_q_names: Vec<&str> = crate::api::COVER_QUALITIES
            .iter()
            .map(|(_, n)| *n)
            .collect();
        cover_q_row.set_model(Some(&gtk::StringList::new(&cover_q_names)));
        cover_q_row.set_selected(
            crate::api::COVER_QUALITIES
                .iter()
                .position(|(id, _)| *id == settings.cover_quality)
                .unwrap_or(1) as u32,
        );
        img_group.add(&cover_q_row);

        let upscale_desc = gtk::Label::new(Some(
            "Yalnızca kaynak çözünürlüğü ekrandan küçükse etki eder.\nHafif: DTD (iGPU dostu, hafif). Ultra: CNN (en kaliteli). Hafif + Keskinleştirme: DTD + keskinleştirme filtresi.",
        ));
        upscale_desc.set_wrap(true);
        upscale_desc.set_xalign(0.0);
        upscale_desc.set_margin_top(2);
        upscale_desc.set_margin_bottom(8);
        upscale_desc.set_margin_start(14);
        upscale_desc.set_selectable(false);
        upscale_desc.add_css_class("dim-label");
        img_group.add(&upscale_desc);
        root.append(&img_group);

        let fansub_group = adw::PreferencesGroup::new();
        fansub_group.set_title("Çeviri (Fansub) Seçimi");
        let (ask_row, ask_sw) = crate::ui::components::switch_row(
            "Her bölümde sor",
            "Kapalıysa otomatik olarak en yüksek puanlı çeviri seçilir",
            settings.fansub_ask_each_time,
        );
        fansub_group.add(&ask_row);
        let fansub_desc = gtk::Label::new(Some(
            "Bir bölüme tıkladığınızda mevcut çeviriler listelenir (örn. Kirigana, Wolwead). Puan yıldızı topluluk oylarına dayanır.",
        ));
        fansub_desc.set_wrap(true);
        fansub_desc.set_xalign(0.0);
        fansub_desc.set_margin_top(2);
        fansub_desc.set_margin_bottom(8);
        fansub_desc.set_margin_start(14);
        fansub_desc.set_selectable(false);
        fansub_desc.add_css_class("dim-label");
        fansub_group.add(&fansub_desc);
        root.append(&fansub_group);
        let update_group = adw::PreferencesGroup::new();
        update_group.set_title("Güncelleme");

        let on_save = Rc::new(on_save);

        let update_supported = crate::update::self_update_supported();
        let update_desc = match crate::update::install_kind() {
            crate::update::InstallKind::AppImage => {
                "Başlatmada yeni sürümü kontrol eder ve AppImage'i kendisi günceller"
            }
            crate::update::InstallKind::Deb | crate::update::InstallKind::Rpm => {
                "Başlatmada yeni sürümü kontrol eder; paketi indirip sistem paket yöneticisiyle kurar (yetki istenir)"
            }
            crate::update::InstallKind::Unknown => {
                "Yalnızca AppImage, .deb veya .rpm kurulumlarında kullanılabilir"
            }
        };
        let (auto_update_row, auto_update_sw) = crate::ui::components::switch_row(
            "Otomatik Güncelleme",
            update_desc,
            settings.auto_update,
        );
        auto_update_row.set_sensitive(update_supported);
        update_group.add(&auto_update_row);

        let (notify_row, notify_sw) = crate::ui::components::switch_row(
            "Güncel Sürüm Bildirimi",
            "Başlatmada güncel sürümdeyken bilgilendirme göster",
            settings.notify_uptodate,
        );
        notify_row.set_sensitive(update_supported);
        update_group.add(&notify_row);

        let check_btn = gtk::Button::with_label("Şimdi Güncelle");
        check_btn.add_css_class("flat");
        check_btn.add_css_class("pill");
        check_btn.set_margin_top(4);
        check_btn.set_sensitive(update_supported);
        let check_btn_c = check_btn.clone();
        let settings_for_suppress = settings.clone();
        let on_save_suppress = on_save.clone();
        check_btn.connect_clicked(move |_| {
            if let Some(win) = check_btn_c.root().and_downcast::<gtk::Window>() {
                let cur = settings_for_suppress.clone();
                let suppress = on_save_suppress.clone();
                crate::update::check_and_prompt(&win, true, move || {
                    let mut sup = cur.clone();
                    sup.notify_uptodate = false;
                    suppress(sup);
                });
            }
        });
        update_group.add(&check_btn);
        root.append(&update_group);

        let shortcut_row_c = shortcut_row.clone();
        search_toggle.connect_active_notify(move |r| {
            shortcut_row_c.set_sensitive(r.is_active());
        });

        let s_base = settings.clone();

        let save_all = {
            let st_r = search_toggle.clone();
            let sc_r = shortcut_row.clone();
            let ssc_r = search_sc_row.clone();
            let scale_spin_c = scale_spin.clone();
            let nf_r = news_font_row.clone();
            let theme_r = theme_row.clone();
            let fs_r = fs_sw.clone();
            let ih_r = intro_hint_sw.clone();
            let mh_r = music_hint_sw.clone();
            let pq_r = play_q_sw.clone();
            let an_r = auto_next_sw.clone();
            let au_r = auto_update_sw.clone();
            let notify_r = notify_sw.clone();
            let up_r = upscale_row.clone();
            let cq_r = cover_q_row.clone();
            let light_r = light_sw.clone();
            let patience_spin_c = patience_spin.clone();
            let maxpar_spin_c = maxpar_spin.clone();
            let ask_r = ask_sw.clone();
            let s = s_base.clone();
            let on_save = on_save.clone();
            Rc::new(move || {
                let mut updated = s.clone();
                updated.quick_search_enabled = st_r.is_active();
                updated.quick_search_shortcut = match sc_r.selected() {
                    1 => "Ctrl+F".into(),
                    2 => "F3".into(),
                    _ => "/".into(),
                };
                updated.search_shortcut = match ssc_r.selected() {
                    1 => "F2".into(),
                    2 => "/".into(),
                    _ => "Ctrl+S".into(),
                };
                updated.ui_scale = (scale_spin_c.value() / 100.0).clamp(1.0, 1.25) as f32;
                updated.news_font_size = crate::api::NEWS_FONT_OPTIONS
                    .get(nf_r.selected() as usize)
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_else(|| "normal".to_string());
                updated.theme = crate::theme::THEMES
                    .get(theme_r.selected() as usize)
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_else(|| crate::theme::DEFAULT_THEME.to_string());
                updated.auto_fullscreen = fs_r.is_active();
                updated.show_intro_hint = ih_r.is_active();
                updated.show_music_hint = mh_r.is_active();
                updated.play_ask_quality = pq_r.is_active();
                updated.auto_next = an_r.is_active();
                updated.auto_update = au_r.is_active();
                updated.notify_uptodate = notify_r.is_active();
                updated.upscale = match up_r.selected() {
                    1 => "sharp".into(),
                    2 => "hafif".into(),
                    3 => "ultra".into(),
                    4 => "hafif_keskin".into(),
                    _ => "off".into(),
                };
                updated.cover_quality = crate::api::COVER_QUALITIES
                    .get(cq_r.selected() as usize)
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_else(|| crate::api::DEFAULT_COVER_QUALITY.to_string());
                updated.light_mode = light_r.is_active();
                updated.source_patience_secs = patience_spin_c.value() as u64;
                updated.max_parallel_downloads =
                    (maxpar_spin_c.value().round() as u64).clamp(1, 32);
                updated.fansub_ask_each_time = ask_r.is_active();
                on_save(updated);
            })
        };

        let sa_ask = save_all.clone();
        ask_sw.connect_active_notify(move |_| sa_ask());

        let sa1 = save_all.clone();
        search_toggle.connect_active_notify(move |_| sa1());
        let sa2 = save_all.clone();
        shortcut_row.connect_selected_notify(move |_| sa2());
        let sa3 = save_all.clone();
        search_sc_row.connect_selected_notify(move |_| sa3());
        let sa_scale = save_all.clone();
        scale_spin.connect_value_changed(move |_| sa_scale());
        let sa_nf = save_all.clone();
        news_font_row.connect_selected_notify(move |_| sa_nf());
        let sa_theme = save_all.clone();
        theme_row.connect_selected_notify(move |_| sa_theme());
        let sa4 = save_all.clone();
        fs_sw.connect_active_notify(move |_| sa4());
        let sa5a = save_all.clone();
        intro_hint_sw.connect_active_notify(move |_| sa5a());
        let sa5b = save_all.clone();
        music_hint_sw.connect_active_notify(move |_| sa5b());
        let sa5c = save_all.clone();
        play_q_sw.connect_active_notify(move |_| sa5c());
        let sa5d = save_all.clone();
        auto_next_sw.connect_active_notify(move |_| sa5d());
        let sa6 = save_all.clone();
        auto_update_sw.connect_active_notify(move |_| sa6());
        let sa7 = save_all.clone();
        notify_sw.connect_active_notify(move |_| sa7());
        let sa8 = save_all.clone();
        upscale_row.connect_selected_notify(move |_| sa8());
        let sa_cq = save_all.clone();
        cover_q_row.connect_selected_notify(move |_| sa_cq());
        let sa_mp = save_all.clone();
        maxpar_spin.connect_value_changed(move |_| sa_mp());
        let sa9 = save_all.clone();
        light_sw.connect_active_notify(move |_| sa9());
        let sa10 = save_all.clone();
        patience_spin.connect_value_changed(move |_| sa10());

        let dl_group = adw::PreferencesGroup::new();
        dl_group.set_title("İndirilenler");
        let dl_dir_row = adw::ActionRow::new();
        dl_dir_row.set_title("İndirme Klasörü");
        let initial_dl = s_base.download_dir.clone().unwrap_or_else(|| {
            crate::download::default_download_dir()
                .to_string_lossy()
                .into_owned()
        });
        dl_dir_row.set_subtitle(&initial_dl);
        let dl_pick = gtk::Button::with_label("Değiştir");
        dl_pick.add_css_class("flat");
        dl_pick.add_css_class("pill");
        dl_pick.set_valign(gtk::Align::Center);
        dl_dir_row.add_suffix(&dl_pick);
        dl_group.add(&dl_dir_row);
        dl_group.add(&maxpar_row);
        root.insert_child_after(&dl_group, Some(&player_group));
        {
            let s_o = s_base.clone();
            let on_o = on_save.clone();
            let row_o = dl_dir_row.clone();
            dl_pick.connect_clicked(move |_| {
                let s_base_c = s_o.clone();
                let on_save_c = on_o.clone();
                let row_c = row_o.clone();
                let dlg = gtk::FileChooserNative::new(
                    Some("İndirme Klasörü Seç"),
                    None::<&gtk::Window>,
                    gtk::FileChooserAction::SelectFolder,
                    Some("_Seç"),
                    Some("_Vazgeç"),
                );
                dlg.set_modal(true);
                dlg.connect_response(move |d, r| {
                    if r == gtk::ResponseType::Accept {
                        if let Some(f) = d.file() {
                            if let Some(path) = f.path() {
                                let dir = path.to_string_lossy().into_owned();
                                let mut s = s_base_c.clone();
                                s.download_dir = Some(dir.clone());
                                row_c.set_subtitle(&dir);
                                on_save_c(s);
                            }
                        }
                    } else {
                        eprintln!("[DL] klasör seçilemedi: vazgeçildi");
                    }
                });
                dlg.show();
            });
        }

        let data_group = adw::PreferencesGroup::new();
        data_group.set_title("Veri Yönetimi");

        let (uninstall_row, uninstall_sw) = crate::ui::components::switch_row(
            "Uygulamayı ve Başlatıcıyı da Sistemden Kaldır",
            "Sıfırlama ile birlikte uygulama binary dosyasını ve masaüstü kısayollarını tamamen siler",
            true,
        );
        data_group.add(&uninstall_row);

        let wipe_btn = gtk::Button::with_label("Tüm Verileri Sıfırla ve Temizle");
        wipe_btn.add_css_class("destructive-action");
        wipe_btn.set_margin_top(8);
        let on_wipe = Rc::new(on_wipe);
        let un_c = uninstall_sw.clone();

        wipe_btn.connect_clicked(move |btn| {
            let remove_app = un_c.is_active();
            let parent_win = btn.root().and_downcast::<gtk::Window>();

            let dialog = adw::MessageDialog::builder()
                .heading("Kalıcı Sıfırlama Onayı ⚠️")
                .body(if remove_app {
                    "Tüm izleme geçmişiniz, ayarlarınız, kapak önbelleği ve UYGULAMA DOSYALARI sisteminizden kalıcı olarak silinecek. Emin misiniz?"
                } else {
                    "Tüm izleme geçmişiniz, ayarlarınız ve kapak önbelleği sıfırlanacak. Emin misiniz?"
                })
                .close_response("cancel")
                .default_response("cancel")
                .build();

            if let Some(win) = parent_win.as_ref() {
                dialog.set_transient_for(Some(win));
            }

            dialog.add_response("cancel", "İptal");
            dialog.add_response("wipe", "Evet, Kalıcı Olarak Sil");
            dialog.set_response_appearance("wipe", adw::ResponseAppearance::Destructive);

            let on_wipe_c = on_wipe.clone();
            dialog.connect_response(None, move |_, resp| {
                if resp == "wipe" {
                    on_wipe_c(remove_app);
                }
            });

            dialog.present();
        });
        data_group.add(&wipe_btn);
        root.append(&data_group);

        let info_group = adw::PreferencesGroup::new();
        info_group.set_title("Uygulama Bilgisi");

        let ver_row = adw::ActionRow::new();
        ver_row.set_title("Sürüm Numarası");
        ver_row.set_subtitle(&format!(
            "AnimeciX Masaüstü İstemcisi  •  v{}",
            env!("CARGO_PKG_VERSION")
        ));

        let ver_badge = gtk::Label::new(Some("Güncel ✓"));
        ver_badge.add_css_class("status-badge-completed");
        ver_badge.set_valign(gtk::Align::Center);
        ver_row.add_suffix(&ver_badge);
        info_group.add(&ver_row);

        let reinstall_btn =
            gtk::Button::with_label("Masaüstü Başlatıcısını Sistemime Kur / Güncelle");
        reinstall_btn.add_css_class("flat");
        reinstall_btn.add_css_class("pill");
        reinstall_btn.set_margin_top(4);
        reinstall_btn.connect_clicked(|_| {
            let _ = crate::install_desktop_entry();
        });
        info_group.add(&reinstall_btn);

        let gh_row = adw::ActionRow::new();
        gh_row.set_title("GitHub");
        gh_row.set_subtitle("https://github.com/veilzon/animecix-linux");
        let gh_icon = crate::ui::brand_icons::github_image(16);
        gh_row.add_prefix(&gh_icon);
        let gh_btn = gtk::Button::with_label("Aç");
        gh_btn.add_css_class("flat");
        gh_btn.add_css_class("pill");
        gh_btn.set_valign(gtk::Align::Center);
        gh_btn.connect_clicked(|_| {
            let url = "https://github.com/veilzon/animecix-linux";
            let ok = std::process::Command::new("xdg-open")
                .arg(url)
                .spawn()
                .is_ok();
            if !ok {
                let _ = std::process::Command::new("gio")
                    .args(["open", url])
                    .spawn();
            }
        });
        gh_row.add_suffix(&gh_btn);
        info_group.add(&gh_row);

        root.append(&info_group);

        // Ayar arama: satır başlık+açar sözcüklerini gezer, uymayanı gizler.
        // Grup başlığı her satırın metnine katılır; satırı kalmayan grup gizlenir.
        // NOT: AdwPreferencesGroup'un ilk çocuğu satırlar değil iç kutudur;
        // satırlar ListBox içinde derinde durur — o yüzden özyinelemeli toplanır.
        let mut search_groups: Vec<(adw::PreferencesGroup, Vec<(gtk::Widget, String)>)> =
            Vec::new();
        let mut gc = root.first_child();
        while let Some(c) = gc {
            let next = c.next_sibling();
            if let Ok(g) = c.clone().downcast::<adw::PreferencesGroup>() {
                let gt = g.title().to_string();
                let mut rows = Vec::new();
                collect_searchable(&g.clone().upcast(), &mut rows);
                let rows = rows
                    .into_iter()
                    .map(|r| {
                        let hay = tr_fold(&format!("{gt} {}", row_search_text(&r)));
                        (r, hay)
                    })
                    .collect();
                search_groups.push((g, rows));
            }
            gc = next;
        }
        settings_search.connect_search_changed(move |e| {
            let q = tr_fold(e.text().trim());
            for (g, rows) in &search_groups {
                let mut any = false;
                for (w, h) in rows {
                    let show = q.is_empty() || h.contains(&q);
                    w.set_visible(show);
                    any = any || show;
                }
                g.set_visible(q.is_empty() || any);
            }
        });

        root
    }
}

/// Türkçe duyarsız arama katlaması: I→ı/İ→i, küçük harf, aksan sadeleştirme.
/// Hem satır metnine (bir kez) hem sorguya (her tuşta) uygulanır.
pub fn tr_fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let mapped = match c {
            'I' => 'ı',
            'İ' => 'i',
            _ => c,
        };
        for lc in mapped.to_lowercase() {
            out.push(match lc {
                'ö' => 'o',
                'ü' => 'u',
                'ğ' => 'g',
                'ş' => 's',
                'ç' => 'c',
                'â' => 'a',
                'î' => 'i',
                'û' => 'u',
                _ => lc,
            });
        }
    }
    out
}

/// Grup içindeki aranabilir satırları toplar (ActionRow + bağımsız butonlar).
/// Satırın içine girilmez (anahtar/suffix ayrıca indekslenmez); başlık
/// etiketleri atlanır (grup başlığı her satırın metnine ayrıca katılır).
fn collect_searchable(w: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    if w.clone().downcast::<adw::ActionRow>().is_ok() || w.clone().downcast::<gtk::Button>().is_ok()
    {
        out.push(w.clone());
        return;
    }
    let mut c = w.first_child();
    while let Some(ch) = c {
        let n = ch.next_sibling();
        collect_searchable(&ch, out);
        c = n;
    }
}

fn row_search_text(w: &gtk::Widget) -> String {
    if let Ok(r) = w.clone().downcast::<adw::ActionRow>() {
        format!("{} {}", r.title(), r.subtitle().unwrap_or_default())
    } else if let Ok(b) = w.clone().downcast::<gtk::Button>() {
        b.label().unwrap_or_default().to_string()
    } else if let Ok(l) = w.clone().downcast::<gtk::Label>() {
        l.text().to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::tr_fold;

    #[test]
    fn tr_fold_turkish_cases() {
        assert_eq!(tr_fold("Görüntü"), "goruntu");
        assert_eq!(tr_fold("goruntu"), "goruntu");
        assert!(tr_fold("Görüntü İyileştirme").contains("goruntu"));
        assert_eq!(tr_fold("Işık"), "ısık");
        assert_eq!(tr_fold("ISIK"), "ısık");
        assert_eq!(tr_fold("Kişisel"), "kisisel");
        assert_eq!(tr_fold("Şırnak ÇĞ"), "sırnak cg");
        assert_eq!(tr_fold("  Tema  "), "  tema  ");
    }
}
