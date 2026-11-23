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
}

/// Host'un o an ne yaptığı. Oda paneli ve misafir yönlendirmesi için.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentKind {
    #[default]
    Idle,
    Browsing,
    ChoosingTitle,
    ChoosingEpisode,
    Watching,
    Break,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomContent {
    pub kind: ContentKind,
    pub title_id: Option<u64>,
    pub title_name: String,
    pub title_type: String,
    pub season: Option<u64>,
    pub episode: Option<u64>,
    pub episode_name: String,
    pub updated_ms: u64,
}

impl RoomContent {
    pub fn browsing(page: impl Into<String>) -> Self {
        Self {
            kind: ContentKind::Browsing,
            title_name: page.into(),
            updated_ms: crate::watch_party::invite::now_ms(),
            ..Default::default()
        }
    }

    pub fn choosing_title(
        title_id: u64,
        title_name: impl Into<String>,
        title_type: impl Into<String>,
    ) -> Self {
        Self {
            kind: ContentKind::ChoosingTitle,
            title_id: Some(title_id),
            title_name: title_name.into(),
            title_type: title_type.into(),
            updated_ms: crate::watch_party::invite::now_ms(),
            ..Default::default()
        }
    }

    pub fn choosing_episode(
        title_id: u64,
        title_name: impl Into<String>,
        title_type: impl Into<String>,
    ) -> Self {
        Self {
            kind: ContentKind::ChoosingEpisode,
            title_id: Some(title_id),
            title_name: title_name.into(),
            title_type: title_type.into(),
            updated_ms: crate::watch_party::invite::now_ms(),
            ..Default::default()
        }
    }

    pub fn watching(
        title_id: u64,
        title_name: impl Into<String>,
        title_type: impl Into<String>,
        season: u64,
        episode: u64,
        episode_name: impl Into<String>,
    ) -> Self {
        Self {
            kind: ContentKind::Watching,
            title_id: Some(title_id),
            title_name: title_name.into(),
            title_type: title_type.into(),
            season: Some(season),
            episode: Some(episode),
            episode_name: episode_name.into(),
            updated_ms: crate::watch_party::invite::now_ms(),
        }
    }

    pub fn label(&self) -> String {
        match self.kind {
            ContentKind::Idle => String::new(),
            ContentKind::Browsing => {
                if self.title_name.is_empty() {
                    "Geziniyor".into()
                } else {
                    format!("{} sayfasında", self.title_name)
                }
            }
            ContentKind::ChoosingTitle => {
                if self.title_name.is_empty() {
                    "Başlık seçiyor".into()
                } else {
                    format!("{} seçiyor", self.title_name)
                }
            }
            ContentKind::ChoosingEpisode => {
                if self.title_name.is_empty() {
                    "Bölüm seçiyor".into()
                } else {
                    format!("{} · bölüm seçiyor", self.title_name)
                }
            }
            ContentKind::Watching => {
                let mut out = self.title_name.clone();
                if let (Some(s), Some(e)) = (self.season, self.episode) {
                    out.push_str(&format!(" · S{s:02}B{e:02}"));
                }
                if !self.episode_name.is_empty() {
                    out.push_str(" · ");
                    out.push_str(&self.episode_name);
                }
                out
            }
            ContentKind::Break => "Ara verdi".into(),
        }
    }
}
