//! Uçtan uca test için mikrofonsuz, GTK'sız oda istemcisi.
//!
//! `animecix --watch-party-bot <davet> [saniye]` ile bir odaya katılır,
//! sohbet mesajı ve kısa bir test tonu gönderir. `--watch-party-host-bot`
//! ise oda kurup davet kodunu stdout'a yazar. İkisi de yalnızca tanılama
//! amaçlıdır; normal uygulama akışını etkilemez.

use std::f64::consts::TAU;
use std::time::{Duration, Instant};

use super::invite::now_ms;
use super::session::Command;
use super::{Event, Invite, SessionHandle};

const RATE: f64 = 16_000.0;
const CHUNK: usize = 320; // 20 ms

pub fn run_guest(code: &str, seconds: u64) {
    let Ok(invite) = Invite::decode(code) else {
        eprintln!("davet kodu okunamadı");
        return;
    };
    if invite.is_expired(now_ms()) {
        eprintln!("davet kodunun süresi dolmuş");
        return;
    }
    let handle = SessionHandle::start();
    handle.join(invite, "AnimeciX Bot");
    println!("[BOT] odaya katılınıyor…");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut joined = false;
    while Instant::now() < deadline {
        while let Some(event) = handle.try_recv() {
            match event {
                Event::Joined { host, peers, .. } => {
                    joined = true;
                    println!("[BOT] katıldı: host={} kişi={}", host.name, peers.len() + 1);
                    handle.chat("Merhaba, AnimeciX test botu katıldı.");
                    play_tone(&handle);
                    handle.chat("Ses testi tamam.");
                }
                Event::Peers(peers) => {
                    println!("[BOT] odada {} kişi", peers.len() + 1);
                }
                Event::Chat(msg) => println!("[BOT] sohbet {}: {}", msg.name, msg.text),
                Event::Voice(v) => println!("[BOT] ses {} ({} örnek)", v.id, v.samples.len()),
                Event::Playback(state) => println!(
                    "[BOT] senkron konum={:.1} duraklatılmış={} nesil={}",
                    state.position, state.paused, state.generation
                ),
                Event::Sync(action) => println!("[BOT] senkron eylemi: {action:?}"),
                Event::Error(e) => eprintln!("[BOT] hata: {e}"),
                Event::Closed => {
                    println!("[BOT] oda kapandı");
                    return;
                }
                Event::Invite(_) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    if !joined {
        println!("[BOT] odaya katılamadı");
    }
    handle.leave();
    std::thread::sleep(Duration::from_millis(200));
    println!("[BOT] ayrıldı");
}

pub fn run_host(seconds: u64) {
    let handle = SessionHandle::start();
    handle.create("AnimeciX Bot Host");
    println!("[BOT] oda kuruluyor…");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        while let Some(event) = handle.try_recv() {
            match event {
                Event::Invite(invite) => println!("DAVET={}", invite.encode()),
                Event::Peers(peers) => {
                    println!("[BOT] odada {} kişi", peers.len() + 1);
                }
                Event::Chat(msg) => println!("[BOT] sohbet {}: {}", msg.name, msg.text),
                Event::Voice(v) => println!("[BOT] ses {} ({} örnek)", v.id, v.samples.len()),
                Event::Error(e) => eprintln!("[BOT] hata: {e}"),
                Event::Closed => return,
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    handle.leave();
    std::thread::sleep(Duration::from_millis(200));
}

fn play_tone(handle: &SessionHandle) {
    handle.ptt(true);
    let sender = handle.command_sender();
    let chunks = (RATE as usize) / CHUNK; // 1 saniye
    for i in 0..chunks {
        let samples: Vec<i16> = (0..CHUNK)
            .map(|n| {
                let t = ((i * CHUNK + n) as f64) / RATE;
                ((t * 440.0 * TAU).sin() * 0.18 * i16::MAX as f64) as i16
            })
            .collect();
        let _ = sender.send(Command::Voice(samples));
        std::thread::sleep(Duration::from_millis(20));
    }
    handle.ptt(false);
}
