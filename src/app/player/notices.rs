use crate::api::SkipTimes;
use crate::skip::SkipPlan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipKind {
    Intro,
    Outro,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkipNotice {
    pub kind: SkipKind,
    pub target: f64,
    pub from: f64,
    pub music: Option<String>,
    pub song_url: Option<String>,
}

impl SkipKind {
    pub fn title(self) -> &'static str {
        match self {
            SkipKind::Intro => "İntro başladı",
            SkipKind::Outro => "Outro başladı",
        }
    }

    pub fn action(self) -> &'static str {
        match self {
            SkipKind::Intro => "İntroyu Atla",
            SkipKind::Outro => "Outro'yu Atla",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            SkipKind::Intro => "İntro",
            SkipKind::Outro => "Outro",
        }
    }
}

/// Segment başlangıcında bir kez gösterilecek kart.
/// `hints` kapalıysa kart yok; atla butonu/kısayollar yine çalışır.
pub fn notice_for(
    times: &SkipTimes,
    pos: f64,
    op_prompted: bool,
    ed_prompted: bool,
    plan: &SkipPlan,
    hints: bool,
) -> Option<SkipNotice> {
    if !hints {
        return None;
    }
    if let (Some(start), Some(end)) = (times.op_start, times.op_end) {
        if !op_prompted && pos >= start - 1.5 && pos <= start + 25.0 {
            return Some(SkipNotice {
                kind: SkipKind::Intro,
                target: end,
                from: start,
                music: plan.music_op.clone(),
                song_url: plan.song_url.clone(),
            });
        }
    }
    if let (Some(start), Some(end)) = (times.ed_start, times.ed_end) {
        if !ed_prompted && pos >= start - 1.5 && pos <= start + 25.0 {
            return Some(SkipNotice {
                kind: SkipKind::Outro,
                target: end,
                from: start,
                music: plan.music_ed.clone(),
                song_url: plan.song_url.clone(),
            });
        }
    }
    None
}

/// Sağ altta sürekli görünen atla butonunun hedefi.
/// Segment penceresinde ve daha önce atlanmamışsa Some.
pub fn skip_target(
    times: &SkipTimes,
    pos: f64,
    op_done: bool,
    ed_done: bool,
) -> Option<(f64, SkipKind)> {
    if !op_done {
        if let (Some(start), Some(end)) = (times.op_start, times.op_end) {
            if pos >= start - 1.5 && pos <= end {
                return Some((end, SkipKind::Intro));
            }
        }
    }
    if !ed_done {
        if let (Some(start), Some(end)) = (times.ed_start, times.ed_end) {
            if pos >= start - 1.5 && pos <= end {
                return Some((end, SkipKind::Outro));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(op: Option<(f64, f64)>, ed: Option<(f64, f64)>, music: Option<&str>) -> SkipPlan {
        SkipPlan {
            times: SkipTimes {
                op_start: op.map(|x| x.0),
                op_end: op.map(|x| x.1),
                ed_start: ed.map(|x| x.0),
                ed_end: ed.map(|x| x.1),
            },
            music_op: music.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn notice_only_in_window_and_once() {
        let p = plan(Some((10.0, 30.0)), None, Some("🎵 Açılış: X"));
        assert!(notice_for(&p.times, 8.0, false, false, &p, true).is_some());
        assert!(notice_for(&p.times, 9.9, false, false, &p, true).is_some());
        assert!(notice_for(&p.times, 31.0, false, false, &p, true).is_none());
        assert!(notice_for(&p.times, 12.0, true, false, &p, true).is_none());
        assert!(notice_for(&p.times, 12.0, false, false, &p, false).is_none());
    }

    #[test]
    fn skip_target_works_and_respects_done() {
        let p = plan(None, Some((100.0, 130.0)), None);
        assert_eq!(
            skip_target(&p.times, 99.0, false, false),
            Some((130.0, SkipKind::Outro))
        );
        assert_eq!(skip_target(&p.times, 99.0, false, true), None);
        assert_eq!(skip_target(&p.times, 140.0, false, false), None);
    }
}
