use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::api::{self, Episode, Title};
use crate::app::{App, Msg};
use crate::ui::{components, episodes_view};

/// Sezon çubuğunda doğrudan gösterilen sezon sayısı; kalanı `…` popover'ına.
const MAX_VISIBLE_SEASONS: usize = 5;

fn split_seasons(seasons: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let split = seasons.len().min(MAX_VISIBLE_SEASONS);
    (seasons[..split].to_vec(), seasons[split..].to_vec())
}

/// Bölüm filtresi: arama doluysa tüm sezonlarda arar; boşsa sezon filtresi
/// uygulanır. İzlenme filtresi her iki durumda da geçerlidir.
fn episode_filter_matches(
    q: &str,
    ep_season: u64,
    ep_episode: u64,
    ep_name: &str,
    season_filter: u64,
    watched: bool,
    watched_filter: i8,
) -> bool {
    let mut show = true;
    if !q.is_empty() {
        let name_match = ep_name.to_lowercase().contains(q);
        let ep_num_match = ep_episode.to_string() == q
            || format!("e{ep_episode}") == q
            || format!("s{ep_season:02}e{ep_episode:02}") == q;
        show &= name_match || ep_num_match;
    } else if season_filter != 0 {
        show &= ep_season == season_filter;
    }
    match watched_filter {
        1 => show &= watched,
        2 => show &= !watched,
        _ => {}
    }
    show
}

impl App {
    pub(crate) fn build_episodes_view(&self, title: &Title, eps: &[Episode]) -> gtk::Overlay {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.add_css_class("clear-scroll");

        let is_movie = title.title_type.as_deref() == Some("movie")
            || (eps.len() <= 1
                && eps
                    .first()
                    .map(|e| e.name.contains("Filmi"))
                    .unwrap_or(false));

        if is_movie {
            let header_poster = self.covers.cover_picture(title.poster.as_deref(), 220, 330);
            let bookmark_btn = components::bookmark_button(&self.client, title);
            let this_bm = self.clone_ref();
            let t_clone = title.clone();
            bookmark_btn.connect_clicked(move |b| {
                let saved = this_bm.client.toggle_saved(&t_clone);
                crate::auth::sync_fav_remote(&this_bm.client, t_clone.id, saved);
                b.set_icon_name(if saved {
                    "starred-symbolic"
                } else {
                    "non-starred-symbolic"
                });
                b.set_tooltip_text(Some(if saved {
                    "Favorilerden Çıkar"
                } else {
                    "Favorilere Ekle"
                }));
            });

            let marathon_btn = components::marathon_button(&self.client, title);
            let this_mar = self.clone_ref();
            let t_clone_mar = title.clone();
            marathon_btn.connect_clicked(move |b| {
                let added = this_mar.client.toggle_marathon(&t_clone_mar);
                b.set_icon_name(if added {
                    "media-playlist-repeat-symbolic"
                } else {
                    "bookmark-new-symbolic"
                });
                b.set_tooltip_text(Some(if added {
                    "Maratondan Çıkar"
                } else {
                    "İzleme Maratonuna Ekle"
                }));
                let msg = if added {
                    "🏆 İzleme Maratonuna eklendi!"
                } else {
                    "İzleme Maratonundan çıkarıldı"
                };
                this_mar
                    .ops
                    .info(msg, "", std::time::Duration::from_secs(2));
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
            self.progress_bars
                .borrow_mut()
                .insert(prog_key, (movie_pb, movie_lbl));
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
                                    title_id: title3.id,
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
            crate::auth::sync_fav_remote(&this_bm.client, t_clone.id, saved);
            b.set_icon_name(if saved {
                "starred-symbolic"
            } else {
                "non-starred-symbolic"
            });
            b.set_tooltip_text(Some(if saved {
                "Favorilerden Çıkar"
            } else {
                "Favorilere Ekle"
            }));
        });

        let marathon_btn = components::marathon_button(&self.client, title);
        let this_mar = self.clone_ref();
        let t_clone_mar = title.clone();
        marathon_btn.connect_clicked(move |b| {
            let added = this_mar.client.toggle_marathon(&t_clone_mar);
            b.set_icon_name(if added {
                "media-playlist-repeat-symbolic"
            } else {
                "bookmark-new-symbolic"
            });
            b.set_tooltip_text(Some(if added {
                "Maratondan Çıkar"
            } else {
                "İzleme Maratonuna Ekle"
            }));
            let msg = if added {
                "🏆 İzleme Maratonuna eklendi!"
            } else {
                "İzleme Maratonundan çıkarıldı"
            };
            this_mar
                .ops
                .info(msg, "", std::time::Duration::from_secs(2));
        });

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
                let n = dl_checks
                    .borrow()
                    .iter()
                    .filter(|(_, c)| c.is_active())
                    .count();
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

        let detail_header = episodes_view::create_title_detail_header(
            title,
            &header_poster,
            &bookmark_btn,
            &marathon_btn,
            &dl_mode_btn,
        );
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

        // Sezon + izlenme filtre çubuğu (satırlar kurulunca dolar;
        // boş listede gösterilmez).
        let mut seasons: Vec<u64> = eps.iter().map(|e| e.season).collect();
        seasons.sort_unstable();
        seasons.dedup();
        let filter_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        filter_bar.set_margin_start(12);
        filter_bar.set_margin_end(12);
        filter_bar.set_margin_bottom(8);
        filter_bar.set_valign(gtk::Align::Center);
        if !eps.is_empty() {
            root.append(&filter_bar);
        }

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
            let rows: Vec<(Episode, gtk::Box, Rc<RefCell<bool>>)> = eps
                .iter()
                .map(|e| {
                    let key = format!("{}:{}:{}", title.id, e.season, e.episode);

                    let label = components::episode_label_spaced(e.season, e.episode, &e.name);
                    let name = gtk::Label::new(Some(&label));
                    name.set_xalign(0.0);
                    name.add_css_class("title-4");
                    name.set_hexpand(true);
                    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    name.set_single_line_mode(true);
                    name.set_wrap(false);
                    name.set_max_width_chars(70);
                    name.set_tooltip_text(Some(&label));

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

                    let (saved_pos, saved_dur) = self
                        .progress
                        .borrow()
                        .get(&key)
                        .copied()
                        .unwrap_or((0.0, 0.0));

                    let progress_bar = gtk::ProgressBar::new();
                    progress_bar.add_css_class("episode-progress");

                    let fmt_time = |s: f64| -> String {
                        let s = s as u64;
                        if s >= 3600 {
                            format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
                        } else {
                            format!("{}:{:02}", s / 60, s % 60)
                        }
                    };

                    if saved_dur > 0.0 && saved_pos > 1.0 {
                        progress_bar.set_fraction((saved_pos / saved_dur).clamp(0.0, 1.0));
                        progress_bar.set_visible(true);
                        time_lbl.set_text(&format!(
                            "{} / {}",
                            fmt_time(saved_pos),
                            fmt_time(saved_dur)
                        ));
                        time_lbl.set_visible(true);
                    } else {
                        progress_bar.set_visible(false);
                        time_lbl.set_visible(false);
                    }
                    right_col.append(&progress_bar);

                    self.progress_bars
                        .borrow_mut()
                        .insert(key.clone(), (progress_bar.clone(), time_lbl.clone()));

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
                            .map(|l| {
                                l.iter()
                                    .any(|x| x.episode == e.episode && x.season == e.season)
                            })
                            .unwrap_or(false)
                            || (saved_dur > 0.0 && saved_pos / saved_dur > 0.9),
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
                                    this2.start_download_prefetch(
                                        title2.clone(),
                                        vec![ep2.clone()],
                                        quality,
                                        true,
                                    );
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
                        for w in [
                            check_c.upcast_ref::<gtk::Widget>(),
                            dl_one_c.upcast_ref::<gtk::Widget>(),
                        ] {
                            if let Some(r) = w.compute_bounds(&row_c) {
                                let (bx, by, bw, bh) = (
                                    r.x() as f64,
                                    r.y() as f64,
                                    r.width() as f64,
                                    r.height() as f64,
                                );
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
                            this_refresh
                                .ops
                                .info(msg, "", std::time::Duration::from_secs(2));
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
                            this_refresh_d.ops.info(
                                "🧹 Bölüm temizlendi",
                                "",
                                std::time::Duration::from_secs(2),
                            );
                        });

                        crate::ui::row_menu::RowMenu::build(vec![
                            (toggle_label, on_toggle),
                            ("🧹 Temizle".to_string(), on_clear),
                        ])
                        .popup_at(&row_ctx, x, y);
                    });
                    row.add_controller(right_click);

                    (e.clone(), row, is_watched.clone())
                })
                .collect();

            for (_, row_widget, _) in &rows {
                list_box.append(row_widget);
            }

            /// Bölüm satırı görünürlüğü: iç kutuyla birlikte otomatik sarılan
            /// `ListBoxRow` sarmalayıcıyı da gizler (yoksa boş çizgili bant kalır).
            fn set_ep_row_visible(inner: &gtk::Box, visible: bool) {
                inner.set_visible(visible);
                if let Some(p) = inner.parent() {
                    if p.is::<gtk::ListBoxRow>() {
                        p.set_visible(visible);
                    }
                }
            }

            let rows_rc = Rc::new(rows);
            // Birleşik filtre: arama + sezon + izlenme (Lowell apply_filter uyarlaması).
            let f_state = Rc::new(RefCell::new((
                String::new(),
                if seasons.len() > 1 { seasons[0] } else { 0 },
                0i8,
            )));
            let apply_ep_filter = {
                let rows = rows_rc.clone();
                let f_state = f_state.clone();
                Rc::new(move || {
                    let (q, season, w) = f_state.borrow().clone();
                    for (ep_data, row_widget, watched) in rows.iter() {
                        let show = episode_filter_matches(
                            &q,
                            ep_data.season,
                            ep_data.episode,
                            &ep_data.name,
                            season,
                            *watched.borrow(),
                            w,
                        );
                        set_ep_row_visible(row_widget, show);
                    }
                })
            };
            let season_pills: Rc<RefCell<Vec<(u64, gtk::Button)>>> =
                Rc::new(RefCell::new(Vec::new()));
            let overflow_season_btn: Rc<RefCell<Option<gtk::Button>>> = Rc::new(RefCell::new(None));
            // Sezon hapları: ilk 5 doğrudan, kalanı `…` popover'ında.
            if seasons.len() > 1 {
                let lbl = gtk::Label::new(Some("Sezon:"));
                lbl.add_css_class("dim-label");
                lbl.set_valign(gtk::Align::Center);
                filter_bar.append(&lbl);
                let (visible_seasons, overflow_seasons) = split_seasons(&seasons);
                let select_season: Rc<dyn Fn(u64)> = {
                    let pills = season_pills.clone();
                    let f_state = f_state.clone();
                    let apply = apply_ep_filter.clone();
                    let overflow_btn = overflow_season_btn.clone();
                    let visible = visible_seasons.clone();
                    Rc::new(move |s: u64| {
                        {
                            let mut st = f_state.borrow_mut();
                            st.1 = if st.1 == s { 0 } else { s };
                        }
                        let selected = f_state.borrow().1;
                        for (season, btn) in pills.borrow().iter() {
                            if *season == selected {
                                btn.add_css_class("suggested-action");
                            } else {
                                btn.remove_css_class("suggested-action");
                            }
                        }
                        if let Some(btn) = overflow_btn.borrow().as_ref() {
                            if selected != 0 && !visible.contains(&selected) {
                                btn.add_css_class("suggested-action");
                            } else {
                                btn.remove_css_class("suggested-action");
                            }
                        }
                        apply();
                    })
                };
                for s in &visible_seasons {
                    let b = gtk::Button::with_label(&format!("S{s}"));
                    b.add_css_class("pill");
                    if *s == seasons[0] {
                        b.add_css_class("suggested-action");
                    }
                    b.set_valign(gtk::Align::Center);
                    let select = select_season.clone();
                    let ss = *s;
                    b.connect_clicked(move |_| select(ss));
                    season_pills.borrow_mut().push((*s, b.clone()));
                    filter_bar.append(&b);
                }
                if !overflow_seasons.is_empty() {
                    let btn = gtk::Button::with_label("…");
                    btn.add_css_class("pill");
                    btn.set_valign(gtk::Align::Center);
                    btn.set_tooltip_text(Some("Diğer sezonlar"));
                    let pop = gtk::Popover::new();
                    pop.set_has_arrow(false);
                    pop.set_autohide(true);
                    pop.set_size_request(160, -1);
                    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 4);
                    box_.set_margin_top(8);
                    box_.set_margin_bottom(8);
                    box_.set_margin_start(8);
                    box_.set_margin_end(8);
                    for s in &overflow_seasons {
                        let b = gtk::Button::with_label(&format!("S{s}"));
                        b.add_css_class("flat");
                        b.add_css_class("pill");
                        b.set_halign(gtk::Align::Fill);
                        let select = select_season.clone();
                        let pop_c = pop.clone();
                        let ss = *s;
                        b.connect_clicked(move |_| {
                            select(ss);
                            pop_c.popdown();
                        });
                        season_pills.borrow_mut().push((*s, b.clone()));
                        box_.append(&b);
                    }
                    let scroll = gtk::ScrolledWindow::new();
                    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
                    scroll.set_max_content_height(320);
                    scroll.set_propagate_natural_height(true);
                    scroll.set_child(Some(&box_));
                    pop.set_child(Some(&scroll));
                    pop.connect_closed(|p| p.unparent());
                    let btn_c = btn.clone();
                    let pop_c = pop.clone();
                    btn.connect_clicked(move |_| {
                        pop_c.set_parent(&btn_c);
                        pop_c.popup();
                    });
                    *overflow_season_btn.borrow_mut() = Some(btn.clone());
                    filter_bar.append(&btn);
                }
            }
            {
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                spacer.set_hexpand(true);
                filter_bar.append(&spacer);
                let btns: Rc<RefCell<Vec<gtk::ToggleButton>>> = Rc::new(RefCell::new(Vec::new()));
                let busy = Rc::new(Cell::new(false));
                for (i, n) in ["Tümü", "İzlendi", "İzlenmemiş"].iter().enumerate() {
                    let tb = gtk::ToggleButton::with_label(n);
                    tb.add_css_class("pill");
                    if i == 0 {
                        tb.set_active(true);
                    }
                    {
                        let btns = btns.clone();
                        let busy = busy.clone();
                        let f_state = f_state.clone();
                        let apply = apply_ep_filter.clone();
                        tb.connect_toggled(move |b| {
                            if busy.get() {
                                return;
                            }
                            busy.set(true);
                            if b.is_active() {
                                for (j, x) in btns.borrow().iter().enumerate() {
                                    if j != i {
                                        x.set_active(false);
                                    }
                                }
                                f_state.borrow_mut().2 = i as i8;
                            } else if !btns.borrow().iter().any(|x| x.is_active()) {
                                b.set_active(true);
                            }
                            busy.set(false);
                            apply();
                        });
                    }
                    btns.borrow_mut().push(tb.clone());
                    filter_bar.append(&tb);
                }
            }
            apply_ep_filter();
            {
                let f_state = f_state.clone();
                let apply = apply_ep_filter.clone();
                let season_pills = season_pills.clone();
                let overflow_btn = overflow_season_btn.clone();
                ep_search_entry.connect_search_changed(move |e| {
                    let q = e.text().trim().to_lowercase();
                    {
                        let mut st = f_state.borrow_mut();
                        st.0 = q.clone();
                        // Arama tüm sezonlarda çalışsın: sezon seçimini bırak.
                        if !q.is_empty() {
                            st.1 = 0;
                        }
                    }
                    if !q.is_empty() {
                        for (_, b) in season_pills.borrow().iter() {
                            b.remove_css_class("suggested-action");
                        }
                        if let Some(b) = overflow_btn.borrow().as_ref() {
                            b.remove_css_class("suggested-action");
                        }
                    }
                    apply();
                });
            }

            root.append(&list_box);

            // Detay sekmeleri: Bölümler (canlı) + Ekip/Benzerler/İncelemeler
            // (auth-gated; hesap desteği gelene kadar giriş-isteyen yer tutucu).
            // (Lowell sekme çubuğu uyarlaması.)
            let tab_names = ["Bölümler", "Ekip", "Benzerler", "İncelemeler"];
            let tabbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            tabbar.add_css_class("linked");
            tabbar.set_halign(gtk::Align::Fill);
            tabbar.set_hexpand(true);
            tabbar.set_margin_bottom(8);
            tabbar.set_margin_start(12);
            tabbar.set_margin_end(12);
            let placeholders: Vec<gtk::Widget> = [
                (
                    "Ekip",
                    "Oyuncu ve ekip künyesi için giriş gerekli.\nHesap desteği yakında.",
                    "system-users-symbolic",
                ),
                (
                    "Benzerler",
                    "Benzer başlıklar için giriş gerekli.\nHesap desteği yakında.",
                    "applications-graphics-symbolic",
                ),
                (
                    "İncelemeler",
                    "İncelemeler için giriş gerekli.\nHesap desteği yakında.",
                    "document-edit-symbolic",
                ),
            ]
            .iter()
            .map(|(t, b, icon)| {
                let sp = components::create_status_page(t, b, icon);
                sp.set_visible(false);
                sp.set_halign(gtk::Align::Fill);
                sp.set_hexpand(true);
                sp.set_valign(gtk::Align::Center);
                root.append(&sp);
                sp.upcast::<gtk::Widget>()
            })
            .collect();
            let tab_btns: Rc<RefCell<Vec<gtk::ToggleButton>>> = Rc::new(RefCell::new(Vec::new()));
            let tab_busy = Rc::new(Cell::new(false));
            for (i, n) in tab_names.iter().enumerate() {
                let tb = gtk::ToggleButton::with_label(n);
                // Dar pencerede küçül + kısalt (kaydırma yok).
                tb.set_hexpand(true);
                if let Some(l) = tb.child().and_downcast::<gtk::Label>() {
                    l.set_hexpand(true);
                    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                }
                if i == 0 {
                    tb.set_active(true);
                }
                {
                    let tab_btns = tab_btns.clone();
                    let tab_busy = tab_busy.clone();
                    let list_box_c = list_box.clone();
                    let placeholders_c = placeholders.clone();
                    tb.connect_toggled(move |b| {
                        if tab_busy.get() {
                            return;
                        }
                        tab_busy.set(true);
                        if b.is_active() {
                            for (j, x) in tab_btns.borrow().iter().enumerate() {
                                if j != i {
                                    x.set_active(false);
                                }
                            }
                            list_box_c.set_visible(i == 0);
                            for (j, ph) in placeholders_c.iter().enumerate() {
                                ph.set_visible(i == j + 1);
                            }
                        } else if !tab_btns.borrow().iter().any(|x| x.is_active()) {
                            b.set_active(true);
                        }
                        tab_busy.set(false);
                    });
                }
                tab_btns.borrow_mut().push(tb.clone());
                tabbar.append(&tb);
            }
            root.insert_child_after(&tabbar, Some(&filter_bar));
        }

        scroll.set_child(Some(&root));
        let page_overlay = gtk::Overlay::new();
        page_overlay.set_child(Some(&scroll));
        page_overlay.add_overlay(&dl_reveal);
        page_overlay
    }
}

#[cfg(test)]
mod tests {
    use super::{episode_filter_matches, split_seasons};

    #[test]
    fn season_parts_show_five_then_overflow() {
        let (visible, overflow) = split_seasons(&[1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(visible, vec![1, 2, 3, 4, 5]);
        assert_eq!(overflow, vec![6, 7]);
        let (visible, overflow) = split_seasons(&[3, 4]);
        assert_eq!(visible, vec![3, 4]);
        assert!(overflow.is_empty());
    }

    #[test]
    fn episode_search_ignores_season_filter() {
        assert!(episode_filter_matches("2", 3, 2, "Ad", 1, false, 0));
        assert!(episode_filter_matches("s03e02", 3, 2, "Ad", 1, false, 0));
        assert!(!episode_filter_matches("", 3, 2, "Ad", 1, false, 0));
        assert!(episode_filter_matches("", 1, 2, "Ad", 1, false, 0));
        assert!(episode_filter_matches("2", 3, 2, "Ad", 1, true, 1));
        assert!(!episode_filter_matches("2", 3, 2, "Ad", 1, false, 1));
    }
}
