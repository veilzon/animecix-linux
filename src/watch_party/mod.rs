//! Birlikte izleme çekirdeği.
//!
//! Bu modül GTK ve mpv'den bağımsızdır. Ağ, ses ve oynatıcı adaptörleri
//! ayrı modüllerde yaşar; burada yalnızca durum, protokol ve senkron
//! kararları bulunur.

pub mod audio;
pub mod chat;
pub mod invite;
pub mod protocol;
pub mod session;
pub mod sync;
pub mod transport;
pub mod types;

pub use invite::Invite;
pub use session::{Event, SessionHandle};
pub use types::{PlaybackSnapshot, SyncAction};
