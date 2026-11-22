use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use super::types::{PeerId, PeerInfo, PlaybackState};

pub const MAX_FRAME: usize = 64 * 1024;
const KIND_JSON: u8 = b'J';
const KIND_VOICE: u8 = b'V';

/// Kontrol/sohbet mesajları. Ses ayrı `VoiceFrame` olarak taşınır.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WireMsg {
    Hello {
        peer: PeerInfo,
        room: String,
    },
    Welcome {
        room: String,
        host: PeerInfo,
        peers: Vec<PeerInfo>,
        state: Option<PlaybackState>,
    },
    PeerJoined(PeerInfo),
    PeerLeft {
        id: PeerId,
    },
    Playback(PlaybackState),
    Ready {
        id: PeerId,
        ready: bool,
        buffering: bool,
        position: f64,
    },
    Chat {
        id: PeerId,
        name: String,
        text: String,
        ts: u64,
    },
    Ping {
        nonce: u64,
        host_ms: u64,
    },
    Pong {
        nonce: u64,
        host_ms: u64,
    },
    Bye,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceFrame {
    pub id: PeerId,
    pub seq: u32,
    /// 16 kHz mono, i16 LE örnekler.
    pub samples: Vec<i16>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Json(WireMsg),
    Voice(VoiceFrame),
}

pub fn write_frame<W: Write>(out: &mut W, frame: &Frame) -> io::Result<()> {
    let (kind, payload) = match frame {
        Frame::Json(msg) => (
            KIND_JSON,
            serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        ),
        Frame::Voice(v) => {
            let mut payload = Vec::with_capacity(12 + v.samples.len() * 2);
            payload.extend_from_slice(&v.id.to_be_bytes());
            payload.extend_from_slice(&v.seq.to_be_bytes());
            for s in &v.samples {
                payload.extend_from_slice(&s.to_le_bytes());
            }
            (KIND_VOICE, payload)
        }
    };
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "watch-party frame too large",
        ));
    }
    out.write_all(&[kind])?;
    out.write_all(&(payload.len() as u32).to_be_bytes())?;
    out.write_all(&payload)?;
    out.flush()
}

/// `Ok(None)` = karşı taraf bağlantıyı kapattı.
pub fn read_frame<R: Read>(input: &mut R) -> io::Result<Option<Frame>> {
    let mut head = [0u8; 5];
    match input.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "watch-party frame too large",
        ));
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload)?;
    match head[0] {
        KIND_JSON => {
            let msg = serde_json::from_slice(&payload)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Some(Frame::Json(msg)))
        }
        KIND_VOICE => {
            if payload.len() < 12 || !(payload.len() - 12).is_multiple_of(2) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "bozuk ses çerçevesi",
                ));
            }
            let id = u64::from_be_bytes(payload[0..8].try_into().unwrap());
            let seq = u32::from_be_bytes(payload[8..12].try_into().unwrap());
            let samples = payload[12..]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            Ok(Some(Frame::Voice(VoiceFrame { id, seq, samples })))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bilinmeyen çerçeve türü",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch_party::types::Role;

    fn peer(id: u64) -> PeerInfo {
        PeerInfo {
            id,
            name: "test".into(),
            role: Role::Guest,
            addr: "127.0.0.1:1".into(),
        }
    }

    #[test]
    fn json_frame_roundtrip() {
        let frame = Frame::Json(WireMsg::Hello {
            peer: peer(7),
            room: "abc".into(),
        });
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).unwrap();
        let mut cur = std::io::Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), Some(frame));
    }

    #[test]
    fn voice_frame_roundtrip() {
        let frame = Frame::Voice(VoiceFrame {
            id: 7,
            seq: 42,
            samples: vec![-1, 0, 1, 2],
        });
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).unwrap();
        let mut cur = std::io::Cursor::new(buf);
        assert_eq!(read_frame(&mut cur).unwrap(), Some(frame));
    }

    #[test]
    fn truncated_frame_is_none() {
        let mut cur = std::io::Cursor::new(Vec::new());
        assert_eq!(read_frame(&mut cur).unwrap(), None);
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut buf = vec![KIND_JSON];
        buf.extend_from_slice(&((MAX_FRAME + 1) as u32).to_be_bytes());
        let mut cur = std::io::Cursor::new(buf);
        assert!(read_frame(&mut cur).is_err());
    }
}
