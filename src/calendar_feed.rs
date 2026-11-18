//! Yayın takvimi yardımcıları: kopya kayıt birleştirme, arama ve ISO zaman.
//!
//! Bu modül bilinçli olarak GTK'dan bağımsızdır: hem GUI hem de hafif arka
//! plan takipçisi (`animecix-calendar`) aynı kod yolunu kullanır.

use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};

/// Jenerik/kısa adların yanlış birleştirme üretmesini engelleyen eşikler.
const ALIAS_MIN_CHARS: usize = 4;
const ASCII_ALIAS_MIN_CHARS: usize = 8;
const MAX_ALIASES: usize = 16;

/// Başlık eşleştirmesi için normalize edilmiş, ayırt edici bir ad üretir.
/// Kısa veya tamamen jenerik adlarda `None` döner.
pub fn normalize_alias(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        let mapped = match ch {
            'İ' | 'I' | 'ı' | 'i' => Some('i'),
            _ => None,
        };
        if let Some(c) = mapped {
            out.push(c);
            last_space = false;
        } else if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        return None;
    }
    let chars = out.chars().count();
    let words: Vec<&str> = out.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let generic = words.iter().all(|w| {
        matches!(
            *w,
            "hello"
                | "the"
                | "a"
                | "an"
                | "anime"
                | "movie"
                | "movies"
                | "film"
                | "series"
                | "season"
                | "part"
                | "ova"
                | "ona"
                | "special"
                | "specials"
                | "episode"
                | "episodes"
                | "new"
                | "untitled"
        )
    });
    if generic {
        return None;
    }
    if out.chars().any(|c| !c.is_ascii()) {
        if chars < ALIAS_MIN_CHARS {
            return None;
        }
    } else if chars < ASCII_ALIAS_MIN_CHARS || words.len() < 2 {
        return None;
    }
    Some(out)
}

/// ISO-8601 damgasını UTC epoch saniyesine çevirir.
/// `Z`, `+03:00`, `-05:00`, milisaniye ve saniyesiz (`HH:MM`) biçimler desteklenir.
pub fn iso_to_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once('T')?;
    let mut dp = date.split('-');
    let y = dp.next()?.parse::<i64>().ok()?;
    let m = dp.next()?.parse::<u32>().ok()?;
    let d = dp.next()?.parse::<u32>().ok()?;
    if dp.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    if d > days_in_month(y, m) {
        return None;
    }

    let (time, offset_secs) = if let Some(stripped) = rest.strip_suffix('Z') {
        (stripped, 0i64)
    } else if let Some(idx) = rest.rfind(['+', '-']) {
        let (t, off) = rest.split_at(idx);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let body = &off[1..];
        let (oh, om) = body.split_once(':').unwrap_or((body, "0"));
        let oh = oh.parse::<i64>().ok()?;
        let om = om.parse::<i64>().ok()?;
        if oh > 23 || om > 59 {
            return None;
        }
        (t, sign * (oh * 3600 + om * 60))
    } else {
        (rest, 0)
    };

    let mut tp = time.split(':');
    let hh = tp.next()?.parse::<i64>().ok()?;
    let mm = tp.next()?.parse::<i64>().ok()?;
    let sec_raw = tp.next().unwrap_or("0");
    if tp.next().is_some() {
        return None;
    }
    let sec = sec_raw
        .split('.')
        .next()
        .unwrap_or("0")
        .parse::<i64>()
        .ok()?;
    if !(0..=23).contains(&hh) || !(0..=59).contains(&mm) || !(0..=60).contains(&sec) {
        return None;
    }

    Some(days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + sec - offset_secs)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// Howard Hinnant'ın `days_from_civil` algoritması (1970-01-01 = 0).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Arama için başlık + alternatif adlar + bölüm kodunu tek metinde toplar.
/// Filtreleme `tr_fold` ile yapılır; burada ham metin tutulur.
pub fn search_haystack(
    name: &str,
    aliases: &[String],
    episode_name: &str,
    season: u64,
    episode: u64,
) -> String {
    let mut s = String::with_capacity(name.len() + episode_name.len() + 64);
    s.push_str(name);
    for a in aliases {
        s.push(' ');
        s.push_str(a);
    }
    if !episode_name.is_empty() {
        s.push(' ');
        s.push_str(episode_name);
    }
    s.push_str(&format!(" S{season:02}E{episode:02}"));
    s.push_str(&format!(" {season}x{episode}"));
    s
}

#[derive(Default)]
struct Meta {
    id: u64,
    mal: u64,
    tmdb: u64,
    year: u64,
    title_type: String,
    aliases: BTreeSet<String>,
    display: BTreeSet<String>,
}

impl Meta {
    fn ingest(&mut self, t: &Value) {
        self.mal = self.mal.max(t["mal_id"].as_u64().unwrap_or(0));
        self.tmdb = self.tmdb.max(t["tmdb_id"].as_u64().unwrap_or(0));
        if self.year == 0 {
            self.year = t["year"].as_u64().unwrap_or(0);
        }
        if self.title_type.is_empty() {
            if let Some(s) = t["title_type"].as_str() {
                self.title_type = s.to_lowercase();
            }
        }
        for key in ["name", "name_english", "name_romanji", "original_title"] {
            if let Some(s) = t[key].as_str() {
                self.add_alias(s);
            }
        }
        if let Some(arr) = t["titles"].as_array() {
            for item in arr {
                if let Some(s) = item["title"].as_str() {
                    self.add_alias(s);
                }
            }
        }
    }

    fn add_alias(&mut self, s: &str) {
        let s = s.trim();
        if s.chars().count() < 2 {
            return;
        }
        self.display.insert(s.to_string());
        if let Some(n) = normalize_alias(s) {
            self.aliases.insert(n);
        }
    }
}

/// Takvim yanıtındaki kopya satırları tekilleştirir.
///
/// Gruplama `mal_id`, `tmdb_id` ve ayırt edici alternatif adlarla yapılır;
/// satır anahtarı `(grup, gün, sezon, bölüm)` olduğu için farklı gün veya
/// farklı S/E kayıtları korunur. Tutulan satıra grubun tüm adları ve
/// `alt_title_ids` eklenir.
pub fn dedup_calendar_json(v: &mut Value) {
    let Some(days) = v.get_mut("data").and_then(Value::as_array_mut) else {
        return;
    };
    if days.is_empty() {
        return;
    }

    let mut metas: Vec<Meta> = Vec::new();
    let mut id_slot: HashMap<u64, usize> = HashMap::new();
    for day in days.iter() {
        let Some(eps) = day.get("episodes").and_then(Value::as_array) else {
            continue;
        };
        for e in eps {
            let Some(id) = episode_title_id(e) else {
                continue;
            };
            let slot = match id_slot.get(&id).copied() {
                Some(s) => s,
                None => {
                    let s = metas.len();
                    metas.push(Meta {
                        id,
                        ..Default::default()
                    });
                    id_slot.insert(id, s);
                    s
                }
            };
            if let Some(t) = e.get("title").filter(|t| t.is_object()) {
                metas[slot].ingest(t);
            }
        }
    }

    let n = metas.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut keys: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in metas.iter().enumerate() {
        if m.mal > 0 {
            keys.entry(format!("mal:{}", m.mal)).or_default().push(i);
        }
        if m.tmdb > 0 {
            keys.entry(format!("tmdb:{}", m.tmdb)).or_default().push(i);
        }
        for a in &m.aliases {
            keys.entry(format!("alias:{a}")).or_default().push(i);
        }
    }

    for (key, slots) in &keys {
        if key.starts_with("mal:") || key.starts_with("tmdb:") {
            let first = slots[0];
            for &s in &slots[1..] {
                union(&mut parent, first, s);
            }
        }
    }
    for (key, slots) in &keys {
        if !key.starts_with("alias:") || slots.len() < 2 {
            continue;
        }
        let first = slots[0];
        for &s in &slots[1..] {
            if can_union(&mut parent, &metas, first, s) {
                union(&mut parent, first, s);
            }
        }
    }

    let mut group_alias: HashMap<usize, BTreeSet<String>> = HashMap::new();
    let mut group_ids: HashMap<usize, BTreeSet<u64>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        group_alias
            .entry(root)
            .or_default()
            .extend(metas[i].display.iter().cloned());
        group_ids.entry(root).or_default().insert(metas[i].id);
    }

    let mut orphan = 0usize;
    for day in days.iter_mut() {
        let Some(raw) = day.get_mut("episodes").and_then(Value::as_array_mut) else {
            continue;
        };
        if raw.is_empty() {
            continue;
        }
        let mut buckets: HashMap<(usize, u64, u64), Vec<Value>> = HashMap::new();
        for e in raw.drain(..) {
            let group = episode_title_id(&e)
                .and_then(|id| id_slot.get(&id).copied())
                .map(|s| find(&mut parent, s))
                .unwrap_or_else(|| {
                    orphan += 1;
                    usize::MAX - orphan
                });
            let season = e["season_number"].as_u64().unwrap_or(0);
            let episode = e["episode_number"].as_u64().unwrap_or(0);
            buckets.entry((group, season, episode)).or_default().push(e);
        }

        let empty: BTreeSet<String> = BTreeSet::new();
        let mut out: Vec<Value> = Vec::with_capacity(buckets.len());
        for (key, cands) in buckets {
            let aliases = group_alias.get(&key.0).unwrap_or(&empty);
            let ids = group_ids.get(&key.0).cloned().unwrap_or_default();
            out.push(merge_group(cands, aliases, &ids));
        }
        out.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        *raw = out;
    }
}

fn episode_title_id(e: &Value) -> Option<u64> {
    e.get("title")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_u64)
        .or_else(|| e["title_id"].as_u64())
        .filter(|id| *id > 0)
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[rb] = ra;
    }
}

fn compatible(a: &Meta, b: &Meta) -> bool {
    if a.mal != 0 && b.mal != 0 && a.mal != b.mal {
        return false;
    }
    if a.tmdb != 0 && b.tmdb != 0 && a.tmdb != b.tmdb {
        return false;
    }
    if a.year != 0 && b.year != 0 && a.year != b.year {
        return false;
    }
    if !a.title_type.is_empty()
        && !b.title_type.is_empty()
        && is_movie(&a.title_type) != is_movie(&b.title_type)
    {
        return false;
    }
    true
}

/// İki kümeyi birleştirmeden önce tüm üyelerin birbiriyle uyumlu olduğunu
/// doğrular; zincirleme (A-B, B-C uyumlu ama A-C değil) yanlış birleşmeyi önler.
fn can_union(parent: &mut [usize], metas: &[Meta], a: usize, b: usize) -> bool {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra == rb {
        return true;
    }
    let members_a: Vec<usize> = (0..metas.len())
        .filter(|i| find(parent, *i) == ra)
        .collect();
    let members_b: Vec<usize> = (0..metas.len())
        .filter(|i| find(parent, *i) == rb)
        .collect();
    for i in &members_a {
        for j in &members_b {
            if !compatible(&metas[*i], &metas[*j]) {
                return false;
            }
        }
    }
    true
}

fn is_movie(t: &str) -> bool {
    matches!(t, "movie" | "movies" | "film")
}

fn title_has_data(e: &Value) -> bool {
    let t = &e["title"];
    !t["upload_slot"].is_null()
        || t["is_ready"].as_bool() == Some(true)
        || t["show_videos"].as_bool() == Some(true)
}

fn episode_is_real(e: &Value) -> bool {
    if e["estimated"].as_bool().unwrap_or(false) || e["synthetic"].as_bool().unwrap_or(false) {
        return false;
    }
    !e["id"]
        .as_str()
        .map(|s| s.starts_with("estimate-"))
        .unwrap_or(false)
}

fn title_richness(e: &Value) -> u32 {
    let t = &e["title"];
    let mut r = 0;
    for key in ["updated_at", "description", "poster", "backdrop", "seasons"] {
        let present = match &t[key] {
            Value::Null => false,
            Value::String(s) => !s.trim().is_empty(),
            Value::Array(a) => !a.is_empty(),
            _ => true,
        };
        if present {
            r += 1;
        }
    }
    if t["episode_count"].as_u64().unwrap_or(0) > 0 {
        r += 1;
    }
    r
}

fn score(e: &Value) -> (u8, u8, u32) {
    (
        title_has_data(e) as u8,
        episode_is_real(e) as u8,
        title_richness(e),
    )
}

fn title_id_of(e: &Value) -> u64 {
    episode_title_id(e).unwrap_or(0)
}

fn better(a: &Value, b: &Value) -> bool {
    let (sa, sb) = (score(a), score(b));
    if sa != sb {
        return sa > sb;
    }
    let (ia, ib) = (title_id_of(a), title_id_of(b));
    match (ia, ib) {
        (0, 0) => false,
        (0, _) => false,
        (_, 0) => true,
        _ => ia < ib,
    }
}

fn merge_group(cands: Vec<Value>, aliases: &BTreeSet<String>, group_ids: &BTreeSet<u64>) -> Value {
    let mut best = 0usize;
    for i in 1..cands.len() {
        if better(&cands[i], &cands[best]) {
            best = i;
        }
    }

    // Saat kaynağı: önce gerçek (tahmini olmayan) kayıt, sonra en erken.
    let mut time_src: Option<usize> = None;
    for (i, e) in cands.iter().enumerate() {
        if !episode_is_real(e) || iso_to_epoch(e["release_date"].as_str().unwrap_or("")).is_none() {
            continue;
        }
        let e_epoch = iso_to_epoch(e["release_date"].as_str().unwrap_or("")).unwrap();
        let take = time_src
            .map(|s| {
                let cur = iso_to_epoch(cands[s]["release_date"].as_str().unwrap_or(""))
                    .unwrap_or(i64::MAX);
                e_epoch < cur
            })
            .unwrap_or(true);
        if take {
            time_src = Some(i);
        }
    }
    if time_src.is_none() {
        for (i, e) in cands.iter().enumerate() {
            let epoch = iso_to_epoch(e["release_date"].as_str().unwrap_or(""));
            let Some(epoch) = epoch else { continue };
            let take = time_src
                .map(|s| {
                    let cur = iso_to_epoch(cands[s]["release_date"].as_str().unwrap_or(""))
                        .unwrap_or(i64::MAX);
                    epoch < cur
                })
                .unwrap_or(true);
            if take {
                time_src = Some(i);
            }
        }
    }

    let mut merged = cands[best].clone();
    if let Some(src) = time_src {
        let s = &cands[src];
        if let Some(rel) = s.get("release_date") {
            merged["release_date"] = rel.clone();
        }
        merged["estimated"] = json!(s["estimated"].as_bool().unwrap_or(false));
        merged["synthetic"] = json!(s["synthetic"].as_bool().unwrap_or(false));
    }

    for key in ["name", "description", "poster"] {
        let missing = merged
            .get(key)
            .map(|v| v.is_null() || v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false))
            .unwrap_or(true);
        if missing {
            if let Some(found) = cands.iter().find_map(|e| {
                e.get(key).filter(|v| {
                    !v.is_null() && v.as_str().map(|s| !s.trim().is_empty()).unwrap_or(true)
                })
            }) {
                merged[key] = found.clone();
            }
        }
    }

    if !aliases.is_empty() {
        let list: Vec<&String> = aliases.iter().take(MAX_ALIASES).collect();
        merged["aliases"] = json!(list);
    }

    let kept_id = title_id_of(&merged);
    let alt: Vec<u64> = group_ids
        .iter()
        .copied()
        .filter(|id| *id != kept_id && *id > 0)
        .collect();
    if !alt.is_empty() {
        merged["alt_title_ids"] = json!(alt);
    }

    merged
}

fn sort_key(e: &Value) -> (i64, u64, u64, u64, String) {
    (
        iso_to_epoch(e["release_date"].as_str().unwrap_or("")).unwrap_or(i64::MAX),
        title_id_of(e),
        e["season_number"].as_u64().unwrap_or(0),
        e["episode_number"].as_u64().unwrap_or(0),
        e["name"].as_str().unwrap_or("").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ep(
        title: Value,
        season: u64,
        episode: u64,
        id: Value,
        estimated: bool,
        synthetic: bool,
        rel: &str,
    ) -> Value {
        let mut v = json!({
            "title": title,
            "season_number": season,
            "episode_number": episode,
            "id": id,
            "estimated": estimated,
            "release_date": rel,
            "name": format!("{episode}. Bölüm"),
        });
        if synthetic {
            v["synthetic"] = json!(true);
        }
        v
    }

    fn cal(days: Vec<(&str, Vec<Value>)>) -> Value {
        let mut data = Vec::new();
        for (date, eps) in days {
            data.push(json!({ "date": date, "episodes": eps }));
        }
        json!({ "data": data })
    }

    fn day_eps(v: &Value, day: usize) -> &Vec<Value> {
        v["data"][day]["episodes"].as_array().unwrap()
    }

    fn firefly_titles() -> (Value, Value) {
        let old = json!({
            "id": 13445,
            "name": "Firefly Wedding",
            "name_english": "Firefly Wedding",
            "name_romanji": "Hotaru no Yomeiri",
            "original_title": "ホタルの嫁入り",
            "mal_id": 63293,
            "tmdb_id": 320909,
            "is_ready": false,
            "title_type": "anime",
            "year": 2026,
            "titles": [
                { "title": "Hotaru no Yomeiri" },
                { "title": "ホタルの嫁入り" },
                { "title": "Firefly Wedding" }
            ]
        });
        let new = json!({
            "id": 13562,
            "name": "Hotaru no Yomeiri",
            "upload_slot": { "source": "history" },
            "title_type": "anime"
        });
        (old, new)
    }

    #[test]
    fn firefly_pair_dedups_and_keeps_episode_bearing_record() {
        let (old, new) = firefly_titles();
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![
                ep(
                    old,
                    1,
                    7,
                    json!(291638),
                    false,
                    false,
                    "2026-11-20T00:00:00.000Z",
                ),
                ep(
                    new,
                    1,
                    7,
                    json!("estimate-13562:1:7"),
                    true,
                    true,
                    "2026-11-20T19:00:00.000Z",
                ),
            ],
        )]);

        dedup_calendar_json(&mut v);

        let eps = day_eps(&v, 0);
        assert_eq!(eps.len(), 1, "aynı gün + S/E tek satıra inmeli");
        let e = &eps[0];
        assert_eq!(
            e["title"]["id"],
            json!(13562),
            "bölüm verisi olan kayıt kalmalı"
        );
        assert_eq!(
            e["release_date"],
            json!("2026-11-20T00:00:00.000Z"),
            "gerçek (tahmini olmayan) yayın saati kullanılmalı"
        );
        assert_eq!(e["estimated"], json!(false));
        assert_eq!(e["synthetic"], json!(false));
        assert_eq!(e["alt_title_ids"], json!([13445]));
        let aliases = e["aliases"].as_array().unwrap();
        assert!(aliases.iter().any(|a| a == "Firefly Wedding"));
        assert!(aliases.iter().any(|a| a == "Hotaru no Yomeiri"));

        let alias_vec: Vec<String> = aliases
            .iter()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
        let hay = search_haystack("Hotaru no Yomeiri", &alias_vec, "7. Bölüm", 1, 7);
        assert!(hay.contains("Firefly Wedding"));
        assert!(hay.contains("S01E07"));
    }

    #[test]
    fn hello_pair_dedups_by_mal_id_and_prefers_ready_record() {
        let a = json!({
            "id": 13452,
            "name": "Hello, I Am a Witch and My Crush Wants Me to Make a Love Potion!",
            "name_english": "I'm a Witch Who's Crush Requested a Love Potion",
            "name_romanji": "Doumo, Suki na Hito ni Horegusuri wo Irai sareta Majo desu.",
            "original_title": "どうも、好きな人に惚れ薬を依頼された魔女です。",
            "mal_id": 63409,
            "tmdb_id": 313068,
            "is_ready": false,
            "title_type": "anime",
            "year": 2026,
            "titles": [
                { "title": "Doumo, Suki na Hito ni Horegusuri wo Irai sareta Majo desu." },
                { "title": "Hello" }
            ]
        });
        let b = json!({
            "id": 13561,
            "name": "Doumo, Suki na Hito ni Horegusuri wo Irai Sareta Majo desu.",
            "name_english": "Hello, I am a Witch and my Crush Wants me to Make a Love Potion!",
            "original_title": "どうも、好きな人に惚れ薬を依頼された魔女です。",
            "mal_id": 63409,
            "tmdb_id": 0,
            "is_ready": true,
            "show_videos": true,
            "upload_slot": { "source": "auto" },
            "title_type": "anime"
        });
        let mut v = cal(vec![(
            "2026-11-16T00:00:00.000Z",
            vec![
                ep(
                    a,
                    1,
                    7,
                    json!(291110),
                    false,
                    false,
                    "2026-11-16T00:00:00.000Z",
                ),
                ep(
                    b,
                    1,
                    7,
                    json!(292327),
                    false,
                    false,
                    "2026-11-16T17:50:50.965Z",
                ),
            ],
        )]);

        dedup_calendar_json(&mut v);

        let eps = day_eps(&v, 0);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0]["title"]["id"], json!(13561));
        assert_eq!(
            eps[0]["release_date"],
            json!("2026-11-16T00:00:00.000Z"),
            "en erken gerçek saat korunmalı"
        );
        assert_eq!(eps[0]["alt_title_ids"], json!([13452]));
    }

    #[test]
    fn different_seasons_stay_separate() {
        let t = json!({ "id": 11713, "name": "Blue Box", "mal_id": 1, "title_type": "anime" });
        let mut v = cal(vec![(
            "2026-11-22T00:00:00.000Z",
            vec![
                ep(
                    t.clone(),
                    1,
                    33,
                    json!(1),
                    true,
                    false,
                    "2026-11-22T00:00:00.000Z",
                ),
                ep(t, 2, 7, json!(2), true, false, "2026-11-22T00:00:00.000Z"),
            ],
        )]);

        dedup_calendar_json(&mut v);

        let eps = day_eps(&v, 0);
        assert_eq!(eps.len(), 2, "S1E33 ve S2E7 gerçek ayrı satırlardır");
        assert_eq!(eps[0]["season_number"], json!(1));
        assert_eq!(eps[1]["season_number"], json!(2));
    }

    #[test]
    fn different_days_stay_separate() {
        let t = json!({ "id": 8585, "name": "Wushen Zhuzai", "title_type": "anime" });
        let mut v = cal(vec![
            (
                "2026-11-17T00:00:00.000Z",
                vec![ep(
                    t.clone(),
                    1,
                    710,
                    json!(1),
                    true,
                    false,
                    "2026-11-17T00:00:00.000Z",
                )],
            ),
            (
                "2026-11-22T00:00:00.000Z",
                vec![ep(
                    t,
                    1,
                    711,
                    json!(2),
                    true,
                    false,
                    "2026-11-22T00:00:00.000Z",
                )],
            ),
        ]);

        dedup_calendar_json(&mut v);

        assert_eq!(day_eps(&v, 0).len(), 1);
        assert_eq!(day_eps(&v, 1).len(), 1);
    }

    #[test]
    fn generic_alias_does_not_merge_unrelated_titles() {
        let a = json!({ "id": 1, "name": "Hello", "title_type": "anime" });
        let b = json!({ "id": 2, "name": "Hello", "title_type": "anime" });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![
                ep(a, 1, 7, json!(1), false, false, "2026-11-20T00:00:00.000Z"),
                ep(b, 1, 7, json!(2), false, false, "2026-11-20T00:00:00.000Z"),
            ],
        )]);

        dedup_calendar_json(&mut v);

        assert_eq!(day_eps(&v, 0).len(), 2, "jenerik kısa ad birleştirmemeli");
        assert_eq!(normalize_alias("Hello"), None);
    }

    #[test]
    fn long_alias_merges_without_any_ids() {
        let a = json!({ "id": 10, "name": "Hotaru no Yomeiri", "title_type": "anime" });
        let b = json!({ "id": 11, "name": "Hotaru no Yomeiri", "title_type": "anime" });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![
                ep(a, 1, 7, json!(1), false, false, "2026-11-20T00:00:00.000Z"),
                ep(b, 1, 7, json!(2), false, false, "2026-11-20T00:00:00.000Z"),
            ],
        )]);

        dedup_calendar_json(&mut v);

        let eps = day_eps(&v, 0);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0]["title"]["id"], json!(10));
        assert_eq!(eps[0]["alt_title_ids"], json!([11]));
    }

    #[test]
    fn conflicting_year_blocks_alias_merge() {
        let a = json!({
            "id": 10, "name": "Hotaru no Yomeiri", "title_type": "anime", "year": 2020
        });
        let b = json!({
            "id": 11, "name": "Hotaru no Yomeiri", "title_type": "anime", "year": 2026
        });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![
                ep(a, 1, 7, json!(1), false, false, "2026-11-20T00:00:00.000Z"),
                ep(b, 1, 7, json!(2), false, false, "2026-11-20T00:00:00.000Z"),
            ],
        )]);

        dedup_calendar_json(&mut v);

        assert_eq!(day_eps(&v, 0).len(), 2, "farklı yıl remake olabilir");
    }

    #[test]
    fn conflicting_mal_ids_do_not_merge() {
        let a = json!({
            "id": 1, "name": "Hotaru no Yomeiri", "mal_id": 111, "title_type": "anime"
        });
        let b = json!({
            "id": 2, "name": "Hotaru no Yomeiri", "mal_id": 222, "title_type": "anime"
        });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![
                ep(a, 1, 7, json!(1), false, false, "2026-11-20T00:00:00.000Z"),
                ep(b, 1, 7, json!(2), false, false, "2026-11-20T00:00:00.000Z"),
            ],
        )]);

        dedup_calendar_json(&mut v);

        assert_eq!(day_eps(&v, 0).len(), 2, "farklı mal_id'ler birleşmemeli");
    }

    #[test]
    fn alias_chain_respects_year_conflict() {
        let a = json!({
            "id": 10, "name": "Hotaru no Yomeiri", "title_type": "anime"
        });
        let b = json!({
            "id": 11, "name": "Hotaru no Yomeiri", "year": 2020, "title_type": "anime"
        });
        let c = json!({
            "id": 12, "name": "Hotaru no Yomeiri", "year": 2026, "title_type": "anime"
        });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![
                ep(a, 1, 7, json!(1), false, false, "2026-11-20T00:00:00.000Z"),
                ep(b, 1, 7, json!(2), false, false, "2026-11-20T00:00:00.000Z"),
                ep(c, 1, 7, json!(3), false, false, "2026-11-20T00:00:00.000Z"),
            ],
        )]);

        dedup_calendar_json(&mut v);

        assert_eq!(
            day_eps(&v, 0).len(),
            2,
            "yıl çelişkisi zincirleme birleşmeyi engellemeli"
        );
    }

    #[test]
    fn same_title_id_duplicate_row_collapses() {
        let t = json!({ "id": 42, "name": "A", "title_type": "anime" });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![
                ep(
                    t.clone(),
                    1,
                    7,
                    json!(1),
                    false,
                    false,
                    "2026-11-20T00:00:00.000Z",
                ),
                ep(t, 1, 7, json!(2), false, false, "2026-11-20T00:00:00.000Z"),
            ],
        )]);

        dedup_calendar_json(&mut v);

        assert_eq!(day_eps(&v, 0).len(), 1);
    }

    #[test]
    fn singleton_rows_get_aliases_for_search() {
        let t = json!({
            "id": 13445,
            "name": "Firefly Wedding",
            "name_romanji": "Hotaru no Yomeiri",
            "mal_id": 63293,
            "title_type": "anime"
        });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![ep(
                t,
                1,
                7,
                json!(1),
                false,
                false,
                "2026-11-20T00:00:00.000Z",
            )],
        )]);

        dedup_calendar_json(&mut v);

        let aliases = day_eps(&v, 0)[0]["aliases"].as_array().unwrap();
        assert!(aliases.iter().any(|a| a == "Hotaru no Yomeiri"));
        assert!(day_eps(&v, 0)[0].get("alt_title_ids").is_none());
    }

    #[test]
    fn missing_title_does_not_panic() {
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![json!({
                "title_id": 999,
                "season_number": 1,
                "episode_number": 7,
                "release_date": "2026-11-20T00:00:00.000Z"
            })],
        )]);

        dedup_calendar_json(&mut v);

        assert_eq!(day_eps(&v, 0).len(), 1);
    }

    #[test]
    fn is_empty_and_unparseable_dates_are_preserved() {
        let mut v = json!({ "other": true });
        dedup_calendar_json(&mut v);
        assert_eq!(v, json!({ "other": true }));

        let t = json!({ "id": 1, "name": "A" });
        let mut v = cal(vec![(
            "2026-11-20T00:00:00.000Z",
            vec![ep(t.clone(), 1, 7, json!(1), false, false, "bozuk")],
        )]);
        dedup_calendar_json(&mut v);
        assert_eq!(day_eps(&v, 0).len(), 1);
    }

    #[test]
    fn iso_to_epoch_handles_real_calendar_formats() {
        assert_eq!(iso_to_epoch("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(iso_to_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            iso_to_epoch("2026-11-18T12:00:00+03:00"),
            iso_to_epoch("2026-11-18T09:00:00Z")
        );
        assert_eq!(
            iso_to_epoch("2026-11-18T00:30:00-05:00"),
            iso_to_epoch("2026-11-18T05:30:00Z")
        );
        assert_eq!(
            iso_to_epoch("2024-02-29T00:00:00.000Z"),
            Some(1_709_164_800)
        );
        assert_eq!(
            iso_to_epoch("2026-11-18T12:30Z"),
            iso_to_epoch("2026-11-18T12:30:00Z")
        );
    }

    #[test]
    fn iso_to_epoch_rejects_invalid_input() {
        assert_eq!(iso_to_epoch(""), None);
        assert_eq!(iso_to_epoch("2026-11-18"), None);
        assert_eq!(iso_to_epoch("2026-13-01T00:00:00Z"), None);
        assert_eq!(iso_to_epoch("2026-02-30T00:00:00Z"), None);
        assert_eq!(iso_to_epoch("2026-11-18T25:00:00Z"), None);
        assert_eq!(iso_to_epoch("bozuk"), None);
    }

    #[test]
    fn search_haystack_contains_names_and_episode_code() {
        let aliases = vec!["Firefly Wedding".to_string(), "ホタルの嫁入り".to_string()];
        let hay = search_haystack("Hotaru no Yomeiri", &aliases, "7. Bölüm", 2, 11);
        assert!(hay.contains("Hotaru no Yomeiri"));
        assert!(hay.contains("Firefly Wedding"));
        assert!(hay.contains("7. Bölüm"));
        assert!(hay.contains("S02E11"));
        assert!(hay.contains("2x11"));
    }
}
