use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

pub const INVITE_VERSION: u8 = 1;
pub const INVITE_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub version: u8,
    pub room: String,
    pub host_addr: String,
    pub host_name: String,
    pub room_key: [u8; 32],
    pub created_ms: u64,
    pub expires_ms: u64,
}

impl Invite {
    pub fn new(host_addr: impl Into<String>, host_name: impl Into<String>) -> Result<Self, String> {
        let now = now_ms();
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| "oda anahtarı üretilemedi".to_string())?;
        let mut room_bytes = [0u8; 8];
        SystemRandom::new()
            .fill(&mut room_bytes)
            .map_err(|_| "oda kimliği üretilemedi".to_string())?;
        Ok(Self {
            version: INVITE_VERSION,
            room: room_bytes.iter().map(|b| format!("{b:02x}")).collect(),
            host_addr: host_addr.into(),
            host_name: host_name.into(),
            room_key: key,
            created_ms: now,
            expires_ms: now.saturating_add(INVITE_TTL_MS),
        })
    }

    pub fn encode(&self) -> String {
        let body = serde_json::to_vec(self).unwrap_or_default();
        URL_SAFE_NO_PAD.encode(body)
    }

    pub fn decode(input: &str) -> Result<Self, String> {
        let raw = URL_SAFE_NO_PAD
            .decode(input.trim())
            .map_err(|_| "davet kodu okunamadı".to_string())?;
        let invite: Self =
            serde_json::from_slice(&raw).map_err(|_| "davet kodu geçersiz".to_string())?;
        if invite.version != INVITE_VERSION {
            return Err(format!("desteklenmeyen davet sürümü: {}", invite.version));
        }
        if invite.room.is_empty() || invite.host_addr.is_empty() {
            return Err("davet kodu eksik".to_string());
        }
        Ok(invite)
    }

    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_ms
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_roundtrip() {
        let invite = Invite::new("100.64.0.1:47820", "host").unwrap();
        let encoded = invite.encode();
        let back = Invite::decode(&encoded).unwrap();
        assert_eq!(back, invite);
        assert_eq!(back.room.len(), 16);
        assert!(back.room_key.iter().any(|b| *b != 0));
    }

    #[test]
    fn invite_expires() {
        let invite = Invite::new("127.0.0.1:1", "host").unwrap();
        assert!(!invite.is_expired(invite.created_ms));
        assert!(invite.is_expired(invite.expires_ms));
    }

    #[test]
    fn bad_invite_is_rejected() {
        assert!(Invite::decode("bozuk").is_err());
    }
}
