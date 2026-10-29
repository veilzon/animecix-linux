//! İşlem yığını: sağ üstte gösterilen en fazla üç satırlık işlem kuyruğunun
//! saf veri modeli. GTK'dan bağımsızdır; widget katmanı bu modeli okur.

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Kuyrukta tutulan toplam kayıt üst sınırı (bellek için).
const MAX_RECORDS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationState {
    Running,
    Done,
    Error,
}

#[derive(Clone, Debug)]
pub struct Operation {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub state: OperationState,
    pub progress: Option<f64>,
    pub started_at: Instant,
    pub expires_at: Option<Instant>,
    pub seq: u64,
}

pub struct OperationQueue {
    /// En yeni kayıt başta olacak şekilde tutulur.
    ops: Vec<Operation>,
    max_visible: usize,
    next_seq: u64,
}

impl OperationQueue {
    pub fn new(max_visible: usize) -> Self {
        Self {
            ops: Vec::new(),
            max_visible,
            next_seq: 0,
        }
    }

    /// İşlemi başlatır; aynı kimlik varsa yerinde günceller.
    pub fn begin(&mut self, id: &str, title: &str, detail: &str) {
        if let Some(op) = self.ops.iter_mut().find(|op| op.id == id) {
            op.title = title.to_string();
            op.detail = detail.to_string();
            op.state = OperationState::Running;
            op.expires_at = None;
            return;
        }

        let seq = self.fresh_seq();
        self.ops.insert(
            0,
            Operation {
                id: id.to_string(),
                title: title.to_string(),
                detail: detail.to_string(),
                state: OperationState::Running,
                progress: None,
                started_at: Instant::now(),
                expires_at: None,
                seq,
            },
        );
        self.enforce_limit();
    }

    /// Var olan işlemin ayrıntısını ve ilerlemesini günceller; durumu değiştirmez.
    pub fn update(&mut self, id: &str, detail: &str, progress: Option<f64>) {
        if let Some(op) = self.ops.iter_mut().find(|op| op.id == id) {
            op.detail = detail.to_string();
            op.progress = progress;
        }
    }

    /// İşlemi sonlandırır. Zaten sonlanmış kaydın TTL'si sıfırlanmaz.
    pub fn finish(&mut self, id: &str, state: OperationState, ttl: Duration) {
        let Some(op) = self.ops.iter_mut().find(|op| op.id == id) else {
            return;
        };
        if matches!(op.state, OperationState::Done | OperationState::Error) {
            return;
        }
        op.state = state;
        op.expires_at = if state == OperationState::Running {
            None
        } else {
            Some(Instant::now() + ttl)
        };
        if state == OperationState::Done {
            op.progress = Some(1.0);
        }
    }

    /// Kısa süreli bilgi kaydı ekler.
    pub fn info(&mut self, title: &str, detail: &str, ttl: Duration) {
        let seq = self.fresh_seq();
        self.push_notice(
            format!("info:{seq}"),
            title,
            detail,
            OperationState::Done,
            seq,
            ttl,
        );
    }

    /// Kısa süreli hata kaydı ekler.
    pub fn error(&mut self, title: &str, detail: &str, ttl: Duration) {
        let seq = self.fresh_seq();
        self.push_notice(
            format!("error:{seq}"),
            title,
            detail,
            OperationState::Error,
            seq,
            ttl,
        );
    }

    /// Süresi dolan kayıtları siler.
    pub fn tick(&mut self, now: Instant) {
        self.ops
            .retain(|op| op.expires_at.is_none_or(|at| at > now));
    }

    /// Görünür kayıtlar: önce Running, sonra Error, sonra Done; her grupta
    /// en yeni önce. Toplam `max_visible` ile sınırlıdır.
    pub fn visible(&self) -> Vec<&Operation> {
        let mut out: Vec<&Operation> = Vec::new();
        for state in [
            OperationState::Running,
            OperationState::Error,
            OperationState::Done,
        ] {
            let mut group: Vec<&Operation> =
                self.ops.iter().filter(|op| op.state == state).collect();
            group.sort_by(|a, b| b.seq.cmp(&a.seq));
            out.extend(group);
        }
        out.truncate(self.max_visible);
        out
    }

    /// Görünmeyen kayıt sayısı.
    pub fn overflow_count(&self) -> usize {
        self.ops.len().saturating_sub(self.visible().len())
    }

    /// Verilen önekle başlayan kimlikleri siler.
    pub fn remove_prefix(&mut self, prefix: &str) {
        self.ops.retain(|op| !op.id.starts_with(prefix));
    }

    /// Verilen önekle başlayanlardan yalnızca `ids` içindekileri korur.
    pub fn retain_prefix_ids(&mut self, prefix: &str, ids: &HashSet<String>) {
        self.ops
            .retain(|op| !op.id.starts_with(prefix) || ids.contains(&op.id));
    }

    fn fresh_seq(&mut self) -> u64 {
        self.next_seq += 1;
        self.next_seq
    }

    fn push_notice(
        &mut self,
        id: String,
        title: &str,
        detail: &str,
        state: OperationState,
        seq: u64,
        ttl: Duration,
    ) {
        let now = Instant::now();
        self.ops.insert(
            0,
            Operation {
                id,
                title: title.to_string(),
                detail: detail.to_string(),
                state,
                progress: None,
                started_at: now,
                expires_at: Some(now + ttl),
                seq,
            },
        );
        self.enforce_limit();
    }

    /// Toplam kayıt sayısını sınırlar; yalnızca en eski Done/Error kayıtları
    /// atılır, Running kayıtlar asla silinmez.
    fn enforce_limit(&mut self) {
        while self.ops.len() > MAX_RECORDS {
            let victim = self
                .ops
                .iter()
                .enumerate()
                .filter(|(_, op)| op.state != OperationState::Running)
                .min_by_key(|(_, op)| op.seq)
                .map(|(index, _)| index);
            let Some(index) = victim else {
                break;
            };
            self.ops.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn queue_shows_at_most_three_operations() {
        let mut q = OperationQueue::new(3);
        for i in 0..4 {
            q.begin(&format!("op{i}"), "Başlık", "Ayrıntı");
        }
        assert_eq!(q.visible().len(), 3);
        assert_eq!(q.overflow_count(), 1);
    }

    #[test]
    fn queue_updates_same_id_in_place() {
        let mut q = OperationQueue::new(3);
        q.begin("dl:1", "İndiriliyor", "A");
        q.update("dl:1", "B", Some(0.5));
        assert_eq!(q.visible().len(), 1);
        assert_eq!(q.visible()[0].detail, "B");
        assert_eq!(q.visible()[0].progress, Some(0.5));
    }

    #[test]
    fn queue_keeps_running_ops_when_info_arrives() {
        let mut q = OperationQueue::new(3);
        q.begin("a", "A", "");
        q.begin("b", "B", "");
        q.begin("c", "C", "");
        q.info("Bilgi", "", Duration::from_secs(4));
        assert!(q
            .visible()
            .iter()
            .all(|o| o.state == OperationState::Running));
    }

    #[test]
    fn queue_expires_finished_ops() {
        let mut q = OperationQueue::new(3);
        q.begin("a", "A", "");
        q.finish("a", OperationState::Done, Duration::from_secs(1));
        q.tick(Instant::now() + Duration::from_secs(2));
        assert!(q.visible().is_empty());
    }

    #[test]
    fn queue_removes_download_prefix() {
        let mut q = OperationQueue::new(3);
        q.begin("dl:1", "A", "");
        q.begin("dl:2", "B", "");
        q.remove_prefix("dl:");
        assert!(q.visible().is_empty());
    }
}
