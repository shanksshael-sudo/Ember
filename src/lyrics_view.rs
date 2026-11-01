// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{glib, graphene, prelude::*, subclass::prelude::*, CompositeTemplate};
use crate::audio::{LyricLine, Lyrics};
use std::cell::Cell;

pub struct LineWidgetData {
    pub container: LyricRow,
    pub main_label: gtk::Label,
    pub sub_label: Option<gtk::Label>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LyricState {
    #[default]
    Inactive,
    NearActive,
    Active,
}

mod row_imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct LyricRow {
        pub state: Cell<LyricState>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LyricRow {
        const NAME: &'static str = "AmberolLyricRow";
        type Type = super::LyricRow;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for LyricRow {}
    impl WidgetImpl for LyricRow {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            match self.state.get() {
                LyricState::Active => {
                    self.parent_snapshot(snapshot);
                }
                LyricState::NearActive => {
                    snapshot.push_blur(1.2);
                    self.parent_snapshot(snapshot);
                    snapshot.pop();
                }
                LyricState::Inactive => {
                    snapshot.push_blur(2.2);
                    self.parent_snapshot(snapshot);
                    snapshot.pop();
                }
            }
        }
    }
    impl BoxImpl for LyricRow {}
}

glib::wrapper! {
    pub struct LyricRow(ObjectSubclass<row_imp::LyricRow>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl LyricRow {
    pub fn new() -> Self {
        let row: Self = glib::Object::new();
        row.set_orientation(gtk::Orientation::Vertical);
        row.set_spacing(4);
        row.set_halign(gtk::Align::Center);
        row.set_hexpand(true);
        row
    }

    pub fn set_state(&self, state: LyricState) {
        if self.imp().state.get() != state {
            self.imp().state.set(state);
            self.queue_draw();
        }
    }
}

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
        pub line_widgets: RefCell<Vec<LineWidgetData>>,
        pub current_line: RefCell<Option<usize>>,
        pub base_position_ms: Cell<u64>,
        pub last_sync_time: Cell<Option<i64>>,
        pub is_playing: Cell<bool>,
        pub last_position_ms: Cell<u64>,
        pub target_scroll: Cell<f64>,
        pub current_scroll: Cell<f64>,
        pub tick_id: RefCell<Option<gtk::TickCallbackId>>,
        pub seek_callback: RefCell<Option<Box<dyn Fn(u64) + 'static>>>,
        pub cached_active_word: Cell<Option<usize>>,
        pub cached_sub_word: Cell<Option<usize>>,
        pub user_scrolling_until: Cell<i64>,
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

            let scroll_ctrl = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::KINETIC,
            );
            scroll_ctrl.connect_scroll(glib::clone!(
                #[weak(rename_to = view)]
                obj,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, _, _| {
                    let now = glib::monotonic_time();
                    view.imp().user_scrolling_until.set(now + 4_000_000);
                    let val = view.imp().scrolled_window.vadjustment().value();
                    view.imp().current_scroll.set(val);
                    view.imp().target_scroll.set(val);
                    glib::Propagation::Proceed
                }
            ));
            self.scrolled_window.add_controller(scroll_ctrl);

            adj.connect_value_changed(glib::clone!(
                #[weak(rename_to = view)]
                obj,
                move |adj| {
                    let imp = view.imp();
                    let cur = imp.current_scroll.get();
                    let val = adj.value();
                    if (val - cur).abs() > 1.5 {
                        let now = glib::monotonic_time();
                        imp.user_scrolling_until.set(now + 4_000_000);
                        imp.current_scroll.set(val);
                        imp.target_scroll.set(val);
                    }
                }
            ));
        }

        fn dispose(&self) {
            if let Some(id) = self.tick_id.borrow_mut().take() {
                id.remove();
            }
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
                let capped_nat = nat.min(420);
                let capped_min = min.min(180);
                (capped_min, capped_nat, min_base, nat_base)
            } else {
                let capped_min = min.min(120);
                let capped_nat = nat.min(360);
                (capped_min, capped_nat, min_base, nat_base)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.overlay.allocate(width, height, baseline, None);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.parent_snapshot(snapshot);
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

    pub fn set_seek_callback<F: Fn(u64) + 'static>(&self, callback: F) {
        self.imp().seek_callback.replace(Some(Box::new(callback)));
    }

    pub fn set_blurred_cover(&self, _cover: Option<gtk::gdk::Texture>) {}

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
        imp.line_widgets.borrow_mut().clear();

        if lyrics.lines.is_empty() {
            let label = gtk::Label::builder()
                .label("// NO LYRICS AVAILABLE //")
                .halign(gtk::Align::Center)
                .justify(gtk::Justification::Center)
                .build();
            label.add_css_class("no-lyrics-label");
            imp.lyrics_box.append(&label);
        } else {
            let mut widgets = Vec::with_capacity(lyrics.lines.len());
            for line in &lyrics.lines {
                let line_box = LyricRow::new();
                line_box.add_css_class("lyric-block");

                let main_label = gtk::Label::builder()
                    .label(&line.text)
                    .halign(gtk::Align::Center)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .justify(gtk::Justification::Center)
                    .build();
                if line.is_background {
                    main_label.add_css_class("lyric-sub-line");
                } else {
                    main_label.add_css_class("lyric-line");
                }
                line_box.append(&main_label);

                let sub_label = if let Some(ref sub) = line.sub_line {
                    let s_label = gtk::Label::builder()
                        .label(&sub.text)
                        .halign(gtk::Align::Center)
                        .wrap(true)
                        .wrap_mode(gtk::pango::WrapMode::WordChar)
                        .justify(gtk::Justification::Center)
                        .build();
                    s_label.add_css_class("lyric-sub-line");
                    line_box.append(&s_label);
                    Some(s_label)
                } else {
                    None
                };

                // Click-to-seek gesture on each lyric line
                let click = gtk::GestureClick::new();
                let line_time = line.time;
                click.connect_pressed(glib::clone!(
                    #[weak(rename_to = view)]
                    self,
                    move |_, _, _, _| {
                        let imp = view.imp();
                        imp.user_scrolling_until.set(0);
                        let cb_opt = imp.seek_callback.borrow();
                        if let Some(ref cb) = *cb_opt {
                            cb(line_time);
                        }
                    }
                ));
                line_box.add_controller(click);

                imp.lyrics_box.append(&line_box);
                widgets.push(LineWidgetData {
                    container: line_box,
                    main_label,
                    sub_label,
                });
            }
            *imp.line_widgets.borrow_mut() = widgets;
        }

        imp.lyrics.replace(lyrics);
        imp.current_line.replace(None);
        imp.last_position_ms.set(0);
        imp.base_position_ms.set(0);
        imp.last_sync_time.set(None);
        imp.cached_active_word.set(None);
        imp.cached_sub_word.set(None);
        imp.user_scrolling_until.set(0);
    }

    pub fn resume_autoscroll(&self) {
        let imp = self.imp();
        imp.user_scrolling_until.set(0);
        self.scroll_to_active();
    }

    pub fn scroll_to_active(&self) {
        let imp = self.imp();
        let now = glib::monotonic_time();
        if now < imp.user_scrolling_until.get() {
            return;
        }
        let best_line = *imp.current_line.borrow();
        if let Some(idx) = best_line {
            let widgets = imp.line_widgets.borrow();
            if let Some(line_data) = widgets.get(idx) {
                if let Some(point) = line_data.container.compute_point(
                    &*imp.lyrics_box,
                    &graphene::Point::zero(),
                ) {
                    let widget_y = point.y() as f64;
                    let widget_h = line_data.container.height() as f64;
                    let widget_center = widget_y + widget_h / 2.0;

                    let adj = imp.scrolled_window.vadjustment();
                    let viewport_h = adj.page_size();

                    if viewport_h > 0.0 {
                        // Place active line around 38% from top for optimal visibility
                        let target = (widget_center - viewport_h * 0.38)
                            .max(0.0)
                            .min((adj.upper() - adj.page_size()).max(0.0));

                        imp.target_scroll.set(target);
                        self.ensure_tick_callback();
                    }
                }
            }
        }
    }

    pub fn update_position(&self, position_ms: u64, is_playing: bool) {
        let imp = self.imp();
        imp.base_position_ms.set(position_ms);
        imp.last_sync_time.set(Some(glib::monotonic_time()));
        imp.is_playing.set(is_playing);

        self.render_frame_at(position_ms);
    }

    fn render_frame_at(&self, position_ms: u64) {
        let imp = self.imp();
        let lyrics = imp.lyrics.borrow();
        if lyrics.lines.is_empty() {
            return;
        }

        let best_line = lyrics.lines.iter().rposition(|l| l.time <= position_ms);

        let last_pos = imp.last_position_ms.get();
        let is_seek = (position_ms as i64 - last_pos as i64).abs() > 1500;
        imp.last_position_ms.set(position_ms);

        let line_changed = best_line != *imp.current_line.borrow();

        if line_changed || is_seek {
            let old_idx = *imp.current_line.borrow();
            imp.current_line.replace(best_line);
            imp.cached_active_word.set(None);
            imp.cached_sub_word.set(None);

            let widgets = imp.line_widgets.borrow();

            // Reset old line styles and plain text (return them to Inactive blurred state)
            if let Some(old) = old_idx {
                if let Some(w) = widgets.get(old) {
                    w.container.set_state(LyricState::Inactive);
                    w.container.remove_css_class("active");
                    w.container.remove_css_class("near-active");
                    w.main_label.set_text(&lyrics.lines[old].text);
                    if let (Some(ref sub_label), Some(ref sub)) = (&w.sub_label, &lyrics.lines[old].sub_line) {
                        sub_label.set_text(&sub.text);
                    }
                }
                if old > 0 {
                    if let Some(w) = widgets.get(old - 1) {
                        w.container.set_state(LyricState::Inactive);
                        w.container.remove_css_class("near-active");
                    }
                }
                if let Some(w) = widgets.get(old + 1) {
                    w.container.set_state(LyricState::Inactive);
                    w.container.remove_css_class("near-active");
                }
            }

            // Set new active and near-active line styles
            if let Some(new) = best_line {
                if let Some(w) = widgets.get(new) {
                    w.container.set_state(LyricState::Active);
                    w.container.add_css_class("active");
                    w.container.remove_css_class("near-active");
                }
                if new > 0 {
                    if let Some(w) = widgets.get(new - 1) {
                        w.container.set_state(LyricState::NearActive);
                        w.container.add_css_class("near-active");
                    }
                }
                if let Some(w) = widgets.get(new + 1) {
                    w.container.set_state(LyricState::NearActive);
                    w.container.add_css_class("near-active");
                }
            }

            self.scroll_to_active();
        }

        // Real-time word-by-word karaoke highlight for the active line
        if let Some(curr) = *imp.current_line.borrow() {
            let widgets = imp.line_widgets.borrow();
            if let (Some(w), Some(line)) = (widgets.get(curr), lyrics.lines.get(curr)) {
                // Update main line
                if !line.words.is_empty() {
                    let active_word_idx = line.words.iter().rposition(|word| word.time <= position_ms).unwrap_or(0);
                    let is_last_word_done = active_word_idx + 1 == line.words.len() && position_ms > line.words[active_word_idx].time + 800;
                    let cache_key = if is_last_word_done { (active_word_idx << 1) | 1 } else { active_word_idx << 1 };
                    if imp.cached_active_word.get() != Some(cache_key) {
                        imp.cached_active_word.set(Some(cache_key));
                        let markup = LyricLine::generate_karaoke_markup(&line.words, position_ms);
                        w.main_label.set_markup(&markup);
                    }
                } else if imp.cached_active_word.get() != Some(0) {
                    imp.cached_active_word.set(Some(0));
                    let escaped = glib::markup_escape_text(&line.text);
                    w.main_label.set_markup(&format!("<span foreground=\"#00ffff\" weight=\"bold\">{}</span>", escaped));
                }

                // Update subline if present
                if let (Some(ref sub_label), Some(ref sub)) = (&w.sub_label, &line.sub_line) {
                    if !sub.words.is_empty() {
                        let sub_word_idx = sub.words.iter().rposition(|word| word.time <= position_ms).unwrap_or(0);
                        let is_last_sub_done = sub_word_idx + 1 == sub.words.len() && position_ms > sub.words[sub_word_idx].time + 800;
                        let sub_cache_key = if is_last_sub_done { (sub_word_idx << 1) | 1 } else { sub_word_idx << 1 };
                        if imp.cached_sub_word.get() != Some(sub_cache_key) {
                            imp.cached_sub_word.set(Some(sub_cache_key));
                            let markup = LyricLine::generate_karaoke_markup(&sub.words, position_ms);
                            sub_label.set_markup(&markup);
                        }
                    } else {
                        let active = position_ms >= sub.time;
                        let active_key = if active { 1 } else { 0 };
                        if imp.cached_sub_word.get() != Some(active_key) {
                            imp.cached_sub_word.set(Some(active_key));
                            let escaped = glib::markup_escape_text(&sub.text);
                            if active {
                                sub_label.set_markup(&format!("<span foreground=\"#00ffff\" weight=\"bold\">{}</span>", escaped));
                            } else {
                                sub_label.set_markup(&format!("<span foreground=\"#7a7a88\" weight=\"medium\">{}</span>", escaped));
                            }
                        }
                    }
                }
            }
        }
    }

    fn ensure_tick_callback(&self) {
        let imp = self.imp();
        if imp.tick_id.borrow().is_some() {
            return;
        }

        let last_frame_time = Cell::new(None);
        let tick_id = self.add_tick_callback(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |_, clock| {
                let current_time = clock.frame_time();
                let delta_time = if let Some(prev) = last_frame_time.get() {
                    ((current_time - prev) as f64 / 1_000_000.0).clamp(0.001, 0.1)
                } else {
                    1.0 / 60.0
                };
                last_frame_time.set(Some(current_time));

                let imp = view.imp();

                // Silky smooth exponential autoscroll
                let now = glib::monotonic_time();
                if now >= imp.user_scrolling_until.get() {
                    let target = imp.target_scroll.get();
                    let adj = imp.scrolled_window.vadjustment();
                    let current = adj.value();
                    let diff = target - current;

                    if diff.abs() > 0.5 {
                        let factor = 1.0 - (-8.0 * delta_time).exp();
                        let next = current + diff * factor;
                        imp.current_scroll.set(next);
                        adj.set_value(next);
                    } else if diff.abs() > 0.001 {
                        imp.current_scroll.set(target);
                        adj.set_value(target);
                    }
                }

                // Stop tick callback as soon as scroll animation reaches target
                let needs_scroll = (imp.target_scroll.get() - imp.scrolled_window.vadjustment().value()).abs() > 0.5;
                if needs_scroll {
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
