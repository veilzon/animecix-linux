//! Hesap girişi (animecix.tv cookie oturumu).
//!
//! Akış (sitedeki Angular istemcisiyle birebir):
//! 1. `GET {BASE}/` → `XSRF-TOKEN` cookie'si alınır.
//! 2. `POST {BASE}/secure/auth/login` + `Cookie` + `X-XSRF-TOKEN`
//!    header'ları, JSON `{email, password}`.
//! 3. Başarılıysa session cookie'leri saklanır (`session.json`).
//!
//! Bearer token YOKTUR.
//! (Lowell137/animecix-linux `auth.rs` MIT portu; manga + avatar
//! yükleme hariç — onlar Faz 4'te.)

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::api::{Client, BASE};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
    pub avatar: Option<String>,
    pub background: Option<String>,
    pub about: Option<String>,
}

#[allow(dead_code)] // gecici-yerel saplama: giriş yok, tip korunur.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub user: Option<User>,
}

impl Session {
    pub fn load() -> Self {
        Self::default()
    }
}

fn parse_user(v: &serde_json::Value) -> Option<User> {
    // Başarılı yanıtta kullanıcı `user` altındadır (login yanıtı),
    // bazen `data` altında gelir (bootstrapper).
    let d = if v["user"].is_object() {
        &v["user"]
    } else if v["data"].is_object() {
        &v["data"]
    } else {
        v
    };
    let name = d["username"]
        .as_str()
        .or_else(|| d["name"].as_str())
        .or_else(|| d["display_name"].as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() && d["id"].as_u64().is_none() {
        return None;
    }
    Some(User {
        id: d["id"].as_u64().unwrap_or(0),
        name,
        email: d["email"].as_str().unwrap_or("").to_string(),
        avatar: d["avatar"]
            .as_str()
            .or_else(|| d["avatar_url"].as_str())
            .map(|s| s.to_string()),
        background: d["background"].as_str().map(|s| s.to_string()),
        about: d["about"].as_str().map(|s| s.to_string()),
    })
}

fn resp_ok(v: &serde_json::Value) -> bool {
    if v["success"] == serde_json::Value::Bool(true) {
        return true;
    }
    // success alanı yoksa: user objesi varsa + hata yoksa başarılı say.
    (v["user"].is_object() || v["data"].is_object() || v["id"].as_u64().is_some())
        && v["error"].is_null()
        && v["message"].as_str().is_none_or(|m| {
            let m = m.to_lowercase();
            !m.contains("başarısız") && !m.contains("fail") && !m.contains("hata")
        })
}

/// `put-title` gövdesi (saf; tarih enjekte edilir, test edilebilir).
pub fn put_title_body(
    title_id: u64,
    name: &str,
    date_ms: u64,
    first_video: Option<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "id": title_id,
        "name": name,
        "date": date_ms,
        "videos": first_video.map(|v| vec![v]).unwrap_or_default(),
        "episodes": [],
    })
}

/// Favori birleştirme çekirdeği (saf): sunucuda olup yerelde yoksa başa
/// ekler. Dönen: eklenen sayısı. Asla silmez.
pub fn merge_titles(local: &mut Vec<crate::api::Title>, remote: &[crate::api::Title]) -> usize {
    let mut added = 0;
    for t in remote {
        if !local.iter().any(|x| x.id == t.id) {
            local.insert(0, t.clone());
            added += 1;
        }
    }
    added
}

/// ms → sn eşiği: 5sn üstü konum geçerli sayılır.
pub fn remote_ms_to_secs(ms: f64) -> Option<f64> {
    if ms > 5000.0 {
        Some(ms / 1000.0)
    } else {
        None
    }
}

/// Konum bildirim eşiği: 10sn altı gönderilmez.
pub fn should_report_pos(pos_secs: f64) -> bool {
    pos_secs >= 10.0
}

/// Periyodik bildirim kısma (saf): son bildirimden 60sn geçtiyse gönder.
/// İlk bildirim için `last_secs = 0` verilir. (Lowell birebir: 60sn.)
pub fn should_report_now(last_secs: u64, now_secs: u64) -> bool {
    now_secs.saturating_sub(last_secs) >= 60
}

/// Konum bildirim gövdesi (saf): `time` milisaniye tamsayıdır.
pub fn report_pos_body(
    title_id: u64,
    season: u64,
    episode: u64,
    pos_secs: f64,
) -> serde_json::Value {
    serde_json::json!({
        "titleId": title_id,
        "seasonNumber": season,
        "episodeNumber": episode,
        "time": (pos_secs * 1000.0) as u64,
    })
}

/// Devam eşiği: 5sn üstü + %95 altı yerden devam edilir.
pub fn should_resume(pos: f64, dur: f64) -> bool {
    pos > 5.0 && dur > 0.0 && pos / dur < 0.95
}

/// Tek dosyalık multipart gövde (saf): (bayt, content-type).
/// Sınır sabittir → test edilebilir; tek seferlik yüklemede çakışmaz.
pub fn multipart_body(
    boundary: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> (Vec<u8>, String) {
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (body, format!("multipart/form-data; boundary={boundary}"))
}

/// Açılış itişi (saf): sunucuda kaydı olmayan yerel girdiler.
pub fn missing_on_server(
    local: &[crate::api::HistoryEntry],
    remote_ids: &std::collections::HashSet<u64>,
) -> Vec<crate::api::HistoryEntry> {
    local
        .iter()
        .filter(|h| !remote_ids.contains(&h.title.id))
        .cloned()
        .collect()
}

/// Yerel + sunucu geçmiş birleşimi (saf): id-dedup, en yeni önce.
/// Sunucu `date` ms, yerel `ts` sn APS cinsindendir.
pub fn merged_history(
    local: &[crate::api::HistoryEntry],
    remote: &[crate::api::ServerEntry],
) -> Vec<crate::api::HistoryEntry> {
    let mut rows: Vec<(u64, u64, crate::api::HistoryEntry)> = Vec::new();
    for h in local {
        rows.push((h.title.id, h.ts.saturating_mul(1000), h.clone()));
    }
    for e in remote {
        let ep = crate::api::Episode {
            season: e.season,
            episode: e.episode,
            name: String::new(),
        };
        rows.push((
            e.title.id,
            e.date,
            crate::api::HistoryEntry {
                title: e.title.clone(),
                episode: ep,
                ts: e.date / 1000,
            },
        ));
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter(|r| seen.insert(r.0))
        .map(|(_, _, h)| h)
        .collect()
}

/// Yıldız değişimini sunucuya yazar (sessiz; girişsizde atlanır).
/// Çağıran: detay yıldızları + favori silme (toggle sonrası).
/// `Arc` ile çağrılır (ağ işçiye taşınır, UI bloklanmaz).
/// Yıldız değişimi (gecici-yerel): sunucuya yazılmaz (yerel favori durur).
pub fn sync_fav_remote(_client: &std::sync::Arc<crate::api::Client>, _title_id: u64, _saved: bool) {
}

impl Client {
    /// Kayıtlı oturum (yerel modda daima yok).
    pub fn session_user(&self) -> Option<User> {
        None
    }

    pub fn is_logged_in(&self) -> bool {
        false
    }

    pub fn auth_expired(&self) -> bool {
        false
    }

    /// Yerel modda yazma yok.
    pub fn put_title(&self, _title: &crate::api::Title, _ep: &crate::api::Episode) -> bool {
        false
    }

    pub fn merge_server_favs(&self) -> usize {
        0
    }

    pub fn server_top(
        &self,
        _max_pages: u32,
    ) -> Result<(Vec<crate::api::ServerEntry>, usize), String> {
        Ok((Vec::new(), 0))
    }

    /// Devam konumu: yalnız yerel (uzak sorulmaz).
    pub fn start_pos(&self, tid: u64, season: u64, episode: u64) -> Option<f64> {
        self.get_progress(tid, season, episode)
            .filter(|(pos, dur)| crate::auth::should_resume(*pos, *dur))
            .map(|(pos, _)| pos)
    }

    /// Geçmiş yanıtını ayrıştırır (saf; tarih-azalan garantili).
    fn parse_history_response(
        v: &serde_json::Value,
    ) -> Result<(Vec<crate::api::ServerEntry>, usize), String> {
        if v["success"] == serde_json::Value::Bool(false) {
            return Err(v["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .unwrap_or("Sunucu geçmişi vermedi")
                .to_string());
        }
        let total = v["data"]["totalCount"][0]["count"].as_u64().unwrap_or(0) as usize;
        let items = match v["data"]["totalData"].as_array() {
            Some(a) => a.clone(),
            None => {
                return Err(v["message"]
                    .as_str()
                    .unwrap_or("Geçmiş yanıtı anlaşılmadı")
                    .to_string())
            }
        };
        let mut entries: Vec<crate::api::ServerEntry> = items
            .iter()
            .filter_map(|r| {
                let title = crate::api::Title::from_value(r)?;
                let date = r["date"].as_u64().unwrap_or(0);
                let v0 = r["videos"].as_array().and_then(|a| a.first());
                let season = v0.and_then(|v| v["season_num"].as_u64()).unwrap_or(0);
                let episode = v0.and_then(|v| v["episode_num"].as_u64()).unwrap_or(0);
                Some(crate::api::ServerEntry {
                    title,
                    date,
                    season,
                    episode,
                })
            })
            .collect();
        // API zaten tarih-azalan dönüyor; garantiye al.
        entries.sort_by(|a, b| b.date.cmp(&a.date));
        Ok((entries, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_login_shape() {
        let v = serde_json::json!({
            "success": true,
            "user": {"id": 7, "username": "veil", "email": "v@x.tv", "avatar": "https://cdn/a.png"}
        });
        let u = parse_user(&v).expect("login user");
        assert_eq!(u.id, 7);
        assert_eq!(u.name, "veil");
        assert_eq!(u.email, "v@x.tv");
        assert_eq!(u.avatar.as_deref(), Some("https://cdn/a.png"));
    }

    #[test]
    fn parse_user_bootstrapper_shape() {
        let v = serde_json::json!({
            "data": {"id": 9, "display_name": "Anon", "avatar_url": "https://cdn/b.png"}
        });
        let u = parse_user(&v).expect("data user");
        assert_eq!(u.name, "Anon");
        assert!(resp_ok(&v), "user yok ama data var + hata yok");
    }

    #[test]
    fn parse_user_rejects_empty() {
        assert!(parse_user(&serde_json::json!({})).is_none());
        assert!(parse_user(&serde_json::json!({"user": {}})).is_none());
    }

    #[test]
    fn merge_titles_adds_only_missing() {
        use crate::api::Title;
        let t = |id: u64| Title {
            id,
            name: format!("t{id}"),
            ..Default::default()
        };
        let mut local = vec![t(1), t(2)];
        let added = super::merge_titles(&mut local, &[t(2), t(3)]);
        assert_eq!(added, 1);
        assert_eq!(local[0].id, 3, "yeni başa eklenir");
        assert_eq!(local.len(), 3);
        assert_eq!(super::merge_titles(&mut local, &[]), 0, "boş uzak yazmaz");
    }

    #[test]
    fn put_title_body_schema() {
        let b = super::put_title_body(5, "Frieren", 1_700_000_000_000, None);
        assert_eq!(b["id"], 5);
        assert_eq!(b["name"], "Frieren");
        assert_eq!(b["date"], 1_700_000_000_000u64);
        assert!(b["videos"].as_array().unwrap().is_empty());
        let v = serde_json::json!({"season_num": 1, "episode_num": 2});
        let b2 = super::put_title_body(5, "Frieren", 1, Some(v.clone()));
        assert_eq!(b2["videos"][0], v);
    }

    #[test]
    fn history_response_parses_and_sorts() {
        let v = serde_json::json!({
            "success": true,
            "data": {
                "totalCount": [{"count": 2}],
                "totalData": [
                    {"id": 1, "name": "Eski", "title_type": "anime",
                     "date": 1000u64, "videos": [{"season_num": 1u64, "episode_num": 2u64}]},
                    {"id": 2, "name": "Yeni", "title_type": "anime",
                     "date": 2000u64, "videos": []}
                ]
            }
        });
        let (entries, total) = crate::api::Client::parse_history_response(&v).expect("parse");
        assert_eq!(total, 2);
        assert_eq!(entries[0].title.id, 2, "yeniden eskiye");
        assert_eq!(
            (entries[0].season, entries[0].episode),
            (0, 0),
            "videosuz 0,0"
        );
        assert_eq!((entries[1].season, entries[1].episode), (1, 2));
        let bad = serde_json::json!({"success": false, "message": "yok"});
        assert!(crate::api::Client::parse_history_response(&bad).is_err());
    }

    #[test]
    fn time_thresholds() {
        assert_eq!(super::remote_ms_to_secs(5000.0), None, "sınır altı elenir");
        assert_eq!(super::remote_ms_to_secs(5001.0), Some(5.001));
        assert!(!super::should_report_pos(9.9));
        assert!(super::should_report_pos(10.0));
        assert!(super::should_resume(6.0, 100.0));
        assert!(!super::should_resume(4.0, 100.0), "5sn altı baştan");
        assert!(!super::should_resume(96.0, 100.0), "%95 üstü baştan");
    }

    #[test]
    fn report_pos_body_ms_and_fields() {
        let b = super::report_pos_body(7, 2, 5, 90.5);
        assert_eq!(b["titleId"], 7);
        assert_eq!(b["seasonNumber"], 2);
        assert_eq!(b["episodeNumber"], 5);
        assert_eq!(b["time"], 90_500u64, "saniye → milisaniye tamsayı");
        let b0 = super::report_pos_body(7, 2, 5, 0.0);
        assert_eq!(b0["time"], 0u64);
    }

    #[test]
    fn report_throttle_boundaries() {
        assert!(super::should_report_now(0, 60), "ilk bildirim 60sn'de");
        assert!(super::should_report_now(100, 160));
        assert!(!super::should_report_now(100, 159), "59sn yetmez");
        assert!(!super::should_report_now(200, 100), "saat geriye alınmış");
    }

    #[test]
    fn merged_history_union_newest_first() {
        use crate::api::{Episode, HistoryEntry, ServerEntry, Title};
        let t = |id: u64| Title {
            id,
            name: format!("t{id}"),
            ..Default::default()
        };
        let ep = Episode {
            episode: 1,
            season: 1,
            name: String::new(),
        };
        // Yerel eski, sunucu yeni → sunucu kazanır, tekrar yok.
        let local = vec![HistoryEntry {
            title: t(1),
            episode: ep.clone(),
            ts: 100,
        }];
        let remote = vec![
            ServerEntry {
                title: t(1),
                date: 200_000,
                season: 1,
                episode: 2,
            },
            ServerEntry {
                title: t(2),
                date: 50_000,
                season: 1,
                episode: 1,
            },
        ];
        let out = super::merged_history(&local, &remote);
        assert_eq!(out.len(), 2, "union silmez, dedup yapar");
        assert_eq!(out[0].title.id, 1);
        assert_eq!(out[0].ts, 200, "ms → sn");
        assert_eq!(out[0].episode.episode, 2, "yeni bölüm bilgisi");
        assert_eq!(out[1].title.id, 2);
        // Yalnız-yerel de korunur.
        let only_local = super::merged_history(&local, &[]);
        assert_eq!(only_local.len(), 1);
        assert_eq!(super::merged_history(&[], &[]).len(), 0);
    }

    #[test]
    fn multipart_body_shape() {
        let (body, ct) = super::multipart_body("B1", "a.png", "image/png", &[1, 2, 3]);
        assert_eq!(ct, "multipart/form-data; boundary=B1");
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("name=\"file\""));
        assert!(s.contains("filename=\"a.png\""));
        assert!(s.contains("image/png"));
        assert!(body.windows(3).any(|w| w == [1, 2, 3]), "bayt gömülmeli");
        assert!(s.ends_with("--B1--\r\n"));
    }

    #[test]
    fn missing_on_server_filters() {
        use crate::api::{Episode, HistoryEntry, Title};
        let t = |id: u64| Title {
            id,
            name: format!("t{id}"),
            ..Default::default()
        };
        let ep = Episode {
            episode: 1,
            season: 1,
            name: String::new(),
        };
        let local = vec![
            HistoryEntry {
                title: t(1),
                episode: ep.clone(),
                ts: 1,
            },
            HistoryEntry {
                title: t(2),
                episode: ep.clone(),
                ts: 2,
            },
        ];
        let remote: std::collections::HashSet<u64> = [2u64].into_iter().collect();
        let out = super::missing_on_server(&local, &remote);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title.id, 1, "sunucuda olmayan itilir");
        assert!(super::missing_on_server(&local, &[1, 2].into_iter().collect()).is_empty());
    }
}
