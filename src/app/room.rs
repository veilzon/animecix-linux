//! Uygulama seviyesinde birlikte izleme odası.
//!
//! Oda, gömülü oynatıcıdan bağımsızdır: oyuncu kapanınca oda yaşamaya
//! devam eder. Oynatıcı yalnızca snapshot verir ve senkron eylemi tüketir.

use std::rc::Rc;

use crate::watch_party::audio::AudioEngine;
use crate::watch_party::chat::ChatLog;
use crate::watch_party::types::{PeerInfo, Role, RoomContent, SyncAction};
use crate::watch_party::{Event, Invite, PlaybackSnapshot, SessionHandle};

#[derive(Default)]
pub struct RoomService {
    pub handle: Option<Rc<SessionHandle>>,
    pub guest: bool,
    pub me: Option<PeerInfo>,
    pub host: Option<PeerInfo>,
    pub peers: Vec<PeerInfo>,
    pub invite: Option<Invite>,
    pub content: RoomContent,
    pub status: String,
    pub active: bool,
    pub unread: bool,
    pub invite_shown: bool,
    pub mic_error: Option<String>,
    pub chat: ChatLog,
    audio: Option<AudioEngine>,
    ptt: bool,
    pending_sync: Option<SyncAction>,
}

impl RoomService {
    pub fn create(&mut self, name: &str) {
        self.leave();
        let handle = Rc::new(SessionHandle::start());
        handle.create(name);
        self.handle = Some(handle.clone());
        self.guest = false;
        self.active = true;
        self.status = "Oda kuruluyor…".to_string();
        self.start_audio(&handle);
    }

    pub fn join(&mut self, invite: Invite, name: &str) {
        self.leave();
        let handle = Rc::new(SessionHandle::start());
        handle.join(invite, name);
        self.handle = Some(handle.clone());
        self.guest = true;
        self.active = true;
        self.status = "Odaya katılınıyor…".to_string();
        self.start_audio(&handle);
    }

    pub fn leave(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.leave();
        }
        self.guest = false;
        self.me = None;
        self.host = None;
        self.peers.clear();
        self.invite = None;
        self.content = RoomContent::default();
        self.status.clear();
        self.active = false;
        self.unread = false;
        self.invite_shown = false;
        self.mic_error = None;
        self.pending_sync = None;
        self.ptt = false;
        self.drop_audio();
        self.chat = ChatLog::default();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_guest(&self) -> bool {
        self.guest
    }

    pub fn is_host(&self) -> bool {
        self.active && !self.guest
    }

    pub fn count(&self) -> usize {
        if self.active {
            (self.peers.len() + 1).min(crate::watch_party::types::MAX_PEERS)
        } else {
            0
        }
    }

    pub fn mic_ok(&self) -> bool {
        self.audio.is_some()
    }

    pub fn ptt_active(&self) -> bool {
        self.ptt
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        let Some(handle) = self.handle.clone() else {
            return out;
        };
        while let Some(event) = handle.try_recv() {
            match &event {
                Event::Me(me) => {
                    self.me = Some(me.clone());
                    if me.role == Role::Host {
                        self.host = Some(me.clone());
                    }
                    self.status = if self.guest {
                        "Oynatmayı host yönetiyor".to_string()
                    } else {
                        "Oynatmayı sen yönetiyorsun".to_string()
                    };
                }
                Event::Joined { host, peers, .. } => {
                    self.host = Some(host.clone());
                    self.peers = peers.clone();
                    self.status = format!("Oynatmayı {} yönetiyor", host.name);
                }
                Event::Peers(peers) => {
                    self.peers = peers.clone();
                }
                Event::Invite(invite) => {
                    self.invite = Some(invite.clone());
                }
                Event::Chat(msg) => {
                    self.chat.push(msg.clone());
                    self.unread = true;
                }
                Event::Content(content) => {
                    self.content = content.clone();
                }
                Event::Sync(action) => {
                    self.pending_sync = Some(action.clone());
                }
                Event::Error(e) => {
                    self.status = format!("Bağlanılamadı: {e}");
                    self.active = false;
                    self.drop_audio();
                }
                Event::Closed => {
                    self.status = "Oda kapandı".to_string();
                    self.active = false;
                    self.drop_audio();
                }
                Event::Voice(voice) => {
                    if let Some(audio) = self.audio.as_ref() {
                        audio.play(&voice.samples);
                    }
                }
                _ => {}
            }
            out.push(event);
        }
        out
    }

    pub fn snapshot(&mut self, snapshot: PlaybackSnapshot) {
        if !self.active {
            return;
        }
        if let Some(handle) = self.handle.as_ref() {
            handle.snapshot(snapshot);
        }
    }

    /// Misafir oynatıcısının uygulayacağı en son senkron kararı.
    pub fn take_sync(&mut self) -> Option<SyncAction> {
        if !self.guest {
            return None;
        }
        self.pending_sync
            .take()
            .filter(|action| !matches!(action, SyncAction::None))
    }

    pub fn chat(&mut self, text: &str) {
        if let Some(handle) = self.handle.as_ref() {
            handle.chat(text);
        }
    }

    /// Host'un sayfa/başlık/bölüm durumunu yayınlar.
    pub fn publish_content(&mut self, content: RoomContent) {
        if !self.is_host() || self.content == content {
            return;
        }
        self.content = content.clone();
        if let Some(handle) = self.handle.as_ref() {
            handle.content(content);
        }
    }

    pub fn set_ptt(&mut self, held: bool) -> Result<(), String> {
        if !self.active {
            return Err("Oda aktif değil".to_string());
        }
        let Some(audio) = self.audio.as_ref() else {
            return Err("Mikrofon açılamadı".to_string());
        };
        self.ptt = held;
        if let Some(handle) = self.handle.as_ref() {
            handle.ptt(held);
        }
        audio.set_ptt(held);
        Ok(())
    }

    fn start_audio(&mut self, handle: &Rc<SessionHandle>) {
        match AudioEngine::start(handle.command_sender()) {
            Ok(engine) => self.audio = Some(engine),
            Err(e) => {
                eprintln!("[WATCHPARTY] ses motoru açılamadı: {e}");
                self.mic_error = Some(e);
            }
        }
    }

    fn drop_audio(&mut self) {
        if let Some(audio) = self.audio.take() {
            audio.set_ptt(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch_party::types::{PlaybackSnapshot, RoomContent};
    use std::time::{Duration, Instant};

    fn wait<F>(room: &mut RoomService, mut pred: F, label: &str) -> Event
    where
        F: FnMut(&Event) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            for event in room.poll() {
                if pred(&event) {
                    return event;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("olay gelmedi: {label}");
    }

    fn wait_invite(room: &mut RoomService) -> Invite {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let _ = room.poll();
            if let Some(invite) = room.invite.clone() {
                return invite;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("davet gelmedi");
    }

    fn snap(position: f64, episode: &str) -> PlaybackSnapshot {
        PlaybackSnapshot {
            position,
            duration: 100.0,
            paused: false,
            buffering: false,
            speed: 1.0,
            episode: episode.into(),
        }
    }

    #[test]
    fn app_room_lobby_content_chat_and_sync() {
        let mut host = RoomService::default();
        host.create("host");
        let invite = wait_invite(&mut host);
        let mut guest = RoomService::default();
        guest.join(invite, "guest");
        let _ = wait(&mut guest, |e| matches!(e, Event::Joined { .. }), "joined");
        let _ = wait(&mut host, |e| matches!(e, Event::Peers(_)), "peers");
        assert!(host.is_active() && host.is_host());
        assert!(guest.is_active() && guest.is_guest());

        host.publish_content(RoomContent::watching(7, "Test", "tv", 1, 2, "Bölüm 2"));
        let Event::Content(content) =
            wait(&mut guest, |e| matches!(e, Event::Content(_)), "content")
        else {
            unreachable!()
        };
        assert_eq!(content.title_id, Some(7));
        assert_eq!(content.season, Some(1));
        assert_eq!(content.episode, Some(2));

        guest.chat("selam");
        let _ = wait(&mut host, |e| matches!(e, Event::Chat(_)), "chat");

        host.snapshot(snap(10.0, "7:1:2"));
        let _ = wait(&mut guest, |e| matches!(e, Event::Playback(_)), "playback");
        guest.snapshot(snap(0.0, "7:1:2"));
        let _ = wait(&mut guest, |e| matches!(e, Event::Sync(_)), "sync");
        assert!(guest.take_sync().is_some());

        host.leave();
        guest.leave();
    }
}
