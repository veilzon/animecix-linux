use std::io;
use std::net::TcpListener;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use super::chat::{ChatLog, ChatMessage};
use super::invite::{now_ms, Invite};
use super::protocol::{Frame, VoiceFrame, WireMsg};
use super::sync::{GuestSync, HostSync};
use super::transport::ygg::YggHelper;
use super::transport::{advertised_addr, bind_listener, connect, PeerLink};
use super::types::{
    PeerId, PeerInfo, PlaybackSnapshot, PlaybackState, Role, SyncAction, MAX_PEERS,
};

pub enum Command {
    Create { name: String },
    Join { invite: Invite, name: String },
    Snapshot(PlaybackSnapshot),
    Chat(String),
    Ptt(bool),
    Voice(Vec<i16>),
    Leave,
}

pub enum Event {
    Invite(Invite),
    Joined {
        room: String,
        host: PeerInfo,
        peers: Vec<PeerInfo>,
    },
    Peers(Vec<PeerInfo>),
    Playback(PlaybackState),
    Sync(SyncAction),
    Chat(ChatMessage),
    Voice(VoiceFrame),
    Error(String),
    Closed,
}

pub struct SessionHandle {
    tx: Sender<Command>,
    rx: Receiver<Event>,
}

impl SessionHandle {
    pub fn start() -> Self {
        let (tx, cmd_rx) = mpsc::channel();
        let (evt_tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("watch-party".into())
            .spawn(move || Session::run(cmd_rx, evt_tx))
            .expect("watch-party thread");
        Self { tx, rx }
    }

    pub fn create(&self, name: impl Into<String>) {
        let _ = self.tx.send(Command::Create { name: name.into() });
    }

    pub fn join(&self, invite: Invite, name: impl Into<String>) {
        let _ = self.tx.send(Command::Join {
            invite,
            name: name.into(),
        });
    }

    pub fn snapshot(&self, snapshot: PlaybackSnapshot) {
        let _ = self.tx.send(Command::Snapshot(snapshot));
    }

    pub fn chat(&self, text: impl Into<String>) {
        let _ = self.tx.send(Command::Chat(text.into()));
    }

    pub fn ptt(&self, held: bool) {
        let _ = self.tx.send(Command::Ptt(held));
    }

    /// Ses motorunun doğrudan çerçeve göndermesi için komut hattı.
    pub fn command_sender(&self) -> Sender<Command> {
        self.tx.clone()
    }

    pub fn leave(&self) {
        let _ = self.tx.send(Command::Leave);
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Leave);
    }
}

struct HostPeer {
    info: PeerInfo,
    link: PeerLink,
}

struct HostState {
    listener: TcpListener,
    invite: Invite,
    _ygg: Option<YggHelper>,
    host: PeerInfo,
    peers: Vec<HostPeer>,
    sync: HostSync,
    chat: ChatLog,
    ptt: bool,
    voice_seq: u32,
}

struct GuestState {
    link: PeerLink,
    _ygg: Option<YggHelper>,
    peers: Vec<PeerInfo>,
    sync: GuestSync,
    chat: ChatLog,
    ptt: bool,
    voice_seq: u32,
}

enum State {
    Idle,
    Host(HostState),
    Guest(GuestState),
}

struct Session {
    events: Sender<Event>,
    peer_rx: Receiver<(PeerId, Frame)>,
    peer_tx: Sender<(PeerId, Frame)>,
}

impl Session {
    fn run(commands: Receiver<Command>, events: Sender<Event>) {
        let (peer_tx, peer_rx) = mpsc::channel();
        let s = Self {
            events,
            peer_rx,
            peer_tx,
        };
        let mut state = State::Idle;
        loop {
            let mut leave = false;
            while let Ok(cmd) = commands.try_recv() {
                match cmd {
                    Command::Create { name } => match Self::start_host(name.clone()) {
                        Ok(host) => {
                            let _ = s.events.send(Event::Invite(host.invite.clone()));
                            state = State::Host(host);
                        }
                        Err(e) => {
                            let _ = s.events.send(Event::Error(e));
                        }
                    },
                    Command::Join { invite, name } => {
                        match Self::start_guest(&invite, name, &s.peer_tx) {
                            Ok((guest, host)) => {
                                let peers = guest.peers.clone();
                                let room = invite.room.clone();
                                state = State::Guest(guest);
                                let _ = s.events.send(Event::Joined { room, host, peers });
                            }
                            Err(e) => {
                                let _ = s.events.send(Event::Error(e));
                            }
                        }
                    }
                    Command::Snapshot(snapshot) => match &mut state {
                        State::Host(host) => {
                            if let Some(playback) = host.sync.on_snapshot(&snapshot, now_ms()) {
                                Self::host_broadcast(
                                    host,
                                    &Frame::Json(WireMsg::Playback(playback.clone())),
                                );
                                let _ = s.events.send(Event::Playback(playback));
                            }
                        }
                        State::Guest(guest) => {
                            let action = guest.sync.action_for(&snapshot, now_ms());
                            if !matches!(action, SyncAction::None) {
                                let _ = s.events.send(Event::Sync(action));
                            }
                        }
                        State::Idle => {}
                    },
                    Command::Chat(text) => match &mut state {
                        State::Host(host) => {
                            let msg = ChatMessage {
                                id: host.host.id,
                                name: host.host.name.clone(),
                                text,
                                ts: now_ms(),
                            };
                            host.chat.push(msg.clone());
                            Self::host_broadcast(
                                host,
                                &Frame::Json(WireMsg::Chat {
                                    id: msg.id,
                                    name: msg.name.clone(),
                                    text: msg.text.clone(),
                                    ts: msg.ts,
                                }),
                            );
                            let _ = s.events.send(Event::Chat(msg));
                        }
                        State::Guest(guest) => {
                            let msg = WireMsg::Chat {
                                id: 0,
                                name: String::new(),
                                text,
                                ts: now_ms(),
                            };
                            let _ = guest.link.send(&Frame::Json(msg));
                        }
                        State::Idle => {}
                    },
                    Command::Ptt(held) => match &mut state {
                        State::Host(host) => host.ptt = held,
                        State::Guest(guest) => guest.ptt = held,
                        State::Idle => {}
                    },
                    Command::Voice(samples) => match &mut state {
                        State::Host(host) => {
                            if host.ptt && !samples.is_empty() {
                                let frame = VoiceFrame {
                                    id: host.host.id,
                                    seq: host.voice_seq,
                                    samples,
                                };
                                host.voice_seq = host.voice_seq.wrapping_add(1);
                                Self::host_broadcast(host, &Frame::Voice(frame));
                            }
                        }
                        State::Guest(guest) => {
                            if guest.ptt && !samples.is_empty() {
                                let frame = VoiceFrame {
                                    id: 0,
                                    seq: guest.voice_seq,
                                    samples,
                                };
                                guest.voice_seq = guest.voice_seq.wrapping_add(1);
                                let _ = guest.link.send(&Frame::Voice(frame));
                            }
                        }
                        State::Idle => {}
                    },
                    Command::Leave => leave = true,
                }
            }

            while let Ok((peer_id, frame)) = s.peer_rx.try_recv() {
                match &mut state {
                    State::Host(host) => Self::host_on_frame(&s.events, host, peer_id, frame),
                    State::Guest(guest) => Self::guest_on_frame(&s.events, guest, frame),
                    State::Idle => {}
                }
            }

            if leave {
                match &mut state {
                    State::Host(host) => Self::host_broadcast(host, &Frame::Json(WireMsg::Bye)),
                    State::Guest(guest) => {
                        let _ = guest.link.send(&Frame::Json(WireMsg::Bye));
                    }
                    State::Idle => {}
                }
                // TCP/Yggdrasil tamponunun Bye'yi taşımasına izin ver; ardından
                // yardımcı süreç kapanacak.
                std::thread::sleep(Duration::from_millis(120));
                let _ = s.events.send(Event::Closed);
                return;
            }

            if let State::Host(host) = &mut state {
                if let Err(e) = Self::host_accept(&s.events, host, &s.peer_tx) {
                    eprintln!("[WATCHPARTY] accept: {e}");
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn start_host(name: String) -> Result<HostState, String> {
        let listener = bind_listener().map_err(|e| format!("oda açılamadı: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("oda dinleyicisi: {e}"))?;
        let local_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let ygg = match YggHelper::start_host(local_port) {
            Ok(y) => Some(y),
            Err(e) => {
                eprintln!("[WATCHPARTY] Yggdrasil başlatılamadı, doğrudan TCP: {e}");
                None
            }
        };
        let addr = ygg
            .as_ref()
            .map(|y| y.host_addr())
            .unwrap_or_else(|| advertised_addr(&listener));
        let host = PeerInfo {
            id: 1,
            name,
            role: Role::Host,
            addr: addr.clone(),
        };
        let invite = Invite::new(addr, host.name.clone())?;
        let state = HostState {
            listener,
            invite: invite.clone(),
            _ygg: ygg,
            host: host.clone(),
            peers: Vec::new(),
            sync: HostSync::default(),
            chat: ChatLog::default(),
            ptt: false,
            voice_seq: 0,
        };
        Ok(state)
    }

    fn start_guest(
        invite: &Invite,
        name: String,
        peer_tx: &Sender<(PeerId, Frame)>,
    ) -> Result<(GuestState, PeerInfo), String> {
        if invite.is_expired(now_ms()) {
            return Err("davet kodunun süresi dolmuş".into());
        }
        let (mut stream, ygg) = if invite.host_addr.starts_with('[') {
            let y = YggHelper::start_guest()?;
            let s = y
                .connect(&invite.host_addr)
                .map_err(|e| format!("odaya bağlanılamadı: {e}"))?;
            (s, Some(y))
        } else {
            (
                connect(&invite.host_addr).map_err(|e| format!("odaya bağlanılamadı: {e}"))?,
                None,
            )
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| e.to_string())?;
        let hello = WireMsg::Hello {
            peer: PeerInfo {
                id: 0,
                name,
                role: Role::Guest,
                addr: String::new(),
            },
            room: invite.room.clone(),
        };
        super::protocol::write_frame(&mut stream, &Frame::Json(hello))
            .map_err(|e| format!("merhaba gönderilemedi: {e}"))?;
        let welcome = super::protocol::read_frame(&mut stream)
            .map_err(|e| format!("oda yanıtı okunamadı: {e}"))?
            .ok_or_else(|| "oda bağlantısı kapandı".to_string())?;
        stream.set_read_timeout(None).map_err(|e| e.to_string())?;
        let Frame::Json(WireMsg::Welcome {
            room,
            host,
            peers,
            state,
        }) = welcome
        else {
            return Err("oda yanıtı geçersiz".into());
        };
        if room != invite.room {
            return Err("oda kimliği uyuşmuyor".into());
        }
        let reader_stream = stream.try_clone().map_err(|e| e.to_string())?;
        let link = PeerLink::new(stream).map_err(|e| e.to_string())?;
        PeerLink::reader(0, reader_stream, peer_tx.clone());
        let mut guest = GuestState {
            link,
            _ygg: ygg,
            peers,
            sync: GuestSync::default(),
            chat: ChatLog::default(),
            ptt: false,
            voice_seq: 0,
        };
        if let Some(state) = state {
            guest.sync.on_state(state);
        }
        Ok((guest, host))
    }

    fn host_accept(
        events: &Sender<Event>,
        host: &mut HostState,
        peer_tx: &Sender<(PeerId, Frame)>,
    ) -> io::Result<()> {
        let (mut stream, _addr) = match host.listener.accept() {
            Ok(v) => v,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) => return Err(e),
        };
        if host.peers.len() + 1 >= MAX_PEERS {
            let _ = super::protocol::write_frame(&mut stream, &Frame::Json(WireMsg::Bye));
            return Ok(());
        }
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let hello = super::protocol::read_frame(&mut stream)?;
        let Some(Frame::Json(WireMsg::Hello { peer, room })) = hello else {
            return Ok(());
        };
        if room != host.invite.room {
            let _ = super::protocol::write_frame(&mut stream, &Frame::Json(WireMsg::Bye));
            return Ok(());
        }
        stream.set_read_timeout(None)?;
        let id = host
            .peers
            .iter()
            .map(|p| p.info.id)
            .max()
            .unwrap_or(host.host.id)
            .saturating_add(1);
        let info = PeerInfo {
            id,
            name: peer.name,
            role: Role::Guest,
            addr: peer.addr,
        };
        let welcome = WireMsg::Welcome {
            room: host.invite.room.clone(),
            host: host.host.clone(),
            peers: host.peers.iter().map(|p| p.info.clone()).collect(),
            state: host.sync.state().cloned(),
        };
        super::protocol::write_frame(&mut stream, &Frame::Json(welcome))?;
        let reader_stream = stream.try_clone()?;
        let link = PeerLink::new(stream)?;
        PeerLink::reader(id, reader_stream, peer_tx.clone());
        host.peers.push(HostPeer {
            info: info.clone(),
            link,
        });
        Self::host_broadcast_except(host, id, &Frame::Json(WireMsg::PeerJoined(info.clone())));
        let peers = host.peers.iter().map(|p| p.info.clone()).collect();
        let _ = events.send(Event::Peers(peers));
        Ok(())
    }

    fn host_broadcast(host: &HostState, frame: &Frame) {
        for p in &host.peers {
            let _ = p.link.send(frame);
        }
    }

    fn host_broadcast_except(host: &HostState, skip: PeerId, frame: &Frame) {
        for p in &host.peers {
            if p.info.id != skip {
                let _ = p.link.send(frame);
            }
        }
    }

    fn host_on_frame(events: &Sender<Event>, host: &mut HostState, peer_id: PeerId, frame: Frame) {
        let Some(sender) = host.peers.iter().find(|p| p.info.id == peer_id) else {
            return;
        };
        match frame {
            Frame::Json(WireMsg::Chat { id, name, text, ts }) => {
                let msg = ChatMessage {
                    id,
                    name: if name.is_empty() {
                        sender.info.name.clone()
                    } else {
                        name
                    },
                    text,
                    ts,
                };
                host.chat.push(msg.clone());
                Self::host_broadcast(
                    host,
                    &Frame::Json(WireMsg::Chat {
                        id: msg.id,
                        name: msg.name.clone(),
                        text: msg.text.clone(),
                        ts: msg.ts,
                    }),
                );
                let _ = events.send(Event::Chat(msg));
            }
            Frame::Voice(v) => {
                Self::host_broadcast_except(host, peer_id, &Frame::Voice(v.clone()));
                let _ = events.send(Event::Voice(v));
            }
            Frame::Json(WireMsg::Ping { nonce, .. }) => {
                if let Some(p) = host.peers.iter().find(|p| p.info.id == peer_id) {
                    let _ = p.link.send(&Frame::Json(WireMsg::Pong {
                        nonce,
                        host_ms: now_ms(),
                    }));
                }
            }
            Frame::Json(WireMsg::Bye) => {
                host.peers.retain(|p| p.info.id != peer_id);
                Self::host_broadcast(host, &Frame::Json(WireMsg::PeerLeft { id: peer_id }));
                let peers = host.peers.iter().map(|p| p.info.clone()).collect();
                let _ = events.send(Event::Peers(peers));
            }
            _ => {}
        }
    }

    fn guest_on_frame(events: &Sender<Event>, guest: &mut GuestState, frame: Frame) {
        match frame {
            Frame::Json(WireMsg::Playback(state)) => {
                guest.sync.on_state(state.clone());
                let _ = events.send(Event::Playback(state));
            }
            Frame::Json(WireMsg::Chat { id, name, text, ts }) => {
                let msg = ChatMessage { id, name, text, ts };
                guest.chat.push(msg.clone());
                let _ = events.send(Event::Chat(msg));
            }
            Frame::Voice(v) => {
                let _ = events.send(Event::Voice(v));
            }
            Frame::Json(WireMsg::PeerJoined(info)) => {
                if !guest.peers.iter().any(|p| p.id == info.id) {
                    guest.peers.push(info);
                }
                let _ = events.send(Event::Peers(guest.peers.clone()));
            }
            Frame::Json(WireMsg::PeerLeft { id }) => {
                guest.peers.retain(|p| p.id != id);
                let _ = events.send(Event::Peers(guest.peers.clone()));
            }
            Frame::Json(WireMsg::Ping { nonce, .. }) => {
                let _ = guest.link.send(&Frame::Json(WireMsg::Pong {
                    nonce,
                    host_ms: now_ms(),
                }));
            }
            Frame::Json(WireMsg::Bye) => {
                let _ = events.send(Event::Closed);
            }
            _ => {}
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.events.send(Event::Closed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait<F>(handle: &SessionHandle, mut pred: F, label: &str) -> Event
    where
        F: FnMut(&Event) -> bool,
    {
        for _ in 0..1200 {
            while let Some(event) = handle.try_recv() {
                if pred(&event) {
                    return event;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("olay gelmedi: {label}");
    }

    fn snap(pos: f64) -> PlaybackSnapshot {
        PlaybackSnapshot {
            position: pos,
            duration: 100.0,
            paused: false,
            buffering: false,
            speed: 1.0,
            episode: "7:1:1".into(),
        }
    }

    #[test]
    fn invite_host_and_guest_handshake() {
        let host = SessionHandle::start();
        host.create("host");
        let Event::Invite(invite) = wait(&host, |e| matches!(e, Event::Invite(_)), "invite") else {
            unreachable!()
        };
        assert!(!invite.is_expired(now_ms()));
        assert!(invite.host_addr.contains(':'));

        let guest = SessionHandle::start();
        guest.join(invite, "guest");
        let Event::Joined {
            host: host_info,
            peers,
            ..
        } = wait(&guest, |e| matches!(e, Event::Joined { .. }), "joined")
        else {
            unreachable!()
        };
        assert_eq!(host_info.name, "host");
        assert!(peers.is_empty());

        let Event::Peers(peers) = wait(&host, |e| matches!(e, Event::Peers(_)), "peers") else {
            unreachable!()
        };
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "guest");
    }

    #[test]
    fn host_playback_chat_and_voice_reach_guest() {
        let host = SessionHandle::start();
        host.create("host");
        let Event::Invite(invite) = wait(&host, |e| matches!(e, Event::Invite(_)), "invite") else {
            unreachable!()
        };
        let guest = SessionHandle::start();
        guest.join(invite, "guest");
        let _ = wait(&guest, |e| matches!(e, Event::Joined { .. }), "joined");
        let _ = wait(&host, |e| matches!(e, Event::Peers(_)), "peers");

        host.snapshot(snap(10.0));
        let Event::Playback(state) = wait(&guest, |e| matches!(e, Event::Playback(_)), "playback")
        else {
            unreachable!()
        };
        assert_eq!(state.position, 10.0);

        guest.snapshot(snap(20.0));
        let Event::Sync(SyncAction::Seek(pos)) =
            wait(&guest, |e| matches!(e, Event::Sync(_)), "sync")
        else {
            unreachable!()
        };
        assert!(pos > 9.0);

        guest.chat("selam");
        let Event::Chat(msg) = wait(&host, |e| matches!(e, Event::Chat(_)), "host chat") else {
            unreachable!()
        };
        assert_eq!(msg.text, "selam");
        let Event::Chat(msg) = wait(&guest, |e| matches!(e, Event::Chat(_)), "guest chat") else {
            unreachable!()
        };
        assert_eq!(msg.text, "selam");

        guest.ptt(true);
        let _ = guest.command_sender().send(Command::Voice(vec![1, 2, 3]));
        let Event::Voice(v) = wait(&host, |e| matches!(e, Event::Voice(_)), "host voice") else {
            unreachable!()
        };
        assert_eq!(v.samples, vec![1, 2, 3]);

        host.leave();
        let _ = wait(&host, |e| matches!(e, Event::Closed), "host closed");
        let _ = wait(&guest, |e| matches!(e, Event::Closed), "guest closed");
    }
}
