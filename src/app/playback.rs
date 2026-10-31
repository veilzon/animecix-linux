use gtk::prelude::*;

use crate::api::{self, Episode, Title};
use crate::app::{
    opening_finish_delay, play_terminal, App, Msg, PlayEvent, PlaySources, PlayTerminal,
};
use crate::ui::components;
use crate::ui::operation_stack::OperationState;

impl App {
    pub(crate) fn play(&self, title: &Title, ep: &Episode) {
        if let Some(rec) = crate::local_playback::find_for_episode(
            &self.dl_manager.snapshot(),
            title.id,
            &title.name,
            ep.season,
            ep.episode,
        ) {
            self.play_local_record(title, ep, &rec);
            return;
        }
        let title = title.clone();
        let ep = ep.clone();
        eprintln!(
            "[PLAY] çağrıldı: {} S{:02}E{:02}",
            title.name, ep.season, ep.episode
        );
        // İzleme geçmişi/değişir → eve dönüşte devam rafı tazelensin.
        self.home_dirty.set(true);
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
                Err(e) if api::is_server_error(e) => c
                    .resolve_best_video(title_s.id, ep_s.episode, ep_s.season)
                    .ok(),
                _ => None,
            };
            move || match fallback_url {
                Some(url) => {
                    let plan = crate::skip::fetch_plan_for_embeds(
                        &c,
                        title_s.id,
                        ep_s.season,
                        ep_s.episode,
                        &[],
                        &[],
                        &manual_s,
                    );
                    Msg::Play(
                        title_s,
                        ep_s,
                        Ok(PlaySources {
                            candidates: vec![url],
                            fast_embeds: Vec::new(),
                            fallback_embeds: Vec::new(),
                            plan,
                        }),
                    )
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

    pub(crate) fn after_fansubs_loaded(
        &self,
        title: Title,
        ep: Episode,
        fansubs: Vec<api::FansubInfo>,
        default_template: Option<i64>,
    ) {
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
            &format!(
                "{} — {}",
                title.name,
                components::episode_label_compact(ep.season, ep.episode, &ep.name)
            ),
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
                let quals = crate::play_quality::available_play_qualities(&c, &fs_c.mirrors);
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

    pub(crate) fn play_with_fansub_inner(
        &self,
        title: &Title,
        ep: &Episode,
        fs: &api::FansubInfo,
        rest: Vec<api::FansubInfo>,
        forced: Option<crate::play_quality::PlayQuality>,
    ) {
        self.ops.info(
            &format!(
                "🎬 {} hazırlanıyor ({} · {:.1}★)…",
                title.name, fs.name, fs.rating
            ),
            "",
            std::time::Duration::from_secs(2),
        );
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
                    .map(|fast_pairs| {
                        let mut fb = client
                            .episode_candidates(title_c.id, ep_c.episode, ep_c.season)
                            .unwrap_or_default();
                        let tried_n = 3.min(fb.len());
                        fb.drain(..tried_n);
                        let fast: Vec<String> = fast_pairs.iter().map(|(m, _)| m.clone()).collect();
                        let fast_emb: Vec<String> =
                            fast_pairs.iter().map(|(_, e)| e.clone()).collect();
                        (fast, fast_emb, fb)
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
                        &client,
                        title_c.id,
                        ep_c.season,
                        ep_c.episode,
                        &fast_emb,
                        &fb,
                        &manual_c,
                    );
                    Ok(PlaySources {
                        candidates: fast,
                        fast_embeds: fast_emb,
                        fallback_embeds: fb,
                        plan,
                    })
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
        self.ops.info(
            &format!("🎬 {} hazırlanıyor…", title.name),
            "",
            std::time::Duration::from_secs(2),
        );
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
                                if api::Client::source_host_hint(u) == p.as_str() {
                                    0
                                } else {
                                    1
                                }
                            });
                        }
                        let fast: Vec<String> = fast_pairs.iter().map(|(m, _)| m.clone()).collect();
                        let fast_emb: Vec<String> =
                            fast_pairs.iter().map(|(_, e)| e.clone()).collect();
                        let plan = crate::skip::fetch_plan_for_embeds(
                            &c, title.id, ep.season, ep.episode, &fast_emb, &fb, &manual_r,
                        );
                        Ok(PlaySources {
                            candidates: fast,
                            fast_embeds: fast_emb,
                            fallback_embeds: fb,
                            plan,
                        })
                    })
            };
            move || Msg::Play(title, ep, res)
        });
    }

    pub(crate) fn decide_retry(exited: bool, success: bool, playing: bool) -> (bool, bool) {
        if !exited {
            return (false, playing);
        }
        if playing && !success {
            return (true, false);
        }
        (false, playing)
    }

    /// Komşu bölüme geç: bölüm listesini worker'da çekip sıralar, yöndeki
    /// komşuyu bulur, `Msg::NextEp` ile oynatır. Sınırda toast'lık hata döner
    /// (filmde liste boşsa da aynı yol). `dir`: +1 sonraki, -1 önceki.
    fn request_neighbor(&self, title: &Title, cur: &Episode, dir: i8) {
        if title.title_type.as_deref() == Some("movie") {
            self.ops.info(
                "Filmde sonraki bölüm yok",
                "",
                std::time::Duration::from_secs(3),
            );
            return;
        }
        let label = if dir > 0 {
            "⏭ Sonraki bölüm açılıyor…"
        } else {
            "⏮ Önceki bölüm açılıyor…"
        };
        self.ops.info(label, "", std::time::Duration::from_secs(2));
        let title_c = title.clone();
        let cur_c = cur.clone();
        self.spawn(move |c| {
            let res =
                c.episodes(&c.enrich_title(&title_c))
                    .map(|mut eps| {
                        eps.sort_by_key(|e| (e.season, e.episode));
                        eps
                    })
                    .map_err(|e| format!("Bölüm listesi alınamadı: {e}"))
                    .and_then(|eps| {
                        crate::api::neighbor_episode(&eps, cur_c.season, cur_c.episode, dir)
                            .ok_or_else(|| {
                                if dir > 0 {
                                    "Son bölümdesin".to_string()
                                } else {
                                    "İlk bölümdesin".to_string()
                                }
                            })
                    });
            let title_r = title_c.clone();
            move || Msg::NextEp(title_r, res)
        });
    }

    const SOCKET_TIMEOUT_SECS: u64 = 25;

    pub(crate) fn source_is_dead(
        elapsed_secs: u64,
        core_idle: bool,
        duration: f64,
        media_loaded: bool,
        threshold_secs: u64,
    ) -> bool {
        !media_loaded && core_idle && duration <= 0.0 && elapsed_secs >= threshold_secs
    }

    pub(crate) fn socket_timeout_hit(elapsed_secs: u64, socket_seen: bool) -> bool {
        !socket_seen && elapsed_secs >= Self::SOCKET_TIMEOUT_SECS
    }

    /// Yerel indirilmiş dosyayı mevcut mpv/RPC supervisor'ıyla oynatır.
    pub(crate) fn play_local(&self, rec: &crate::download::DownloadRecord) {
        let title_id = self.download_title_id(rec);
        if title_id == 0 {
            self.ops.info(
                "Eski indirme",
                "Başlık kimliği bulunamadı; ilerleme kaydı sınırlı olabilir.",
                std::time::Duration::from_secs(4),
            );
        }
        let title = Title {
            id: title_id,
            name: rec.title.clone(),
            ..Default::default()
        };
        let ep = Episode {
            season: rec.season,
            episode: rec.episode,
            name: rec.ep_name.clone(),
        };
        self.play_local_record(&title, &ep, rec);
    }

    /// Yerel dosyayı intro/outro planını çözerek oynatır.
    fn play_local_record(
        &self,
        title: &Title,
        ep: &Episode,
        rec: &crate::download::DownloadRecord,
    ) {
        if !rec.dest.is_file() {
            self.ops.error(
                "Dosya bulunamadı",
                &rec.dest.display().to_string(),
                std::time::Duration::from_secs(6),
            );
            return;
        }
        self.home_dirty.set(true);
        let path = rec.dest.to_string_lossy().into_owned();
        if title.id == 0 {
            self.play_candidates(title, ep, std::slice::from_ref(&path), &[], &[], None);
            return;
        }
        let manual_s = self.settings.borrow().official_skip_secret.clone();
        let title_c = title.clone();
        let ep_c = ep.clone();
        self.busy(true);
        self.spawn(move |c| {
            let plan = crate::skip::fetch_plan_for_embeds(
                &c,
                title_c.id,
                ep_c.season,
                ep_c.episode,
                &[],
                &[],
                &manual_s,
            );
            let title_m = title_c.clone();
            let ep_m = ep_c.clone();
            move || Msg::PlayLocal {
                title: title_m,
                ep: ep_m,
                path,
                plan,
            }
        });
    }

    /// Eski indirme kayıtlarında başlık kimliğini yerel verilerden çözer.
    fn download_title_id(&self, rec: &crate::download::DownloadRecord) -> u64 {
        if rec.title_id != 0 {
            return rec.title_id;
        }
        let name = rec.title.trim();
        if let Some(t) = self
            .cats
            .borrow()
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|t| t.name == name)
        {
            return t.id;
        }
        let st = self.client.load_state();
        st.saved
            .iter()
            .chain(st.history.iter().map(|h| &h.title))
            .chain(st.marathon.iter().map(|m| &m.title))
            .find(|t| t.name == name)
            .map(|t| t.id)
            .unwrap_or(0)
    }

    pub(crate) fn play_candidates(
        &self,
        title: &Title,
        ep: &Episode,
        candidates: &[String],
        fast_embeds: &[String],
        fallback_embeds: &[String],
        plan: Option<crate::skip::SkipPlan>,
    ) {
        let gen = self
            .play_gen
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let old_pid = self
            .current_mpv_pid
            .swap(0, std::sync::atomic::Ordering::SeqCst);
        if old_pid != 0 {
            let _ = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(old_pid.to_string())
                .status();
        }
        let (mpv_cache_forward_mb, mpv_cache_back_mb) = {
            let s = self.settings.borrow();
            (
                s.mpv_cache_forward_mb
                    .clamp(api::MPV_CACHE_FORWARD_MIN_MB, api::MPV_CACHE_FORWARD_MAX_MB),
                s.mpv_cache_back_mb
                    .clamp(api::MPV_CACHE_BACK_MIN_MB, api::MPV_CACHE_BACK_MAX_MB),
            )
        };
        let w = api::Watched {
            title_id: title.id,
            episode: ep.episode,
            season: ep.season,
            title_name: Some(title.name.clone()),
            title_type: title.title_type.clone(),
            ..Default::default()
        };
        self.client.set_current(&w);
        self.client.add_history(title, ep);
        // Sunucu kaydı (oynatma başında, sessiz; girişsizde atlanır).
        {
            let c = self.client.clone();
            let t = title.clone();
            let e = ep.clone();
            std::thread::spawn(move || {
                c.put_title(&t, &e);
            });
        }
        eprintln!(
            "[PLAY-CAND] yeni mpv başlatılıyor: {} S{:02}E{:02} (kaynak sayısı={}, plan={})",
            title.name,
            ep.season,
            ep.episode,
            candidates.len(),
            plan.as_ref().map(|p| p.source.as_str()).unwrap_or("yok"),
        );

        let media_title = format!(
            "{} | {}",
            title.name,
            components::episode_label_compact(ep.season, ep.episode, &ep.name)
        );
        let tid = title.id;
        let season = ep.season;
        let episode = ep.episode;
        let title_name_c = title.name.clone();
        let title_type_c = title.title_type.clone();
        let _prog_key = format!("{tid}:{season}:{episode}");

        let saved_pos = self.client.start_pos(tid, season, episode);

        let sock_path = format!("/tmp/animecix-mpv-{tid}-{season}-{episode}.sock");
        let _ = std::fs::remove_file(&sock_path);

        let auto_fullscreen = self.settings.borrow().auto_fullscreen;
        let upscale = self.settings.borrow().upscale.clone();
        let show_intro_hint = self.settings.borrow().show_intro_hint;
        let show_music_hint = self.settings.borrow().show_music_hint;
        let skip_times = plan.as_ref().map(|p| p.times.clone()).unwrap_or_default();
        // OSD şarkı eki için planın müzik satırları da paylaşılır.
        let skip_music_op = plan.as_ref().and_then(|p| p.music_op.clone());
        let skip_music_ed = plan.as_ref().and_then(|p| p.music_ed.clone());
        let skip_shared = std::sync::Arc::new(std::sync::Mutex::new((
            skip_times,
            skip_music_op,
            skip_music_ed,
        )));

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
        let covers_h = self.covers.clone_ref();
        let client_h = self.client.clone();
        let ops = self.ops.clone();
        let op_id = format!("play:{tid}:{season}:{episode}");

        if let Some(previous) = self.opening_op_id.borrow_mut().take() {
            if previous != op_id {
                ops.finish(&previous, OperationState::Done, std::time::Duration::ZERO);
            }
        }
        ops.begin(&op_id, "Açılıyor", &media_title);
        self.opening_op_id.borrow_mut().replace(op_id.clone());
        self.opening_op_shown_at
            .borrow_mut()
            .replace(std::time::Instant::now());

        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let current_shared = std::sync::Arc::new(std::sync::Mutex::new((episode, season)));

        let mpv_child = std::sync::Arc::new(std::sync::Mutex::new(None::<std::process::Child>));

        let (play_tx, play_rx) = std::sync::mpsc::channel::<PlayEvent>();
        {
            let play_rx = std::sync::Arc::new(std::sync::Mutex::new(play_rx));
            let alive_play = alive.clone();
            let ops_h = ops.clone();
            let op_id_h = op_id.clone();
            let media_title_h = media_title.clone();
            let gen_play = gen;
            let play_gen_h = self.play_gen.clone();
            let opening_op_id_h = self.opening_op_id.clone();
            let opening_op_shown_at_h = self.opening_op_shown_at.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                let mut final_pass = false;
                loop {
                    let rx = play_rx.lock().unwrap();
                    let mut events: Vec<PlayEvent> = Vec::new();
                    loop {
                        match rx.try_recv() {
                            Ok(ev) => events.push(ev),
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                        }
                    }
                    drop(rx);
                    for ev in events {
                        match ev {
                            PlayEvent::DismissOpening(ev_gen) => {
                                if play_gen_h.load(std::sync::atomic::Ordering::SeqCst) != ev_gen {
                                    continue;
                                }
                                let current =
                                    opening_op_id_h.borrow().as_deref() == Some(op_id_h.as_str());
                                if !current {
                                    continue;
                                }
                                // `opening_op_id` terminal olana kadar korunur;
                                // hızlı bölüm değişiminde yeni oynatma eski
                                // satırı Done yapabilsin.
                                let elapsed = match opening_op_shown_at_h.borrow_mut().take() {
                                    Some(shown_at) => shown_at.elapsed(),
                                    // Aynı oynatma için ikinci Dismiss (failover):
                                    // zamanlayıcı zaten kuruldu.
                                    None => continue,
                                };
                                if let Some(remain) = opening_finish_delay(elapsed) {
                                    ops_h.begin(&op_id_h, "Oynatılıyor", &media_title_h);
                                    let ops_later = ops_h.clone();
                                    let id_later = op_id_h.clone();
                                    let play_gen_later = play_gen_h.clone();
                                    let opening_op_id_later = opening_op_id_h.clone();
                                    let opening_op_shown_at_later = opening_op_shown_at_h.clone();
                                    glib::timeout_add_local(remain, move || {
                                        if play_gen_later.load(std::sync::atomic::Ordering::SeqCst)
                                            == gen_play
                                        {
                                            ops_later.finish(
                                                &id_later,
                                                OperationState::Done,
                                                std::time::Duration::ZERO,
                                            );
                                            let is_current =
                                                opening_op_id_later.borrow().as_deref()
                                                    == Some(id_later.as_str());
                                            if is_current {
                                                opening_op_id_later.borrow_mut().take();
                                                opening_op_shown_at_later.borrow_mut().take();
                                            }
                                        }
                                        glib::ControlFlow::Break
                                    });
                                } else {
                                    ops_h.finish(
                                        &op_id_h,
                                        OperationState::Done,
                                        std::time::Duration::ZERO,
                                    );
                                    let is_current = opening_op_id_h.borrow().as_deref()
                                        == Some(op_id_h.as_str());
                                    if is_current {
                                        opening_op_id_h.borrow_mut().take();
                                        opening_op_shown_at_h.borrow_mut().take();
                                    }
                                }
                            }
                            PlayEvent::Info(ev_gen, msg) => {
                                if play_gen_h.load(std::sync::atomic::Ordering::SeqCst) != ev_gen {
                                    continue;
                                }
                                ops_h.update(&op_id_h, &msg, None);
                            }
                            PlayEvent::Closed(ev_gen) => {
                                if play_gen_h.load(std::sync::atomic::Ordering::SeqCst) == ev_gen {
                                    crate::memory::trim_after_playback(&covers_h, &client_h);
                                }
                            }
                            PlayEvent::Failed(ev_gen, msg) => {
                                if play_gen_h.load(std::sync::atomic::Ordering::SeqCst) != ev_gen {
                                    continue;
                                }
                                let current =
                                    opening_op_id_h.borrow().as_deref() == Some(op_id_h.as_str());
                                if current {
                                    opening_op_id_h.borrow_mut().take();
                                    opening_op_shown_at_h.borrow_mut().take();
                                }
                                // Eski satır Done/silinmiş olsa bile toplam
                                // başarısızlık görünür bir hata satırı açar.
                                ops_h.begin(&op_id_h, "Açılamadı", &msg);
                                ops_h.finish(
                                    &op_id_h,
                                    OperationState::Error,
                                    std::time::Duration::from_secs(6),
                                );
                            }
                        }
                    }
                    if alive_play.load(std::sync::atomic::Ordering::Relaxed) {
                        return glib::ControlFlow::Continue;
                    }
                    if final_pass {
                        return glib::ControlFlow::Break;
                    }
                    // Supervisor son olayı gönderdikten hemen sonra `alive`i
                    // düşürebilir; son Dismiss/Failed kaybolmasın diye bir kez
                    // daha boşalt.
                    final_pass = true;
                }
            });
        }

        // Plan bildirimleri: hazır + şarkılar, ya da dürüst yokluk.
        match &plan {
            Some(p) => {
                self.ops.info(
                    &format!("⏩ Atlama hazır ({} · 's' intro, 'e' outro)", p.source),
                    "",
                    std::time::Duration::from_secs(3),
                );
                for m in [&p.music_op, &p.music_ed].into_iter().flatten() {
                    self.ops.info(m, "", std::time::Duration::from_secs(4));
                }
            }
            None => {
                self.ops.info(
                    "⚠️ İntro/outro zamanları bulunamadı",
                    "",
                    std::time::Duration::from_secs(3),
                );
            }
        }

        {
            let alive = alive.clone();
            let sock_poll = sock_path.clone();
            let sock_c = sock_path.clone();
            let skip_c = skip_shared.clone();
            let show_intro_hint_c = show_intro_hint;
            let _client_c = client.clone();
            let _current_shared_c = current_shared.clone();
            let (sender, receiver) = std::sync::mpsc::channel::<(f64, f64)>();
            std::thread::spawn(move || {
                let mut op_prompted = false;
                let mut ed_prompted = false;

                while alive.load(std::sync::atomic::Ordering::Relaxed)
                    && !std::path::Path::new(&sock_poll).exists()
                {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }

                loop {
                    if std::path::Path::new(&sock_c).exists() {
                        let snap = skip_c.lock().unwrap().clone();
                        let (pos, dur) =
                            crate::player::query_mpv_position(&sock_c).unwrap_or((0.0, 0.0));
                        if show_intro_hint_c {
                            if let Some(st) = snap.0.op_start {
                                if !op_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                                    op_prompted = true;
                                    let msg = crate::skip::prompt_op(snap.1.as_deref());
                                    for cmd in crate::skip::skip_osd_cmds(&msg, 4000) {
                                        crate::player::send_mpv_cmd_retry(&sock_c, &cmd, 2);
                                    }
                                }
                            }
                            if let Some(st) = snap.0.ed_start {
                                if !ed_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                                    ed_prompted = true;
                                    let msg = crate::skip::prompt_ed(snap.2.as_deref());
                                    for cmd in crate::skip::skip_osd_cmds(&msg, 4000) {
                                        crate::player::send_mpv_cmd_retry(&sock_c, &cmd, 2);
                                    }
                                }
                            }
                        }
                        if sender.send((pos, dur)).is_err() {
                            break;
                        }
                    } else {
                        if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
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
                        Ok(v) => {
                            latest = Some(v);
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                drop(rx);
                let cur = *current_shared_t.lock().unwrap();
                let (cur_ep, cur_season) = cur;
                let pk_cur = format!("{tid}:{cur_season}:{cur_ep}");

                if disconnected {
                    // Lowell birebir: kopuşta yalnız yerel kayıt; sunucuya
                    // rapor YOK (son konum bir sonraki tick'te gider).
                    let last = progress2.borrow().get(&pk_cur).copied();
                    if let Some((pos, dur)) = last {
                        client_prog.save_progress(tid, cur.1, cur.0, pos, dur);
                    }
                    return glib::ControlFlow::Break;
                }

                if let Some((pos, dur)) = latest {
                    if pos < 1.0 {
                        return glib::ControlFlow::Continue;
                    }
                    progress2.borrow_mut().insert(pk_cur.clone(), (pos, dur));
                    if let Some(entry) = progress_bars2.borrow().get(&pk_cur) {
                        if *entry.watched.borrow() {
                            entry.bar.set_fraction(1.0);
                            entry.bar.set_visible(true);
                            entry.label.set_text("İzlendi");
                            entry.label.set_visible(true);
                        } else {
                            let (fraction, text) = crate::app::episode_bar_display(false, pos, dur);
                            entry.bar.set_fraction(fraction);
                            if let Some(text) = text {
                                entry.label.set_text(&text);
                                entry.label.set_visible(true);
                                entry.bar.set_visible(true);
                            }
                        }
                    }
                    client_prog.save_progress(tid, cur.1, cur.0, pos, dur);
                    if api::Client::played_enough(pos, dur) && marked_ep != Some(cur) {
                        client_prog.save_watched(
                            &api::Watched {
                                title_id: tid,
                                episode: cur.0,
                                season: cur.1,
                                title_name: Some(title_name_c.clone()),
                                title_type: title_type_c.clone(),
                                ..Default::default()
                            },
                            &title_name_c,
                        );
                        marked_ep = Some(cur);
                        if let Some(entry) = progress_bars2.borrow().get(&pk_cur) {
                            *entry.watched.borrow_mut() = true;
                            entry.bar.set_fraction(1.0);
                            entry.bar.set_visible(true);
                            entry.label.set_text("İzlendi");
                            entry.label.set_visible(true);
                        }
                    }
                    // Konum gönderimi kapalı: yalnız geçmiş + favori yazılır.
                    // (Yerel kayıt yukarıdaki save_progress ile tutulur.
                    // Otomatik geçiş supervisor'dadır.)
                }
                glib::ControlFlow::Continue
            });
        }

        {
            let alive = alive.clone();
            let sock_path_c = sock_path.clone();
            let input_conf_path_c = input_conf_path.clone();
            let media_title_c = media_title.clone();
            let play_tx_c = play_tx.clone();
            let saved_pos_c = saved_pos;
            let auto_fullscreen_c = auto_fullscreen;
            let upscale_c = upscale;
            let ass_path_c = ass_path.clone();
            let mpv_child_c = mpv_child.clone();
            let play_gen_c = self.play_gen.clone();
            let current_mpv_pid_c = self.current_mpv_pid.clone();
            let candidates: Vec<String> = candidates.to_vec();
            let fallback_embeds_c = fallback_embeds.to_vec();
            let fast_embeds_c = fast_embeds.to_vec();
            let candidates_len_c = candidates.len();
            let client_fb = self.client.clone();
            let tid_c = title.id;
            let patience_c = self.client.load_settings().source_patience_secs.max(10);
            let auto_next_on = self.client.load_settings().auto_next;
            let total = candidates.len() + fallback_embeds_c.len();
            // Otomatik geçiş tek-atımlık (oynatma başına bir kez).
            let mut auto_sent = false;
            // Manuel geçiş kanalı: supervisor `n quit 42` / `p quit 43`
            // çıkış kodunu okuyup yönü GTK'ya yollar. Mesaj önce işlenir,
            // `alive` sonra bakılır (kayıp mesaj yok).
            let (next_tx, next_rx) = std::sync::mpsc::channel::<i8>();
            let next_tx_c = next_tx;
            {
                let next_rx = std::sync::Arc::new(std::sync::Mutex::new(next_rx));
                let alive_n = alive.clone();
                let this_n = self.clone_ref();
                let title_n = title.clone();
                let ep_n = ep.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                    let mut dirs: Vec<i8> = Vec::new();
                    let mut disconnected = false;
                    loop {
                        match next_rx.lock().unwrap().try_recv() {
                            Ok(d) => dirs.push(d),
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                disconnected = true;
                                break;
                            }
                        }
                    }
                    // Oynatma başına tek geçiş: aynı anda ateşlenen
                    // otomatik+manuel çiftini tek oynatmaya indir
                    // (yinelenen istek eski mpv'yi ikilerdi).
                    if let Some(d) = dirs.into_iter().next() {
                        this_n.request_neighbor(&title_n, &ep_n, d);
                    }
                    if disconnected || !alive_n.load(std::sync::atomic::Ordering::Relaxed) {
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
            }
            std::thread::spawn(move || {
                let mut terminal_sent = false;
                let mut ever_media = false;
                let mut active_pid: u64 = 0;
                'supervisor: for i in 0..total {
                    if play_gen_c.load(std::sync::atomic::Ordering::SeqCst) != gen {
                        break;
                    }
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
                    if auto_fullscreen_c {
                        cmd.arg("--fullscreen");
                    }
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
                        .arg(format!("--demuxer-max-bytes={mpv_cache_forward_mb}MiB"))
                        .arg(format!("--demuxer-max-back-bytes={mpv_cache_back_mb}MiB"))
                        .arg("--demuxer-readahead-secs=120")
                        .arg("--cache-pause=yes")
                        .arg("--cache-pause-wait=3")
                        .arg("--cache-secs=120")
                        .arg("--stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_delay_max=5")
                        .arg("--network-timeout=10")
                        .arg("--hwdec=auto-safe")
                        .arg("--ytdl-format=bestvideo[height<=1080]+bestaudio/best")
                        .args(crate::api::upscale_mpv_args(&upscale_c, match upscale_c.as_str() {
                            "hafif" => crate::app::resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
                            "ultra" => crate::app::resolve_upscale_shader("Anime4K_Upscale_CNN_x2_UL.glsl"),
                            "hafif_keskin" => crate::app::resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
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
                    if play_gen_c.load(std::sync::atomic::Ordering::SeqCst) != gen {
                        break;
                    }
                    eprintln!(
                        "[SUP] mpv spawn deneniyor (ep={}, kaynak={}, url={:.80})",
                        episode, i, url
                    );
                    let child = match cmd.spawn() {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!(
                                "[SUP] HATA mpv başlatılamadı (ep={}, kaynak={}): {}",
                                episode, i, e
                            );
                            continue;
                        }
                    };
                    if play_gen_c.load(std::sync::atomic::Ordering::SeqCst) != gen {
                        let mut stale = child;
                        let _ = stale.kill();
                        let _ = stale.wait();
                        break;
                    }
                    eprintln!("[SUP] mpv spawn edildi (ep={}, kaynak={})", episode, i);
                    active_pid = child.id() as u64;
                    current_mpv_pid_c.store(active_pid, std::sync::atomic::Ordering::SeqCst);
                    *mpv_child_c.lock().unwrap() = Some(child);

                    let start = std::time::Instant::now();
                    let mut playing = false;
                    let mut media_loaded = false;
                    loop {
                        let (exited, success, code) = {
                            let mut g = mpv_child_c.lock().unwrap();
                            match g.as_mut().unwrap().try_wait() {
                                Ok(Some(status)) => (true, status.success(), status.code()),
                                Ok(None) => (false, false, None),
                                Err(_) => (true, false, None),
                            }
                        };
                        if exited {
                            // Manuel geçiş (`n`/`p`): kaynak failover DEĞİL,
                            // komşu bölüme geçilir. Mesaj GTK anketine gider.
                            if let Some(dir) = crate::player::mpv_exit_direction(code) {
                                eprintln!(
                                    "[SUP] manuel geçiş (ep={}, kaynak={}, yön={dir})",
                                    episode, i
                                );
                                let _ = next_tx_c.send(dir);
                                if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                                    let _ = c.wait();
                                }
                                // Kullanıcı bilinçli geçti: toplam hata değil,
                                // açılış satırı en kısa sürede Done olsun.
                                let _ = play_tx_c.send(PlayEvent::DismissOpening(gen));
                                terminal_sent = true;
                                playing = true;
                                break;
                            }
                            let retry = Self::decide_retry(true, success, playing).0;
                            if retry {
                                eprintln!("[SUP] kaynak hatalı çıktı, sonraki kaynağa geçiliyor (ep={}, kaynak={})", episode, i);
                                playing = false;
                            } else if playing {
                                // Kullanıcı mpv'yi kapatmış: toplam başarısızlık
                                // değil, normal bitiş.
                                let _ = play_tx_c.send(PlayEvent::DismissOpening(gen));
                                terminal_sent = true;
                            }
                            break;
                        }
                        if std::path::Path::new(&sock_path_c).exists() {
                            if !playing {
                                eprintln!(
                                    "[SUP] socket belirdi, oynatma başladı (ep={}, kaynak={})",
                                    episode, i
                                );
                                let _ = play_tx_c.send(PlayEvent::DismissOpening(gen));
                            }
                            playing = true;
                        }
                        if playing {
                            let idle = crate::player::query_mpv_prop(&sock_path_c, "core-idle")
                                .unwrap_or(0.0);
                            let dur = crate::player::query_mpv_prop(&sock_path_c, "duration")
                                .unwrap_or(0.0);
                            if dur > 0.0 {
                                media_loaded = true;
                                ever_media = true;
                            }
                            let elapsed_secs = start.elapsed().as_secs();
                            if Self::source_is_dead(
                                elapsed_secs,
                                idle >= 1.0,
                                dur,
                                media_loaded,
                                patience_c,
                            ) {
                                eprintln!("[SUP] kaynak hiç yüklemedi (idle+duration=0, {}sn), sonraki kaynağa geçiliyor (ep={}, kaynak={})", elapsed_secs, episode, i);
                                let _ = play_tx_c.send(PlayEvent::Info(
                                    gen,
                                    "Kaynak açıldı ama oynatamadı, diğer kaynağa geçiliyor…"
                                        .to_string(),
                                ));
                                if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                                    let _ = c.kill();
                                }
                                playing = false;
                                break;
                            }
                            // Otomatik sonraki bölüm: yalnız çocuk sürece
                            // supervisor dokunur (kilit yarışı yok). Koşul:
                            // ayar açık + EOF + %90 eşiği (failover'da dur=0
                            // elenir). Tek-atımlık; eski mpv öldürülüp GTK
                            // anketine yön (1) yollanır.
                            if auto_next_on && !auto_sent && media_loaded {
                                let eof =
                                    crate::player::query_mpv_prop(&sock_path_c, "eof-reached")
                                        .unwrap_or(0.0)
                                        >= 1.0;
                                if eof {
                                    let pos =
                                        crate::player::query_mpv_prop(&sock_path_c, "time-pos")
                                            .unwrap_or(0.0);
                                    if crate::player::should_auto_next(pos, dur, true, true, false)
                                    {
                                        eprintln!(
                                            "[SUP] otomatik geçiş (ep={}, kaynak={})",
                                            episode, i
                                        );
                                        auto_sent = true;
                                        let _ = next_tx_c.send(1);
                                        if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                                            let _ = c.kill();
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                        if Self::socket_timeout_hit(start.elapsed().as_secs(), playing) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }

                    eprintln!(
                        "[SUP] döngü bitti (ep={}, kaynak={}, playing={})",
                        episode, i, playing
                    );

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
                        if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                            let _ = c.wait();
                        }
                        break 'supervisor;
                    } else {
                        if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                            let _ = c.kill();
                            let _ = c.wait();
                        }
                        if i + 1 < total {
                            let _ = play_tx_c.send(PlayEvent::Info(
                                gen,
                                "Kaynak açılamadı, diğer kaynağa geçiliyor…".to_string(),
                            ));
                            continue;
                        } else {
                            let _ = play_tx_c.send(PlayEvent::Failed(
                                gen,
                                "Bölüm hiçbir kaynakta açılamadı.".to_string(),
                            ));
                            terminal_sent = true;
                            break;
                        }
                    }
                }
                if !terminal_sent {
                    match play_terminal(ever_media) {
                        PlayTerminal::Done => {
                            // Kullanıcı mpv'yi kapattıysa veya oynatma
                            // sessizce sonlandıysa açılış satırı asılı kalmasın.
                            let _ = play_tx_c.send(PlayEvent::DismissOpening(gen));
                        }
                        PlayTerminal::Failed => {
                            // Hiçbir kaynak mpv'yi bile başlatamadıysa bu bir
                            // toplam başarısızlıktır; Done değil Error göster.
                            let _ = play_tx_c.send(PlayEvent::Failed(
                                gen,
                                "Bölüm hiçbir kaynakta açılamadı.".to_string(),
                            ));
                        }
                    }
                }
                if active_pid != 0 {
                    let _ = current_mpv_pid_c.compare_exchange(
                        active_pid,
                        0,
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                    );
                }
                if play_gen_c.load(std::sync::atomic::Ordering::SeqCst) == gen {
                    let _ = play_tx_c.send(PlayEvent::Closed(gen));
                }
                alive.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = std::fs::remove_file(&sock_path_c);
            });
        }
    }
}
