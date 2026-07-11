// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{glib, graphene, prelude::*, subclass::prelude::*, CompositeTemplate};
use crate::audio::Lyrics;
use std::cell::Cell;

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/io/bassi/Amberol/lyrics-view.ui")]
    pub struct LyricsView {
        #[template_child]
        pub overlay: TemplateChild<gtk::Overlay>,
        #[template_child]
        pub scrolled_window: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub lyrics_box: TemplateChild<gtk::Box>,
        pub lyrics: RefCell<Lyrics>,
        pub current_line: RefCell<Option<u32>>,
        pub last_position_ms: Cell<u64>,
        pub target_scroll: Cell<f64>,
        pub current_scroll: Cell<f64>,
        pub tick_id: RefCell<Option<gtk::TickCallbackId>>,
    }

    impl std::fmt::Debug for LyricsView {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("LyricsView").finish_non_exhaustive()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LyricsView {
        const NAME: &'static str = "AmberolLyricsView";
        type Type = super::LyricsView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for LyricsView {
        fn constructed(&self) {
            self.parent_constructed();
            
            let obj = self.obj();
            let adj = self.scrolled_window.vadjustment();
            adj.connect_changed(glib::clone!(
                #[weak(rename_to = view)]
                obj,
                move |_| {
                    view.scroll_to_active();
                }
            ));
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for LyricsView {
        fn measure(
            &self,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let (min, nat, min_base, nat_base) =
                self.overlay.measure(orientation, for_size);

            if orientation == gtk::Orientation::Vertical {
                // Cap the natural height so the lyrics panel doesn't swallow
                // the playbar and vinyl. The parent vexpand will still stretch
                // the widget to fill available space, but this prevents the
                // widget from *requesting* more than its share.
                let capped_nat = nat.min(420);
                let capped_min = min.min(180);
                (capped_min, capped_nat, min_base, nat_base)
            } else {
                (min, nat, min_base, nat_base)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.overlay.allocate(width, height, baseline, None);
        }
    }
}

glib::wrapper! {
    pub struct LyricsView(ObjectSubclass<imp::LyricsView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for LyricsView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl LyricsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_lyrics(&self, lyrics: Lyrics) {
        let imp = self.imp();
        
        // Reset scroll offsets
        imp.target_scroll.set(0.0);
        imp.current_scroll.set(0.0);
        let adj = imp.scrolled_window.vadjustment();
        adj.set_value(0.0);
        
        // Clear existing lines
        let mut child = imp.lyrics_box.first_child();
        while let Some(widget) = child {
            let next = widget.next_sibling();
            imp.lyrics_box.remove(&widget);
            child = next;
        }

        for line in &lyrics.lines {
            let label = gtk::Label::builder()
                .label(&line.text)
                .halign(gtk::Align::Center)
                .wrap(true)
                .justify(gtk::Justification::Center)
                .build();
            label.add_css_class("lyric-line");
            imp.lyrics_box.append(&label);
        }

        imp.lyrics.replace(lyrics);
        imp.current_line.replace(None);
    }

    pub fn scroll_to_active(&self) {
        let imp = self.imp();
        let best_line = *imp.current_line.borrow();
        if let Some(idx) = best_line {
            let mut i = 0;
            let mut child = imp.lyrics_box.first_child();
            while let Some(widget) = child {
                let next = widget.next_sibling();
                if i == idx {
                    if let Some(point) = widget.compute_point(
                        &*imp.lyrics_box,
                        &graphene::Point::zero(),
                    ) {
                        let widget_y = point.y() as f64;
                        let widget_h = widget.height() as f64;
                        let widget_center = widget_y + widget_h / 2.0;

                        let adj = imp.scrolled_window.vadjustment();
                        let viewport_h = adj.page_size();
                        
                        if viewport_h > 0.0 {
                            // Center target: align vertical center of the widget with vertical center of the viewport
                            let target = (widget_center - viewport_h / 2.0)
                                .max(0.0)
                                .min((adj.upper() - adj.page_size()).max(0.0));

                            imp.target_scroll.set(target);
                            if imp.tick_id.borrow().is_none() {
                                imp.current_scroll.set(adj.value());
                            }
                            self.animate_scroll();
                        }
                    }
                    break;
                }
                child = next;
                i += 1;
            }
        }
    }

    pub fn update_position(&self, position_ms: u64) {
        let imp = self.imp();
        let lyrics = imp.lyrics.borrow();
        let mut best_line = None;
        for (i, line) in lyrics.lines.iter().enumerate() {
            if line.time <= position_ms {
                best_line = Some(i as u32);
            } else {
                break;
            }
        }

        let last_pos = imp.last_position_ms.get();
        let is_seek = (position_ms as i64 - last_pos as i64).abs() > 1500;
        imp.last_position_ms.set(position_ms);

        if best_line != *imp.current_line.borrow() || is_seek {
            imp.current_line.replace(best_line);
            
            let mut i = 0;
            let mut child = imp.lyrics_box.first_child();
            while let Some(widget) = child {
                let next = widget.next_sibling();
                if let Some(idx) = best_line {
                    if i == idx {
                        widget.add_css_class("active-lyric");
                        widget.remove_css_class("near-active-lyric");
                    } else if i == idx.saturating_sub(1) || i == idx + 1 {
                        widget.remove_css_class("active-lyric");
                        widget.add_css_class("near-active-lyric");
                    } else {
                        widget.remove_css_class("active-lyric");
                        widget.remove_css_class("near-active-lyric");
                    }
                } else {
                    widget.remove_css_class("active-lyric");
                    widget.remove_css_class("near-active-lyric");
                }
                child = next;
                i += 1;
            }

            self.scroll_to_active();
        }
    }

    fn animate_scroll(&self) {
        let imp = self.imp();
        let target = imp.target_scroll.get();
        let current = imp.current_scroll.get();
        if (target - current).abs() > 0.1 && imp.tick_id.borrow().is_none() {
            let last_frame_time = Cell::new(None);
            let tick_id = self.add_tick_callback(glib::clone!(
                #[weak(rename_to = view)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move |_, clock| {
                    let current_time = clock.frame_time();
                    let delta_time = if let Some(prev) = last_frame_time.get() {
                        ((current_time - prev) as f64 / 1_000_000.0).min(0.1)
                    } else {
                        1.0 / 60.0
                    };
                    last_frame_time.set(Some(current_time));

                    let imp = view.imp();
                    let target = imp.target_scroll.get();
                    let current = imp.current_scroll.get();
                    let diff = target - current;

                    if diff.abs() > 0.1 && delta_time > 0.0 {
                        // Smooth interpolation (exponential decay)
                        let factor = 1.0 - (-12.0 * delta_time).exp();
                        let next = current + diff * factor;
                        imp.current_scroll.set(next);
                        let adj = imp.scrolled_window.vadjustment();
                        adj.set_value(next);
                        glib::ControlFlow::Continue
                    } else {
                        imp.current_scroll.set(target);
                        let adj = imp.scrolled_window.vadjustment();
                        adj.set_value(target);
                        view.imp().tick_id.replace(None);
                        glib::ControlFlow::Break
                    }
                }
            ));
            imp.tick_id.replace(Some(tick_id));
        }
    }
}
