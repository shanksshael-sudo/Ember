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

        // Cache for carousel node
        pub cached_carousel_node: RefCell<Option<gsk::RenderNode>>,
        pub cached_carousel_scroll: Cell<f64>,
        pub cached_carousel_hovered: Cell<Option<u32>>,
        pub cached_carousel_current_song: Cell<Option<u32>>,
        pub cached_carousel_height: Cell<f64>,

        // Cache for center album art (pre-scaled and circular clipped)
        pub cached_center_cover_node: RefCell<Option<gsk::RenderNode>>,
        pub cached_center_cover_height: Cell<f64>,

        pub queue: RefCell<Option<crate::audio::Queue>>,
        pub hovered_idx: Cell<Option<u32>>,
        pub hover_callback: RefCell<Option<Box<dyn Fn(Option<&crate::audio::Song>) + 'static>>>,
        pub click_callback: RefCell<Option<Box<dyn Fn(u32) + 'static>>>,
        pub toggle_play_callback: RefCell<Option<Box<dyn Fn() + 'static>>>,
        pub carousel_scroll_offset: Cell<f64>,
        pub target_scroll_offset: Cell<f64>,
        pub is_dragging_vinyl: Cell<bool>,
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
                vec![
                    glib::subclass::Signal::builder("seek")
                        .param_types([f64::static_type()])
                        .build(),
                    glib::subclass::Signal::builder("seek-preview")
                        .param_types([f64::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "progress" => {
                    self.progress.set(value.get().unwrap());
                }
                "playing" => {
                    let playing = value.get::<bool>().unwrap();
                    self.playing.set(playing);
                    self.obj().update_animation();
                }
                "cover" => {
                    let cover = value.get::<gdk::Texture>().ok();
                    self.cover.replace(cover);
                    self.cached_center_cover_node.replace(None);
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
        pub fn get_dimensions(height: f64) -> (f64, f64, f64, f64) {
            let scale = (height / 640.0).clamp(0.65, 1.0);
            (210.0 * scale, 70.0 * scale, 226.5 * scale, 140.0 * scale)
        }

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
            let (radius, center_radius, _, _) = Self::get_dimensions(height);

            // 1. Draw Black Vinyl Disk
            let rect = graphene::Rect::new((cx - radius) as f32, (cy - radius) as f32, (radius * 2.0) as f32, (radius * 2.0) as f32);
            let rounded_rect = gsk::RoundedRect::from_rect(rect.clone(), radius as f32);
            snapshot.push_rounded_clip(&rounded_rect);
            snapshot.append_color(&gdk::RGBA::new(0.05, 0.05, 0.05, 1.0), &rect);
            snapshot.pop();

            // 2. Draw Vinyl Grooves
            let border_width = [0.5f32; 4];
            let border_color = [gdk::RGBA::new(1.0, 1.0, 1.0, 0.04); 4];
            let g_start = (center_radius + 10.0) as i32;
            let g_end = (radius - 10.0) as i32;
            for r in (g_start..g_end).step_by(10) {
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

        fn get_carousel_node(&self, height: f64) -> Option<gsk::RenderNode> {
            let queue_guard = self.queue.borrow();
            let queue = queue_guard.as_ref()?;
            let n_songs = queue.n_songs() as i32;
            if n_songs == 0 {
                return None;
            }

            let scroll_offset = self.carousel_scroll_offset.get();
            let hovered_idx = self.hovered_idx.get();
            let current_song = queue.current_song_index();

            let mut cache = self.cached_carousel_node.borrow_mut();
            if let Some(ref node) = *cache {
                if (self.cached_carousel_scroll.get() - scroll_offset).abs() < 0.0001
                    && self.cached_carousel_hovered.get() == hovered_idx
                    && self.cached_carousel_current_song.get() == current_song
                    && (self.cached_carousel_height.get() - height).abs() < 0.1
                {
                    return Some(node.clone());
                }
            }

            let spacing_angle = 2.0 * PI / 32.0;
            let loop_carousel = n_songs > 12;

            let (s_min, s_max) = if loop_carousel {
                (((-2.0 - scroll_offset) / spacing_angle).floor() as i32,
                 ((2.0 - scroll_offset) / spacing_angle).ceil() as i32)
            } else {
                (0.max(((-2.0 - scroll_offset) / spacing_angle).floor() as i32),
                 (n_songs - 1).min(((2.0 - scroll_offset) / spacing_angle).ceil() as i32))
            };

            let (_, _, r_carousel, size) = Self::get_dimensions(height);
            let cy = height / 2.0;

            let snapshot = gtk::Snapshot::new();

            for s in s_min..=s_max {
                let theta = s as f64 * spacing_angle + scroll_offset;
                let cos_t = theta.cos();

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

                    let cover_texture = song.thumb_texture().or_else(|| song.cover_texture());
                    if let Some(ref texture) = cover_texture {
                        let rounded = gsk::RoundedRect::from_rect(rect.clone(), 4.0);
                        snapshot.push_rounded_clip(&rounded);
                        snapshot.append_scaled_texture(texture, gsk::ScalingFilter::Linear, &rect);
                        snapshot.pop();
                    } else {
                        snapshot.append_color(&gdk::RGBA::new(0.15, 0.15, 0.15, 1.0), &rect);
                    }

                    let is_hovered = hovered_idx == Some(song_idx);
                    let is_playing = current_song == Some(song_idx);
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

                    if opacity < 0.999 { snapshot.pop(); }
                    snapshot.restore();
                }
            }

            let node = snapshot.to_node();
            *cache = node.clone();
            self.cached_carousel_scroll.set(scroll_offset);
            self.cached_carousel_hovered.set(hovered_idx);
            self.cached_carousel_current_song.set(current_song);
            self.cached_carousel_height.set(height);
            node
        }

        fn get_center_cover_node(&self, height: f64) -> Option<gsk::RenderNode> {
            let (_, center_radius, _, _) = Self::get_dimensions(height);
            let mut cache = self.cached_center_cover_node.borrow_mut();
            if let Some(ref node) = *cache {
                if (self.cached_center_cover_height.get() - height).abs() < 0.1 {
                    return Some(node.clone());
                }
            }

            let cover_guard = self.cover.borrow();
            let cover = cover_guard.as_ref()?;
            let rect = graphene::Rect::new(
                -center_radius as f32,
                -center_radius as f32,
                (center_radius * 2.0) as f32,
                (center_radius * 2.0) as f32,
            );
            let rounded_rect = gsk::RoundedRect::from_rect(rect.clone(), center_radius as f32);

            let snapshot = gtk::Snapshot::new();
            snapshot.push_rounded_clip(&rounded_rect);
            snapshot.append_scaled_texture(cover, gsk::ScalingFilter::Linear, &rect);
            snapshot.pop();

            let node = snapshot.to_node();
            *cache = node.clone();
            self.cached_center_cover_height.set(height);
            node
        }
    }

    impl WidgetImpl for VinylView {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (270, 360, -1, -1),
                gtk::Orientation::Vertical => (320, 480, -1, -1),
                _ => (0, 0, -1, -1),
            }
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
            if let Some(carousel_node) = self.get_carousel_node(height) {
                snapshot.append_node(&carousel_node);
            }

            // 2. Draw static background (Black Vinyl Disk on top of carousel!)
            let bg_node = self.get_background_node(&widget, width, height, has_cover);
            snapshot.append_node(&bg_node);

            // 3. Draw Album Art in the center of the vinyl if present (rotated)
            let cx = 0.0;
            if let Some(center_node) = self.get_center_cover_node(height) {
                snapshot.save();
                snapshot.translate(&graphene::Point::new(cx as f32, cy as f32));
                snapshot.rotate(self.rotation.get() as f32);
                snapshot.append_node(&center_node);
                snapshot.restore();
            }
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

    pub fn connect_seek_preview<F: Fn(&Self, f64) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("seek-preview", false, move |values| {
            let view = values[0].get::<VinylView>().unwrap();
            let progress = values[1].get::<f64>().unwrap();
            f(&view, progress);
            None
        })
    }

    pub fn set_queue(&self, queue: Option<&crate::audio::Queue>) {
        self.imp().queue.replace(queue.cloned());
        self.imp().cached_carousel_node.replace(None);
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
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin(clone!(
            #[weak(rename_to = view)]
            self,
            move |_, start_x, start_y| {
                let height = view.height() as f64;
                let cy = height / 2.0;
                let (radius, _, _, _) = imp::VinylView::get_dimensions(height);
                let dx = start_x;
                let dy = start_y - cy;
                let dist = (dx * dx + dy * dy).sqrt();

                // Check if on vinyl disc grooves (outside center label and not on carousel)
                if view.find_carousel_hit(start_x, start_y).is_none() && dist <= radius * 1.15 && dist > radius * 0.38 {
                    view.imp().is_dragging_vinyl.set(true);
                    let angle = dy.atan2(dx);
                    let progress = ((angle - (-1.35)) / 2.70).clamp(0.0, 1.0);
                    view.emit_by_name::<()>("seek-preview", &[&progress]);
                } else {
                    view.imp().is_dragging_vinyl.set(false);
                }
            }
        ));

        drag.connect_drag_update(clone!(
            #[weak(rename_to = view)]
            self,
            move |gesture, offset_x, offset_y| {
                if !view.imp().is_dragging_vinyl.get() {
                    return;
                }
                if offset_x.hypot(offset_y) > 3.0 {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                }
                if let Some((start_x, start_y)) = gesture.start_point() {
                    let height = view.height() as f64;
                    let cy = height / 2.0;
                    let cur_x = start_x + offset_x;
                    let cur_y = start_y + offset_y;
                    let dx = cur_x;
                    let dy = cur_y - cy;
                    let angle = dy.atan2(dx);
                    let progress = ((angle - (-1.35)) / 2.70).clamp(0.0, 1.0);
                    view.emit_by_name::<()>("seek-preview", &[&progress]);
                }
            }
        ));

        drag.connect_drag_end(clone!(
            #[weak(rename_to = view)]
            self,
            move |gesture, offset_x, offset_y| {
                if view.imp().is_dragging_vinyl.get() {
                    view.imp().is_dragging_vinyl.set(false);
                    if let Some((start_x, start_y)) = gesture.start_point() {
                        let height = view.height() as f64;
                        let cy = height / 2.0;
                        let cur_x = start_x + offset_x;
                        let cur_y = start_y + offset_y;
                        let dx = cur_x;
                        let dy = cur_y - cy;
                        let angle = dy.atan2(dx);
                        let progress = ((angle - (-1.35)) / 2.70).clamp(0.0, 1.0);
                        view.emit_by_name::<()>("seek", &[&progress]);
                    }
                }
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

        let (_, _, r_carousel, size) = imp::VinylView::get_dimensions(height);
        let hit = size / 2.0;

        let mut best_s: Option<i32> = None;
        let mut min_dist = f64::MAX;

        for s in s_min..=s_max {
            let theta = s as f64 * spacing_angle + scroll_offset;
            if theta.cos() < -0.35 { continue; }
            let tx = r_carousel * theta.cos();
            let ty = cy + r_carousel * theta.sin();
            let dx = x - tx;
            let dy = y - ty;
            if dx.abs() <= hit && dy.abs() <= hit {
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
        let height = self.height() as f64;
        let cy = height / 2.0;
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
            let (radius, _, _, _) = imp::VinylView::get_dimensions(height);
            let dx = x;
            let dy = y - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist <= radius * 0.38 {
                // Click on vinyl disk center label → toggle play/pause
                if let Some(ref cb) = *self.imp().toggle_play_callback.borrow() {
                    cb();
                }
            } else if dist <= radius * 1.15 {
                // Click on vinyl disk grooves → seek directly to that angle
                let angle = dy.atan2(dx);
                let progress = ((angle - (-1.35)) / 2.70).clamp(0.0, 1.0);
                self.emit_by_name::<()>("seek", &[&progress]);
            }
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
                    if !view.is_mapped() {
                        return glib::ControlFlow::Continue;
                    }

                    let current_time = clock.frame_time();
                    let delta_time = if let Some(prev) = last_frame_time.get() {
                        ((current_time - prev) as f64 / 1_000_000.0).clamp(0.001, 0.1)
                    } else {
                        1.0 / 60.0
                    };
                    last_frame_time.set(Some(current_time));

                    let imp = view.imp();
                    let playing = imp.playing.get();
                    let target = imp.target_scroll_offset.get();
                    let current = imp.carousel_scroll_offset.get();
                    let diff = target - current;
                    let scroll_animating = diff.abs() > 0.0001;

                    // 1. Rotate vinyl record smoothly based on exact elapsed frame time
                    if playing && delta_time > 0.0 {
                        let cur_rot = imp.rotation.get();
                        let increment = 48.0 * delta_time;
                        imp.rotation.set((cur_rot + increment) % 360.0);
                    }

                    // 2. Smoothly animate scroll offset
                    if scroll_animating && delta_time > 0.0 {
                        let factor = 1.0 - (-15.0 * delta_time).exp();
                        let next_offset = current + diff * factor;
                        imp.carousel_scroll_offset.set(next_offset);
                    } else if !scroll_animating {
                        imp.carousel_scroll_offset.set(target);
                    }

                    view.queue_draw();

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


