use crate::api::SkipTimes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipKind {
    Intro,
    Outro,
}

impl SkipKind {
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
    use crate::skip::SkipPlan;

    fn plan(op: Option<(f64, f64)>, ed: Option<(f64, f64)>) -> SkipPlan {
        SkipPlan {
            times: SkipTimes {
                op_start: op.map(|x| x.0),
                op_end: op.map(|x| x.1),
                ed_start: ed.map(|x| x.0),
                ed_end: ed.map(|x| x.1),
            },
            ..Default::default()
        }
    }

    #[test]
    fn skip_target_works_and_respects_done() {
        let p = plan(None, Some((100.0, 130.0)));
        assert_eq!(
            skip_target(&p.times, 99.0, false, false),
            Some((130.0, SkipKind::Outro))
        );
        assert_eq!(skip_target(&p.times, 99.0, false, true), None);
        assert_eq!(skip_target(&p.times, 140.0, false, false), None);
    }
}
