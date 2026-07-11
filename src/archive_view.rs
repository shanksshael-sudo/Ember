// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Cell, RefCell};

use glib::clone;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango, prelude::*};

mod imp {
    use super::*;
    use glib::{ParamSpec, ParamSpecDouble, ParamSpecInt, Value};
    use once_cell::sync::Lazy;

    #[derive(Default)]
    pub struct ArchiveView {
        pub songs: RefCell<Vec<crate::audio::Song>>,
        pub selected_index: Cell<i32>,
        pub hovered_index: Cell<Option<i32>>,

        pub scroll_offset: Cell<f64>,
        pub target_offset: Cell<f64>,
        pub scroll_velocity: Cell<f64>,

        pub open_progress: Cell<f64>,
        pub opening_index: Cell<Option<i32>>,

        pub tick_id: RefCell<Option<gtk::TickCallbackId>>,
        pub last_mouse_pos: Cell<Option<(f64, f64)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ArchiveView {
        const NAME: &'static str = "EmberArchiveView";
        type Type = super::ArchiveView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("archiveview");
        }
    }

    impl ObjectImpl for ArchiveView {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_controllers();
            self.obj().start_animation_tick();
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecInt::builder("selected-index")
                        .minimum(-1)
                        .default_value(-1)
                        .build(),
                    ParamSpecDouble::builder("open-progress")
                        .minimum(0.0)
                        .maximum(1.0)
                        .default_value(0.0)
                        .build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "selected-index" => {
                    let idx = value.get::<i32>().unwrap();
                    self.selected_index.set(idx);
                    if idx >= 0 {
                        self.target_offset.set(idx as f64);
                    }
                    self.obj().queue_draw();
                }
                "open-progress" => {
                    self.open_progress.set(value.get().unwrap());
                    self.obj().queue_draw();
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "selected-index" => self.selected_index.get().to_value(),
                "open-progress" => self.open_progress.get().to_value(),
                _ => unimplemented!(),
            }
        }

        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: Lazy<Vec<glib::subclass::Signal>> = Lazy::new(|| {
                vec![
                    glib::subclass::Signal::builder("activated")
                        .param_types([i32::static_type()])
                        .build(),
                    glib::subclass::Signal::builder("selection-changed")
                        .param_types([i32::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for ArchiveView {
        fn measure(&self, _orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (500, 800, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let width = widget.width() as f64;
            let height = widget.height() as f64;
            if width <= 0.0 || height <= 0.0 { return; }

            // Restrict all widget drawings strictly to the widget bounds to prevent any reflection overflow/leaks
            let widget_bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
            snapshot.push_clip(&widget_bounds);

            let songs = self.songs.borrow();
            let n_songs = songs.len();
            if n_songs == 0 {
                let cr = snapshot.append_cairo(&graphene::Rect::new(0.0, 0.0, width as f32, height as f32));
                cr.set_source_rgb(0.3, 0.3, 0.3);
                cr.select_font_face("Audiowide", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
                cr.set_font_size(18.0);
                cr.move_to(width / 2.0 - 100.0, height / 2.0);
                cr.show_text("ARCHIVE IS EMPTY").unwrap();
                snapshot.pop(); // pop widget clip bounds
                return;
            }

            // ---- Layout constants ----
            let f_size: f64     = 300.0;   // square sleeve (300×300)
            let step: f64       = 110.0;   // px between sleeve centers
            let disc_r: f64     = 140.0;   // vinyl disc radius
            let disc_slide: f64 = 210.0;   // disc slides this far right on open
            let slv_shift: f64  = 140.0;   // selected sleeve slides this far left
            let nbr_push: f64   = 270.0;   // neighbors pushed this far outward

            let cx = width / 2.0;
            let cy = height / 2.0;

            let scroll   = self.scroll_offset.get();
            let sel_idx  = self.selected_index.get();
            let open_p   = self.obj().property::<f64>("open-progress");
            let is_open  = open_p > 0.01 && sel_idx >= 0;

            // Build draw slots
            let mut slots: Vec<(i32, usize, bool)> = Vec::new();
            let loop_carousel = n_songs > 12;
            let (s_min, s_max) = if loop_carousel {
                ((scroll - 7.0).floor() as i32, (scroll + 7.0).ceil() as i32)
            } else {
                (0.max((scroll - 7.0).floor() as i32), (n_songs as i32 - 1).min((scroll + 7.0).ceil() as i32))
            };

            for si in s_min..=s_max {
                let song_i = if loop_carousel {
                    ((si % n_songs as i32 + n_songs as i32) % n_songs as i32) as usize
                } else {
                    si as usize
                };
                let is_sel = sel_idx >= 0
                    && song_i == sel_idx as usize
                    && (!loop_carousel || (si as f64 - scroll).abs() < n_songs as f64 / 2.0);
                slots.push((si, song_i, is_sel));
            }
            slots.sort_by(|a, b| a.0.cmp(&b.0));
            let sel_slot: Option<i32> = slots.iter().find(|s| s.2).map(|s| s.0);

            // Compute the on-screen x of a sleeve
            let sx = |si: i32, is_sel: bool| -> f64 {
                let base = cx + (si as f64 - scroll) * step - f_size / 2.0;
                match sel_slot {
                    Some(_) if is_sel   => base - slv_shift * open_p,
                    Some(ss) if si < ss => base - nbr_push  * open_p,
                    Some(_)             => base + nbr_push  * open_p,
                    None                => base,
                }
            };

            // --- Vinyl disc (widget-space center) ---
            let draw_disc = |snap: &gtk::Snapshot, cx_d: f64, cy_d: f64, song: &crate::audio::Song, opacity: f64| {
                if opacity <= 0.001 { return; }
                let r = disc_r;
                snap.save();
                if opacity < 0.999 { snap.push_opacity(opacity); }

                // 1. Draw disc body (solid dark metallic grey circle)
                let body_rect = graphene::Rect::new(
                    (cx_d - r) as f32, (cy_d - r) as f32,
                    (r * 2.0) as f32, (r * 2.0) as f32,
                );
                let body_rounded = gsk::RoundedRect::from_rect(body_rect.clone(), r as f32);
                snap.push_rounded_clip(&body_rounded);
                snap.append_color(&gdk::RGBA::new(0.12, 0.12, 0.13, 1.0), &body_rect);
                snap.pop();

                // 2. Draw metallic grooves (concentric borders)
                let border_width = [0.7f32; 4];
                let border_color = [gdk::RGBA::new(1.0, 1.0, 1.0, 0.09); 4];
                let mut gr = 28.0_f64;
                while gr < r - 8.0 {
                    let rect = graphene::Rect::new(
                        (cx_d - gr) as f32, (cy_d - gr) as f32,
                        (gr * 2.0) as f32, (gr * 2.0) as f32,
                    );
                    let outline = gsk::RoundedRect::from_rect(rect, gr as f32);
                    snap.append_border(&outline, &border_width, &border_color);
                    gr += 5.0;
                }

                // 3. Draw Center label background
                let lab_r = 46.0_f64;
                let label_rect = graphene::Rect::new(
                    (cx_d - lab_r) as f32, (cy_d - lab_r) as f32,
                    (lab_r * 2.0) as f32, (lab_r * 2.0) as f32,
                );
                let label_rounded = gsk::RoundedRect::from_rect(label_rect.clone(), lab_r as f32);
                snap.push_rounded_clip(&label_rounded);
                snap.append_color(&gdk::RGBA::new(0.18, 0.18, 0.18, 1.0), &label_rect);
                snap.pop();

                // 4. Draw Hole background
                let hole_rect = graphene::Rect::new(
                    (cx_d - 5.0) as f32, (cy_d - 5.0) as f32,
                    10.0, 10.0,
                );
                let hole_rounded = gsk::RoundedRect::from_rect(hole_rect.clone(), 5.0);
                snap.push_rounded_clip(&hole_rounded);
                snap.append_color(&gdk::RGBA::new(0.0, 0.0, 0.0, 1.0), &hole_rect);
                snap.pop();

                // 5. Draw Cover art circular clip & scaled texture
                let art_r = lab_r - 3.0;
                if let Some(cover) = song.cover_texture() {
                    let clip = graphene::Rect::new(
                        (cx_d - art_r) as f32, (cy_d - art_r) as f32,
                        (art_r * 2.0) as f32,  (art_r * 2.0) as f32,
                    );
                    snap.push_rounded_clip(&gsk::RoundedRect::from_rect(clip.clone(), art_r as f32));
                    snap.append_scaled_texture(&cover, gsk::ScalingFilter::Linear, &clip);
                    snap.pop();
                }

                // 6. Draw center hole on top of artwork
                let hole_top_rect = graphene::Rect::new(
                    (cx_d - 5.0) as f32, (cy_d - 5.0) as f32,
                    10.0, 10.0,
                );
                let hole_top_rounded = gsk::RoundedRect::from_rect(hole_top_rect.clone(), 5.0);
                snap.push_rounded_clip(&hole_top_rounded);
                snap.append_color(&gdk::RGBA::new(0.0, 0.0, 0.0, 1.0), &hole_top_rect);
                snap.pop();

                if opacity < 0.999 { snap.pop(); }
                snap.restore();
            };

            // ===== 1. Solid black background (required for blur to composite against) =====
            snapshot.append_color(&gdk::RGBA::new(0.0, 0.0, 0.0, 1.0), &graphene::Rect::new(0.0, 0.0, width as f32, height as f32));

            // ===== 2. Render all sleeves and discs in unified Z-order (hovered first/on top, others left-to-right) =====
            for &(si, song_i, is_sel) in &slots {
                let song   = &songs[song_i];
                let is_hov = Some(song_i as i32) == self.hovered_index.get();
                let fx     = sx(si, is_sel);
                let fy     = cy - f_size / 2.0;

                // Only fade at the very screen edge (beyond slot 5.5)
                let edge = (1.0 - ((si as f64 - scroll).abs() - 5.5).max(0.0)).clamp(0.0, 1.0);
                if edge <= 0.001 { continue; }

                snapshot.save();
                if edge < 0.999 { snapshot.push_opacity(edge); }

                // Blur and dim non-selected sleeves ONLY when focused
                let needs_blur = is_open && !is_sel;
                if needs_blur {
                    // Animate the blur radius smoothly. With other optimizations (GPU nodes, time-delta tick,
                    // and skipping non-selected reflections), this is now extremely smooth.
                    snapshot.push_blur(open_p * 14.0);
                    snapshot.push_opacity(1.0 - open_p * 0.65);
                }

                let ref_h = f_size * 0.60;

                // --- 2.1 Draw the reflection (upside down, faint, clipped to reflection bounds) ---
                // Only draw reflections for sleeves if the archive is closed, or for the active selected sleeve itself.
                // This saves rendering up to 10 invisible blurred reflections when the carousel is open.
                let draw_reflection = !is_open || is_sel;
                if draw_reflection {
                    let clip_rect = graphene::Rect::new(
                        (fx - f_size) as f32,
                        (fy + f_size) as f32,
                        (f_size * 3.0) as f32,
                        ref_h as f32
                    );
                    snapshot.push_clip(&clip_rect);

                    snapshot.save();
                    // Apply mirror transform with bottom of sleeve as pivot
                    snapshot.translate(&graphene::Point::new(0.0, (fy + f_size) as f32));
                    snapshot.scale(1.0, -1.0);
                    snapshot.translate(&graphene::Point::new(0.0, -(fy + f_size) as f32));

                    snapshot.push_opacity(0.32);

                    // Flipped disc (behind sleeve)
                    if is_sel {
                        let disc_cx = fx + f_size / 2.0 + disc_slide * open_p;
                        draw_disc(snapshot, disc_cx, cy, song, open_p);
                    }

                    // Flipped sleeve
                    snapshot.save();
                    if is_sel {
                        let scale  = 1.0 + open_p * 0.10;
                        let sc_cx  = fx + f_size / 2.0;
                        snapshot.translate(&graphene::Point::new(sc_cx as f32, cy as f32));
                        snapshot.scale(scale as f32, scale as f32);
                        snapshot.translate(&graphene::Point::new(-sc_cx as f32, -(cy as f32)));
                    }

                    if let Some(cover) = song.cover_texture() {
                        let rect = graphene::Rect::new(fx as f32, fy as f32, f_size as f32, f_size as f32);
                        snapshot.push_clip(&rect);
                        snapshot.append_scaled_texture(&cover, gtk::gsk::ScalingFilter::Linear, &rect);
                        snapshot.pop();
                    } else {
                        snapshot.append_color(
                            &gdk::RGBA::new(0.10, 0.10, 0.12, 1.0),
                            &graphene::Rect::new(fx as f32, fy as f32, f_size as f32, f_size as f32),
                        );
                    }
                    snapshot.restore(); // pop scale
                    snapshot.pop(); // pop opacity
                    snapshot.restore(); // pop mirror transform

                    snapshot.pop(); // pop clip

                    // Overlay gradient to fade the reflection to black
                    let grad_rect = graphene::Rect::new(
                        (fx - f_size) as f32, (fy + f_size) as f32, (f_size * 3.0) as f32, ref_h as f32
                    );
                    let start_point = graphene::Point::new(0.0, (fy + f_size) as f32);
                    let end_point = graphene::Point::new(0.0, (fy + f_size + ref_h) as f32);
                    let stops = [
                        gsk::ColorStop::new(0.0, gdk::RGBA::new(0.0, 0.0, 0.0, 0.15)),
                        gsk::ColorStop::new(1.0, gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                    ];
                    snapshot.append_linear_gradient(&grad_rect, &start_point, &end_point, &stops);
                }

                // --- 2.2 Draw the actual sleeve and disc on top ---
                if is_sel {
                    let disc_cx = fx + f_size / 2.0 + disc_slide * open_p;
                    draw_disc(snapshot, disc_cx, cy, song, open_p);
                }

                snapshot.save();
                if is_sel {
                    let scale  = 1.0 + open_p * 0.10;
                    let sc_cx  = fx + f_size / 2.0;
                    snapshot.translate(&graphene::Point::new(sc_cx as f32, cy as f32));
                    snapshot.scale(scale as f32, scale as f32);
                    snapshot.translate(&graphene::Point::new(-sc_cx as f32, -(cy as f32)));
                }

                if let Some(cover) = song.cover_texture() {
                    let rect = graphene::Rect::new(fx as f32, fy as f32, f_size as f32, f_size as f32);
                    snapshot.push_clip(&rect);
                    snapshot.append_scaled_texture(&cover, gtk::gsk::ScalingFilter::Linear, &rect);
                    snapshot.pop();
                } else {
                    snapshot.append_color(
                        &gdk::RGBA::new(0.10, 0.10, 0.12, 1.0),
                        &graphene::Rect::new(fx as f32, fy as f32, f_size as f32, f_size as f32),
                    );
                }

                let border_rect = graphene::Rect::new(fx as f32, fy as f32, f_size as f32, f_size as f32);
                let outline = gsk::RoundedRect::from_rect(border_rect, 0.0);
                let border_w = if is_sel { 2.0f32 } else { 1.0f32 };
                let border_c = if is_sel {
                    gdk::RGBA::new(0.0, 1.0, 1.0, 0.75 * open_p as f32)
                } else {
                    gdk::RGBA::new(1.0, 1.0, 1.0, if is_hov { 0.35f32 } else { 0.12f32 })
                };
                snapshot.append_border(&outline, &[border_w; 4], &[border_c; 4]);

                snapshot.restore(); // pop scale

                if needs_blur {
                    snapshot.pop(); // opacity
                    snapshot.pop(); // blur
                }

                if edge < 0.999 { snapshot.pop(); }
                snapshot.restore();
            }

            // ===== 3. Label rendering (focused or hovered index) =====
            let loop_carousel = n_songs > 12;
            if let Some(ss) = sel_slot {
                if open_p > 0.05 {
                    let song_i = if loop_carousel {
                        ((ss % n_songs as i32 + n_songs as i32) % n_songs as i32) as usize
                    } else {
                        ss as usize
                    };
                    let song   = &songs[song_i];
                    let text = format!("{} — {}", song.title().to_uppercase(), song.artist().to_uppercase());
                    let pango_ctx = widget.pango_context();
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&pango::FontDescription::from_string("Audiowide 13")));
                    layout.set_text(&text);
                    let (lw, _) = layout.pixel_size();
                    snapshot.save();
                    snapshot.push_opacity(open_p);
                    snapshot.translate(&graphene::Point::new(((width - lw as f64) / 2.0) as f32, (height - 90.0) as f32));
                    let attrs = pango::AttrList::new();
                    attrs.insert(pango::AttrColor::new_foreground(0, 65535, 65535));
                    layout.set_attributes(Some(&attrs));
                    snapshot.append_layout(&layout, &gdk::RGBA::WHITE);
                    snapshot.pop();
                    snapshot.restore();
                }
            } else if let Some(hidx) = self.hovered_index.get() {
                let song_i = if loop_carousel {
                    ((hidx % n_songs as i32 + n_songs as i32) % n_songs as i32) as usize
                } else {
                    hidx as usize
                };
                if song_i < n_songs {
                    let song = &songs[song_i];
                    let text = format!("{} — {}", song.title().to_uppercase(), song.artist().to_uppercase());
                    let pango_ctx = widget.pango_context();
                    let layout = pango::Layout::new(&pango_ctx);
                    layout.set_font_description(Some(&pango::FontDescription::from_string("Audiowide 13")));
                    layout.set_text(&text);
                    let (lw, _) = layout.pixel_size();
                    snapshot.save();
                    snapshot.translate(&graphene::Point::new(((width - lw as f64) / 2.0) as f32, (height - 90.0) as f32));
                    let attrs = pango::AttrList::new();
                    attrs.insert(pango::AttrColor::new_foreground(0, 65535, 65535));
                    layout.set_attributes(Some(&attrs));
                    snapshot.append_layout(&layout, &gdk::RGBA::WHITE);
                    snapshot.restore();
                }
            }
            snapshot.pop(); // pop widget clip bounds
        }
    }
}

glib::wrapper! {
    pub struct ArchiveView(ObjectSubclass<imp::ArchiveView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ArchiveView {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn set_songs(&self, songs: Vec<crate::audio::Song>) {
        self.imp().songs.replace(songs);
        self.queue_draw();
    }

    pub fn set_selected_index(&self, index: i32) {
        self.set_property("selected-index", index);
    }

    pub fn selected_index(&self) -> i32 {
        self.property::<i32>("selected-index")
    }

    pub fn connect_activated<F: Fn(&Self, i32) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("activated", false, move |values| {
            let view = values[0].get::<ArchiveView>().unwrap();
            let idx = values[1].get::<i32>().unwrap();
            f(&view, idx);
            None
        })
    }

    pub fn connect_selection_changed<F: Fn(&Self, i32) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("selection-changed", false, move |values| {
            let view = values[0].get::<ArchiveView>().unwrap();
            let idx = values[1].get::<i32>().unwrap();
            f(&view, idx);
            None
        })
    }

    fn setup_controllers(&self) {
        self.set_focusable(true);

        // Escape key to unfocus/close
        let key_controller = gtk::EventControllerKey::new();
        key_controller.connect_key_pressed(clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::Propagation::Stop,
            move |_, keyval, _, _| {
                if keyval.name().as_ref().map(|s| s.as_str()) == Some("Escape") {
                    let sel_idx = view.imp().selected_index.get();
                    let open_progress = view.property::<f64>("open-progress");
                    if sel_idx >= 0 && open_progress > 0.0 {
                        view.animate_open_progress(0.0, sel_idx);
                        return glib::Propagation::Stop;
                    }
                }
                glib::Propagation::Proceed
            }
        ));
        self.add_controller(key_controller);

        // Scroll
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.connect_scroll(clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, dx, dy| {
                view.handle_scroll(dx, dy);
                glib::Propagation::Proceed
            }
        ));
        self.add_controller(scroll);

        // Click
        let click = gtk::GestureClick::new();
        click.connect_pressed(clone!(
            #[weak(rename_to = view)]
            self,
            move |gesture, _, x, y| {
                view.grab_focus();
                view.handle_click(x, y);
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
        ));
        self.add_controller(click);

        // Hover motion
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(clone!(
            #[weak(rename_to = view)]
            self,
            move |_, x, y| {
                view.imp().last_mouse_pos.set(Some((x, y)));
                view.handle_motion(x, y);
            }
        ));
        motion.connect_leave(clone!(
            #[weak(rename_to = view)]
            self,
            move |_| {
                view.imp().last_mouse_pos.set(None);
                view.imp().hovered_index.set(None);
                view.queue_draw();
            }
        ));
        self.add_controller(motion);
    }

    fn handle_scroll(&self, dx: f64, dy: f64) {
        let delta = if dy != 0.0 { dy } else { dx };
        let imp = self.imp();
        let n_songs = imp.songs.borrow().len() as f64;
        if n_songs == 0.0 { return; }

        // Close preview if open (without stopping song)
        let open_progress = self.property::<f64>("open-progress");
        let sel_idx = imp.selected_index.get();
        if sel_idx >= 0 && open_progress > 0.0 {
            if imp.opening_index.get() != Some(-1) {
                self.animate_open_progress(0.0, sel_idx);
            }
        }

        let target = if n_songs > 12.0 {
            let mut t = imp.target_offset.get() + delta * 0.35;
            while t < 0.0    { t += n_songs; }
            while t >= n_songs { t -= n_songs; }
            t
        } else {
            (imp.target_offset.get() + delta * 0.35).clamp(0.0, n_songs - 1.0)
        };
        imp.target_offset.set(target);
    }

    fn handle_click(&self, x: f64, y: f64) {
        let imp = self.imp();
        let songs = imp.songs.borrow();
        let n_songs = songs.len();
        if n_songs == 0 { return; }

        let width  = self.width() as f64;
        let height = self.height() as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;

        // Must match snapshot constants
        let f_size: f64 = 300.0;
        let step: f64   = 110.0;
        let disc_r: f64 = 140.0;
        let disc_slide: f64 = 210.0;
        let slv_shift: f64  = 140.0;
        let nbr_push: f64   = 270.0;

        let scroll     = imp.scroll_offset.get();
        let sel_idx    = imp.selected_index.get();
        let open_p     = self.property::<f64>("open-progress");
        let loop_carousel = n_songs > 12;

        let sel_slot: Option<i32> = {
            let (min_s, max_s) = if loop_carousel {
                ((scroll - 7.0).floor() as i32, (scroll + 7.0).ceil() as i32)
            } else {
                (0.max((scroll - 7.0).floor() as i32), (n_songs as i32 - 1).min((scroll + 7.0).ceil() as i32))
            };
            (min_s..=max_s).find(|&si| {
                let song_i = if loop_carousel {
                    ((si % n_songs as i32 + n_songs as i32) % n_songs as i32) as usize
                } else {
                    si as usize
                };
                sel_idx >= 0 && song_i == sel_idx as usize && (!loop_carousel || (si as f64 - scroll).abs() < n_songs as f64 / 2.0)
            })
        };

        let sleeve_x = |si: i32, is_sel: bool| -> f64 {
            let base = cx + (si as f64 - scroll) * step - f_size / 2.0;
            match sel_slot {
                Some(_) if is_sel   => base - slv_shift * open_p,
                Some(ss) if si < ss => base - nbr_push  * open_p,
                Some(_)             => base + nbr_push  * open_p,
                None                => base,
            }
        };

        // Check click on top/bottom black bars → close
        if sel_idx >= 0 && (y < cy - f_size / 2.0 - 20.0 || y > cy + f_size / 2.0 + 20.0) {
            self.animate_open_progress(0.0, sel_idx);
            return;
        }

        // Check click on the vinyl disc → play
        if sel_idx >= 0 && open_p > 0.0 {
            if let Some(ss) = sel_slot {
                let fx     = sleeve_x(ss, true);
                let disc_cx = fx + f_size / 2.0 + disc_slide * open_p;
                let dx = x - disc_cx;
                let dy = y - cy;
                if dx * dx + dy * dy <= disc_r * disc_r {
                    self.emit_by_name::<()>("activated", &[&sel_idx]);
                    return;
                }
            }
        }

        // Check click on a sleeve → focus it
        let (min_s, max_s) = if loop_carousel {
            ((scroll - 7.0).floor() as i32, (scroll + 7.0).ceil() as i32)
        } else {
            (0.max((scroll - 7.0).floor() as i32), (n_songs as i32 - 1).min((scroll + 7.0).ceil() as i32))
        };
        let mut check: Vec<(i32, usize, bool)> = (min_s..=max_s).map(|si| {
            let song_i = if loop_carousel {
                ((si % n_songs as i32 + n_songs as i32) % n_songs as i32) as usize
            } else {
                si as usize
            };
            let is_sel = sel_idx >= 0 && song_i == sel_idx as usize && (!loop_carousel || (si as f64 - scroll).abs() < n_songs as f64 / 2.0);
            (si, song_i, is_sel)
        }).collect();
        check.sort_by(|a, b| {
            let is_a_hov = Some(a.1 as i32) == imp.hovered_index.get();
            let is_b_hov = Some(b.1 as i32) == imp.hovered_index.get();

            if is_a_hov && !is_b_hov {
                std::cmp::Ordering::Less
            } else if !is_a_hov && is_b_hov {
                std::cmp::Ordering::Greater
            } else {
                b.0.cmp(&a.0)
            }
        });

        for &(si, song_i, is_sel) in &check {
            let fx     = sleeve_x(si, is_sel);
            let fy     = cy - f_size / 2.0;
            let body_hit = x >= fx && x <= fx + f_size && y >= fy && y <= fy + f_size;
            if body_hit {
                if is_sel && open_p > 0.5 {
                    self.animate_open_progress(0.0, song_i as i32);
                } else {
                    self.set_selected_index(song_i as i32);
                    self.set_property("open-progress", 0.0_f64);
                    self.animate_open_progress(1.0, song_i as i32);
                    self.emit_by_name::<()>("selection-changed", &[&(song_i as i32)]);
                }
                break;
            }
        }
    }

    fn handle_motion(&self, x: f64, y: f64) {
        let imp = self.imp();
        let songs = imp.songs.borrow();
        let n_songs = songs.len();
        if n_songs == 0 { return; }

        let width  = self.width() as f64;
        let height = self.height() as f64;
        let cx = width / 2.0;
        let cy = height / 2.0;

        let f_size: f64  = 300.0;
        let step: f64    = 110.0;
        let disc_r: f64  = 140.0;
        let disc_slide   = 210.0_f64;
        let slv_shift    = 140.0_f64;
        let nbr_push     = 270.0_f64;

        let scroll  = imp.scroll_offset.get();
        let sel_idx = imp.selected_index.get();
        let open_p  = self.property::<f64>("open-progress");

        let loop_carousel = n_songs > 12;
        let sel_slot: Option<i32> = {
            let (min_s, max_s) = if loop_carousel {
                ((scroll - 7.0).floor() as i32, (scroll + 7.0).ceil() as i32)
            } else {
                (0.max((scroll - 7.0).floor() as i32), (n_songs as i32 - 1).min((scroll + 7.0).ceil() as i32))
            };
            (min_s..=max_s).find(|&si| {
                let song_i = if loop_carousel {
                    ((si % n_songs as i32 + n_songs as i32) % n_songs as i32) as usize
                } else {
                    si as usize
                };
                sel_idx >= 0 && song_i == sel_idx as usize && (!loop_carousel || (si as f64 - scroll).abs() < n_songs as f64 / 2.0)
            })
        };

        let sleeve_x = |si: i32, is_sel: bool| -> f64 {
            let base = cx + (si as f64 - scroll) * step - f_size / 2.0;
            match sel_slot {
                Some(_) if is_sel  => base - slv_shift * open_p,
                Some(_) if si < sel_slot.unwrap() => base - nbr_push  * open_p,
                Some(_)             => base + nbr_push  * open_p,
                None                => base,
            }
        };

        // When a sleeve is focused AND the open animation is in progress, only
        // check hover on that sleeve + disc. Otherwise fall through to check all.
        if sel_idx >= 0 && open_p > 0.05 {
            let hit = sel_slot.map(|ss| {
                let fx = sleeve_x(ss, true);
                let fy = cy - f_size / 2.0;
                let disc_cx = fx + f_size / 2.0 + disc_slide * open_p;
                let in_sleeve = x >= fx && x <= fx + f_size && y >= fy && y <= fy + f_size;
                let dx = x - disc_cx;
                let dy = y - cy;
                in_sleeve || dx * dx + dy * dy <= disc_r * disc_r
            }).unwrap_or(false);
            let new_hover = if hit { Some(sel_idx) } else { None };
            if new_hover != imp.hovered_index.get() {
                imp.hovered_index.set(new_hover);
                self.queue_draw();
            }
            return;
        }

        // Not focused: check hover on all sleeves in right-to-left order (top-to-bottom Z-order)
        let (min_s, max_s) = if loop_carousel {
            ((scroll - 7.0).floor() as i32, (scroll + 7.0).ceil() as i32)
        } else {
            (0.max((scroll - 7.0).floor() as i32), (n_songs as i32 - 1).min((scroll + 7.0).ceil() as i32))
        };
        let mut new_hover = None;
        for si in (min_s..=max_s).rev() {
            let song_i = if loop_carousel {
                ((si % n_songs as i32 + n_songs as i32) % n_songs as i32) as usize
            } else {
                si as usize
            };
            let fx = sleeve_x(si, false);
            let fy = cy - f_size / 2.0;
            let body_hit = x >= fx && x <= fx + f_size && y >= fy && y <= fy + f_size;
            if body_hit {
                new_hover = Some(song_i as i32);
                break;
            }
        }
        if new_hover != imp.hovered_index.get() {
            imp.hovered_index.set(new_hover);
            self.queue_draw();
        }
    }

    fn animate_open_progress(&self, target: f64, idx: i32) {
        let imp = self.imp();
        let start_progress = self.property::<f64>("open-progress");
        let start_time = std::time::Instant::now();
        let anim_duration = 350.0_f64;

        if target == 1.0 {
            imp.opening_index.set(Some(idx));
        } else {
            imp.opening_index.set(Some(-1));
        }

        let tick_id = self.add_tick_callback(clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |_, _clock| {
                let elapsed = start_time.elapsed().as_millis() as f64;
                let t = (elapsed / anim_duration).clamp(0.0, 1.0);
                let ease_t = 1.0 - (1.0 - t).powi(3);
                let current = start_progress + (target - start_progress) * ease_t;
                view.set_property("open-progress", current);
                view.queue_draw();
                if t >= 1.0 {
                    if target == 0.0 {
                        view.set_selected_index(-1);
                        view.imp().opening_index.set(None);
                    }
                    view.imp().tick_id.replace(None);
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            }
        ));

        if let Some(prev) = imp.tick_id.replace(Some(tick_id)) {
            prev.remove();
        }
    }

    fn start_animation_tick(&self) {
        let last_frame_time = Cell::new(None);
        self.add_tick_callback(clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |_, clock| {
                let current_time = clock.frame_time();
                let delta_time = if let Some(prev) = last_frame_time.get() {
                    ((current_time - prev) as f64 / 1_000_000.0).min(0.1)
                } else {
                    1.0 / 60.0 // fallback to 60fps frame time
                };
                last_frame_time.set(Some(current_time));

                let imp = view.imp();
                let n_songs = imp.songs.borrow().len() as f64;
                if n_songs == 0.0 { return glib::ControlFlow::Continue; }

                let current = imp.scroll_offset.get();
                let target  = imp.target_offset.get();
                let loop_carousel = n_songs > 12.0;

                let mut diff = target - current;
                if loop_carousel {
                    if diff > n_songs / 2.0  { diff -= n_songs; }
                    else if diff < -n_songs / 2.0 { diff += n_songs; }
                }

                if diff.abs() > 0.001 {
                    // Frame-rate independent exponential decay:
                    // At 60fps, we moved by diff * 0.15 on each frame.
                    // This means we keep 0.85 of the diff.
                    // Over delta_time seconds, we keep 0.85^(delta_time * 60.0).
                    let factor = 1.0 - 0.85_f64.powf(delta_time * 60.0);
                    let next = current + diff * factor;
                    let next = if loop_carousel {
                        let mut n = next;
                        while n < 0.0     { n += n_songs; }
                        while n >= n_songs { n -= n_songs; }
                        n
                    } else {
                        next.clamp(0.0, n_songs - 1.0)
                    };
                    imp.scroll_offset.set(next);

                    // Instant evaluation of hover state under mouse during scroll
                    if let Some((x, y)) = imp.last_mouse_pos.get() {
                        view.handle_motion(x, y);
                    }

                    view.queue_draw();
                }
                glib::ControlFlow::Continue
            }
        ));
    }
}

impl Default for ArchiveView {
    fn default() -> Self {
        Self::new()
    }
}
