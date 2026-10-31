//! Duyarlı yerleşim ölçüleri ve fullscreen/pencere boyutu izleyicisi.
//!
//! `app.rs` yalnızca `install`/`refresh` çağırır. Kart genişliği, sütun
//! sayısı, hero yüksekliği, fullscreen ölçeklemesi ve rebuild kararı burada
//! yaşar. Sidebar/header fullscreen'de de görünür kalır; ölçü yalnızca
//! sidebar'ın sağındaki içerik alanını doldurur.

use crate::app::{App, Page};
use adw::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

pub const CONTENT_MAX: i32 = 2400;
pub const MIN_CARD_W: i32 = 140;
pub const MAX_CARD_W: i32 = 320;
pub const MIN_COLS: u32 = 2;
pub const MAX_COLS: u32 = 8;
const GAP: i32 = 16;
const TARGET_CARD_W: f32 = 180.0;
const FULLSCREEN_SCALE: f32 = 1.08;
const MAX_HERO_H: i32 = 600;
const SIDEBAR_EXPANDED: i32 = 177;
const SIDEBAR_COLLAPSED: i32 = 65;
const MAIN_MARGINS: i32 = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutMetrics {
    pub hero_h: i32,
    pub grid_cols: u32,
    pub card_w: i32,
    pub fullscreen: bool,
}

pub fn initial_metrics() -> LayoutMetrics {
    compute(1280, 800, SIDEBAR_EXPANDED, false)
}

pub fn compute(window_w: i32, window_h: i32, sidebar_w: i32, fullscreen: bool) -> LayoutMetrics {
    let content_w = (window_w - sidebar_w - MAIN_MARGINS).clamp(300, CONTENT_MAX);
    let base_cols = (((content_w + GAP) as f32) / (TARGET_CARD_W + GAP as f32))
        .round()
        .clamp(MIN_COLS as f32, MAX_COLS as f32) as u32;
    let base_card = card_width(content_w, base_cols);
    let scale = if fullscreen { FULLSCREEN_SCALE } else { 1.0 };
    let target = ((base_card as f32 * scale) as i32).clamp(MIN_CARD_W, MAX_CARD_W);
    let cols = ((content_w + GAP) / (target + GAP)).clamp(MIN_COLS as i32, MAX_COLS as i32) as u32;
    let card_w = card_width(content_w, cols).clamp(MIN_CARD_W, MAX_CARD_W);
    LayoutMetrics {
        hero_h: hero_h_for_window(window_h, cols, fullscreen),
        grid_cols: cols,
        card_w,
        fullscreen,
    }
}

fn card_width(content_w: i32, cols: u32) -> i32 {
    let raw = (content_w - (cols as i32 - 1) * GAP) / cols.max(1) as i32;
    (raw / 2 * 2).clamp(MIN_CARD_W, MAX_CARD_W)
}

fn hero_h_for_window(win_h: i32, cols: u32, fullscreen: bool) -> i32 {
    let from_h = (win_h as f32 * 0.52) as i32 / 20 * 20;
    let from_c = 260 + cols.clamp(MIN_COLS, MAX_COLS) as i32 * 20;
    let base = from_h.max(from_c).clamp(360, 560);
    if fullscreen {
        ((base as f32 * FULLSCREEN_SCALE) as i32 / 20 * 20).clamp(360, MAX_HERO_H)
    } else {
        base
    }
}

pub fn install(app: &Rc<App>) {
    let last = Rc::new(Cell::new(initial_metrics()));
    let generation = Rc::new(Cell::new(0u64));
    let pending_size = Rc::new(Cell::new(None::<(i32, i32)>));
    let schedule: Rc<dyn Fn(Option<(i32, i32)>)> = {
        let app = app.clone();
        let last = last.clone();
        let generation = generation.clone();
        let pending_size = pending_size.clone();
        Rc::new(move |size| {
            if let Some(size) = size {
                pending_size.set(Some(size));
            }
            let my = generation.get() + 1;
            generation.set(my);
            let app_c = app.clone();
            let last_c = last.clone();
            let generation_c = generation.clone();
            let pending_c = pending_size.clone();
            glib::timeout_add_local_once(Duration::from_millis(90), move || {
                if generation_c.get() != my {
                    return;
                }
                apply(&app_c, &last_c, pending_c.take());
            });
        })
    };

    {
        let schedule = schedule.clone();
        app.window
            .connect_fullscreened_notify(move |_| schedule(None));
    }
    {
        let schedule = schedule.clone();
        app.window.connect_maximized_notify(move |_| schedule(None));
    }
    {
        let connected = Rc::new(Cell::new(false));
        let schedule = schedule.clone();
        app.window.connect_map(move |w| {
            if connected.get() {
                return;
            }
            let Some(surface) = w.surface() else {
                schedule(None);
                return;
            };
            connected.set(true);
            let schedule_c = schedule.clone();
            surface.connect_layout(move |_, ww, h| schedule_c(Some((ww, h))));
            schedule(Some((w.width(), w.height())));
        });
    }
    schedule(None);
}

/// Sidebar/UI scale değişimlerinde anında metrikleri tazeler.
pub fn refresh(app: &App) {
    if !app.window.is_mapped() {
        return;
    }
    let next = current_metrics(app);
    if next.hero_h == app.hero_h.get()
        && next.card_w == app.card_w.get()
        && next.grid_cols == app.grid_cols.get()
    {
        return;
    }
    apply_metrics(app, next);
}

fn current_metrics(app: &App) -> LayoutMetrics {
    let window_w = app.window.width().max(1);
    let window_h = app.window.height().max(1);
    let sidebar_w = if app.sidebar_collapsed.get() {
        SIDEBAR_COLLAPSED
    } else {
        SIDEBAR_EXPANDED
    };
    compute(window_w, window_h, sidebar_w, app.window.is_fullscreen())
}

fn apply(app: &Rc<App>, last: &Cell<LayoutMetrics>, size: Option<(i32, i32)>) {
    if !app.window.is_mapped() {
        return;
    }
    let next = match size {
        Some((w, h)) if w > 1 && h > 1 => {
            let sidebar_w = if app.sidebar_collapsed.get() {
                SIDEBAR_COLLAPSED
            } else {
                SIDEBAR_EXPANDED
            };
            compute(w, h, sidebar_w, app.window.is_fullscreen())
        }
        _ => current_metrics(app),
    };
    if next == last.get() {
        return;
    }
    last.set(next);
    apply_metrics(app, next);
}

fn apply_metrics(app: &App, next: LayoutMetrics) {
    app.hero_h.set(next.hero_h);
    app.card_w.set(next.card_w);
    app.grid_cols.set(next.grid_cols);
    app.home_dirty.set(true);

    match app.stack.visible_child_name().as_deref() {
        Some("home") => app.show_page(&Page::Home),
        Some("kesfet") => app.show_page(&Page::Kesfet),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(w: i32, h: i32, fs: bool) -> LayoutMetrics {
        compute(w, h, SIDEBAR_EXPANDED, fs)
    }

    #[test]
    fn exact_standard_widths() {
        assert_eq!(m(1280, 800, false).grid_cols, 6);
        assert_eq!(m(1280, 800, false).card_w, 166);
        assert_eq!(m(1440, 900, false).grid_cols, 6);
        assert_eq!(m(1440, 900, false).card_w, 192);
        assert_eq!(m(1920, 1080, false).grid_cols, 8);
        assert_eq!(m(1920, 1080, false).card_w, 200);
        assert_eq!(m(1920, 1080, true).grid_cols, 7);
        assert_eq!(m(1920, 1080, true).card_w, 230);
        assert_eq!(m(2560, 1440, false).card_w, 280);
        assert_eq!(m(2560, 1440, true).card_w, 320);
    }

    #[test]
    fn windowed_to_fullscreen_grows() {
        let windowed = m(1280, 800, false);
        let full = m(1920, 1080, true);
        assert!(full.card_w > windowed.card_w);
        assert!(full.hero_h > windowed.hero_h);
    }

    #[test]
    fn maximized_to_fullscreen_still_grows() {
        let maximized = m(1920, 1080, false);
        let full = m(1920, 1080, true);
        assert!(full.card_w > maximized.card_w);
        assert!(full.hero_h > maximized.hero_h);
    }

    #[test]
    fn same_size_and_state_is_stable() {
        assert_eq!(m(1920, 1080, false), m(1920, 1080, false));
        assert_eq!(m(1920, 1080, true), m(1920, 1080, true));
    }

    #[test]
    fn nearby_widths_do_not_shrink_cards() {
        let a = m(1439, 900, false);
        let b = m(1440, 900, false);
        assert!(b.card_w >= a.card_w);
    }

    #[test]
    fn computed_columns_fit_inner_width() {
        for w in [1280, 1366, 1440, 1600, 1920, 2560] {
            for fs in [false, true] {
                let metrics = m(w, 1080, fs);
                let content = (w - SIDEBAR_EXPANDED - MAIN_MARGINS).clamp(300, CONTENT_MAX);
                let needed = metrics.grid_cols as i32 * metrics.card_w
                    + (metrics.grid_cols as i32 - 1) * GAP;
                assert!(
                    needed <= content,
                    "{w} fs={fs}: {} cols x {} = {} > {}",
                    metrics.grid_cols,
                    metrics.card_w,
                    needed,
                    content
                );
            }
        }
    }

    #[test]
    fn small_window_keeps_floor() {
        let metrics = m(500, 600, false);
        assert!(metrics.card_w >= MIN_CARD_W);
        assert_eq!(metrics.grid_cols, MIN_COLS);
        assert!(metrics.hero_h >= 360);
    }

    #[test]
    fn fullscreen_is_part_of_metrics() {
        assert!(m(1920, 1080, true).fullscreen);
        assert!(!m(1920, 1080, false).fullscreen);
    }

    #[test]
    fn hero_height_table() {
        assert_eq!(hero_h_for_window(885, 8, false), 460);
        assert_eq!(hero_h_for_window(800, 6, false), 400);
        assert_eq!(hero_h_for_window(680, 3, false), 360);
        assert_eq!(hero_h_for_window(2000, 8, false), 560);
        assert!(hero_h_for_window(2000, 8, true) > 560);
    }
}
