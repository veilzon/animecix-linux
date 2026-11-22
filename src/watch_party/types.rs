use serde::{Deserialize, Serialize};

/// Bir odadaki eş sayısı. Yggdrasil mesh için üst sınır.
pub const MAX_PEERS: usize = 4;

pub type PeerId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Host,
    Guest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: PeerId,
    pub name: String,
    pub role: Role,
    /// Peer'ın dinlediği mesh adresi (`ip:port`).
    pub addr: String,
}

/// Oynatıcıdan alınan anlık durum. mpv'nin `time-pos`, `pause`,
/// `paused-for-cache` ve `cache-time` değerlerinden üretilir.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackSnapshot {
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub buffering: bool,
    pub speed: f64,
    /// Bölüm kimliği: `<title_id>:<season>:<episode>`.
    pub episode: String,
}

/// Host'un yayınladığı kanonik oynatma durumu.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackState {
    /// Host'un her seek/pause/play kararında artırdığı nesil.
    pub generation: u64,
    pub position: f64,
    pub paused: bool,
    pub rate: f64,
    /// Host saatinde milisaniye (epoch).
    pub host_ms: u64,
    pub episode: String,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            generation: 0,
            position: 0.0,
            paused: true,
            rate: 1.0,
            host_ms: 0,
            episode: String::new(),
        }
    }
}

/// Guest tarafında mpv'ye uygulanacak karar.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncAction {
    None,
    Pause,
    Play,
    Seek(f64),
    /// Kısa süreli hız düzeltmesi; adaptör birkaç saniye sonra 1.0'a döndürür.
    SetSpeed(f64),
}
