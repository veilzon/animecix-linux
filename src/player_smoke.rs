use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;

use crate::api::{Client, Episode, Title};
use crate::app::player::{self, EmbedRequest, PlayerHost};
use crate::discord_rpc::{DiscordConfig, DiscordRpc};
use crate::ui::operation_stack::OperationStack;

pub fn validate_path(path: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("video yolu boş".into());
    }
    if !Path::new(path).is_file() {
        return Err(format!("video bulunamadı: {path}"));
    }
    Ok(())
}

/// Gömülü oynatıcıyı yerel bir videoyla açar. Test/CI ve görsel kontrol
/// içindir; normal uygulama akışına girmez.
pub fn run(path: &str) {
    if let Err(e) = validate_path(path) {
        eprintln!("PLAYER_SMOKE_FAIL: {e}");
        std::process::exit(1);
    }

    let video = path.to_string();
    let success = Rc::new(Cell::new(false));
    let success_a = success.clone();
    let app = adw::Application::builder()
        .application_id("tr.com.animecix.smoke")
        .build();

    app.connect_activate(move |app| {
        let client = std::sync::Arc::new(Client::new());
        let api = match crate::embed_mpv::loader::load()
            .and_then(crate::embed_mpv::player::MpvApi::from_loaded)
        {
            Ok(api) => Rc::new(api),
            Err(e) => {
                eprintln!("PLAYER_SMOKE_FAIL: libmpv: {e}");
                app.quit();
                return;
            }
        };

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("AnimeciX Oynatıcı Testi")
            .default_width(1280)
            .default_height(800)
            .build();
        let theme = std::env::var("ANIMECIX_SMOKE_THEME").unwrap_or_else(|_| "koyu".into());
        crate::theme::apply_theme(&window, &theme);

        let title = Title {
            name: "AnimeciX Oynatıcı Testi".into(),
            ..Default::default()
        };
        let ep = Episode {
            season: 1,
            episode: 1,
            name: "Yerel video".into(),
        };
        let host = PlayerHost {
            client: client.clone(),
            ops: OperationStack::new(),
            discord: DiscordRpc::start(DiscordConfig {
                enabled: false,
                socket: None,
            }),
            progress: Rc::new(std::cell::RefCell::new(HashMap::new())),
            on_back: Rc::new(|| {}),
            on_play_episode: Rc::new(|_| {}),
            on_select_fansub: Rc::new(|_| {}),
            on_neighbor: Rc::new(|_| {}),
            on_download: Rc::new(|_| {}),
            on_watch_party: Rc::new(|| {}),
            on_ready: Rc::new(|| eprintln!("PLAYER_SMOKE_READY")),
            on_terminal: Rc::new(|state, msg| eprintln!("PLAYER_SMOKE_TERMINAL: {state:?} {msg}")),
        };
        let episode_count = std::env::var("ANIMECIX_SMOKE_EPISODE_COUNT")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let smoke_episodes: Vec<Episode> = (1..=episode_count)
            .map(|i| Episode {
                season: 1,
                episode: i,
                name: format!("{i}. Bölüm"),
            })
            .collect();
        let req = EmbedRequest {
            title,
            ep,
            candidates: vec![video.clone()],
            fast_embeds: Vec::new(),
            fallback_embeds: Vec::new(),
            source_labels: Vec::new(),
            fallback_labels: Vec::new(),
            fansubs: Vec::new(),
            fansub_id: 0,
            quality_map: HashMap::new(),
            plan: None,
            saved_pos: None,
            upscale: "off".into(),
            hwdec: std::env::var("ANIMECIX_SMOKE_HWDEC").unwrap_or_else(|_| "off".into()),
            auto_fullscreen: false,
            show_skip_button: true,
            show_song_card: true,
            show_music_hint: false,
            cache_forward_mb: 64,
            cache_back_mb: 16,
            patience_secs: 10,
            auto_next: false,
            auto_skip_outro: false,
            episodes: smoke_episodes,
        };
        let Some(player) = player::build(&window, host, req, api) else {
            eprintln!("PLAYER_SMOKE_FAIL: gömülü oynatıcı başlatılamadı");
            app.quit();
            return;
        };
        let player = Rc::new(player);
        window.set_content(Some(player.widget()));
        window.present();

        let hold_secs = std::env::var("ANIMECIX_SMOKE_HOLD")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(2)
            .max(1);
        let started = Instant::now();
        let ready_at = Rc::new(Cell::new(None::<Instant>));
        let app_t = app.clone();
        let player_t = player.clone();
        let success_t = success_a.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if started.elapsed() > Duration::from_secs(15 + hold_secs) {
                eprintln!("PLAYER_SMOKE_FAIL: 15 sn içinde video hazır olmadı");
                app_t.quit();
                return glib::ControlFlow::Break;
            }
            let pos = player_t.position();
            if pos > 0.3 && ready_at.get().is_none() {
                ready_at.set(Some(Instant::now()));
                println!("PLAYER_SMOKE_READY pos={pos:.2}");
            }
            if let Some(t) = ready_at.get() {
                if t.elapsed() >= Duration::from_secs(hold_secs) {
                    player_t.shutdown();
                    success_t.set(true);
                    app_t.quit();
                    return glib::ControlFlow::Break;
                }
            }
            glib::ControlFlow::Continue
        });
    });

    let run = app.run_with_args(&["animecix-smoke"]);
    if run.value() == 0 && success.get() {
        println!("PLAYER_SMOKE_OK");
    } else {
        if !success.get() {
            eprintln!("PLAYER_SMOKE_FAIL");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_rejects_missing_path() {
        assert!(super::validate_path("/tmp/animecix-smoke-yok.mp4").is_err());
    }
}
