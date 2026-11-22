use super::types::{PlaybackSnapshot, PlaybackState, SyncAction};

const SEEK_THRESHOLD: f64 = 1.0;
const NUDGE_THRESHOLD: f64 = 0.08;
const HEARTBEAT_MS: u64 = 1000;

/// Host tarafı: mpv anlık görüntüsünden kanonik durum üretir.
#[derive(Default)]
pub struct HostSync {
    state: Option<PlaybackState>,
    last_emit_ms: u64,
}

impl HostSync {
    pub fn state(&self) -> Option<&PlaybackState> {
        self.state.as_ref()
    }

    /// Değişiklik veya 1 saniyelik heartbeat varsa yeni durum döndürür.
    pub fn on_snapshot(
        &mut self,
        snapshot: &PlaybackSnapshot,
        now_ms: u64,
    ) -> Option<PlaybackState> {
        if snapshot.episode.is_empty() {
            return None;
        }
        let previous = self.state.clone();
        let mut generation = previous.as_ref().map(|s| s.generation).unwrap_or(0);
        let mut changed = previous.is_none();
        let mut force_heartbeat = false;

        if let Some(prev) = &previous {
            let elapsed = now_ms.saturating_sub(prev.host_ms) as f64 / 1000.0;
            let expected = if prev.paused {
                prev.position
            } else {
                prev.position + elapsed * prev.rate
            };
            let seek = (snapshot.position - expected).abs() > SEEK_THRESHOLD && !snapshot.buffering;
            let pause_changed = snapshot.paused != prev.paused;
            if seek || pause_changed || prev.episode != snapshot.episode {
                generation = generation.saturating_add(1);
                changed = true;
            }
            force_heartbeat = now_ms.saturating_sub(self.last_emit_ms) >= HEARTBEAT_MS;
        }

        if !changed && !force_heartbeat {
            return None;
        }

        let state = PlaybackState {
            generation,
            position: snapshot.position,
            paused: snapshot.paused,
            rate: 1.0,
            host_ms: now_ms,
            episode: snapshot.episode.clone(),
        };
        self.last_emit_ms = now_ms;
        self.state = Some(state.clone());
        Some(state)
    }
}

/// Guest tarafı: host durumuna göre mpv'ye uygulanacak kararı üretir.
#[derive(Default)]
pub struct GuestSync {
    state: Option<PlaybackState>,
    /// host_ms - local_ms. 0 ise saatler aynı varsayılır.
    offset_ms: i64,
    last_paused: Option<bool>,
    last_action_ms: u64,
}

impl GuestSync {
    pub fn on_state(&mut self, state: PlaybackState) -> Option<SyncAction> {
        self.state = Some(state);
        None
    }

    /// Host durumundan gelen pause/play değişimini ve sapmayı uygular.
    pub fn action_for(&mut self, snapshot: &PlaybackSnapshot, now_local_ms: u64) -> SyncAction {
        let Some(state) = self.state.clone() else {
            return SyncAction::None;
        };
        if snapshot.episode != state.episode {
            return SyncAction::Seek(state.position);
        }

        let had_pause = self.last_paused.is_some();
        let pause_changed = had_pause && self.last_paused != Some(state.paused);
        self.last_paused = Some(state.paused);
        if pause_changed {
            return if state.paused {
                SyncAction::Pause
            } else {
                SyncAction::Play
            };
        }
        if !had_pause && state.paused {
            return SyncAction::Pause;
        }
        if state.paused || snapshot.buffering {
            return SyncAction::None;
        }

        let host_at_local = state.host_ms as i64 - self.offset_ms;
        let elapsed = (now_local_ms as i64 - host_at_local).max(0) as f64 / 1000.0;
        let expected = state.position + elapsed * state.rate;
        let drift = snapshot.position - expected;

        // Aynı düzeltmeyi her 100 ms'de bir tekrar göndermeyi engelle.
        if now_local_ms.saturating_sub(self.last_action_ms) < 700 {
            return SyncAction::None;
        }
        if drift.abs() > SEEK_THRESHOLD {
            self.last_action_ms = now_local_ms;
            SyncAction::Seek(expected)
        } else if drift.abs() > NUDGE_THRESHOLD {
            self.last_action_ms = now_local_ms;
            SyncAction::SetSpeed(if drift > 0.0 { 0.97 } else { 1.03 })
        } else {
            SyncAction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pos: f64, paused: bool, episode: &str) -> PlaybackSnapshot {
        PlaybackSnapshot {
            position: pos,
            duration: 100.0,
            paused,
            buffering: false,
            speed: 1.0,
            episode: episode.into(),
        }
    }

    #[test]
    fn host_initial_pause_seek_and_heartbeat() {
        let mut h = HostSync::default();
        let first = h.on_snapshot(&snap(10.0, false, "1:1:1"), 1_000).unwrap();
        assert_eq!(first.generation, 0);
        assert_eq!(first.position, 10.0);
        assert!(h.on_snapshot(&snap(10.0, false, "1:1:1"), 1_500).is_none());

        let paused = h.on_snapshot(&snap(10.1, true, "1:1:1"), 1_600).unwrap();
        assert_eq!(paused.generation, 1);
        assert!(paused.paused);

        let seek = h.on_snapshot(&snap(40.0, true, "1:1:1"), 1_700).unwrap();
        assert_eq!(seek.generation, 2);
        assert_eq!(seek.position, 40.0);

        let hb = h.on_snapshot(&snap(40.0, true, "1:1:1"), 3_000).unwrap();
        assert_eq!(hb.generation, 2);
    }

    #[test]
    fn guest_pause_play_and_drift() {
        let mut g = GuestSync::default();
        g.on_state(PlaybackState {
            generation: 0,
            position: 10.0,
            paused: false,
            rate: 1.0,
            host_ms: 1_000,
            episode: "1:1:1".into(),
        });
        // İlk durumda pause değişimi yok; birkaç saniye sonra 10 sn beklenir.
        assert_eq!(
            g.action_for(&snap(10.0, false, "1:1:1"), 1_000),
            SyncAction::None
        );
        assert_eq!(
            g.action_for(&snap(10.0, false, "1:1:1"), 11_000),
            SyncAction::Seek(20.0)
        );

        g.on_state(PlaybackState {
            generation: 1,
            position: 20.0,
            paused: true,
            rate: 1.0,
            host_ms: 11_000,
            episode: "1:1:1".into(),
        });
        assert_eq!(
            g.action_for(&snap(20.0, false, "1:1:1"), 11_100),
            SyncAction::Pause
        );

        g.on_state(PlaybackState {
            generation: 2,
            position: 20.0,
            paused: false,
            rate: 1.0,
            host_ms: 12_000,
            episode: "1:1:1".into(),
        });
        assert_eq!(
            g.action_for(&snap(20.0, true, "1:1:1"), 12_100),
            SyncAction::Play
        );
    }

    #[test]
    fn guest_nudges_small_drift() {
        let mut g = GuestSync::default();
        g.on_state(PlaybackState {
            generation: 0,
            position: 10.0,
            paused: false,
            rate: 1.0,
            host_ms: 1_000,
            episode: "1:1:1".into(),
        });
        let action = g.action_for(&snap(10.3, false, "1:1:1"), 1_000);
        assert_eq!(action, SyncAction::SetSpeed(0.97));
    }

    #[test]
    fn guest_buffering_does_not_correct() {
        let mut g = GuestSync::default();
        g.on_state(PlaybackState {
            generation: 0,
            position: 10.0,
            paused: false,
            rate: 1.0,
            host_ms: 1_000,
            episode: "1:1:1".into(),
        });
        let mut s = snap(10.3, false, "1:1:1");
        s.buffering = true;
        assert_eq!(g.action_for(&s, 1_000), SyncAction::None);
    }
}
