//! İşlem yığını: ekranın alt ortasında gösterilen en fazla üç satırlık işlem
//! kuyruğunun modeli ve GTK widget'ı.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;

use crate::download::{DownloadRecord, DownloadStatus};
use crate::ui::downloads_view::DownloadsView;

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
            op.progress = None;
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

    /// Kaydın güncel durumunu döndürür; kayıt yoksa `None`.
    pub fn state_of(&self, id: &str) -> Option<OperationState> {
        self.ops.iter().find(|op| op.id == id).map(|op| op.state)
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

/// İndirme kaydını işlem yığını satırına eşler:
/// `(başlık, ayrıntı, durum, ilerleme)`.
pub(crate) fn download_operation(
    rec: &DownloadRecord,
) -> (String, String, OperationState, Option<f64>) {
    let detail = DownloadsView::row_title(rec);
    let progress = if rec.total > 0 {
        Some(DownloadsView::row_state(rec).0.clamp(0.0, 1.0))
    } else {
        None
    };
    match &rec.status {
        DownloadStatus::Queued => (
            "Sırada".to_string(),
            detail,
            OperationState::Running,
            progress,
        ),
        DownloadStatus::Downloading => (
            "İndiriliyor".to_string(),
            detail,
            OperationState::Running,
            progress,
        ),
        DownloadStatus::Paused => (
            "Duraklatıldı".to_string(),
            detail,
            OperationState::Running,
            progress,
        ),
        DownloadStatus::Done => (
            "Tamamlandı".to_string(),
            detail,
            OperationState::Done,
            Some(1.0),
        ),
        DownloadStatus::Error(err) => (
            "Hata".to_string(),
            format!("{detail} · {err}"),
            OperationState::Error,
            Some(0.0),
        ),
    }
}

struct RowWidgets {
    container: gtk::Box,
    spinner: gtk::Spinner,
    status: gtk::Label,
    title: gtk::Label,
    detail: gtk::Label,
    progress: gtk::ProgressBar,
}

struct Inner {
    revealer: gtk::Revealer,
    rows_box: gtk::Box,
    overflow: gtk::Label,
    queue: RefCell<OperationQueue>,
    rows: RefCell<HashMap<String, RowWidgets>>,
    /// Terminal durumu bir kez duyurulan indirme kimlikleri; TTL sonrası
    /// aynı kayıt yeniden satır açmasın diye `tick` boyunca korunur.
    announced: RefCell<HashSet<String>>,
    /// İlk senkron tamamlandı mı? Geçmiş terminal kayıtlar açılışta duyurulmaz.
    seeded: Cell<bool>,
}

/// Alt ortada beliren, en fazla üç satırlık işlem yığını widget'ı.
#[derive(Clone)]
pub struct OperationStack {
    inner: Rc<Inner>,
}

impl OperationStack {
    pub fn new() -> Self {
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        revealer.set_transition_duration(180);
        revealer.set_halign(gtk::Align::Center);
        revealer.set_valign(gtk::Align::End);
        revealer.set_margin_bottom(24);
        revealer.set_margin_start(16);
        revealer.set_margin_end(16);
        revealer.set_can_target(false);
        revealer.set_reveal_child(false);

        let stack = gtk::Box::new(gtk::Orientation::Vertical, 6);
        stack.add_css_class("op-stack");
        let rows_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let overflow = gtk::Label::new(None);
        overflow.add_css_class("op-overflow");
        overflow.set_halign(gtk::Align::Start);
        overflow.set_visible(false);
        stack.append(&rows_box);
        stack.append(&overflow);
        revealer.set_child(Some(&stack));

        Self {
            inner: Rc::new(Inner {
                revealer,
                rows_box,
                overflow,
                queue: RefCell::new(OperationQueue::new(3)),
                rows: RefCell::new(HashMap::new()),
                announced: RefCell::new(HashSet::new()),
                seeded: Cell::new(false),
            }),
        }
    }

    pub fn widget(&self) -> &gtk::Revealer {
        &self.inner.revealer
    }

    pub fn begin(&self, id: &str, title: &str, detail: &str) {
        self.inner.queue.borrow_mut().begin(id, title, detail);
        self.render();
    }

    pub fn update(&self, id: &str, detail: &str, progress: Option<f64>) {
        self.inner.queue.borrow_mut().update(id, detail, progress);
        self.render();
    }

    pub fn finish(&self, id: &str, state: OperationState, ttl: Duration) {
        self.inner.queue.borrow_mut().finish(id, state, ttl);
        self.render();
    }

    pub fn info(&self, title: &str, detail: &str, ttl: Duration) {
        self.inner.queue.borrow_mut().info(title, detail, ttl);
        self.render();
    }

    pub fn error(&self, title: &str, detail: &str, ttl: Duration) {
        self.inner.queue.borrow_mut().error(title, detail, ttl);
        self.render();
    }

    pub fn tick(&self) {
        self.tick_with(Instant::now());
    }

    /// Testlerde TTL'yi beklemeden süresi dolmuş satırları temizler.
    #[cfg(test)]
    pub fn tick_at(&self, now: Instant) {
        self.tick_with(now);
    }

    fn tick_with(&self, now: Instant) {
        self.inner.queue.borrow_mut().tick(now);
        self.render();
    }

    pub fn sync_downloads(&self, records: &[DownloadRecord]) {
        let ids: HashSet<String> = records.iter().map(|rec| format!("dl:{}", rec.id)).collect();
        {
            let mut announced = self.inner.announced.borrow_mut();
            announced.retain(|id| ids.contains(id));
            if !self.inner.seeded.get() {
                // Önceki oturumdan kalan terminal indirmeler açılışta satır
                // açmasın; yalnızca bu oturumdaki geçişler duyurulur.
                for rec in records {
                    if matches!(rec.status, DownloadStatus::Done | DownloadStatus::Error(_)) {
                        announced.insert(format!("dl:{}", rec.id));
                    }
                }
                self.inner.seeded.set(true);
            }
            let mut queue = self.inner.queue.borrow_mut();
            queue.retain_prefix_ids("dl:", &ids);
            for rec in records {
                let id = format!("dl:{}", rec.id);
                let (title, detail, state, progress) = download_operation(rec);
                match state {
                    OperationState::Running => {
                        queue.begin(&id, &title, &detail);
                        queue.update(&id, &detail, progress);
                        announced.remove(&id);
                    }
                    OperationState::Done | OperationState::Error => {
                        // Terminal satır bir kez duyurulur: TTL dolup satır
                        // silinse bile sonraki pump'ta yeniden açılmaz.
                        if matches!(
                            queue.state_of(&id),
                            Some(OperationState::Done | OperationState::Error)
                        ) || announced.contains(&id)
                        {
                            continue;
                        }
                        let ttl = if state == OperationState::Error {
                            Duration::from_secs(6)
                        } else {
                            Duration::from_secs(4)
                        };
                        queue.begin(&id, &title, &detail);
                        queue.update(&id, &detail, progress);
                        queue.finish(&id, state, ttl);
                        announced.insert(id);
                    }
                }
            }
        }
        self.render();
    }

    /// Testlerde görünür satır sayısını doğrulamak için.
    #[cfg(test)]
    pub fn row_count(&self) -> usize {
        self.inner.rows.borrow().len()
    }

    fn render(&self) {
        // Kuyruk ödünç alınırken GTK çağrısı yapmamak için önce kopyala.
        let (visible, overflow) = {
            let queue = self.inner.queue.borrow();
            let visible: Vec<Operation> = queue.visible().into_iter().cloned().collect();
            (visible, queue.overflow_count())
        };

        self.inner.overflow.set_text(&format!("+{overflow}"));
        self.inner.overflow.set_visible(overflow > 0);

        let visible_ids: HashSet<String> = visible.iter().map(|op| op.id.clone()).collect();
        {
            let mut rows = self.inner.rows.borrow_mut();
            let stale: Vec<String> = rows
                .keys()
                .filter(|id| !visible_ids.contains(*id))
                .cloned()
                .collect();
            for id in stale {
                if let Some(row) = rows.remove(&id) {
                    self.inner.rows_box.remove(&row.container);
                }
            }
        }

        let mut ordered: Vec<gtk::Box> = Vec::with_capacity(visible.len());
        for op in &visible {
            let container = {
                let mut rows = self.inner.rows.borrow_mut();
                let row = rows
                    .entry(op.id.clone())
                    .or_insert_with(|| self.create_row());
                Self::apply(row, op);
                row.container.clone()
            };
            ordered.push(container);
        }

        let mut previous: Option<gtk::Widget> = None;
        for container in ordered {
            self.inner
                .rows_box
                .reorder_child_after(&container, previous.as_ref());
            previous = Some(container.upcast());
        }

        self.inner.revealer.set_reveal_child(!visible.is_empty());
    }

    fn create_row(&self) -> RowWidgets {
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        container.add_css_class("op-row");
        container.set_hexpand(true);

        let spinner = gtk::Spinner::new();
        spinner.set_valign(gtk::Align::Center);
        spinner.set_visible(false);

        let status = gtk::Label::new(None);
        status.set_valign(gtk::Align::Center);
        status.set_visible(false);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);

        let title = Self::text_label("op-title");
        let detail = Self::text_label("op-detail");

        let progress = gtk::ProgressBar::new();
        progress.add_css_class("op-progress");
        progress.set_visible(false);

        text.append(&title);
        text.append(&detail);
        text.append(&progress);
        container.append(&spinner);
        container.append(&status);
        container.append(&text);
        self.inner.rows_box.append(&container);

        RowWidgets {
            container,
            spinner,
            status,
            title,
            detail,
            progress,
        }
    }

    fn text_label(class: &str) -> gtk::Label {
        let label = gtk::Label::new(None);
        label.add_css_class(class);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_single_line_mode(true);
        label.set_wrap(false);
        label
    }

    fn apply(row: &RowWidgets, op: &Operation) {
        row.title.set_text(&op.title);
        row.detail.set_text(&op.detail);
        row.container.remove_css_class("op-done");
        row.container.remove_css_class("op-error");
        match op.state {
            OperationState::Running => {
                row.spinner.set_visible(true);
                row.spinner.start();
                row.status.set_visible(false);
            }
            OperationState::Done => {
                row.spinner.stop();
                row.spinner.set_visible(false);
                row.status.set_text("✓");
                row.status.set_visible(true);
                row.container.add_css_class("op-done");
            }
            OperationState::Error => {
                row.spinner.stop();
                row.spinner.set_visible(false);
                row.status.set_text("!");
                row.status.set_visible(true);
                row.container.add_css_class("op-error");
            }
        }
        match op.progress {
            Some(progress) => {
                row.progress.set_fraction(progress.clamp(0.0, 1.0));
                row.progress.set_visible(true);
            }
            None => row.progress.set_visible(false),
        }
    }
}

impl Default for OperationStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::{DownloadRecord, DownloadStatus};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn record(status: DownloadStatus) -> DownloadRecord {
        DownloadRecord {
            id: "abc".into(),
            title: "Anime".into(),
            season: 1,
            episode: 1,
            ep_name: "Ad".into(),
            fansub: "Fansub".into(),
            quality: "1080p".into(),
            url: "https://example.com/video.mp4".into(),
            referer: None,
            dest: PathBuf::from("/tmp/anime.mp4"),
            total: 10,
            have: 5,
            status,
        }
    }

    #[gtk::test]
    fn stack_renders_at_most_three_rows() {
        let stack = OperationStack::new();
        for i in 0..4 {
            stack.begin(&format!("op{i}"), "Başlık", "Ayrıntı");
        }
        assert_eq!(stack.row_count(), 3);
    }

    #[test]
    fn download_operation_maps_status() {
        let downloading = record(DownloadStatus::Downloading);
        let (title, detail, state, progress) = download_operation(&downloading);
        assert_eq!(title, "İndiriliyor");
        assert_eq!(state, OperationState::Running);
        assert_eq!(progress, Some(0.5));
        assert!(detail.contains("S01E01"));

        let done = record(DownloadStatus::Done);
        let (title, _, state, progress) = download_operation(&done);
        assert_eq!(title, "Tamamlandı");
        assert_eq!(state, OperationState::Done);
        assert_eq!(progress, Some(1.0));

        let queued = record(DownloadStatus::Queued);
        let (title, _, state, progress) = download_operation(&queued);
        assert_eq!(title, "Sırada");
        assert_eq!(state, OperationState::Running);
        assert_eq!(progress, Some(0.5));

        let paused = record(DownloadStatus::Paused);
        let (title, _, state, progress) = download_operation(&paused);
        assert_eq!(title, "Duraklatıldı");
        assert_eq!(state, OperationState::Running);
        assert_eq!(progress, Some(0.5));

        let error = record(DownloadStatus::Error("bağlantı koptu".into()));
        let (title, detail, state, progress) = download_operation(&error);
        assert_eq!(title, "Hata");
        assert_eq!(state, OperationState::Error);
        assert_eq!(progress, Some(0.0));
        assert!(detail.contains("bağlantı koptu"));

        let mut unknown_total = record(DownloadStatus::Downloading);
        unknown_total.total = 0;
        unknown_total.have = 0;
        let (title, _, state, progress) = download_operation(&unknown_total);
        assert_eq!(title, "İndiriliyor");
        assert_eq!(state, OperationState::Running);
        assert_eq!(progress, None);
    }

    #[gtk::test]
    fn stack_does_not_retoast_expired_terminal() {
        let stack = OperationStack::new();
        let mut rec = record(DownloadStatus::Downloading);
        stack.sync_downloads(&[rec.clone()]);
        assert_eq!(stack.row_count(), 1);

        rec.status = DownloadStatus::Done;
        stack.sync_downloads(&[rec.clone()]);
        assert_eq!(stack.row_count(), 1);

        stack.tick_at(Instant::now() + Duration::from_secs(5));
        assert_eq!(stack.row_count(), 0);

        stack.sync_downloads(&[rec]);
        assert_eq!(stack.row_count(), 0);
    }

    #[gtk::test]
    fn stack_first_sync_skips_historical_terminal() {
        let stack = OperationStack::new();
        let done = record(DownloadStatus::Done);
        stack.sync_downloads(&[done]);
        assert_eq!(stack.row_count(), 0);

        let downloading = record(DownloadStatus::Downloading);
        stack.sync_downloads(&[downloading]);
        assert_eq!(stack.row_count(), 1);
    }

    #[test]
    fn queue_begin_restart_clears_progress() {
        let mut q = OperationQueue::new(3);
        q.begin("a", "A", "");
        q.update("a", "A", Some(0.4));
        q.finish("a", OperationState::Done, Duration::from_secs(4));
        q.begin("a", "A", "");
        assert_eq!(q.visible()[0].progress, None);
    }

    #[test]
    fn queue_state_of_reports_state() {
        let mut q = OperationQueue::new(3);
        q.begin("a", "A", "");
        assert_eq!(q.state_of("a"), Some(OperationState::Running));
        q.finish("a", OperationState::Done, Duration::from_secs(4));
        assert_eq!(q.state_of("a"), Some(OperationState::Done));
    }

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
