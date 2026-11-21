//! Takip edilen yapımların **bölüm listesi** kaynağı.
//!
//! Yayın takvimi yalnızca Takvim ekranı içindir; bildirim kararı bu modülde
//! doğrudan `/secure/related-videos` yanıtındaki bölüm numaralarına dayanır.
//! GTK'dan bağımsızdır; GUI ve `animecix-calendar` ortak kullanır.

use crate::follow_store::{CalendarRow, FollowedTitle};
use crate::http::Http;
use serde_json::Value;

const BASE: &str = "https://animecix.tv";
/// Eski takip kayıtlarında sezon sayısı yoksa en fazla bu kadar sezon yokla.
const MAX_PROBE_SEASONS: u64 = 20;

/// `/secure/related-videos` yanıtındaki bölümleri ortak satıra çevirir.
pub fn rows_from_related_videos(
    title_id: u64,
    title_name: &str,
    season: u64,
    value: &Value,
    release_epoch: i64,
) -> Vec<CalendarRow> {
    let Some(videos) = value["videos"].as_array() else {
        return Vec::new();
    };
    videos
        .iter()
        .filter_map(|v| {
            let episode = v["episode_num"].as_u64()?;
            let sez = v["season_num"].as_u64().unwrap_or(season);
            if episode == 0 || sez != season {
                return None;
            }
            let name = v["name"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{episode}. Bölüm"));
            Some(CalendarRow {
                title_id,
                alt_ids: Vec::new(),
                title_name: title_name.to_string(),
                season,
                episode,
                episode_name: name,
                release_epoch,
            })
        })
        .collect()
}

/// Takip listesindeki yapımların bölüm listelerini çeker.
///
/// `release_epoch` olarak `now` yazılır: buradaki her bölüm API'de mevcut
/// olduğu için "yayınlanmış" kabul edilir. Takvim tarihi kullanılmaz.
pub fn fetch_rows(
    http: &Http,
    follows: &[FollowedTitle],
    now: u64,
) -> Result<Vec<CalendarRow>, String> {
    let mut rows = Vec::new();
    let mut last_err: Option<String> = None;

    for follow in follows.iter().filter(|f| f.notify) {
        if follow.title_type.as_deref() == Some("movie") {
            continue;
        }
        match fetch_title_rows(http, follow, now) {
            Ok(mut list) => rows.append(&mut list),
            Err(e) => {
                eprintln!("[WATCH] {} bölüm listesi: {e}", follow.name);
                last_err = Some(e);
            }
        }
    }

    if rows.is_empty() {
        if let Some(e) = last_err {
            return Err(e);
        }
    }
    Ok(rows)
}

fn fetch_title_rows(
    http: &Http,
    follow: &FollowedTitle,
    now: u64,
) -> Result<Vec<CalendarRow>, String> {
    let mut rows = Vec::new();
    let known = follow.season_count.unwrap_or(0);
    let seasons = if known > 0 {
        known.min(MAX_PROBE_SEASONS)
    } else {
        MAX_PROBE_SEASONS
    };

    for season in 1..=seasons {
        let value: Value = http
            .get(format!("{BASE}/secure/related-videos"))
            .header("Accept", "application/json")
            .query(&[
                ("episode", "1"),
                ("season", &season.to_string()),
                ("videoId", "0"),
                ("titleId", &follow.title_id.to_string()),
            ])
            .timeout(30)
            .send()
            .map_err(|e| format!("sezon {season}: {e}"))?
            .error_for_status()
            .map_err(|e| format!("sezon {season}: {e}"))?
            .json()
            .map_err(|e| format!("sezon {season} JSON: {e}"))?;
        let mut season_rows =
            rows_from_related_videos(follow.title_id, &follow.name, season, &value, now as i64);
        let empty = season_rows.is_empty();
        rows.append(&mut season_rows);
        if empty && known == 0 && season > 1 {
            break;
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_only_requested_season_and_human_names() {
        let value = json!({"videos":[
            {"season_num":1,"episode_num":7,"name":"7. Bölüm"},
            {"season_num":2,"episode_num":1,"name":"Yeni Sezon"},
            {"season_num":1,"episode_num":8,"name":""}
        ]});
        let rows = rows_from_related_videos(5, "Test", 1, &value, 100);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].episode, 7);
        assert_eq!(rows[0].episode_name, "7. Bölüm");
        assert_eq!(rows[1].episode_name, "8. Bölüm");
        assert!(rows.iter().all(|r| r.season == 1 && r.title_id == 5));
    }
}
