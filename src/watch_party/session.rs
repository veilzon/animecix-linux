use std::collections::VecDeque;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use super::chat::{ChatLog, ChatMessage};
use super::invite::{now_ms, Invite};
use super::protocol::{Frame, VoiceFrame, WireMsg};
use super::sync::{GuestSync, HostSync};
use super::transport::ygg::{connect_with_retry, YggHelper};
use super::transport::{advertised_addr, bind_listener, connect, PeerLink};
use super::types::{
    PeerId, PeerInfo, PlaybackSnapshot, PlaybackState, Role, RoomContent, SyncAction, MAX_PEERS,
};

pub enum Command {
    Create {
        name: String,
    },
    Join {
        invite: Invite,
        name: String,
    },
    Snapshot(PlaybackSnapshot),
    Chat(String),
    Ptt(bool),
    Voice(Vec<i16>),
    Content(RoomContent),
    Leave,
    /// Testlerde doğrudan mesh bağlantı sayısını okumak için.
    #[cfg(test)]
    MeshCount(Sender<usize>),
}

pub enum Event {
    Invite(Invite),
    /// Bu istemcinin odadaki kimliği (avatar ve kendi mesajları için).
    Me(PeerInfo),
    Joined {
        #[allow(dead_code)]
        room: String,
        host: PeerInfo,
        peers: Vec<PeerInfo>,
    },
    Peers(Vec<PeerInfo>),
    Playback(PlaybackState),
    Content(RoomContent),
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

    /// Host'un sayfa/başlık/bölüm durumunu yayınlar.
    pub fn content(&self, content: RoomContent) {
        let _ = self.tx.send(Command::Content(content));
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

    #[cfg(test)]
    fn mesh_links(&self) -> usize {
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(Command::MeshCount(tx));
        rx.recv_timeout(Duration::from_secs(2)).unwrap_or(0)
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Leave);
    }
}

struct MeshPeer {
    info: PeerInfo,
    link: PeerLink,
}

/// Aynı mesajın mesh üzerinde birden çok yoldan gelmesini engeller.
struct Seen<T> {
    items: VecDeque<T>,
}

impl<T> Default for Seen<T> {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
        }
    }
}

impl<T: PartialEq> Seen<T> {
    /// İlk kez görülen anahtar için `true` döner.
    fn fresh(&mut self, key: T) -> bool {
        if self.items.iter().any(|k| *k == key) {
            return false;
        }
        self.items.push_back(key);
        if self.items.len() > 256 {
            self.items.pop_front();
        }
        true
    }
}

/// Bir odadaki tüm doğrudan bağlantılar. Host'ta yalnızca misafirler,
/// misafirde host ve diğer misafirler bulunur.
struct Mesh {
    me: PeerInfo,
    room: String,
    links: Vec<MeshPeer>,
    seen_chat: Seen<(PeerId, u64, u64)>,
    seen_voice: Seen<(PeerId, u32)>,
    seen_playback: Seen<(u64, u64)>,
}

impl Mesh {
    fn new(me: PeerInfo, room: String) -> Self {
        Self {
            me,
            room,
            links: Vec::new(),
            seen_chat: Seen::default(),
            seen_voice: Seen::default(),
            seen_playback: Seen::default(),
        }
    }

    fn send_all(&self, frame: &Frame) {
        for p in &self.links {
            let _ = p.link.send(frame);
        }
    }

    fn send_except(&self, skip: PeerId, frame: &Frame) {
        for p in &self.links {
            if p.info.id != skip {
                let _ = p.link.send(frame);
            }
        }
    }

    fn send_to(&self, id: PeerId, frame: &Frame) {
        if let Some(p) = self.links.iter().find(|p| p.info.id == id) {
            let _ = p.link.send(frame);
        }
    }

    fn has(&self, id: PeerId) -> bool {
        self.links.iter().any(|p| p.info.id == id)
    }

    fn add(&mut self, info: PeerInfo, link: PeerLink) {
        if !self.has(info.id) {
            self.links.push(MeshPeer { info, link });
        }
    }

    fn remove(&mut self, id: PeerId) -> Option<MeshPeer> {
        let index = self.links.iter().position(|p| p.info.id == id)?;
        Some(self.links.remove(index))
    }

    fn peers(&self) -> Vec<PeerInfo> {
        self.links.iter().map(|p| p.info.clone()).collect()
    }
}

struct HostState {
    listener: TcpListener,
    invite: Invite,
    _ygg: Option<YggHelper>,
    mesh: Mesh,
    next_id: PeerId,
    sync: HostSync,
    chat: ChatLog,
    ptt: bool,
    voice_seq: u32,
}

struct GuestState {
    listener: TcpListener,
    _ygg: Option<YggHelper>,
    mesh: Mesh,
    mesh_rx: Receiver<(MeshPeer, Vec<PeerInfo>)>,
    host_id: PeerId,
    peers: Vec<PeerInfo>,
    ping_seq: u64,
    ping_sent: Option<(u64, u64)>,
    last_ping_ms: u64,
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

/// Kabul edilen bir TCP bağlantısı; el sıkışma ayrı iş parçacığında
/// okunur, oturum döngüsü yalnızca hazır bağlantıyı işler.
enum Incoming {
    HostHello {
        peer: PeerInfo,
        room: String,
        stream: TcpStream,
    },
    MeshHello {
        peer: PeerInfo,
        room: String,
        stream: TcpStream,
    },
}

struct Session {
    events: Sender<Event>,
    peer_rx: Receiver<(PeerId, Frame)>,
    peer_tx: Sender<(PeerId, Frame)>,
    accept_rx: Receiver<Incoming>,
    accept_tx: Sender<Incoming>,
}

impl Session {
    fn run(commands: Receiver<Command>, events: Sender<Event>) {
        let (peer_tx, peer_rx) = mpsc::channel();
        let (accept_tx, accept_rx) = mpsc::channel();
        let s = Self {
            events,
            peer_rx,
            peer_tx,
            accept_rx,
            accept_tx,
        };
        let mut state = State::Idle;
        loop {
            let mut leave = false;
            while let Ok(cmd) = commands.try_recv() {
                match cmd {
                    Command::Create { name } => match Self::start_host(name.clone()) {
                        Ok(host) => {
                            let me = host.mesh.me.clone();
                            let invite = host.invite.clone();
                            state = State::Host(host);
                            let _ = s.events.send(Event::Me(me));
                            let _ = s.events.send(Event::Invite(invite));
                        }
                        Err(e) => {
                            let _ = s.events.send(Event::Error(e));
                        }
                    },
                    Command::Join { invite, name } => {
                        match Self::start_guest(&invite, name, &s.peer_tx) {
                            Ok((guest, host)) => {
                                let me = guest.mesh.me.clone();
                                let peers = guest.peers.clone();
                                let room = invite.room.clone();
                                state = State::Guest(guest);
                                let _ = s.events.send(Event::Me(me));
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
                                host.mesh
                                    .seen_playback
                                    .fresh((playback.generation, playback.host_ms));
                                host.mesh
                                    .send_all(&Frame::Json(WireMsg::Playback(playback.clone())));
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
                            if let Some(msg) =
                                chat_message(host.mesh.me.id, host.mesh.me.name.clone(), text)
                            {
                                host.mesh
                                    .seen_chat
                                    .fresh((msg.id, msg.ts, text_hash(&msg.text)));
                                host.chat.push(msg.clone());
                                host.mesh.send_all(&chat_frame(&msg));
                                let _ = s.events.send(Event::Chat(msg));
                            }
                        }
                        State::Guest(guest) => {
                            if let Some(msg) =
                                chat_message(guest.mesh.me.id, guest.mesh.me.name.clone(), text)
                            {
                                guest
                                    .mesh
                                    .seen_chat
                                    .fresh((msg.id, msg.ts, text_hash(&msg.text)));
                                guest.chat.push(msg.clone());
                                guest.mesh.send_all(&chat_frame(&msg));
                                let _ = s.events.send(Event::Chat(msg));
                            }
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
                                let id = host.mesh.me.id;
                                let seq = host.voice_seq;
                                host.voice_seq = host.voice_seq.wrapping_add(1);
                                host.mesh.seen_voice.fresh((id, seq));
                                host.mesh
                                    .send_all(&Frame::Voice(VoiceFrame { id, seq, samples }));
                            }
                        }
                        State::Guest(guest) => {
                            if guest.ptt && !samples.is_empty() {
                                let id = guest.mesh.me.id;
                                let seq = guest.voice_seq;
                                guest.voice_seq = guest.voice_seq.wrapping_add(1);
                                guest.mesh.seen_voice.fresh((id, seq));
                                guest
                                    .mesh
                                    .send_all(&Frame::Voice(VoiceFrame { id, seq, samples }));
                            }
                        }
                        State::Idle => {}
                    },
                    Command::Content(content) => match &mut state {
                        State::Host(host) => {
                            host.mesh.send_all(&Frame::Json(WireMsg::Content(content)));
                        }
                        State::Guest(_) | State::Idle => {}
                    },
                    Command::Leave => leave = true,
                    #[cfg(test)]
                    Command::MeshCount(tx) => {
                        let count = match &state {
                            State::Host(host) => host.mesh.links.len(),
                            State::Guest(guest) => guest.mesh.links.len(),
                            State::Idle => 0,
                        };
                        let _ = tx.send(count);
                    }
                }
            }

            while let Ok((peer_id, frame)) = s.peer_rx.try_recv() {
                match &mut state {
                    State::Host(host) => Self::host_on_frame(&s.events, host, peer_id, frame),
                    State::Guest(guest) => Self::guest_on_frame(&s.events, guest, peer_id, frame),
                    State::Idle => {}
                }
            }

            if let State::Guest(guest) = &mut state {
                let now = now_ms();
                if now.saturating_sub(guest.last_ping_ms) >= 5_000 {
                    guest.last_ping_ms = now;
                    guest.ping_seq = guest.ping_seq.wrapping_add(1);
                    guest.ping_sent = Some((guest.ping_seq, now));
                    guest.mesh.send_to(
                        guest.host_id,
                        &Frame::Json(WireMsg::Ping {
                            nonce: guest.ping_seq,
                            host_ms: now,
                        }),
                    );
                }
                let mut changed = false;
                while let Ok((mesh_peer, extra)) = guest.mesh_rx.try_recv() {
                    let info = mesh_peer.info.clone();
                    if !guest.mesh.has(info.id) {
                        guest.mesh.add(info, mesh_peer.link);
                    }
                    for peer in extra {
                        changed |= add_presence(guest, peer);
                    }
                }
                if changed {
                    let _ = s.events.send(Event::Peers(guest.peers.clone()));
                }
            }

            while let Ok(incoming) = s.accept_rx.try_recv() {
                match (&mut state, incoming) {
                    (State::Host(host), Incoming::HostHello { peer, room, stream }) => {
                        Self::host_add_guest(&s.events, host, peer, room, stream, &s.peer_tx);
                    }
                    (State::Guest(guest), Incoming::MeshHello { peer, room, stream }) => {
                        Self::guest_add_neighbor(&s.events, guest, peer, room, stream, &s.peer_tx);
                    }
                    _ => {}
                }
            }

            if leave {
                match &mut state {
                    State::Host(host) => host.mesh.send_all(&Frame::Json(WireMsg::Bye)),
                    State::Guest(guest) => guest.mesh.send_all(&Frame::Json(WireMsg::Bye)),
                    State::Idle => {}
                }
                // TCP/Yggdrasil tamponunun Bye'yi taşımasına izin ver; ardından
                // yardımcı süreç kapanacak.
                std::thread::sleep(Duration::from_millis(120));
                let _ = s.events.send(Event::Closed);
                return;
            }

            match &mut state {
                State::Host(host) => {
                    if let Err(e) = pump_accepts(&host.listener, true, &s.accept_tx) {
                        eprintln!("[WATCHPARTY] accept: {e}");
                    }
                }
                State::Guest(guest) => {
                    if let Err(e) = pump_accepts(&guest.listener, false, &s.accept_tx) {
                        eprintln!("[WATCHPARTY] mesh accept: {e}");
                    }
                }
                State::Idle => {}
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
        let ygg = match YggHelper::start(local_port) {
            Ok(y) => Some(y),
            Err(e) => {
                eprintln!("[WATCHPARTY] Yggdrasil başlatılamadı, doğrudan TCP: {e}");
                None
            }
        };
        let addr = ygg
            .as_ref()
            .map(|y| y.mesh_addr())
            .unwrap_or_else(|| advertised_addr(&listener));
        let me = PeerInfo {
            id: 1,
            name,
            role: Role::Host,
            addr: addr.clone(),
        };
        let invite = Invite::new(addr, me.name.clone())?;
        let mesh = Mesh::new(me, invite.room.clone());
        Ok(HostState {
            listener,
            invite,
            _ygg: ygg,
            mesh,
            next_id: 2,
            sync: HostSync::default(),
            chat: ChatLog::default(),
            ptt: false,
            voice_seq: 0,
        })
    }

    fn start_guest(
        invite: &Invite,
        name: String,
        peer_tx: &Sender<(PeerId, Frame)>,
    ) -> Result<(GuestState, PeerInfo), String> {
        if invite.is_expired(now_ms()) {
            return Err("davet kodunun süresi dolmuş".into());
        }
        // Misafir de dinler: diğer misafirler doğrudan bağlanabilsin.
        let listener = bind_listener().map_err(|e| format!("oda açılamadı: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("oda dinleyicisi: {e}"))?;
        let local_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let ygg = match YggHelper::start(local_port) {
            Ok(y) => Some(y),
            Err(e) => {
                eprintln!("[WATCHPARTY] Yggdrasil başlatılamadı, doğrudan TCP: {e}");
                None
            }
        };
        let mesh_addr = ygg
            .as_ref()
            .map(|y| y.mesh_addr())
            .unwrap_or_else(|| advertised_addr(&listener));
        let placeholder = PeerInfo {
            id: 0,
            name,
            role: Role::Guest,
            addr: mesh_addr,
        };

        let mut stream = if invite.host_addr.starts_with('[') {
            let y = ygg
                .as_ref()
                .ok_or_else(|| "Yggdrasil yardımcısı gerekli".to_string())?;
            y.connect(&invite.host_addr)
                .map_err(|e| format!("odaya bağlanılamadı: {e}"))?
        } else {
            connect(&invite.host_addr).map_err(|e| format!("odaya bağlanılamadı: {e}"))?
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        let hello = WireMsg::Hello {
            peer: placeholder.clone(),
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
            you,
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
        PeerLink::reader(host.id, reader_stream, peer_tx.clone());

        let mut mesh = Mesh::new(you.clone(), invite.room.clone());
        mesh.add(host.clone(), link);
        let (mesh_tx, mesh_rx) = mpsc::channel();
        let mut guest = GuestState {
            listener,
            _ygg: ygg,
            mesh,
            mesh_rx,
            host_id: host.id,
            peers: vec![host.clone()],
            ping_seq: 0,
            ping_sent: None,
            last_ping_ms: 0,
            sync: GuestSync::default(),
            chat: ChatLog::default(),
            ptt: false,
            voice_seq: 0,
        };
        for peer in peers {
            add_presence(&mut guest, peer);
        }
        if let Some(state) = state {
            guest.sync.on_state(state);
        }

        // Yeni katılan misafir, kendinden önce gelen herkesle doğrudan
        // mesh bağlantısı kurar. Host bağlantısı zaten var; diğerleri
        // arka planda kurulur ki odaya giriş gecikmesin.
        let socks_port = guest._ygg.as_ref().map(|y| y.socks_port());
        let targets: Vec<PeerInfo> = guest
            .peers
            .iter()
            .filter(|p| p.id != host.id && p.id != guest.mesh.me.id)
            .cloned()
            .collect();
        for target in targets {
            let tx = mesh_tx.clone();
            let me = guest.mesh.me.clone();
            let room = guest.mesh.room.clone();
            let peer_tx = peer_tx.clone();
            let spawned = std::thread::Builder::new()
                .name("watch-party-mesh".into())
                .spawn(
                    move || match dial_mesh(&me, &room, &target, socks_port, &peer_tx) {
                        Ok(result) => {
                            let _ = tx.send(result);
                        }
                        Err(e) => eprintln!(
                            "[WATCHPARTY] {} ile doğrudan bağlantı kurulamadı: {e}",
                            target.name
                        ),
                    },
                );
            if let Err(e) = spawned {
                eprintln!("[WATCHPARTY] mesh iş parçacığı: {e}");
            }
        }
        drop(mesh_tx);
        Ok((guest, host))
    }

    fn host_add_guest(
        events: &Sender<Event>,
        host: &mut HostState,
        peer: PeerInfo,
        room: String,
        mut stream: TcpStream,
        peer_tx: &Sender<(PeerId, Frame)>,
    ) {
        if room != host.mesh.room || host.mesh.links.len() + 1 >= MAX_PEERS {
            eprintln!(
                "[WATCHPARTY] oda reddedildi (oda={} kişi={})",
                room == host.mesh.room,
                host.mesh.links.len() + 1
            );
            let _ = super::protocol::write_frame(&mut stream, &Frame::Json(WireMsg::Bye));
            return;
        }
        // Aynı dinleme adresinden yeniden bağlanan istemci (ör. kopan
        // bağlantıyı yenileyen) eski bağlantıyı değiştirir.
        if !peer.addr.is_empty() {
            if let Some(old) = host
                .mesh
                .links
                .iter()
                .find(|p| p.info.addr == peer.addr)
                .map(|p| p.info.id)
            {
                host.mesh.remove(old);
                host.mesh
                    .send_all(&Frame::Json(WireMsg::PeerLeft { id: old }));
            }
        }
        let id = host.next_id;
        host.next_id = host.next_id.saturating_add(1);
        let info = PeerInfo {
            id,
            name: peer.name,
            role: Role::Guest,
            addr: peer.addr,
        };
        let welcome = WireMsg::Welcome {
            room: host.mesh.room.clone(),
            host: host.mesh.me.clone(),
            peers: host.mesh.peers(),
            you: info.clone(),
            state: host.sync.state().cloned(),
        };
        if let Err(e) = super::protocol::write_frame(&mut stream, &Frame::Json(welcome)) {
            eprintln!("[WATCHPARTY] welcome yazılamadı: {e}");
            return;
        }
        let history: Vec<ChatMessage> = host
            .chat
            .messages()
            .iter()
            .rev()
            .take(50)
            .rev()
            .cloned()
            .collect();
        if !history.is_empty() {
            let _ = super::protocol::write_frame(
                &mut stream,
                &Frame::Json(WireMsg::ChatHistory(history)),
            );
        }
        let Ok(reader_stream) = stream.try_clone() else {
            return;
        };
        let Ok(link) = PeerLink::new(stream) else {
            return;
        };
        PeerLink::reader(id, reader_stream, peer_tx.clone());
        host.mesh.add(info.clone(), link);
        host.mesh
            .send_except(id, &Frame::Json(WireMsg::PeerJoined(info)));
        let _ = events.send(Event::Peers(host.mesh.peers()));
    }

    fn guest_add_neighbor(
        events: &Sender<Event>,
        guest: &mut GuestState,
        peer: PeerInfo,
        room: String,
        mut stream: TcpStream,
        peer_tx: &Sender<(PeerId, Frame)>,
    ) {
        if room != guest.mesh.room
            || peer.id == guest.mesh.me.id
            || peer.id == guest.host_id
            || guest.mesh.has(peer.id)
            || guest.mesh.links.len() + 1 >= MAX_PEERS
        {
            let _ = super::protocol::write_frame(&mut stream, &Frame::Json(WireMsg::Bye));
            return;
        }
        let welcome = WireMsg::MeshWelcome {
            room: guest.mesh.room.clone(),
            peers: guest.peers.clone(),
        };
        if let Err(e) = super::protocol::write_frame(&mut stream, &Frame::Json(welcome)) {
            eprintln!("[WATCHPARTY] welcome yazılamadı: {e}");
            return;
        }
        let Ok(reader_stream) = stream.try_clone() else {
            return;
        };
        let Ok(link) = PeerLink::new(stream) else {
            return;
        };
        PeerLink::reader(peer.id, reader_stream, peer_tx.clone());
        guest.mesh.add(peer.clone(), link);
        let added = add_presence(guest, peer.clone());
        guest
            .mesh
            .send_except(peer.id, &Frame::Json(WireMsg::PeerJoined(peer)));
        if added {
            let _ = events.send(Event::Peers(guest.peers.clone()));
        }
    }

    fn host_on_frame(events: &Sender<Event>, host: &mut HostState, peer_id: PeerId, frame: Frame) {
        let Some(sender) = host
            .mesh
            .links
            .iter()
            .find(|p| p.info.id == peer_id)
            .map(|p| p.info.clone())
        else {
            return;
        };
        match frame {
            Frame::Json(WireMsg::Chat { id, name, text, ts }) => {
                let author = if id == 0 { peer_id } else { id };
                if !host.mesh.seen_chat.fresh((author, ts, text_hash(&text))) {
                    return;
                }
                let msg = ChatMessage {
                    id: author,
                    name: if name.is_empty() { sender.name } else { name },
                    text,
                    ts,
                };
                host.chat.push(msg.clone());
                host.mesh.send_except(peer_id, &chat_frame(&msg));
                let _ = events.send(Event::Chat(msg));
            }
            Frame::Voice(v) => {
                if !host.mesh.seen_voice.fresh((v.id, v.seq)) {
                    return;
                }
                host.mesh.send_except(peer_id, &Frame::Voice(v.clone()));
                let _ = events.send(Event::Voice(v));
            }
            Frame::Json(WireMsg::Ping { nonce, .. }) => {
                host.mesh.send_to(
                    peer_id,
                    &Frame::Json(WireMsg::Pong {
                        nonce,
                        host_ms: now_ms(),
                    }),
                );
            }
            Frame::Json(WireMsg::Bye) => {
                if host.mesh.remove(peer_id).is_some() {
                    host.mesh
                        .send_all(&Frame::Json(WireMsg::PeerLeft { id: peer_id }));
                    let _ = events.send(Event::Peers(host.mesh.peers()));
                }
            }
            _ => {}
        }
    }

    fn guest_on_frame(
        events: &Sender<Event>,
        guest: &mut GuestState,
        peer_id: PeerId,
        frame: Frame,
    ) {
        match frame {
            Frame::Json(WireMsg::Content(content)) => {
                let _ = events.send(Event::Content(content));
            }
            Frame::Json(WireMsg::ChatHistory(history)) => {
                for msg in history {
                    guest.chat.push(msg.clone());
                    let _ = events.send(Event::Chat(msg));
                }
            }
            Frame::Json(WireMsg::Playback(state)) => {
                if !guest
                    .mesh
                    .seen_playback
                    .fresh((state.generation, state.host_ms))
                {
                    return;
                }
                guest.sync.on_state(state.clone());
                let _ = events.send(Event::Playback(state));
            }
            Frame::Json(WireMsg::Chat { id, name, text, ts }) => {
                let author = if id == 0 { peer_id } else { id };
                if !guest.mesh.seen_chat.fresh((author, ts, text_hash(&text))) {
                    return;
                }
                let fallback = guest
                    .mesh
                    .links
                    .iter()
                    .find(|p| p.info.id == peer_id)
                    .map(|p| p.info.name.clone())
                    .unwrap_or_default();
                let msg = ChatMessage {
                    id: author,
                    name: if name.is_empty() { fallback } else { name },
                    text,
                    ts,
                };
                guest.chat.push(msg.clone());
                let _ = events.send(Event::Chat(msg));
            }
            Frame::Voice(v) => {
                if !guest.mesh.seen_voice.fresh((v.id, v.seq)) {
                    return;
                }
                let _ = events.send(Event::Voice(v));
            }
            Frame::Json(WireMsg::PeerJoined(info)) => {
                if add_presence(guest, info) {
                    let _ = events.send(Event::Peers(guest.peers.clone()));
                }
            }
            Frame::Json(WireMsg::PeerLeft { id }) => {
                guest.mesh.remove(id);
                if remove_presence(guest, id) {
                    let _ = events.send(Event::Peers(guest.peers.clone()));
                }
            }
            Frame::Json(WireMsg::Ping { nonce, .. }) => {
                guest.mesh.send_to(
                    peer_id,
                    &Frame::Json(WireMsg::Pong {
                        nonce,
                        host_ms: now_ms(),
                    }),
                );
            }
            Frame::Json(WireMsg::Pong { nonce, host_ms }) => {
                if let Some((seq, sent)) = guest.ping_sent {
                    if seq == nonce {
                        let now = now_ms();
                        let offset = host_ms as i64 - ((sent + now) / 2) as i64;
                        guest.sync.set_offset_ms(offset);
                        guest.ping_sent = None;
                    }
                }
            }
            Frame::Json(WireMsg::Bye) => {
                if peer_id == guest.host_id {
                    let _ = events.send(Event::Closed);
                } else {
                    // Doğrudan komşu düştü; oda üyeliği host'un PeerLeft'iyle belirlenir.
                    guest.mesh.remove(peer_id);
                }
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

/// Dinleyicideki hazır bağlantıları toplar; ilk çerçeveyi ayrı iş
/// parçacığında okur ki oturum döngüsü bloklanmasın.
fn pump_accepts(listener: &TcpListener, host: bool, tx: &Sender<Incoming>) -> io::Result<()> {
    loop {
        let (stream, _) = match listener.accept() {
            Ok(v) => v,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) => return Err(e),
        };
        let tx = tx.clone();
        std::thread::Builder::new()
            .name("watch-party-hello".into())
            .spawn(move || {
                let mut stream = stream;
                if stream
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .is_err()
                {
                    return;
                }
                let frame = match super::protocol::read_frame(&mut stream) {
                    Ok(Some(frame)) => frame,
                    Ok(None) => return,
                    Err(_) => return,
                };
                let Frame::Json(msg) = frame else {
                    return;
                };
                if stream.set_read_timeout(None).is_err() {
                    return;
                }
                let incoming = match (host, msg) {
                    (true, WireMsg::Hello { peer, room }) => {
                        Incoming::HostHello { peer, room, stream }
                    }
                    (false, WireMsg::MeshHello { peer, room }) => {
                        Incoming::MeshHello { peer, room, stream }
                    }
                    _ => return,
                };
                let _ = tx.send(incoming);
            })
            .ok();
    }
}

fn text_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn chat_message(id: PeerId, name: String, text: String) -> Option<ChatMessage> {
    let text: String = text.trim().chars().take(500).collect();
    if text.is_empty() {
        return None;
    }
    Some(ChatMessage {
        id,
        name,
        text,
        ts: now_ms(),
    })
}

fn chat_frame(msg: &ChatMessage) -> Frame {
    Frame::Json(WireMsg::Chat {
        id: msg.id,
        name: msg.name.clone(),
        text: msg.text.clone(),
        ts: msg.ts,
    })
}

fn add_presence(guest: &mut GuestState, info: PeerInfo) -> bool {
    if info.id == 0 || info.id == guest.mesh.me.id {
        return false;
    }
    if guest.peers.iter().any(|p| p.id == info.id) {
        return false;
    }
    guest.peers.push(info);
    true
}

fn remove_presence(guest: &mut GuestState, id: PeerId) -> bool {
    let before = guest.peers.len();
    guest.peers.retain(|p| p.id != id);
    guest.peers.len() != before
}

fn dial_mesh(
    me: &PeerInfo,
    room: &str,
    target: &PeerInfo,
    socks_port: Option<u16>,
    peer_tx: &Sender<(PeerId, Frame)>,
) -> Result<(MeshPeer, Vec<PeerInfo>), String> {
    if target.addr.is_empty() {
        return Err("adres yok".into());
    }
    let mut stream = if target.addr.starts_with('[') {
        let port = socks_port.ok_or_else(|| "Yggdrasil yardımcısı gerekli".to_string())?;
        connect_with_retry(port, &target.addr).map_err(|e| format!("bağlanılamadı: {e}"))?
    } else {
        connect(&target.addr).map_err(|e| format!("bağlanılamadı: {e}"))?
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let hello = WireMsg::MeshHello {
        peer: me.clone(),
        room: room.to_string(),
    };
    super::protocol::write_frame(&mut stream, &Frame::Json(hello))
        .map_err(|e| format!("mesh merhaba: {e}"))?;
    let welcome = super::protocol::read_frame(&mut stream)
        .map_err(|e| format!("mesh yanıtı: {e}"))?
        .ok_or_else(|| "mesh bağlantısı kapandı".to_string())?;
    stream.set_read_timeout(None).map_err(|e| e.to_string())?;
    let Frame::Json(WireMsg::MeshWelcome { room: got, peers }) = welcome else {
        return Err("mesh yanıtı geçersiz".into());
    };
    if got != room {
        return Err("mesh oda kimliği uyuşmuyor".into());
    }
    let reader_stream = stream.try_clone().map_err(|e| e.to_string())?;
    let link = PeerLink::new(stream).map_err(|e| e.to_string())?;
    PeerLink::reader(target.id, reader_stream, peer_tx.clone());
    Ok((
        MeshPeer {
            info: target.clone(),
            link,
        },
        peers,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait<F>(handle: &SessionHandle, mut pred: F, label: &str) -> Event
    where
        F: FnMut(&Event) -> bool,
    {
        for _ in 0..12000 {
            while let Some(event) = handle.try_recv() {
                if pred(&event) {
                    return event;
                }
                if let Event::Error(e) = &event {
                    panic!("hata: {e}");
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("olay gelmedi: {label}");
    }

    fn wait_peers_at_most(handle: &SessionHandle, max: usize) -> Vec<PeerInfo> {
        let Event::Peers(peers) = wait(
            handle,
            |e| matches!(e, Event::Peers(p) if p.len() <= max),
            &format!("en fazla {max} kişi"),
        ) else {
            unreachable!()
        };
        peers
    }

    fn wait_mesh_links(handle: &SessionHandle, count: usize) {
        for _ in 0..300 {
            if handle.mesh_links() == count {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("mesh bağlantısı kurulmadı: {count}");
    }

    fn wait_peers(handle: &SessionHandle, count: usize) -> Vec<PeerInfo> {
        let Event::Peers(peers) = wait(
            handle,
            |e| matches!(e, Event::Peers(p) if p.len() == count),
            &format!("{count} kişi"),
        ) else {
            unreachable!()
        };
        peers
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
        // Misafirin listesi host dahil diğer üyeleri taşır.
        assert_eq!(peers.len(), 1);

        let peers = wait_peers(&host, 1);
        assert_eq!(peers[0].name, "guest");
        assert_eq!(host.mesh_links(), 1);
        assert_eq!(guest.mesh_links(), 1);
    }

    #[test]
    fn new_guest_receives_chat_history() {
        let host = SessionHandle::start();
        host.create("host");
        let Event::Invite(invite) = wait(&host, |e| matches!(e, Event::Invite(_)), "invite") else {
            unreachable!()
        };
        host.chat("geçmiş mesaj");
        let _ = wait(&host, |e| matches!(e, Event::Chat(_)), "host chat");

        let guest = SessionHandle::start();
        guest.join(invite, "guest");
        let _ = wait(&guest, |e| matches!(e, Event::Joined { .. }), "joined");
        let Event::Chat(msg) = wait(&guest, |e| matches!(e, Event::Chat(_)), "history") else {
            unreachable!()
        };
        assert_eq!(msg.text, "geçmiş mesaj");
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
        let _ = wait_peers(&host, 1);

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
        assert_eq!(msg.name, "guest");
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

    #[test]
    fn four_peers_form_mesh_and_share_media() {
        let host = SessionHandle::start();
        host.create("host");
        let Event::Invite(invite) = wait(&host, |e| matches!(e, Event::Invite(_)), "invite") else {
            unreachable!()
        };
        let mut guests = Vec::new();
        for i in 0..3 {
            let guest = SessionHandle::start();
            guest.join(invite.clone(), format!("guest{i}"));
            let Event::Joined { peers, .. } =
                wait(&guest, |e| matches!(e, Event::Joined { .. }), "joined")
            else {
                unreachable!()
            };
            assert_eq!(peers.len(), i + 1);
            guests.push(guest);
            let _ = wait_peers(&host, i + 1);
        }
        wait_mesh_links(&host, 3);
        for guest in &guests {
            wait_mesh_links(guest, 3);
        }

        guests[0].chat("mesh");
        let _ = wait(
            &host,
            |e| matches!(e, Event::Chat(m) if m.text == "mesh"),
            "host mesh chat",
        );
        for guest in &guests {
            let _ = wait(
                guest,
                |e| matches!(e, Event::Chat(m) if m.text == "mesh"),
                "guest mesh chat",
            );
        }

        guests[0].ptt(true);
        let _ = guests[0]
            .command_sender()
            .send(Command::Voice(vec![7, 8, 9]));
        let Event::Voice(v) = wait(&host, |e| matches!(e, Event::Voice(_)), "host mesh voice")
        else {
            unreachable!()
        };
        assert_eq!(v.samples, vec![7, 8, 9]);
        for guest in guests.iter().skip(1) {
            let _ = wait(guest, |e| matches!(e, Event::Voice(_)), "guest mesh voice");
        }

        host.snapshot(snap(33.0));
        for guest in &guests {
            let Event::Playback(state) =
                wait(guest, |e| matches!(e, Event::Playback(_)), "mesh playback")
            else {
                unreachable!()
            };
            assert_eq!(state.position, 33.0);
        }

        guests[2].leave();
        for handle in std::iter::once(&host).chain(guests.iter().take(2)) {
            let peers = wait_peers_at_most(handle, 2);
            assert_eq!(peers.len(), 2);
        }
    }
}
