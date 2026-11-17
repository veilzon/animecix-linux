//! Circular reveal container used for cover → detail page transitions.
//!
//! The container snapshots the old page first, then clips the new page to a
//! growing circle. It owns both widgets only for the duration of the
//! animation; the navigation layer re-parents the new page when the animation
//! finishes.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct CircularReveal {
        pub old: RefCell<Option<gtk::Widget>>,
        pub new: RefCell<Option<gtk::Widget>>,
        pub progress: Cell<f64>,
        pub center_x: Cell<f64>,
        pub center_y: Cell<f64>,
        pub done: RefCell<Option<Box<dyn FnOnce()>>>,
        pub animation: RefCell<Option<adw::TimedAnimation>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CircularReveal {
        const NAME: &'static str = "AnimecixCircularReveal";
        type Type = super::CircularReveal;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for CircularReveal {}

    impl WidgetImpl for CircularReveal {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let mut min = 0;
            let mut natural = 0;
            for child in [self.old.borrow().clone(), self.new.borrow().clone()]
                .into_iter()
                .flatten()
            {
                let (m, n, _, _) = child.measure(orientation, for_size);
                min = min.max(m);
                natural = natural.max(n);
            }
            (min, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            for child in [self.old.borrow().clone(), self.new.borrow().clone()]
                .into_iter()
                .flatten()
            {
                child.allocate(width, height, baseline, None);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            if let Some(old) = self.old.borrow().as_ref() {
                widget.snapshot_child(old, snapshot);
            }
            let Some(new) = self.new.borrow().clone() else {
                return;
            };
            let progress = self.progress.get().clamp(0.0, 1.0);
            if progress <= 0.0 {
                return;
            }
            if progress >= 1.0 {
                widget.snapshot_child(&new, snapshot);
                return;
            }
            let width = widget.width() as f32;
            let height = widget.height() as f32;
            let cx = self.center_x.get() as f32;
            let cy = self.center_y.get() as f32;
            let max_radius = width.hypot(height).max(1.0);
            let radius = (max_radius * progress as f32).max(1.0);
            let rect =
                gtk::graphene::Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
            let clip = gtk::gsk::RoundedRect::from_rect(rect, radius);
            snapshot.push_rounded_clip(&clip);
            widget.snapshot_child(&new, snapshot);
            snapshot.pop();
        }

        fn realize(&self) {
            self.parent_realize();
            for child in [self.old.borrow().clone(), self.new.borrow().clone()]
                .into_iter()
                .flatten()
            {
                child.realize();
            }
        }

        fn unrealize(&self) {
            for child in [self.old.borrow().clone(), self.new.borrow().clone()]
                .into_iter()
                .flatten()
            {
                child.unrealize();
            }
            self.parent_unrealize();
        }

        fn map(&self) {
            self.parent_map();
            for child in [self.old.borrow().clone(), self.new.borrow().clone()]
                .into_iter()
                .flatten()
            {
                child.map();
            }
        }

        fn unmap(&self) {
            for child in [self.old.borrow().clone(), self.new.borrow().clone()]
                .into_iter()
                .flatten()
            {
                child.unmap();
            }
            self.parent_unmap();
        }
    }
}

gtk::glib::wrapper! {
    pub struct CircularReveal(ObjectSubclass<imp::CircularReveal>)
        @extends gtk::Widget;
}

impl CircularReveal {
    /// `old` must no longer have a parent. `new` is also reparented here.
    pub fn new(old: gtk::Widget, new: gtk::Widget) -> Self {
        let this: Self = gtk::glib::Object::new();
        old.set_parent(&this);
        new.set_parent(&this);
        old.set_sensitive(false);
        new.set_sensitive(false);
        *this.imp().old.borrow_mut() = Some(old);
        *this.imp().new.borrow_mut() = Some(new);
        this
    }

    pub fn start(&self, x: f64, y: f64, done: impl FnOnce() + 'static) {
        let imp = self.imp();
        imp.center_x.set(x);
        imp.center_y.set(y);
        imp.progress.set(0.0);
        *imp.done.borrow_mut() = Some(Box::new(done));

        let weak = self.downgrade();
        let target = adw::CallbackAnimationTarget::new(move |value| {
            if let Some(this) = weak.upgrade() {
                this.imp().progress.set(value);
                this.queue_draw();
            }
        });
        let animation = adw::TimedAnimation::new(self, 0.0, 1.0, 380, target);
        animation.set_easing(adw::Easing::EaseOutCubic);
        let weak_done = self.downgrade();
        animation.connect_done(move |_| {
            if let Some(this) = weak_done.upgrade() {
                if let Some(done) = this.imp().done.borrow_mut().take() {
                    done();
                }
            }
        });
        *imp.animation.borrow_mut() = Some(animation.clone());
        animation.play();
    }

    /// Detaches the animated children and returns the new page. The old page
    /// is dropped so its resources can be released.
    pub fn take_new(&self) -> Option<gtk::Widget> {
        let old = self.imp().old.borrow_mut().take();
        let new = self.imp().new.borrow_mut().take();
        if let Some(old) = old {
            old.unparent();
        }
        if let Some(new) = &new {
            new.unparent();
        }
        self.imp().animation.borrow_mut().take();
        new
    }
}
