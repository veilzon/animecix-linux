//! Duyarlı yerleşim ölçüleri ve fullscreen/pencere boyutu izleyicisi.
//!
//! `app.rs` yalnızca `install` çağırır. Kart genişliği, sütun sayısı, hero
//! yüksekliği, tam ekran ölçeklemesi ve rebuild kararı burada yaşar.

use crate::app::{App, Page};
use adw::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

pub const CONTENT_MAX: i32 = 1600;
pub const MIN_CARD_W: i32 = 140;
pub const MAX_CARD_W: i32 = 280;
pub const MIN_COLS: u32 = 3;
pub const MAX_COLS: u32 = 8;
const GAP: i32 = 16;
const FULLSCREEN_SCALE: f32 = 1.08;
const MAX_HERO_H: i32 = 600;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutMetrics {
    pub hero_h: i32,
    pub grid_cols: u32,
    pub card_w: i32,
    pub fullscreen: bool,
}

pub fn initial_metrics() -> LayoutMetrics {
    compute(1280, 800, 164, false)
}

pub fn compute(window_w: i32, window_h: i32, sidebar_w: i32, fullscreen: bool) -> LayoutMetrics {
    let content_w = (window_w - sidebar_w - 1 - 24).clamp(300, CONTENT_MAX);
    let base_cols = (content_w / 180).clamp(MIN_COLS as i32, MAX_COLS as i32) as u32;
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
    (raw / 10 * 10).clamp(MIN_CARD_W, MAX_CARD_W)
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
    let last = Rc::new(Cell::new(current_metrics(app)));
    let generation = Rc::new(Cell::new(0u64));
    let schedule: Rc<dyn Fn()> = {
        let app = app.clone();
        let last = last.clone();
        let generation = generation.clone();
        Rc::new(move || {
            let my = generation.get() + 1;
            generation.set(my);
            let app_c = app.clone();
            let last_c = last.clone();
            let generation_c = generation.clone();
            glib::timeout_add_local_once(Duration::from_millis(90), move || {
                if generation_c.get() != my {
                    return;
                }
                apply(&app_c, &last_c);
            });
        })
    };

    {
        let schedule = schedule.clone();
        app.window.connect_fullscreened_notify(move |_| schedule());
    }
    {
        let schedule = schedule.clone();
        app.window.connect_maximized_notify(move |_| schedule());
    }
    {
        let connected = Rc::new(Cell::new(false));
        let schedule = schedule.clone();
        app.window.connect_map(move |w| {
            if connected.replace(true) {
                return;
            }
            let Some(surface) = w.surface() else {
                connected.set(false);
                return;
            };
            let schedule = schedule.clone();
            surface.connect_layout(move |_, _, _| schedule());
        });
    }
    schedule();
}

fn current_metrics(app: &App) -> LayoutMetrics {
    let window_w = app.window.width().max(1);
    let window_h = app.window.height().max(1);
    let sidebar_w = if app.sidebar_collapsed.get() { 58 } else { 164 };
    compute(window_w, window_h, sidebar_w, app.window.is_fullscreen())
}

fn apply(app: &Rc<App>, last: &Cell<LayoutMetrics>) {
    let next = current_metrics(app);
    if next == last.get() {
        return;
    }
    last.set(next);
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

    #[test]
    fn windowed_to_fullscreen_grows() {
        let windowed = compute(1280, 800, 164, false);
        let full = compute(1920, 1080, 164, true);
        assert!(full.card_w > windowed.card_w);
        assert!(full.hero_h > windowed.hero_h);
    }

    #[test]
    fn maximized_to_fullscreen_still_grows() {
        let maximized = compute(1920, 1080, 164, false);
        let full = compute(1920, 1080, 164, true);
        assert!(full.card_w > maximized.card_w);
        assert!(full.hero_h > maximized.hero_h);
    }

    #[test]
    fn same_size_and_state_is_stable() {
        assert_eq!(
            compute(1920, 1080, 164, false),
            compute(1920, 1080, 164, false)
        );
        assert_eq!(
            compute(1920, 1080, 164, true),
            compute(1920, 1080, 164, true)
        );
    }

    #[test]
    fn nearby_widths_do_not_shrink_cards() {
        let a = compute(1439, 900, 164, false);
        let b = compute(1440, 900, 164, false);
        assert!(b.card_w >= a.card_w);
    }

    #[test]
    fn small_window_keeps_floor() {
        let m = compute(500, 600, 164, false);
        assert_eq!(m.card_w, MIN_CARD_W);
        assert_eq!(m.grid_cols, MIN_COLS);
        assert!(m.hero_h >= 360);
    }

    #[test]
    fn fullscreen_is_part_of_metrics() {
        assert!(compute(1920, 1080, 164, true).fullscreen);
        assert!(!compute(1920, 1080, 164, false).fullscreen);
    }

    #[test]
    fn hero_height_table() {
        assert_eq!(hero_h_for_window(885, 8, false), 460);
        assert_eq!(hero_h_for_window(800, 5, false), 400);
        assert_eq!(hero_h_for_window(680, 3, false), 360);
        assert_eq!(hero_h_for_window(2000, 8, false), 560);
        assert!(hero_h_for_window(2000, 8, true) > 560);
    }
}
