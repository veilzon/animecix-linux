//! Yerel izleme verisinden istatistik özeti üretir.
//!
//! `State` + indirme kuyruğu + isim çözümleme haritası saf girdilerdir;
//! GTK'ya bağımlılık yoktur. Eski kayıtlarda `title_name`/`watched_at`
//! bulunmadığı için isim ve süre tahminleri dürüstçe işaretlenir.

use crate::api::{State, Watched};
use crate::download::{DownloadRecord, DownloadStatus};
use std::collections::{BTreeSet, HashMap, HashSet};

const DEFAULT_EPISODE_SECS: f64 = 24.0 * 60.0;
const MIN_REAL_DURATION_SECS: f64 = 5.0;
const WEEK_SECS: u64 = 7 * 86_400;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatsSummary {
    pub anime_count: usize,
    pub film_count: usize,
    pub episode_count: usize,
    pub watch_seconds: f64,
    pub watch_estimated: bool,
    pub downloaded_count: usize,
    pub downloaded_bytes: u64,
    pub top_anime: Vec<AnimeSlice>,
    pub seasons: Vec<SeasonSlice>,
    pub activity: Vec<ActivityBucket>,
}

impl StatsSummary {
    pub fn has_data(&self) -> bool {
        self.episode_count > 0
            || self.film_count > 0
            || self.downloaded_count > 0
            || self.watch_seconds > 0.0
            || self.activity.iter().any(|b| b.count > 0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimeSlice {
    pub title_id: u64,
    pub name: String,
    pub episodes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeasonSlice {
    pub label: String,
    pub episodes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivityBucket {
    pub label: String,
    pub count: usize,
}

pub fn build_summary(
    state: &State,
    downloads: &[DownloadRecord],
    names: &HashMap<u64, String>,
    movie_ids: &HashSet<u64>,
    now: u64,
) -> StatsSummary {
    let mut episode_keys: BTreeSet<(u64, u64, u64)> = BTreeSet::new();
    let mut watched_keys: HashSet<(u64, u64, u64)> = HashSet::new();
    let mut film_ids: HashSet<u64> = HashSet::new();
    for (tid, list) in &state.watched {
        let Ok(tid) = tid.parse::<u64>() else {
            continue;
        };
        for w in list {
            let key = (tid, w.season, w.episode);
            if movie_ids.contains(&tid) {
                film_ids.insert(tid);
            } else {
                episode_keys.insert(key);
            }
            watched_keys.insert(key);
        }
    }
    for (key, (pos, dur)) in &state.progress {
        let Some(k) = parse_progress_key(key) else {
            continue;
        };
        if movie_ids.contains(&k.0) {
            film_ids.insert(k.0);
        } else if *dur > 0.0 && pos / dur >= 0.9 {
            episode_keys.insert(k);
        }
    }

    let mut per_title: HashMap<u64, usize> = HashMap::new();
    for (tid, _, _) in &episode_keys {
        *per_title.entry(*tid).or_default() += 1;
    }

    let mut durations: Vec<f64> = state
        .progress
        .values()
        .map(|(_, dur)| *dur)
        .filter(|dur| *dur >= MIN_REAL_DURATION_SECS)
        .collect();
    durations.sort_by(|a, b| a.total_cmp(b));
    let median_duration = durations.get(durations.len() / 2).copied();

    let mut watch_seconds = 0.0;
    let mut watch_estimated = false;
    let mut progress_keys: HashSet<(u64, u64, u64)> = HashSet::new();
    let mut watched_invalid_progress: HashSet<(u64, u64, u64)> = HashSet::new();
    for (key, (pos, dur)) in &state.progress {
        let Some(k) = parse_progress_key(key) else {
            continue;
        };
        progress_keys.insert(k);
        let valid_duration = *dur >= MIN_REAL_DURATION_SECS;
        if watched_keys.contains(&k) {
            if valid_duration {
                watch_seconds += *dur;
            } else {
                watched_invalid_progress.insert(k);
            }
        } else {
            let partial = pos.max(0.0);
            if valid_duration {
                watch_seconds += partial.min(*dur);
            } else {
                watch_seconds += partial;
                watch_estimated = true;
            }
        }
    }
    for key in &watched_keys {
        if !progress_keys.contains(key) || watched_invalid_progress.contains(key) {
            watch_seconds += median_duration.unwrap_or(DEFAULT_EPISODE_SECS);
            watch_estimated = true;
        }
    }

    let mut top_anime: Vec<AnimeSlice> = per_title
        .into_iter()
        .map(|(tid, episodes)| {
            let watched = state
                .watched
                .get(&tid.to_string())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            AnimeSlice {
                title_id: tid,
                name: title_name(tid, names, watched),
                episodes,
            }
        })
        .collect();
    top_anime.sort_by(|a, b| {
        b.episodes
            .cmp(&a.episodes)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.title_id.cmp(&b.title_id))
    });
    top_anime.truncate(6);

    let mut season_counts: HashMap<u64, usize> = HashMap::new();
    for (_, season, _) in &episode_keys {
        *season_counts.entry(*season).or_default() += 1;
    }
    let mut seasons: Vec<SeasonSlice> = Vec::new();
    for season in 1..=5 {
        if let Some(count) = season_counts.remove(&season) {
            seasons.push(SeasonSlice {
                label: format!("S{season}"),
                episodes: count,
            });
        }
    }
    let older: usize = season_counts.into_values().sum();
    if older > 0 {
        seasons.push(SeasonSlice {
            label: "S6+".to_string(),
            episodes: older,
        });
    }

    let mut events: HashMap<(u64, u64, u64), u64> = HashMap::new();
    for h in &state.history {
        if movie_ids.contains(&h.title.id) {
            continue;
        }
        events
            .entry((h.title.id, h.episode.season, h.episode.episode))
            .and_modify(|ts| *ts = (*ts).max(h.ts))
            .or_insert(h.ts);
    }
    for list in state.watched.values() {
        for w in list {
            if w.watched_at > 0 && !movie_ids.contains(&w.title_id) {
                events
                    .entry((w.title_id, w.season, w.episode))
                    .and_modify(|ts| *ts = (*ts).max(w.watched_at))
                    .or_insert(w.watched_at);
            }
        }
    }
    let mut buckets = [0usize; 8];
    for ts in events.values().copied() {
        if ts == 0 {
            continue;
        }
        let age = now.saturating_sub(ts);
        if age >= 8 * WEEK_SECS {
            continue;
        }
        buckets[7 - (age / WEEK_SECS) as usize] += 1;
    }
    let activity: Vec<ActivityBucket> = (0..8)
        .map(|idx| {
            let weeks_ago = 7 - idx;
            ActivityBucket {
                label: if weeks_ago == 0 {
                    "Bu hafta".to_string()
                } else {
                    format!("{weeks_ago} hf")
                },
                count: buckets[idx],
            }
        })
        .collect();

    let done: Vec<&DownloadRecord> = downloads
        .iter()
        .filter(|r| r.status == DownloadStatus::Done)
        .collect();
    let downloaded_bytes = done.iter().map(|r| r.total.max(r.have)).sum();

    StatsSummary {
        anime_count: per_title_len(&episode_keys),
        film_count: film_ids.len(),
        episode_count: episode_keys.len(),
        watch_seconds,
        watch_estimated,
        downloaded_count: done.len(),
        downloaded_bytes,
        top_anime,
        seasons,
        activity,
    }
}

fn per_title_len(episode_keys: &BTreeSet<(u64, u64, u64)>) -> usize {
    episode_keys
        .iter()
        .map(|(tid, _, _)| *tid)
        .collect::<HashSet<_>>()
        .len()
}

fn parse_progress_key(key: &str) -> Option<(u64, u64, u64)> {
    let mut it = key.split(':');
    Some((
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    ))
}

fn title_name(tid: u64, names: &HashMap<u64, String>, watched: &[Watched]) -> String {
    if let Some(n) = watched.iter().find_map(|w| w.title_name.clone()) {
        if !n.is_empty() {
            return n;
        }
    }
    if let Some(n) = names.get(&tid) {
        if !n.is_empty() {
            return n.clone();
        }
    }
    format!("Başlık #{tid}")
}

pub fn format_duration_human(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    if total < 60 {
        "0 dk".to_string()
    } else if total < 3_600 {
        format!("{} dk", total / 60)
    } else if total < 86_400 {
        format!("{} sa {} dk", total / 3_600, (total % 3_600) / 60)
    } else {
        format!("{} gün {} sa", total / 86_400, (total % 86_400) / 3_600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Episode, HistoryEntry, Title};
    use crate::download::{DownloadRecord, DownloadStatus};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn watched(tid: u64, season: u64, episode: u64) -> Watched {
        Watched {
            title_id: tid,
            season,
            episode,
            ..Default::default()
        }
    }

    fn state_with(watched_list: Vec<Watched>, progress: Vec<(&str, f64, f64)>) -> State {
        let mut st = State::default();
        for w in watched_list {
            st.watched
                .entry(w.title_id.to_string())
                .or_default()
                .push(w);
        }
        for (key, pos, dur) in progress {
            st.progress.insert(key.to_string(), (pos, dur));
        }
        st
    }

    fn download(id: &str, status: DownloadStatus, total: u64, have: u64) -> DownloadRecord {
        DownloadRecord {
            id: id.into(),
            title_id: 1,
            title: "Dizi".into(),
            season: 1,
            episode: 1,
            ep_name: "Bölüm".into(),
            fansub: "F".into(),
            quality: "1080p".into(),
            url: "https://example/v.mp4".into(),
            referer: None,
            dest: PathBuf::from("/tmp/v.mp4"),
            total,
            have,
            status,
        }
    }

    #[test]
    fn unique_anime_and_episode_count_include_90_percent_progress() {
        let st = state_with(
            vec![watched(1, 1, 1), watched(1, 1, 2), watched(2, 1, 1)],
            vec![("1:1:3", 95.0, 100.0)],
        );
        let s = build_summary(&st, &[], &HashMap::new(), &HashSet::new(), 1_000_000_000);
        assert_eq!(s.anime_count, 2);
        assert_eq!(s.episode_count, 4);
    }

    #[test]
    fn watch_time_uses_duration_for_watched_and_position_for_partial() {
        let st = state_with(
            vec![watched(1, 1, 1)],
            vec![("1:1:1", 90.0, 100.0), ("1:1:2", 30.0, 100.0)],
        );
        let s = build_summary(&st, &[], &HashMap::new(), &HashSet::new(), 1_000_000_000);
        assert_eq!(s.watch_seconds, 130.0);
        assert!(!s.watch_estimated);
    }

    #[test]
    fn missing_duration_marks_estimate() {
        let st = state_with(
            vec![watched(1, 1, 1), watched(1, 1, 2)],
            vec![("1:1:1", 90.0, 100.0)],
        );
        let s = build_summary(&st, &[], &HashMap::new(), &HashSet::new(), 1_000_000_000);
        assert_eq!(s.watch_seconds, 200.0);
        assert!(s.watch_estimated);
    }

    #[test]
    fn top_anime_sorted_by_episode_count() {
        let st = state_with(
            vec![
                watched(1, 1, 1),
                watched(1, 1, 2),
                watched(2, 1, 1),
                watched(2, 1, 2),
                watched(2, 1, 3),
            ],
            vec![],
        );
        let mut names = HashMap::new();
        names.insert(1, "A Dizi".to_string());
        names.insert(2, "B Dizi".to_string());
        let s = build_summary(&st, &[], &names, &HashSet::new(), 1_000_000_000);
        assert_eq!(s.top_anime[0].name, "B Dizi");
        assert_eq!(s.top_anime[0].episodes, 3);
        assert_eq!(s.top_anime[1].name, "A Dizi");
    }

    #[test]
    fn season_buckets_cover_s1_to_s6_plus() {
        let st = state_with(
            vec![watched(1, 1, 1), watched(1, 2, 1), watched(1, 6, 1)],
            vec![],
        );
        let s = build_summary(&st, &[], &HashMap::new(), &HashSet::new(), 1_000_000_000);
        assert_eq!(
            s.seasons,
            vec![
                SeasonSlice {
                    label: "S1".into(),
                    episodes: 1
                },
                SeasonSlice {
                    label: "S2".into(),
                    episodes: 1
                },
                SeasonSlice {
                    label: "S6+".into(),
                    episodes: 1
                },
            ]
        );
    }

    #[test]
    fn activity_last_eight_weeks_uses_history_and_watched_at() {
        let now = 1_000_000_000;
        let mut st = state_with(vec![], vec![]);
        let mut w = watched(1, 1, 1);
        w.watched_at = now - 8 * 86_400;
        st.watched.insert("1".into(), vec![w]);
        st.history.push(HistoryEntry {
            title: Title {
                id: 2,
                name: "Dizi".into(),
                ..Default::default()
            },
            episode: Episode {
                season: 1,
                episode: 1,
                name: "Bölüm".into(),
            },
            ts: now - 86_400,
        });
        let s = build_summary(&st, &[], &HashMap::new(), &HashSet::new(), now);
        assert_eq!(s.activity.len(), 8);
        let total: usize = s.activity.iter().map(|b| b.count).sum();
        assert_eq!(total, 2);
        assert_eq!(s.activity.last().map(|b| b.count), Some(1));
    }

    #[test]
    fn downloads_count_done_only_and_use_total() {
        let downloads = vec![
            download("a", DownloadStatus::Done, 1_000, 500),
            download("b", DownloadStatus::Queued, 2_000, 0),
            download("c", DownloadStatus::Done, 3_000, 3_000),
        ];
        let s = build_summary(
            &State::default(),
            &downloads,
            &HashMap::new(),
            &HashSet::new(),
            0,
        );
        assert_eq!(s.downloaded_count, 2);
        assert_eq!(s.downloaded_bytes, 4_000);
    }

    #[test]
    fn movies_are_not_counted_as_episodes() {
        let st = state_with(
            vec![watched(1, 1, 1), watched(1, 1, 2), watched(9, 1, 1)],
            vec![],
        );
        let mut movies = HashSet::new();
        movies.insert(9);
        let s = build_summary(&st, &[], &HashMap::new(), &movies, 1_000_000_000);
        assert_eq!(s.anime_count, 1);
        assert_eq!(s.film_count, 1);
        assert_eq!(s.episode_count, 2);
        assert_eq!(
            s.seasons,
            vec![SeasonSlice {
                label: "S1".into(),
                episodes: 2
            }]
        );
        assert_eq!(s.top_anime.len(), 1);
        assert_eq!(s.top_anime[0].title_id, 1);
    }

    #[test]
    fn movie_watch_sentinel_is_estimated() {
        let st = state_with(vec![watched(9, 1, 1)], vec![("9:1:1", 1.0, 1.0)]);
        let s = build_summary(&st, &[], &HashMap::new(), &HashSet::new(), 1_000_000_000);
        assert!(s.watch_estimated, "sentinel süre tahmini sayılmalı");
        assert_eq!(s.watch_seconds, DEFAULT_EPISODE_SECS);
    }

    #[test]
    fn has_data_covers_partial_progress_and_history() {
        let partial = state_with(vec![], vec![("1:1:1", 50.0, 100.0)]);
        assert!(build_summary(
            &partial,
            &[],
            &HashMap::new(),
            &HashSet::new(),
            1_000_000_000
        )
        .has_data());

        let now = 1_000_000_000;
        let mut history = State::default();
        history.history.push(HistoryEntry {
            title: Title {
                id: 2,
                name: "Dizi".into(),
                ..Default::default()
            },
            episode: Episode {
                season: 1,
                episode: 1,
                name: "Bölüm".into(),
            },
            ts: now - 60,
        });
        assert!(build_summary(&history, &[], &HashMap::new(), &HashSet::new(), now).has_data());
        assert!(!StatsSummary::default().has_data());
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration_human(0.0), "0 dk");
        assert_eq!(format_duration_human(90.0), "1 dk");
        assert_eq!(format_duration_human(3_660.0), "1 sa 1 dk");
        assert_eq!(format_duration_human(90_000.0), "1 gün 1 sa");
    }
}
