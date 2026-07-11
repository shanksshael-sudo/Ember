// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;

use glib::clone;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, prelude::*};

mod imp {
    use super::*;
    use glib::{ParamSpec, ParamSpecBoolean, ParamSpecDouble, ParamSpecObject, Value};
    use once_cell::sync::Lazy;

    #[derive(Default)]
    pub struct VinylView {
        pub progress: Cell<f64>,
        pub playing: Cell<bool>,
        pub cover: RefCell<Option<gdk::Texture>>,
        pub rotation: Cell<f64>,
        pub tick_id: RefCell<Option<gtk::TickCallbackId>>,

        // Cache for static background (disk, grooves, center background/fallback)
        pub cached_background_node: RefCell<Option<gsk::RenderNode>>,
        pub cached_background_width: Cell<f64>,
        pub cached_background_height: Cell<f64>,
        pub cached_background_has_cover: Cell<bool>,

        // Cache for progress arc/glow
        pub cached_progress_node: RefCell<Option<gsk::RenderNode>>,
        pub cached_progress_width: Cell<f64>,
        pub cached_progress_height: Cell<f64>,
        pub cached_progress_val: Cell<f64>,

        pub queue: RefCell<Option<crate::audio::Queue>>,
        pub hovered_idx: Cell<Option<u32>>,
        pub hover_callback: RefCell<Option<Box<dyn Fn(Option<&crate::audio::Song>) + 'static>>>,
        pub click_callback: RefCell<Option<Box<dyn Fn(u32) + 'static>>>,
        pub toggle_play_callback: RefCell<Option<Box<dyn Fn() + 'static>>>,
        pub carousel_scroll_offset: Cell<f64>,
        pub target_scroll_offset: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VinylView {
        const NAME: &'static str = "EmberVinylView";
        type Type = super::VinylView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("vinylview");
        }
    }

    impl ObjectImpl for VinylView {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_gestures();
            self.obj().setup_motion();
            self.obj().setup_scroll();
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecDouble::builder("progress")
                        .minimum(0.0)
                        .maximum(1.0)
                        .default_value(0.0)
                        .build(),
                    ParamSpecBoolean::builder("playing")
                        .default_value(false)
                        .build(),
                    ParamSpecObject::builder::<gdk::Texture>("cover").build(),
                    ParamSpecDouble::builder("rotation")
                        .minimum(0.0)
                        .default_value(0.0)
                        .build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: Lazy<Vec<glib::subclass::Signal>> = Lazy::new(|| {
                vec![glib::subclass::Signal::builder("seek")
                    .param_types([f64::static_type()])
                    .build()]
            });
            SIGNALS.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "progress" => {
                    self.progress.set(value.get().unwrap());
                    self.obj().queue_draw();
                }
                "playing" => {
                    let playing = value.get::<bool>().unwrap();
                    self.playing.set(playing);
                    self.obj().update_animation();
                }
                "cover" => {
                    let cover = value.get::<gdk::Texture>().ok();
                    self.cover.replace(cover);
                    self.obj().queue_draw();
                }
                "rotation" => {
                    self.rotation.set(value.get().unwrap());
                    self.obj().queue_draw();
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "progress" => self.progress.get().to_value(),
                "playing" => self.playing.get().to_value(),
                "cover" => self.cover.borrow().to_value(),
                "rotation" => self.rotation.get().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl VinylView {
        fn get_background_node(&self, _widget: &super::VinylView, width: f64, height: f64, has_cover: bool) -> gsk::RenderNode {
            let mut cache = self.cached_background_node.borrow_mut();
            if let Some(ref node) = *cache {
                if self.cached_background_width.get() == width
                    && self.cached_background_height.get() == height
                    && self.cached_background_has_cover.get() == has_cover
                {
                    return node.clone();
                }
            }

            // Not cached or invalid, recreate
            let snapshot = gtk::Snapshot::new();
            let cx = 0.0;
            let cy = height / 2.0;
            let radius = 210.0;
            let center_radius = 70.0;

            // 1. Draw Black Vinyl Disk
            let rect = graphene::Rect::new((cx - radius) as f32, (cy - radius) as f32, (radius * 2.0) as f32, (radius * 2.0) as f32);
            let rounded_rect = gsk::RoundedRect::from_rect(rect.clone(), radius as f32);
            snapshot.push_rounded_clip(&rounded_rect);
            snapshot.append_color(&gdk::RGBA::new(0.05, 0.05, 0.05, 1.0), &rect);
            snapshot.pop();

            // 2. Draw Vinyl Grooves
            let border_width = [0.5f32; 4];
            let border_color = [gdk::RGBA::new(1.0, 1.0, 1.0, 0.04); 4];
            for r in (80..200).step_by(10) {
                let r = r as f32;
                let rect = graphene::Rect::new(
                    (cx - r as f64) as f32,
                    (cy - r as f64) as f32,
                    (r * 2.0) as f32,
                    (r * 2.0) as f32,
                );
                let outline = gsk::RoundedRect::from_rect(rect, r);
                snapshot.append_border(&outline, &border_width, &border_color);
            }

            // 3. Draw Center Label Area (background)
            let center_rect = graphene::Rect::new(
                (cx - center_radius) as f32,
                (cy - center_radius) as f32,
                (center_radius * 2.0) as f32,
                (center_radius * 2.0) as f32,
            );
            let center_rounded = gsk::RoundedRect::from_rect(center_rect.clone(), center_radius as f32);
            snapshot.push_rounded_clip(&center_rounded);
            snapshot.append_color(&gdk::RGBA::new(0.1, 0.1, 0.1, 1.0), &center_rect);
            snapshot.pop();

            // Fallback center if no cover is present
            if !has_cover {
                let fallback_rect = graphene::Rect::new(
                    (cx - 10.0) as f32,
                    (cy - 10.0) as f32,
                    20.0,
                    20.0,
                );
                let fallback_rounded = gsk::RoundedRect::from_rect(fallback_rect.clone(), 10.0);
                snapshot.push_rounded_clip(&fallback_rounded);
                snapshot.append_color(&gdk::RGBA::new(0.0, 1.0, 1.0, 1.0), &fallback_rect);
                snapshot.pop();
            }

            let node = snapshot.to_node().expect("Failed to build background render node");
            *cache = Some(node.clone());
            self.cached_background_width.set(width);
            self.cached_background_height.set(height);
            self.cached_background_has_cover.set(has_cover);
            node
        }

        fn get_progress_node(&self, width: f64, height: f64, progress_val: f64) -> gsk::RenderNode {
            let mut cache = self.cached_progress_node.borrow_mut();
            if let Some(ref node) = *cache {
                if self.cached_progress_width.get() == width
                    && self.cached_progress_height.get() == height
                    && self.cached_progress_val.get() == progress_val
                {
                    return node.clone();
                }
            }

            // Not cached or invalid, recreate
            let snapshot = gtk::Snapshot::new();
            let cx = 0.0;
            let cy = height / 2.0;
            let radius = 210.0;
            let arc_r = radius + 8.0;
            let start_angle = -1.4;
            let end_angle = 1.4;
            let total_span = end_angle - start_angle; // 2.8 rad

            let cr = snapshot.append_cairo(&graphene::Rect::new(0.0, 0.0, width as f32, height as f32));
            cr.set_line_cap(gtk::cairo::LineCap::Round);

            // Draw background track (very dim cyan)
            cr.set_line_width(3.0);
            cr.set_source_rgba(0.0, 1.0, 1.0, 0.08);
            cr.arc(cx, cy, arc_r, start_angle, end_angle);
            cr.stroke().unwrap();

            // Draw active progress
            if progress_val > 0.0 {
                let progress_angle = start_angle + (progress_val * total_span);

                // Layer 1: soft outer glow
                cr.set_line_width(12.0);
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.06);
                cr.arc(cx, cy, arc_r, start_angle, progress_angle);
                cr.stroke().unwrap();

                // Layer 2: middle glow
                cr.set_line_width(8.0);
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.15);
                cr.arc(cx, cy, arc_r, start_angle, progress_angle);
                cr.stroke().unwrap();

                // Layer 3: bright crisp arc
                cr.set_line_width(3.0);
                cr.set_source_rgba(0.0, 1.0, 1.0, 1.0);
                cr.arc(cx, cy, arc_r, start_angle, progress_angle);
                cr.stroke().unwrap();

                // Draw handle (glowing dot)
                let tx = cx + arc_r * progress_angle.cos();
                let ty = cy + arc_r * progress_angle.sin();
                
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.3);
                cr.arc(tx, ty, 8.0, 0.0, 2.0 * PI);
                cr.fill().unwrap();

                cr.set_source_rgba(0.0, 1.0, 1.0, 1.0);
                cr.arc(tx, ty, 4.0, 0.0, 2.0 * PI);
                cr.fill().unwrap();
            }

            drop(cr);

            let node = snapshot.to_node().expect("Failed to build progress render node");
            *cache = Some(node.clone());
            self.cached_progress_width.set(width);
            self.cached_progress_height.set(height);
            self.cached_progress_val.set(progress_val);
            node
        }
    }

    impl WidgetImpl for VinylView {
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (400, 460, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let width = widget.width() as f64;
            let height = widget.height() as f64;
            if width <= 0.0 || height <= 0.0 {
                return;
            }

            let has_cover = self.cover.borrow().is_some();
            let cy = height / 2.0;

            // 1. Draw Round Carousel (Upcoming Song Sleeves) UNDER the vinyl record
            let mut n_songs = 0;
            if let Some(ref queue) = *self.queue.borrow() {
                n_songs = queue.n_songs() as i32;
            }

            if n_songs > 0 {
                let scroll_offset = self.carousel_scroll_offset.get();
                let spacing_angle = 2.0 * PI / 32.0;
                let loop_carousel = n_songs > 12;

                // Extend range so covers fill the visible arc edge-to-edge (no gap)
                let (s_min, s_max) = if loop_carousel {
                    (((-3.0 - scroll_offset) / spacing_angle).floor() as i32,
                     ((3.0 - scroll_offset) / spacing_angle).ceil() as i32)
                } else {
                    (0.max(((-3.0 - scroll_offset) / spacing_angle).floor() as i32),
                     (n_songs - 1).min(((3.0 - scroll_offset) / spacing_angle).ceil() as i32))
                };

                let size = 140.0;
                let r_carousel = 226.5;

                if let Some(ref queue) = *self.queue.borrow() {
                    for s in s_min..=s_max {
                        let theta = s as f64 * spacing_angle + scroll_offset;
                        let cos_t = theta.cos();

                        // Smooth fade: fully visible when cos >= -0.1, fade down to 0 at -0.35
                        let opacity = if cos_t >= -0.10 {
                            1.0
                        } else if cos_t >= -0.35 {
                            (cos_t - (-0.35)) / 0.25
                        } else {
                            0.0
                        };

                        if opacity <= 0.0 { continue; }

                        let tx = r_carousel * cos_t;
                        let ty = cy + r_carousel * theta.sin();

                        let song_idx = if loop_carousel {
                            ((s % n_songs + n_songs) % n_songs) as u32
                        } else {
                            s as u32
                        };
                        if let Some(song) = queue.song_at(song_idx) {
                            snapshot.save();
                            if opacity < 0.999 { snapshot.push_opacity(opacity); }

                            snapshot.translate(&graphene::Point::new(tx as f32, ty as f32));
                            snapshot.rotate(theta.to_degrees() as f32);
                            snapshot.translate(&graphene::Point::new(-size as f32 / 2.0, -size as f32 / 2.0));

                            let rect = graphene::Rect::new(0.0, 0.0, size as f32, size as f32);

                            if let Some(cover_texture) = song.cover_texture() {
                                let rounded = gsk::RoundedRect::from_rect(rect.clone(), 4.0);
                                snapshot.push_rounded_clip(&rounded);
                                snapshot.append_scaled_texture(&cover_texture, gsk::ScalingFilter::Trilinear, &rect);
                                snapshot.pop();
                            } else {
                                snapshot.append_color(&gdk::RGBA::new(0.15, 0.15, 0.15, 1.0), &rect);
                            }

                            let is_hovered = self.hovered_idx.get() == Some(song_idx);
                            let is_playing = queue.current_song_index() == Some(song_idx);
                            let border_w = if is_hovered || is_playing { 3.0f32 } else { 1.0f32 };
                            let border_c = if is_hovered {
                                gdk::RGBA::new(0.0, 1.0, 1.0, 1.0)
                            } else if is_playing {
                                gdk::RGBA::new(1.0, 1.0, 1.0, 0.9)
                            } else {
                                gdk::RGBA::new(1.0, 1.0, 1.0, 0.12)
                            };
                            let outline = gsk::RoundedRect::from_rect(rect, 4.0);
                            snapshot.append_border(&outline, &[border_w; 4], &[border_c; 4]);

                            if opacity < 0.999 { snapshot.pop(); } // pop opacity
                            snapshot.restore();
                        }
                    }
                }
            }

            // 2. Draw static background (Black Vinyl Disk on top of carousel!)
            let bg_node = self.get_background_node(&widget, width, height, has_cover);
            snapshot.append_node(&bg_node);

            // 3. Draw Album Art in the center of the vinyl if present (rotated)
            let cx = 0.0;
            let center_radius = 70.0;

            if let Some(ref cover) = *self.cover.borrow() {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(cx as f32, cy as f32));
                snapshot.rotate(self.rotation.get() as f32);
                snapshot.translate(&graphene::Point::new(-center_radius as f32, -center_radius as f32));

                let rect = graphene::Rect::new(0.0, 0.0, (center_radius * 2.0) as f32, (center_radius * 2.0) as f32);
                let rounded_rect = gsk::RoundedRect::from_rect(rect.clone(), center_radius as f32);
                snapshot.push_rounded_clip(&rounded_rect);
                snapshot.append_scaled_texture(cover, gsk::ScalingFilter::Trilinear, &rect);
                snapshot.pop();
                snapshot.restore();
            }

            // 4. Draw Progress Arc & Glow
            // let progress_node = self.get_progress_node(width, height, self.progress.get());
            // snapshot.append_node(&progress_node);
        }
    }
}

glib::wrapper! {
    pub struct VinylView(ObjectSubclass<imp::VinylView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl VinylView {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn connect_seek<F: Fn(&Self, f64) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("seek", false, move |values| {
            let view = values[0].get::<VinylView>().unwrap();
            let progress = values[1].get::<f64>().unwrap();
            f(&view, progress);
            None
        })
    }

    pub fn set_queue(&self, queue: Option<&crate::audio::Queue>) {
        self.imp().queue.replace(queue.cloned());
        if let Some(ref q) = queue {
            if let Some(current_idx) = q.current_song_index() {
                let spacing_angle = 2.0 * PI / 32.0;
                self.imp().target_scroll_offset.set(-(current_idx as f64) * spacing_angle);
                self.update_animation();
            }
        }
        self.queue_draw();
    }

    pub fn set_hover_callback<F: Fn(Option<&crate::audio::Song>) + 'static>(&self, f: F) {
        self.imp().hover_callback.replace(Some(Box::new(f)));
    }

    pub fn set_click_callback<F: Fn(u32) + 'static>(&self, f: F) {
        self.imp().click_callback.replace(Some(Box::new(f)));
    }

    pub fn set_toggle_play_callback<F: Fn() + 'static>(&self, f: F) {
        self.imp().toggle_play_callback.replace(Some(Box::new(f)));
    }

    fn setup_gestures(&self) {
        // Drag gesture: only claim the sequence once real movement is detected,
        // so short taps still reach the click gesture.
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_update(clone!(
            #[weak(rename_to = view)]
            self,
            move |gesture, offset_x, offset_y| {
                // Only engage seek-drag when pointer has moved enough to be
                // intentional (> 4 px), avoiding false fires on simple clicks.
                if offset_x.hypot(offset_y) < 4.0 { return; }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                let start_x = gesture.start_point().map(|(x, _)| x).unwrap_or(0.0);
                let start_y = gesture.start_point().map(|(_, y)| y).unwrap_or(0.0);
                view.handle_drag(start_x + offset_x, start_y + offset_y);
            }
        ));
        self.add_controller(drag);

        let click = gtk::GestureClick::new();
        click.connect_released(clone!(
            #[weak(rename_to = view)]
            self,
            move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                view.handle_click(x, y);
            }
        ));
        self.add_controller(click);
    }

    fn setup_motion(&self) {
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(clone!(
            #[weak(rename_to = view)]
            self,
            move |_, x, y| {
                view.check_hover(x, y);
            }
        ));
        motion.connect_leave(clone!(
            #[weak(rename_to = view)]
            self,
            move |_| {
                view.clear_hover();
            }
        ));
        self.add_controller(motion);
    }

    fn setup_scroll(&self) {
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.connect_scroll(clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, dx, dy| {
                view.handle_scroll(dx, dy);
                glib::Propagation::Stop
            }
        ));
        self.add_controller(scroll);
    }

    fn handle_scroll(&self, dx: f64, dy: f64) {
        let imp = self.imp();
        let scroll_delta = if dy != 0.0 { dy } else { dx };
        let step = scroll_delta * 0.15;
        let new_target = imp.target_scroll_offset.get() + step;
        imp.target_scroll_offset.set(new_target);
        self.update_animation();
    }

    /// Return the best carousel slot index `s` under point `(x, y)`,
    /// or `None` if no cover is hit. Uses the same geometry as snapshot.
    fn find_carousel_hit(&self, x: f64, y: f64) -> Option<i32> {
        let height = self.height() as f64;
        let cy = height / 2.0;
        let imp = self.imp();
        let queue = imp.queue.borrow();
        let queue = queue.as_ref()?;
        let n_songs = queue.n_songs();
        if n_songs == 0 { return None; }

        let scroll_offset = imp.carousel_scroll_offset.get();
        let spacing_angle = 2.0 * PI / 32.0;
        let loop_carousel = n_songs > 12;

        let (s_min, s_max) = if loop_carousel {
            (((-3.0 - scroll_offset) / spacing_angle).floor() as i32,
             ((3.0 - scroll_offset) / spacing_angle).ceil() as i32)
        } else {
            (0.max(((-3.0 - scroll_offset) / spacing_angle).floor() as i32),
             (n_songs as i32 - 1).min(((3.0 - scroll_offset) / spacing_angle).ceil() as i32))
        };

        const R: f64     = 226.5;
        const HIT: f64   = 70.0; // half-size of 140px cover

        let mut best_s: Option<i32> = None;
        let mut min_dist = f64::MAX;

        for s in s_min..=s_max {
            let theta = s as f64 * spacing_angle + scroll_offset;
            if theta.cos() < -0.35 { continue; }
            let tx = R * theta.cos();
            let ty = cy + R * theta.sin();
            let dx = x - tx;
            let dy = y - ty;
            if dx.abs() <= HIT && dy.abs() <= HIT {
                let dist = dx * dx + dy * dy;
                if dist < min_dist {
                    min_dist = dist;
                    best_s = Some(s);
                }
            }
        }
        best_s
    }

    fn check_hover(&self, x: f64, y: f64) {
        let (hovered_song_idx, hovered_song) = if let Some(s) = self.find_carousel_hit(x, y) {
            let queue = self.imp().queue.borrow();
            if let Some(ref q) = *queue {
                let n_songs = q.n_songs() as i32;
                let idx = if n_songs > 12 {
                    ((s % n_songs + n_songs) % n_songs) as u32
                } else {
                    s as u32
                };
                (Some(idx), q.song_at(idx))
            } else { (None, None) }
        } else { (None, None) };

        if self.imp().hovered_idx.get() != hovered_song_idx {
            self.imp().hovered_idx.set(hovered_song_idx);
            self.queue_draw();
            if let Some(ref cb) = *self.imp().hover_callback.borrow() {
                cb(hovered_song.as_ref());
            }
        }
    }

    fn clear_hover(&self) {
        if self.imp().hovered_idx.get().is_some() {
            self.imp().hovered_idx.set(None);
            self.queue_draw();
            if let Some(ref cb) = *self.imp().hover_callback.borrow() {
                cb(None);
            }
        }
    }

    fn handle_click(&self, x: f64, y: f64) {
        if let Some(s) = self.find_carousel_hit(x, y) {
            let queue = self.imp().queue.borrow();
            if let Some(ref q) = *queue {
                let n_songs = q.n_songs() as i32;
                let song_idx = if n_songs > 12 {
                    ((s % n_songs + n_songs) % n_songs) as u32
                } else {
                    s as u32
                };
                drop(queue);
                if let Some(ref cb) = *self.imp().click_callback.borrow() {
                    cb(song_idx);
                }
            }
        } else {
            // Click on vinyl disk center → toggle play/pause
            let cy = self.height() as f64 / 2.0;
            let dy = y - cy;
            if (x * x + dy * dy).sqrt() <= 210.0 {
                if let Some(ref cb) = *self.imp().toggle_play_callback.borrow() {
                    cb();
                }
            }
        }
    }

    fn handle_drag(&self, x: f64, y: f64) {
        let height = self.height() as f64;
        let cx = 0.0;
        let cy = height / 2.0;

        let dx = x - cx;
        let dy = y - cy;
        let angle = dy.atan2(dx); // [-PI, PI]

        if angle >= -1.4 && angle <= 1.4 {
            let start_angle = -1.4;
            let end_angle = 1.4;
            let total_span = end_angle - start_angle;
            let progress = (angle - start_angle) / total_span;
            let progress = progress.clamp(0.0, 1.0);
            self.emit_by_name::<()>("seek", &[&progress]);
        }
    }

    fn update_animation(&self) {
        let imp = self.imp();
        let playing = imp.playing.get();
        
        let target = imp.target_scroll_offset.get();
        let current = imp.carousel_scroll_offset.get();
        let needs_scroll_animation = (target - current).abs() > 0.0001;

        if (playing || needs_scroll_animation) && imp.tick_id.borrow().is_none() {
            let last_frame_time = Cell::new(None);
            let tick_id = self.add_tick_callback(clone!(
                #[weak(rename_to = view)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move |_, clock| {
                    let current_time = clock.frame_time();
                    let delta_time = if let Some(prev) = last_frame_time.get() {
                        ((current_time - prev) as f64 / 1_000_000.0).min(0.1)
                    } else {
                        0.0
                    };
                    last_frame_time.set(Some(current_time));

                    let imp = view.imp();
                    let playing = imp.playing.get();
                    let target = imp.target_scroll_offset.get();
                    let current = imp.carousel_scroll_offset.get();
                    let diff = target - current;
                    let scroll_animating = diff.abs() > 0.0001;

                    // 1. Rotate vinyl record
                    if playing && delta_time > 0.0 {
                        let r = view.property::<f64>("rotation");
                        let increment = 48.0 * delta_time;
                        view.set_property("rotation", (r + increment) % 360.0);
                    }

                    // 2. Smoothly animate scroll offset
                    if scroll_animating && delta_time > 0.0 {
                        let factor = 1.0 - (-15.0 * delta_time).exp();
                        let next_offset = current + diff * factor;
                        imp.carousel_scroll_offset.set(next_offset);
                        view.queue_draw();
                    } else if !scroll_animating {
                        imp.carousel_scroll_offset.set(target);
                    }

                    // Keep running if playing or still animating scroll
                    let keep_running = playing || (target - imp.carousel_scroll_offset.get()).abs() > 0.0001;
                    if keep_running {
                        glib::ControlFlow::Continue
                    } else {
                        view.imp().tick_id.replace(None);
                        glib::ControlFlow::Break
                    }
                }
            ));
            imp.tick_id.replace(Some(tick_id));
        }
    }
}

impl Default for VinylView {
    fn default() -> Self {
        Self::new()
    }
}


