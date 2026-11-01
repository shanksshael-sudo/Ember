// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    str::FromStr,
    time::Instant,
};

use gtk::subclass::prelude::*;
use glib::clone;
use gtk::{gdk, gio, glib, prelude::*, CompositeTemplate};
use log::debug;


use crate::{
    audio::{AudioPlayer, RepeatMode, ReplayGainMode, Song},
    config::APPLICATION_ID,
    drag_overlay::DragOverlay,
    i18n::{i18n, i18n_k, ni18n_f, ni18n_k},
    playback_control::PlaybackControl,
    playlist_view::PlaylistView,
    queue_row::QueueRow,
    search::FuzzyFilter,
    song_cover::SongCover,
    song_details::SongDetails,
    lyrics_view::LyricsView,
    vinyl_view::VinylView,
    archive_view::ArchiveView,
    sort::FuzzySorter,
    utils,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    InitialView,
    ArchiveView,
    NowPlayingView,
}

const ATTRIBUTE_HOST_PATH: &str = "xattr::document-portal.host-path";

mod imp {
    use glib::{ParamSpec, ParamSpecBoolean, ParamSpecEnum, Value};
    use once_cell::sync::Lazy;

    use super::*;

    #[derive(CompositeTemplate)]
    #[template(resource = "/io/bassi/Amberol/window.ui")]
    pub struct Window {
        // Template widgets
        #[template_child]
        pub drag_overlay: TemplateChild<DragOverlay>,
        #[template_child]
        pub main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub archive_view: TemplateChild<ArchiveView>,
        #[template_child]
        pub song_details: TemplateChild<SongDetails>,
        #[template_child]
        pub vinyl_view: TemplateChild<VinylView>,

        #[template_child]
        pub hover_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub position_progress: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub remaining_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub playback_control: TemplateChild<PlaybackControl>,
        #[template_child]
        pub playlist_view: TemplateChild<PlaylistView>,
        #[template_child]
        pub lyrics_view: TemplateChild<LyricsView>,
        #[template_child]
        pub add_folder_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub restore_playlist_button: TemplateChild<gtk::Button>,


        pub provider: gtk::CssProvider,
        pub settings: gio::Settings,

        pub playlist_shuffled: Cell<bool>,
        pub playlist_visible: Cell<bool>,
        pub playlist_selection: Cell<bool>,
        pub playlist_search: Cell<bool>,
        pub replaygain_mode: Cell<ReplayGainMode>,

        pub playlist_filtermodel: RefCell<Option<gio::ListModel>>,

        pub notify_playing_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_position_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_song_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_cover_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_nsongs_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_current_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_peaks_id: RefCell<Option<glib::SignalHandlerId>>,
        pub seeking_until: Cell<i64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "AmberolWindow";
        type Type = super::Window;
        type ParentType = gtk::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action("win.play", None, move |win, _, _| {
                debug!("Window::win.play()");
                if let Some(p) = win.player() {
                    p.toggle_play();
                }
            });
            klass.install_action("win.seek-backwards", None, move |win, _, _| {
                debug!("Window::win.seek-backwards");
                if let Some(p) = win.player() {
                    p.seek_backwards();
                }
            });
            klass.install_action("win.seek-forward", None, move |win, _, _| {
                debug!("Window::win.seek-forward");
                if let Some(p) = win.player() {
                    p.seek_forward();
                }
            });
            klass.install_action("win.previous", None, move |win, _, _| {
                debug!("Window::win.previous()");
                if let Some(p) = win.player() {
                    p.skip_previous();
                }
            });
            klass.install_action("win.next", None, move |win, _, _| {
                debug!("Window::win.next()");
                if let Some(p) = win.player() {
                    p.skip_next();
                }
            });
            klass.install_action("win.show-archive", None, move |win, _, _| {
                debug!("Window::win.show-archive");
                win.switch_mode(WindowMode::ArchiveView);
            });
            klass.install_action("win.show-main-menu", None, move |win, _, _| {
                debug!("Window::win.show-main-menu");
                if let Some(player) = win.player() {
                    if player.queue().is_empty() {
                        win.switch_mode(WindowMode::InitialView);
                    } else {
                        win.switch_mode(WindowMode::NowPlayingView);
                    }
                } else {
                    win.switch_mode(WindowMode::InitialView);
                }
            });
            klass.install_action("queue.repeat-mode", None, move |win, _, _| {
                debug!("Window::queue.repeat()");
                if let Some(p) = win.player() {
                    p.toggle_repeat_mode();
                }
            });
            klass.install_action_async("queue.add-song", None, |win, _, _| async move {
                debug!("Window::win.add-song()");
                let filters = gio::ListStore::new::<gtk::FileFilter>();
                let filter = gtk::FileFilter::new();
                gtk::FileFilter::set_name(&filter, Some(&i18n("Audio files")));
                filter.add_mime_type("audio/*");
                filters.append(&filter);

                let xdg_music = match glib::user_special_dir(glib::UserDirectory::Music) {
                    Some(p) => p,
                    None => glib::current_dir(),
                };

                let dialog = gtk::FileDialog::builder()
                    .accept_label(&i18n("_Add Song"))
                    .filters(&filters)
                    .initial_folder(&gio::File::for_path(&xdg_music))
                    .modal(true)
                    .title(&i18n("Open File"))
                    .build();

                if let Ok(files) = dialog.open_multiple_future(Some(&win)).await {
                    if files.n_items() == 0 {
                        win.add_toast(i18n("Unable to access files"));
                    } else {
                        if let Some(name) = win.imp().main_stack.visible_child_name() {
                            if name == "initial-view" {
                                if let Some(player) = win.player() {
                                    player.clear_queue();
                                }
                            }
                        }
                        win.add_files_to_queue(&files);
                    }
                }
            });
            klass.install_action_async("queue.add-folder", None, |win, _, _| async move {
                debug!("Window::win.add-folder()");
                let xdg_music = match glib::user_special_dir(glib::UserDirectory::Music) {
                    Some(p) => p,
                    None => glib::current_dir(),
                };

                let dialog = gtk::FileDialog::builder()
                    .accept_label(&i18n("_Add Folder"))
                    .initial_folder(&gio::File::for_path(&xdg_music))
                    .modal(true)
                    .title(&i18n("Open Folder"))
                    .build();

                if let Ok(files) = dialog.select_multiple_folders_future(Some(&win)).await {
                    if files.n_items() == 0 {
                        win.add_toast(i18n("Unable to access files"));
                    } else {
                        if let Some(name) = win.imp().main_stack.visible_child_name() {
                            if name == "initial-view" {
                                if let Some(player) = win.player() {
                                    player.clear_queue();
                                }
                            }
                        }
                        win.add_files_to_queue(&files);
                    }
                }
            });
            klass.install_action("queue.restore-playlist", None, move |win, _, _| {
                debug!("Window::queue.restore-playlist()");
                win.restore_playlist();
            });
            klass.install_action("win.copy", None, move |win, _, _| {
                debug!("Window::win.copy()");
                win.copy_song();
            });
            klass.install_action_async("win.open-directory", None, async move |win, _, _| {
                debug!("Window::win.open-directory()");
                win.open_song_directory().await
            });
            klass.install_action("queue.clear", None, move |win, _, _| {
                debug!("Window::queue.clear()");
                win.clear_queue();
            });
            klass.install_property_action("queue.toggle", "playlist-visible");
            klass.install_property_action("queue.shuffle", "playlist-shuffled");
            klass.install_property_action("queue.select", "playlist-selection");
            klass.install_property_action("queue.search", "playlist-search");
            klass.install_property_action("win.replaygain", "replaygain-mode");

            klass.install_action(
                "win.skip-to",
                Some(glib::VariantTy::UINT32),
                move |win, _, param| {
                    if let Some(pos) = param.and_then(u32::from_variant) {
                        if let Some(player) = win.player() {
                            player.skip_to(pos);
                            player.play();
                        }
                    }
                },
            );
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            DragOverlay::static_type();
            SongCover::static_type();
            SongDetails::static_type();
            PlaybackControl::static_type();
            PlaylistView::static_type();
            LyricsView::static_type();
            ArchiveView::static_type();
            VinylView::static_type();
            obj.init_template();
        }

        fn new() -> Self {
            Self {
                drag_overlay: TemplateChild::default(),
                song_details: TemplateChild::default(),
                archive_view: TemplateChild::default(),
                playback_control: TemplateChild::default(),
                vinyl_view: TemplateChild::default(),

                hover_label: TemplateChild::default(),
                position_progress: TemplateChild::default(),
                remaining_label: TemplateChild::default(),
                main_stack: TemplateChild::default(),
                add_folder_button: TemplateChild::default(),
                restore_playlist_button: TemplateChild::default(),
                playlist_view: TemplateChild::default(),
                lyrics_view: TemplateChild::default(),

                playlist_shuffled: Cell::new(false),
                playlist_visible: Cell::new(true),
                playlist_selection: Cell::new(false),
                playlist_search: Cell::new(false),
                playlist_filtermodel: RefCell::default(),
                replaygain_mode: Cell::new(ReplayGainMode::default()),
                provider: gtk::CssProvider::new(),
                settings: utils::settings_manager(),
                notify_playing_id: RefCell::new(None),
                notify_position_id: RefCell::new(None),
                notify_song_id: RefCell::new(None),
                notify_cover_id: RefCell::new(None),
                notify_nsongs_id: RefCell::new(None),
                notify_current_id: RefCell::new(None),
                notify_peaks_id: RefCell::new(None),
                seeking_until: Cell::new(0),
            }
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();

            if APPLICATION_ID.ends_with("Devel") {
                self.obj().add_css_class("devel");
            }
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecBoolean::builder("playlist-shuffled").build(),
                    ParamSpecBoolean::builder("playlist-visible").build(),
                    ParamSpecBoolean::builder("playlist-selection").build(),
                    ParamSpecBoolean::builder("playlist-search").build(),
                    ParamSpecEnum::builder::<ReplayGainMode>("replaygain-mode").build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            let obj = self.obj();
            match pspec.name() {
                "playlist-shuffled" => obj.set_playlist_shuffled(value.get::<bool>().unwrap()),
                "playlist-visible" => obj.set_playlist_visible(value.get::<bool>().unwrap()),
                "playlist-selection" => obj.set_playlist_selection(value.get::<bool>().unwrap()),
                "playlist-search" => obj.set_playlist_search(value.get::<bool>().unwrap()),
                "replaygain-mode" => obj.set_replaygain(value.get::<ReplayGainMode>().unwrap()),
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            let obj = self.obj();
            match pspec.name() {
                "playlist-shuffled" => obj.playlist_shuffled().to_value(),
                "playlist-visible" => obj.playlist_visible().to_value(),
                "playlist-selection" => obj.playlist_selection().to_value(),
                "playlist-search" => obj.playlist_search().to_value(),
                "replaygain-mode" => obj.replaygain().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        let win = glib::Object::builder::<Window>()
            .property("application", application)
            .build();

        win.setup_waveform();
        win.setup_actions();
        win.setup_playlist();
        win.setup_drop_target();
        win.setup_provider();
        win.bind_state();
        win.bind_queue();
        win.connect_signals();
        win.restore_window_state();
        win.set_initial_state();

        win
    }

    fn player(&self) -> Option<Rc<AudioPlayer>> {
        if let Some(app) = self.application() {
            let player = app
                .downcast::<crate::application::Application>()
                .unwrap()
                .player();
            return Some(player);
        }

        None
    }

    fn setup_actions(&self) {
        let enable_recoloring = self.imp().settings.boolean("enable-recoloring");
        self.add_action_entries([gio::ActionEntry::builder("enable-recoloring")
            .state(enable_recoloring.to_variant())
            .activate(|this: &Window, action, _| {
                let state = action.state().unwrap();
                let action_state: bool = state.get().unwrap();
                let enable_recoloring = !action_state;
                action.set_state(&enable_recoloring.to_variant());

                this.imp()
                    .settings
                    .set_boolean("enable-recoloring", enable_recoloring)
                    .expect("Unable to store setting");
            })
            .build()])
    }

    fn setup_waveform(&self) {}

    fn unbind_waveform(&self) {}


    fn restore_window_state(&self) {
        let settings = utils::settings_manager();
        let mut width = settings.int("window-width");
        let mut height = settings.int("window-height");

        if width < 640 { width = 1024; }
        if height < 480 { height = 680; }

        if let Some(display) = gdk::Display::default() {
            let monitors = display.monitors();
            if let Some(monitor) = monitors.item(0).and_then(|m| m.downcast::<gdk::Monitor>().ok()) {
                let geom = monitor.geometry();
                let max_w = (geom.width() - 60).max(640);
                let max_h = (geom.height() - 80).max(480);
                width = width.min(max_w).min(1048);
                height = height.min(max_h).min(700);
            } else {
                width = width.min(1048);
                height = height.min(700);
            }
        } else {
            width = width.min(1048);
            height = height.min(700);
        }

        self.set_default_size(width, height);
    }

    fn reset_queue(&self) {
        self.set_playlist_visible(false);
        self.set_playlist_shuffled(false);
        self.set_playlist_selection(false);
        self.update_style(None);
    }

    fn clear_queue(&self) {
        if let Some(p) = self.player() {
            p.clear_queue();
        }
    }

    fn playlist_visible(&self) -> bool {
        self.imp().playlist_visible.get()
    }

    fn set_playlist_visible(&self, _visible: bool) {
        if false != self.imp().playlist_visible.replace(false) {
            self.imp().playlist_view.set_visible(false);
            self.notify("playlist-visible");
        }
    }

    fn playlist_shuffled(&self) -> bool {
        self.imp().playlist_shuffled.get()
    }

    fn set_playlist_shuffled(&self, shuffled: bool) {
        let imp = self.imp();

        if shuffled != imp.playlist_shuffled.replace(shuffled) {
            if let Some(player) = self.player() {
                let queue = player.queue();
                let state = player.state();
                let reset_song = queue.is_first_song() && !state.playing();

                queue.set_shuffled(shuffled);

                if reset_song {
                    player.skip_to(0);
                }
            }

            self.notify("playlist-shuffled");
        }
    }

    fn playlist_selection(&self) -> bool {
        self.imp().playlist_selection.get()
    }

    fn set_playlist_selection(&self, selection: bool) {
        let imp = self.imp();

        if selection != imp.playlist_selection.replace(selection) {
            if !selection {
                if let Some(player) = self.player() {
                    let queue = player.queue();
                    queue.unselect_all_songs();
                }
            }

            self.imp()
                .playlist_view
                .queue_actionbar()
                .set_revealed(selection);

            self.notify("playlist-selection");
        }
    }

    fn playlist_search(&self) -> bool {
        self.imp().playlist_search.get()
    }

    fn set_playlist_search(&self, search: bool) {
        let imp = self.imp();

        if search != imp.playlist_search.replace(search) {
            if search {
                self.set_playlist_visible(true);
            }
            imp.playlist_view.set_search(search);
            self.notify("playlist-search");
        }
    }

    pub fn set_repeat_mode(&self, repeat_mode: RepeatMode) {
        self.imp().playback_control.set_repeat_mode(repeat_mode);
    }

    fn restore_playlist(&self) {
        if let Some(songs) = utils::load_cached_songs() {
            self.queue_songs(songs);
        }
    }

    fn queue_songs(&self, queue: Vec<gio::File>) {
        if queue.is_empty() {
            self.add_toast(i18n("No available song found"));
            return;
        }

        self.switch_mode(WindowMode::ArchiveView);

        // Disable actions on the queue; loading is "atomic"
        self.action_set_enabled("queue.add-song", false);
        self.action_set_enabled("queue.add-folder", false);
        self.action_set_enabled("queue.clear", false);

        self.set_playlist_visible(true);
        self.imp().playlist_view.begin_loading();

        // Begin the trace
        let now = Instant::now();

        // Turn the list of files into songs one at a time into the main loop
        let n_files = queue.len() as u32;

        let mut files = queue.into_iter();
        let mut songs = Vec::new();
        let mut cur_file: u32 = 0;
        let mut duplicates: u32 = 0;

        glib::idle_add_local(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                files
                    .next()
                    .map(|f| {
                        win.imp().playlist_view.update_loading(cur_file, n_files);
                        if let Ok(s) = Song::from_uri(f.uri().as_str()) {
                            if let Some(player) = win.player() {
                                let queue = player.queue();
                                if queue.contains(&s) {
                                    duplicates += 1;
                                } else {
                                    songs.push(s);
                                    cur_file += 1;
                                }
                            }
                        }
                    })
                    .map(|_| glib::ControlFlow::Continue)
                    .unwrap_or_else(|| {
                        debug!(
                            "Total loading time for {} files: {} ms",
                            n_files,
                            now.elapsed().as_millis()
                        );

                        // Re-enable the actions
                        win.action_set_enabled("queue.add-song", true);
                        win.action_set_enabled("queue.add-folder", true);
                        win.action_set_enabled("queue.clear", true);

                        if songs.is_empty() {
                            if duplicates == 0 {
                                win.add_toast(i18n("No songs found"));
                            }
                        } else if let Some(player) = win.player() {
                            let queue = player.queue();
                            let was_empty = queue.is_empty();

                            win.imp().playlist_view.end_loading();

                            // Bulk add to avoid hammering the UI with list model updates
                            queue.add_songs(&songs);

                            // Store the current state of the playlist
                            utils::store_playlist(queue);

                            debug!(
                                "Queue was empty: {}, new size: {}",
                                was_empty,
                                queue.n_songs()
                            );
                            if was_empty {
                                player.skip_to(0);
                            }

                            // Allow jumping to the song we just added
                            if songs.len() == 1 {
                                // If we added a single song, and the queue was empty, we
                                // dispense with the pleasantries and we start playing
                                // immediately; otherwise, we let the user choose whether
                                // to jump to the newly added song
                                if was_empty {
                                    player.play();
                                } else {
                                    win.add_skip_to_toast(
                                        i18n("Added a new song"),
                                        i18n("Play"),
                                        queue.n_songs() - 1,
                                    );
                                }
                            } else {
                                let msg = ni18n_f(
                                    // Translators: the `{}` must be left unmodified;
                                    // it will be expanded to the number of songs added
                                    // to the playlist
                                    "Added one song",
                                    "Added {} songs",
                                    songs.len() as u32,
                                    &[&songs.len().to_string()],
                                );

                                win.add_toast(msg);
                            }
                        }

                        glib::ControlFlow::Break
                    })
            }
        ));
    }

    fn add_files_to_queue(&self, model: &gio::ListModel) {
        let mut queue: Vec<gio::File> = vec![];

        for pos in 0..model.n_items() {
            let file = model.item(pos).unwrap().downcast::<gio::File>().unwrap();

            if let Ok(info) = file.query_info(
                "standard::name,standard::display-name,standard::type,standard::content-type",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                gio::Cancellable::NONE,
            ) {
                match info.file_type() {
                    gio::FileType::Regular => {
                        let is_audio = if let Some(content_type) = info.content_type() {
                            gio::content_type_is_mime_type(&content_type, "audio/*")
                        } else {
                            false
                        } || {
                            file.path().map_or(false, |p| {
                                matches!(
                                    p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
                                    Some("mp3" | "flac" | "ogg" | "wav" | "m4a" | "aac" | "opus" | "wma")
                                )
                            })
                        };
                        if is_audio {
                            debug!("Adding file '{}' to the queue", file.uri());
                            queue.push(file);
                        }
                    }
                    gio::FileType::Directory => {
                        debug!("Adding folder '{}' to the queue", file.uri());
                        let files = utils::load_files_from_folder(&file, true);
                        queue.extend(files);
                    }
                    _ => (),
                }
            }
        }

        self.queue_songs(queue);
    }

    // Bind the PlayerState to the UI
    fn bind_state(&self) {
        let imp = self.imp();
        if let Some(player) = self.player() {
            let state = player.state();

            // Use the PlayerState:playing property to control the play/pause button
            self.update_play_button();
            let notify_playing_id = state.connect_notify_local(
                Some("playing"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |_, _| {
                        win.set_playlist_selection(false);
                        win.update_play_button();
                    }
                ),
            );
            imp.notify_playing_id.replace(Some(notify_playing_id));

            // Update the position labels
            self.update_position_labels();
            let notify_position_id = state.connect_notify_local(
                Some("position"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |_, _| {
                        win.update_position_labels();
                    }
                ),
            );
            imp.notify_position_id.replace(Some(notify_position_id));

            // Update the UI
            self.update_song();
            let notify_song_id = state.connect_notify_local(
                Some("song"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |_, _| {
                        win.update_song();
                    }
                ),
            );
            imp.notify_song_id.replace(Some(notify_song_id));

            // Update the cover, if any is available
            self.update_cover();
            let notify_cover_id = state.connect_notify_local(
                Some("cover"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |_, _| {
                        win.update_cover();
                    }
                ),
            );
            imp.notify_cover_id.replace(Some(notify_cover_id));

            // Bind the song properties to the UI
            state
                .bind_property("title", &imp.song_details.get().title_label(), "label")
                .sync_create()
                .build();
            state
                .bind_property("artist", &imp.song_details.get().artist_label(), "label")
                .sync_create()
                .build();

        }
    }

    fn unbind_state(&self) {
        if let Some(player) = self.player() {
            let state = player.state();

            if let Some(id) = self.imp().notify_playing_id.take() {
                state.disconnect(id);
            }
            if let Some(id) = self.imp().notify_position_id.take() {
                state.disconnect(id);
            }
            if let Some(id) = self.imp().notify_song_id.take() {
                state.disconnect(id);
            }
            if let Some(id) = self.imp().notify_cover_id.take() {
                state.disconnect(id);
            }
        }
    }

    // Bind the Queue to the UI
    fn bind_queue(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();

            let notify_nsongs_id = queue.connect_notify_local(
                Some("n-songs"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |queue, _| {
                        debug!("queue.n_songs() = {}", queue.n_songs());
                        if queue.is_empty() {
                            win.set_initial_state();
                            win.reset_queue();
                        } else {
                            win.action_set_enabled("queue.toggle", true);
                            win.action_set_enabled("queue.shuffle", queue.n_songs() > 1);

                            win.action_set_enabled("win.play", true);
                            win.action_set_enabled("win.previous", true);
                            win.action_set_enabled("win.next", queue.n_songs() > 1);
                        }

                        if queue.n_songs() == 1 {
                            if let Some(p) = win.player() {
                                p.skip_next();
                            }
                        }

                        win.update_playlist_time();
                        win.update_archive_songs();
                        win.update_upcoming_list();
                    }
                ),
            );
            self.imp().notify_nsongs_id.replace(Some(notify_nsongs_id));

            queue.connect_notify_local(
                Some("repeat-mode"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |queue, _| {
                        win.set_repeat_mode(queue.repeat_mode());
                    }
                ),
            );

            let notify_current_id = queue.connect_notify_local(
                Some("current"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |queue, _| {
                        if let Some(idx) = queue.current_song_index() {
                            win.imp().archive_view.set_selected_index(idx as i32);
                        }
                        win.update_upcoming_list();
                        if queue.is_last_song() {
                            match queue.repeat_mode() {
                                RepeatMode::Consecutive => {
                                    win.action_set_enabled("win.next", false);
                                }
                                _ => {
                                    win.action_set_enabled("win.next", true);
                                }
                            }
                        } else {
                            win.action_set_enabled("win.next", true);
                        }
                    }
                ),
            );
            self.imp()
                .notify_current_id
                .replace(Some(notify_current_id));

            self.update_archive_songs();
            if let Some(idx) = queue.current_song_index() {
                self.imp().archive_view.set_selected_index(idx as i32);
            }
        }
    }

    fn unbind_queue(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            if let Some(id) = self.imp().notify_nsongs_id.take() {
                queue.disconnect(id);
            }
            if let Some(id) = self.imp().notify_current_id.take() {
                queue.disconnect(id);
            }
        }
    }

    fn connect_signals(&self) {
        let imp = self.imp();

        // ── Global keyboard shortcuts ──────────────────────────────────────
        let key_ctrl = gtk::EventControllerKey::new();
        key_ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
        key_ctrl.connect_key_pressed(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, keyval, _, modifier| {
                // Skip if a text field has focus
                if let Some(focus) = gtk::prelude::GtkWindowExt::focus(&win) {
                    if focus.is::<gtk::Entry>() || focus.is::<gtk::SearchEntry>() {
                        return glib::Propagation::Proceed;
                    }
                }

                // Mask off lock keys (CapsLock, NumLock, ScrollLock)
                let clean_mods = modifier & (
                    gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK
                );
                let no_mod = clean_mods.is_empty();
                let ctrl_only = clean_mods == gdk::ModifierType::CONTROL_MASK;

                match keyval {
                    // Play / Pause toggle
                    gdk::Key::space | gdk::Key::k | gdk::Key::K | gdk::Key::Return if no_mod => {
                        if let Some(p) = win.player() {
                            if p.state().playing() {
                                p.pause();
                            } else {
                                p.play();
                            }
                        }
                    }
                    gdk::Key::AudioPlay | gdk::Key::AudioPause => {
                        if let Some(p) = win.player() {
                            if p.state().playing() {
                                p.pause();
                            } else {
                                p.play();
                            }
                        }
                    }

                    // Seek backward 5s
                    gdk::Key::Left | gdk::Key::j | gdk::Key::J | gdk::Key::h | gdk::Key::H | gdk::Key::comma if no_mod => {
                        if let Some(p) = win.player() {
                            p.seek_offset(-5);
                        }
                    }

                    // Seek forward 5s
                    gdk::Key::Right | gdk::Key::l | gdk::Key::L | gdk::Key::period if no_mod => {
                        if let Some(p) = win.player() {
                            p.seek_offset(5);
                        }
                    }

                    // Previous song
                    gdk::Key::Left if ctrl_only => {
                        if let Some(p) = win.player() {
                            p.skip_previous();
                        }
                    }
                    gdk::Key::p | gdk::Key::P | gdk::Key::less if no_mod => {
                        if let Some(p) = win.player() {
                            p.skip_previous();
                        }
                    }
                    gdk::Key::AudioPrev => {
                        if let Some(p) = win.player() {
                            p.skip_previous();
                        }
                    }

                    // Next song
                    gdk::Key::Right if ctrl_only => {
                        if let Some(p) = win.player() {
                            p.skip_next();
                        }
                    }
                    gdk::Key::n | gdk::Key::N | gdk::Key::greater if no_mod => {
                        if let Some(p) = win.player() {
                            p.skip_next();
                        }
                    }
                    gdk::Key::AudioNext => {
                        if let Some(p) = win.player() {
                            p.skip_next();
                        }
                    }

                    // Stop
                    gdk::Key::AudioStop => {
                        if let Some(p) = win.player() {
                            p.stop();
                        }
                    }

                    // Volume control
                    gdk::Key::Up if no_mod => {
                        if let Some(p) = win.player() {
                            let cur = p.state().volume();
                            p.set_volume((cur + 0.05).min(1.0));
                        }
                    }
                    gdk::Key::Down if no_mod => {
                        if let Some(p) = win.player() {
                            let cur = p.state().volume();
                            p.set_volume((cur - 0.05).max(0.0));
                        }
                    }
                    gdk::Key::AudioRaiseVolume => {
                        if let Some(p) = win.player() {
                            let cur = p.state().volume();
                            p.set_volume((cur + 0.05).min(1.0));
                        }
                    }
                    gdk::Key::AudioLowerVolume => {
                        if let Some(p) = win.player() {
                            let cur = p.state().volume();
                            p.set_volume((cur - 0.05).max(0.0));
                        }
                    }
                    gdk::Key::m | gdk::Key::M if no_mod => {
                        if let Some(p) = win.player() {
                            let cur = p.state().volume();
                            if cur > 0.0 {
                                p.set_volume(0.0);
                            } else {
                                p.set_volume(1.0);
                            }
                        }
                    }
                    gdk::Key::AudioMute => {
                        if let Some(p) = win.player() {
                            let cur = p.state().volume();
                            if cur > 0.0 {
                                p.set_volume(0.0);
                            } else {
                                p.set_volume(1.0);
                            }
                        }
                    }

                    // Shuffle toggle
                    gdk::Key::s | gdk::Key::S if no_mod => {
                        if let Some(p) = win.player() {
                            let cur = p.queue().is_shuffled();
                            win.set_playlist_shuffled(!cur);
                        }
                    }

                    // Repeat mode toggle
                    gdk::Key::r | gdk::Key::R if no_mod => {
                        if let Some(p) = win.player() {
                            let q = p.queue();
                            let next = match q.repeat_mode() {
                                RepeatMode::Consecutive => RepeatMode::RepeatAll,
                                RepeatMode::RepeatAll => RepeatMode::RepeatOne,
                                RepeatMode::RepeatOne => RepeatMode::Consecutive,
                            };
                            q.set_repeat_mode(next);
                            win.set_repeat_mode(next);
                        }
                    }

                    // Toggle archive view / now playing view
                    gdk::Key::a | gdk::Key::A if no_mod => {
                        let is_archive = win.imp().main_stack.visible_child_name().as_deref() == Some("archive-view");
                        if is_archive {
                            win.switch_mode(WindowMode::NowPlayingView);
                        } else {
                            win.switch_mode(WindowMode::ArchiveView);
                        }
                    }

                    // Escape key: exit fullscreen, or back to now playing view
                    gdk::Key::Escape if no_mod => {
                        if win.is_fullscreen() {
                            win.unfullscreen();
                        } else if win.imp().main_stack.visible_child_name().as_deref() == Some("archive-view") {
                            win.switch_mode(WindowMode::NowPlayingView);
                        }
                    }

                    // Fullscreen
                    gdk::Key::F11 if no_mod => {
                        if win.is_fullscreen() {
                            win.unfullscreen();
                        } else {
                            win.fullscreen();
                        }
                    }

                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            }
        ));
        self.add_controller(key_ctrl);

        imp.archive_view.connect_activated(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, idx| {
                if let Some(player) = win.player() {
                    player.skip_to(idx as u32);
                    player.play();
                    win.switch_mode(WindowMode::NowPlayingView);
                }
            }
        ));

        imp.archive_view.connect_selection_changed(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, idx| {
                if let Some(player) = win.player() {
                    player.skip_to(idx as u32);
                }
            }
        ));

        imp.vinyl_view.connect_seek_preview(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, progress| {
                win.imp().seeking_until.set(i64::MAX);
                win.imp().position_progress.set_fraction(progress);
                if let Some(player) = win.player() {
                    let duration = player.state().duration() as f64;
                    let elapsed = (progress * duration) as u64;
                    win.set_song_time(Some(elapsed), None);
                }
            }
        ));

        imp.vinyl_view.connect_seek(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, progress| {
                win.imp().seeking_until.set(glib::monotonic_time() + 600_000);
                win.imp().position_progress.set_fraction(progress);
                if let Some(player) = win.player() {
                    let duration = player.state().duration() as f64;
                    let target_pos = (progress * duration) as u64;
                    player.seek_position_abs(target_pos);
                    let elapsed = target_pos;
                    let remaining = (duration as u64).checked_sub(elapsed).unwrap_or_default();
                    win.set_song_time(Some(elapsed), Some(remaining));
                    win.imp().lyrics_view.update_position((progress * duration * 1000.0) as u64, player.state().playing());
                }
            }
        ));

        imp.lyrics_view.set_seek_callback(clone!(
            #[weak(rename_to = win)]
            self,
            move |target_ms| {
                win.imp().seeking_until.set(glib::monotonic_time() + 600_000);
                if let Some(player) = win.player() {
                    player.seek_position_abs_ms(target_ms);
                    let duration = player.state().duration() as f64;
                    let progress = if duration > 0.0 { (target_ms as f64 / (duration * 1000.0)).clamp(0.0, 1.0) } else { 0.0 };
                    win.imp().position_progress.set_fraction(progress);
                    let elapsed = target_ms / 1000;
                    let remaining = (duration as u64).checked_sub(elapsed).unwrap_or_default();
                    win.set_song_time(Some(elapsed), Some(remaining));
                    win.imp().lyrics_view.update_position(target_ms, player.state().playing());
                }
            }
        ));

        imp.vinyl_view.set_hover_callback(clone!(
            #[weak(rename_to = win)]
            self,
            move |song_opt| {
                let label = win.imp().hover_label.get();
                if let Some(song) = song_opt {
                    label.set_text(&format!("{} - {}", song.title(), song.artist()));
                } else {
                    label.set_text("");
                }
            }
        ));

        imp.vinyl_view.set_click_callback(clone!(
            #[weak(rename_to = win)]
            self,
            move |song_idx| {
                if let Some(player) = win.player() {
                    player.skip_to(song_idx);
                    player.play();
                }
            }
        ));

        // Make the progress bar seekable via click and drag without audio churning or blinking
        {
            let progress_bar = imp.position_progress.get();

            // Drag handles both simple clicks (offset = 0) and dragged seeks
            let drag = gtk::GestureDrag::new();
            drag.set_button(gdk::BUTTON_PRIMARY);

            drag.connect_drag_begin(clone!(
                #[weak]
                progress_bar,
                #[weak(rename_to = win)]
                self,
                move |gesture, start_x, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    win.imp().seeking_until.set(i64::MAX);
                    let w = progress_bar.width() as f64;
                    if w > 0.0 {
                        let fraction = (start_x / w).clamp(0.0, 1.0);
                        progress_bar.set_fraction(fraction);
                        if let Some(player) = win.player() {
                            let duration = player.state().duration();
                            let elapsed = (duration as f64 * fraction) as u64;
                            win.set_song_time(Some(elapsed), None);
                        }
                    }
                }
            ));
            
            drag.connect_drag_update(clone!(
                #[weak]
                progress_bar,
                #[weak(rename_to = win)]
                self,
                move |gesture, dx, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    win.imp().seeking_until.set(i64::MAX);
                    if let Some((start_x, _)) = gesture.start_point() {
                        let x = (start_x + dx).max(0.0);
                        let w = progress_bar.width() as f64;
                        if w > 0.0 {
                            let fraction = (x / w).clamp(0.0, 1.0);
                            progress_bar.set_fraction(fraction);

                            // Update elapsed time label visually during drag without touching audio backend
                            if let Some(player) = win.player() {
                                let duration = player.state().duration();
                                let elapsed = (duration as f64 * fraction) as u64;
                                win.set_song_time(Some(elapsed), None);
                            }
                        }
                    }
                }
            ));

            drag.connect_drag_end(clone!(
                #[weak]
                progress_bar,
                #[weak(rename_to = win)]
                self,
                move |gesture, offset_x, _| {
                    if let Some((start_x, _)) = gesture.start_point() {
                        let x = (start_x + offset_x).max(0.0);
                        let w = progress_bar.width() as f64;
                        if w > 0.0 {
                            let fraction = (x / w).clamp(0.0, 1.0);
                            progress_bar.set_fraction(fraction);
                            win.imp().seeking_until.set(glib::monotonic_time() + 600_000);
                            if let Some(player) = win.player() {
                                player.seek_position_rel(fraction);
                                let duration = player.state().duration();
                                let elapsed = (duration as f64 * fraction) as u64;
                                let remaining = duration.checked_sub(elapsed).unwrap_or_default();
                                win.set_song_time(Some(elapsed), Some(remaining));
                                win.imp().lyrics_view.update_position((duration as f64 * fraction * 1000.0) as u64, player.state().playing());
                            }
                        } else {
                            win.imp().seeking_until.set(0);
                        }
                    } else {
                        win.imp().seeking_until.set(0);
                    }
                }
            ));
            progress_bar.add_controller(drag);
        }


        imp.vinyl_view.set_toggle_play_callback(clone!(
            #[weak(rename_to = win)]
            self,
            move || {
                if let Some(player) = win.player() {
                    if player.state().playing() {
                        player.pause();
                    } else {
                        player.play();
                    }
                }
            }
        ));



        self.imp()
            .playlist_view
            .queue_select_all_button()
            .connect_clicked(clone!(
                #[weak(rename_to = win)]
                self,
                move |_| {
                    if win.playlist_search() {
                        if let Some(ref model) = *win.imp().playlist_filtermodel.borrow() {
                            for idx in 0..model.n_items() {
                                let item = model.item(idx).unwrap();
                                let song = item.downcast_ref::<Song>().unwrap();
                                song.set_selected(true);
                            }
                        }
                    } else if let Some(player) = win.player() {
                        let queue = player.queue();
                        for idx in 0..queue.n_songs() {
                            let song = queue.song_at(idx).unwrap();
                            song.set_selected(true);
                        }
                    }
                }
            ));

        self.imp()
            .playlist_view
            .queue_remove_button()
            .connect_clicked(clone!(
                #[weak(rename_to = win)]
                self,
                move |_| {
                    if let Some(player) = win.player() {
                        let queue = player.queue();
                        let mut remove_songs: Vec<Song> = Vec::new();
                        // Collect all songs to be removed first, since we can't
                        // remove objects from the model while we're iterating it
                        for idx in 0..queue.n_songs() {
                            let song = queue.song_at(idx).unwrap();
                            if song.selected() {
                                remove_songs.push(song);
                            }
                        }

                        for song in remove_songs {
                            win.remove_song(&song);
                        }

                        // Store the current state of the playlist
                        utils::store_playlist(queue);
                    }
                }
            ));

        self.imp()
            .playlist_view
            .playlist_searchbar()
            .connect_notify_local(
                Some("search-mode-enabled"),
                clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |searchbar, _| {
                        win.set_playlist_search(searchbar.is_search_mode());
                    }
                ),
            );

        self.imp().settings.connect_changed(
            Some("enable-recoloring"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |settings, _| {
                    debug!(
                        "GSettings:enable-recoloring: {}",
                        settings.boolean("enable-recoloring")
                    );
                    if let Some(player) = this.player() {
                        let state = player.state();
                        this.update_style(state.current_song().as_ref());
                    }
                }
            ),
        );
        let _dummy = self.imp().settings.boolean("enable-recoloring");

        self.connect_close_request(move |window| {
            debug!("Saving window state");
            if !window.is_maximized() && !window.is_fullscreen() {
                let mut width = window.width();
                let mut height = window.height();
                if width >= 640 && height >= 480 {
                    if let Some(display) = gdk::Display::default() {
                        let monitors = display.monitors();
                        if let Some(monitor) = monitors.item(0).and_then(|m| m.downcast::<gdk::Monitor>().ok()) {
                            let geom = monitor.geometry();
                            let max_w = (geom.width() - 60).max(640);
                            let max_h = (geom.height() - 80).max(480);
                            width = width.min(max_w).min(1048);
                            height = height.min(max_h).min(700);
                        } else {
                            width = width.min(1048);
                            height = height.min(700);
                        }
                    } else {
                        width = width.min(1048);
                        height = height.min(700);
                    }
                    let settings = utils::settings_manager();
                    let _ = settings.set_int("window-width", width);
                    let _ = settings.set_int("window-height", height);
                }
            }

            window.unbind_queue();
            window.unbind_state();
            window.unbind_waveform();

            glib::Propagation::Proceed
        });


        self.connect_maximized_notify(clone!(
            #[weak(rename_to = win)]
            self,
            move |w| {
                let is_max = w.is_maximized();
                win.imp().playback_control.set_compact_mode(!is_max);
            }
        ));
    }

    // The initial state of the playback actions
    fn set_initial_state(&self) {
        let is_max = self.is_maximized();
        self.imp().playback_control.set_compact_mode(!is_max);

        if let Some(player) = self.player() {
            let queue = player.queue();
            self.action_set_enabled("win.play", !queue.is_empty());
            self.action_set_enabled("win.previous", !queue.is_empty());
            self.action_set_enabled("win.next", !queue.is_last_song());

            self.action_set_enabled("queue.toggle", !queue.is_empty());
            self.action_set_enabled("queue.shuffle", queue.n_songs() > 1);
            self.action_set_enabled("win.replaygain", player.replaygain_available());

            let replaygain = self.imp().settings.enum_("replay-gain").into();
            self.set_replaygain(replaygain);

            // Manually set player state, because set_replaygain
            // only updates player state when the value changes.
            player.set_replaygain(replaygain);

            self.set_repeat_mode(queue.repeat_mode());
            self.set_playlist_shuffled(queue.is_shuffled());

            if utils::has_cached_playlist() {
                self.imp().restore_playlist_button.set_visible(true);
                self.imp()
                    .restore_playlist_button
                    .add_css_class("suggested-action");
                self.imp()
                    .add_folder_button
                    .remove_css_class("suggested-action");
            } else {
                self.imp().restore_playlist_button.set_visible(false);
                self.imp()
                    .restore_playlist_button
                    .remove_css_class("suggested-action");
                self.imp()
                    .add_folder_button
                    .add_css_class("suggested-action");
            }

            let state = player.state();
            if state.playing() {
                self.switch_mode(WindowMode::NowPlayingView);
            } else if !queue.is_empty() {
                self.switch_mode(WindowMode::ArchiveView);
            } else {
                self.switch_mode(WindowMode::InitialView);
            }
        }
    }

    fn setup_playlist(&self) {
        let imp = self.imp();

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, item| {
                let row = QueueRow::default();
                let list_item = item.downcast_ref::<gtk::ListItem>().unwrap();
                list_item.set_child(Some(&row));

                row.connect_notify_local(
                    Some("selected"),
                    clone!(
                        #[weak]
                        win,
                        move |_, _| {
                            win.update_selected_count();
                        }
                    ),
                );

                win.bind_property("playlist-selection", &row, "selection-mode")
                    .sync_create()
                    .build();

                list_item
                    .bind_property("item", &row, "song")
                    .sync_create()
                    .build();

                list_item
                    .property_expression("item")
                    .chain_property::<Song>("artist")
                    .bind(&row, "song-artist", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<Song>("title")
                    .bind(&row, "song-title", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<Song>("cover")
                    .bind(&row, "song-cover", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<Song>("playing")
                    .bind(&row, "playing", gtk::Widget::NONE);
                list_item
                    .property_expression("item")
                    .chain_property::<Song>("selected")
                    .bind(&row, "selected", gtk::Widget::NONE);
            }
        ));
        imp.playlist_view
            .queue_view()
            .set_factory(Some(&factory.upcast::<gtk::ListItemFactory>()));

        if let Some(player) = self.player() {
            let queue = player.queue();

            let filter = FuzzyFilter::new();
            let filter_model =
                gtk::FilterListModel::new(Some(queue.model().clone()), Some(filter.clone()));
            let sorter = FuzzySorter::new();
            let sorter_model = gtk::SortListModel::new(Some(filter_model), Some(sorter.clone()));
            let selection = gtk::NoSelection::new(Some(sorter_model.clone()));
            imp.playlist_view
                .queue_view()
                .set_model(Some(selection.upcast_ref::<gtk::SelectionModel>()));
            imp.playlist_view.queue_view().connect_activate(clone!(
                #[weak(rename_to = win)]
                self,
                #[weak]
                selection,
                move |_, pos| {
                    let song = selection
                        .upcast::<gio::ListModel>()
                        .item(pos)
                        .unwrap()
                        .downcast::<Song>()
                        .unwrap();

                    if let Some(player) = win.player() {
                        let queue = player.queue();

                        let mut real_pos = None;
                        for i in 0..queue.model().n_items() {
                            if let Some(item) = queue.model().item(i) {
                                let s = item.downcast::<Song>().unwrap();
                                if s.equals(&song) {
                                    real_pos = Some(i);
                                    break;
                                }
                            }
                        }

                        if let Some(real_pos) = real_pos {
                            if win.playlist_selection() {
                                queue.select_song_at(real_pos);
                            } else if queue.current_song_index() != Some(real_pos) {
                                player.skip_to(real_pos);
                                player.play();
                                win.switch_mode(WindowMode::NowPlayingView);
                            } else if !player.state().playing() {
                                player.play();
                                win.switch_mode(WindowMode::NowPlayingView);
                            } else {
                                win.switch_mode(WindowMode::NowPlayingView);
                            }
                        }
                    }
                }
            ));

            imp.playlist_filtermodel
                .replace(Some(sorter_model.upcast::<gio::ListModel>()));

            imp.playlist_view
                .playlist_searchentry()
                .bind_property("text", &filter, "search")
                .sync_create()
                .build();
            imp.playlist_view
                .playlist_searchentry()
                .bind_property("text", &sorter, "search")
                .sync_create()
                .build();
            imp.playlist_view
                .playlist_searchentry()
                .connect_search_changed(clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |_| {
                        if let Some(adjustment) = win.imp().playlist_view.queue_view().vadjustment()
                        {
                            adjustment.set_value(0.0);
                        }
                    }
                ));
        }
    }

    fn setup_drop_target(&self) {
        let drop_target = gtk::DropTarget::builder()
            .name("file-drop-target")
            .actions(gdk::DragAction::COPY)
            .formats(&gdk::ContentFormats::for_type(gdk::FileList::static_type()))
            .build();

        drop_target.connect_drop(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                if let Ok(file_list) = value.get::<gdk::FileList>() {
                    if file_list.files().is_empty() {
                        win.add_toast(i18n("Unable to access dropped files"));
                        return false;
                    }

                    if let Some(name) = win.imp().main_stack.visible_child_name() {
                        if name == "initial-view" {
                            if let Some(player) = win.player() {
                                player.clear_queue();
                            }
                        }
                    }

                    let model = gio::ListStore::new::<gio::File>();
                    for f in file_list.files() {
                        model.append(&f);
                    }
                    win.add_files_to_queue(model.upcast_ref::<gio::ListModel>());
                    return true;
                }

                false
            }
        ));

        self.imp().drag_overlay.set_drop_target(&drop_target);
    }

    fn update_play_button(&self) {
        if let Some(player) = self.player() {
            let state = player.state();
            let playing = state.playing();
            self.imp().playback_control.set_playing(playing);
            self.imp().vinyl_view.set_property("playing", playing);
            if playing {
                self.imp().lyrics_view.resume_autoscroll();
            }
        }
    }

    fn update_position_labels(&self) {
        if glib::monotonic_time() < self.imp().seeking_until.get() {
            return;
        }
        if let Some(player) = self.player() {
            let state = player.state();
            if state.current_song().is_some() {
                let elapsed_ms = state.position();
                let elapsed = elapsed_ms / 1000;
                let duration = state.duration();
                let remaining = duration.checked_sub(elapsed).unwrap_or_default();
                self.set_song_time(Some(elapsed), Some(remaining));

                let position = if duration > 0 {
                    (elapsed_ms as f64 / (duration * 1000) as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.set_song_position(position);
                self.imp().lyrics_view.update_position(elapsed_ms, state.playing());
            } else {
                self.set_song_time(None, None);
                self.set_song_position(0.0);
            }
        }
    }

    fn update_song(&self) {
        if let Some(player) = self.player() {
            let state = player.state();
            self.scroll_playlist_to_song();
            self.update_playlist_time();
            self.update_title(state.current_song().as_ref());
            self.update_style(state.current_song().as_ref());
            self.update_upcoming_list();

            if let Some(song) = state.current_song() {
                let lyrics = crate::audio::Lyrics::from_song(&song);
                self.imp().lyrics_view.set_lyrics(lyrics);
                self.imp().lyrics_view.set_blurred_cover(song.blur_texture());
            } else {
                self.imp().lyrics_view.set_blurred_cover(None);
            }
        }
    }

    fn update_cover(&self) {
        if let Some(player) = self.player() {
            let state = player.state();
            let vinyl_view = self.imp().vinyl_view.get();
            if let Some(cover) = state.cover() {
                vinyl_view.set_property("cover", Some(&cover));
            } else {
                vinyl_view.set_property("cover", None::<&gdk::Texture>);
            }
            if let Some(song) = state.current_song() {
                self.imp().lyrics_view.set_blurred_cover(song.blur_texture());
            } else {
                self.imp().lyrics_view.set_blurred_cover(None);
            }
        }
    }

    fn update_playlist_time(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            let n_songs = queue.n_songs();
            if let Some(current) = queue.current_song_index() {
                let mut remaining_time = 0;
                for pos in 0..n_songs {
                    let song = queue.song_at(pos).unwrap();
                    if pos >= current {
                        remaining_time += song.duration();
                    }
                }

                let remaining_min = ((remaining_time - (remaining_time % 60)) / 60) as u32;
                let remaining_hrs = (remaining_min - (remaining_min % 60)) / 60;

                let remaining_str = if remaining_hrs > 0 {
                    // Translators: the `{}` must be left unmodified, and
                    // it will be replaced by the number of minutes remaining
                    // in the string "N hours M minutes remaining"
                    let minutes = ni18n_f(
                        "{} minute",
                        "{} minutes",
                        remaining_min % 60,
                        &[&(remaining_min % 60).to_string()],
                    );

                    // Translators: `{hours}` and `{minutes}` must be left
                    // unmodified, and they will be replaced by number of
                    // hours and by the translated number of minutes
                    // remaining, respectively
                    ni18n_k(
                        "{hours} hour {minutes} remaining",
                        "{hours} hours {minutes} remaining",
                        remaining_hrs,
                        &[("hours", &remaining_hrs.to_string()), ("minutes", &minutes)],
                    )
                } else {
                    ni18n_f(
                        // Translators: the '{}' must be left unmodified, and
                        // it will be replaced by the number of minutes remaining
                        // in the playlist
                        "{} minute remaining",
                        "{} minutes remaining",
                        remaining_min,
                        &[&remaining_min.to_string()],
                    )
                };

                self.imp()
                    .playlist_view
                    .queue_length_label()
                    .set_label(&remaining_str);
                self.imp()
                    .playlist_view
                    .queue_length_label()
                    .set_visible(true);
            } else {
                self.imp()
                    .playlist_view
                    .queue_length_label()
                    .set_visible(false);
            }
        }
    }

    fn scroll_playlist_to_song(&self) {
        let queue_view = self.imp().playlist_view.queue_view();
        if let Some(player) = self.player() {
            if let Some(current_idx) = player.queue().current_song_index() {
                debug!("Scrolling playlist to {}", current_idx);
                queue_view
                    .upcast_ref::<gtk::Widget>()
                    .activate_action("list.scroll-to-item", Some(&current_idx.to_variant()))
                    .expect("Failed to activate action");
            }
        }
    }

    fn setup_provider(&self) {
        let imp = self.imp();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &imp.provider, 400);
        }
    }

    fn update_style(&self, song: Option<&Song>) {
        let imp = self.imp();

        if !imp.settings.boolean("enable-recoloring") {
            imp.provider.load_from_string("");
            imp.main_stack.remove_css_class("main-window");
            return;
        }

        if let Some(song) = song {
            if let Some(bg_colors) = song.cover_palette() {
                let mut css = String::new();

                css.push_str(":root {");

                let n_colors = bg_colors.len();
                for (i, color) in bg_colors.iter().enumerate().take(n_colors) {
                    let s = format!("--background-color-{}: {};", i, color);
                    css.push_str(&s);
                }

                for i in n_colors + 1 - 1..5 {
                    css.push_str(&format!(
                        "--background-color-{}: var(--window-bg-color);",
                        i
                    ));
                }

                // Generate accent color variables from the dominant palette color
                if let Some(primary) = bg_colors.first() {
                    let r = (primary.red() * 255.0) as u8;
                    let g = (primary.green() * 255.0) as u8;
                    let b = (primary.blue() * 255.0) as u8;
                    css.push_str(&format!(
                        "--accent-color: rgb({},{},{});",
                        r, g, b
                    ));
                    css.push_str(&format!(
                        "--accent-glow: rgba({},{},{},0.4);",
                        r, g, b
                    ));
                    css.push_str(&format!(
                        "--accent-dim: rgba({},{},{},0.08);",
                        r, g, b
                    ));
                }

                css.push_str("}");

                imp.provider.load_from_string(&css);
                if !imp.main_stack.has_css_class("main-window") {
                    imp.main_stack.add_css_class("main-window");
                }

                self.action_set_enabled("win.enable-recoloring", true);

                return;
            }
        }

        imp.provider.load_from_string("");
        imp.main_stack.remove_css_class("main-window");
        self.action_set_enabled("win.enable-recoloring", false);
    }

    fn update_title(&self, song: Option<&Song>) {
        if let Some(song) = song {
            self.set_title(Some(&format!("{} — {}", song.artist(), song.title())));
        } else {
            self.set_title(Some("Ember"));
        }
    }

    fn update_selected_count(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            let n_selected = queue.n_selected_songs();

            let selected_str = if n_selected == 0 {
                i18n("No song selected")
            } else {
                ni18n_f(
                    // Translators: The '{}' must be left unmodified, and
                    // it is expanded to the number of songs selected
                    "{} song selected",
                    "{} songs selected",
                    n_selected,
                    &[&n_selected.to_string()],
                )
            };

            self.imp()
                .playlist_view
                .queue_selected_label()
                .set_label(&selected_str);
        }
    }

    pub fn open_files(&self, files: &[gio::File]) {
        if files.is_empty() {
            self.add_toast(i18n("Unable to access files"));
            return;
        }

        let model = gio::ListStore::new::<gio::File>();
        for f in files {
            model.append(f);
        }
        self.add_files_to_queue(model.upcast_ref::<gio::ListModel>());
    }

    pub fn remove_song(&self, song: &Song) {
        if let Some(p) = self.player() {
            p.remove_song(song);
        }
        self.update_selected_count();
        self.update_playlist_time();
    }

    pub fn add_toast(&self, msg: String) {
        debug!("Toast: {msg}");
    }

    pub fn add_skip_to_toast(&self, msg: String, _button: String, _pos: u32) {
        debug!("Toast: {msg}");
    }

    fn copy_song(&self) {
        if let Some(player) = self.player() {
            let state = player.state();
            if let Some(song) = state.current_song() {
                let s = i18n_k(
                    // Translators: `{title}` and `{artist}` must be left
                    // untranslated; they will expand to the title and
                    // artist of the currently playing song, respectively
                    "Currently playing “{title}” by “{artist}”",
                    &[("title", &song.title()), ("artist", &song.artist())],
                );
                self.clipboard().set_text(&s);
            }
        }
    }

    async fn open_song_directory(&self) {
        if let Some(player) = self.player() {
            let state = player.state();

            if let Some(song) = state.current_song() {
                let file = song.file();

                match file
                    .query_info_future(
                        &ATTRIBUTE_HOST_PATH,
                        gio::FileQueryInfoFlags::NONE,
                        glib::Priority::DEFAULT,
                    )
                    .await
                {
                    Err(err) => debug!("Unable to get host-path attribute: {err}"),
                    Ok(info) => {
                        let host_path = info
                            .attribute_as_string(&ATTRIBUTE_HOST_PATH)
                            .and_then(|x| PathBuf::from_str(x.as_str()).ok());

                        if let Some(path) = host_path.or_else(|| file.path()) {
                            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
                            match launcher
                                .open_containing_folder_future(Some(
                                    self.upcast_ref::<gtk::Window>(),
                                ))
                                .await
                            {
                                Err(_e) => self.add_toast(i18n("Unable to open folder")),
                                Ok(()) => {}
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn switch_mode(&self, mode: WindowMode) {
        let stack = self.imp().main_stack.get();
        match mode {
            WindowMode::InitialView => {
                stack.set_visible_child_name("initial-view");
                self.set_default_widget(Some(&self.imp().add_folder_button.get()));
            }
            WindowMode::ArchiveView => {
                stack.set_visible_child_name("archive-view");
            }
            WindowMode::NowPlayingView => {
                stack.set_visible_child_name("now-playing-view");
                self.set_default_widget(Some(&self.imp().playback_control.play_button()));
            }
        };
    }

    pub fn set_replaygain(&self, replaygain: ReplayGainMode) {
        let imp = self.imp();

        if replaygain != imp.replaygain_mode.replace(replaygain) {
            if let Some(p) = self.player() {
                p.set_replaygain(replaygain);
            }
            self.imp()
                .settings
                .set_enum("replay-gain", replaygain.into())
                .expect("Unable to store setting");

            self.notify("replaygain-mode");
        }
    }

    pub fn replaygain(&self) -> ReplayGainMode {
        self.imp().replaygain_mode.get()
    }

    pub fn set_song_time(&self, elapsed: Option<u64>, _remaining: Option<u64>) {
        let label = self.imp().remaining_label.get();
        if let Some(elapsed) = elapsed {
            let formatted = utils::format_time(elapsed as i64);
            if label.text() != formatted {
                label.set_text(&formatted);
            }
        } else if label.text() != "0:00" {
            label.set_text("0:00");
        }
    }

    pub fn set_song_position(&self, position: f64) {
        let imp = self.imp();
        let cur = imp.position_progress.fraction();
        if (cur - position).abs() >= 0.001 {
            imp.position_progress.set_fraction(position);
        }
    }

    pub fn update_archive_songs(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            let mut songs = Vec::new();
            for i in 0..queue.n_songs() {
                if let Some(song) = queue.song_at(i) {
                    songs.push(song.clone());
                }
            }
            self.imp().archive_view.set_songs(songs);
        }
    }

    fn update_upcoming_list(&self) {
        let imp = self.imp();
        if let Some(player) = self.player() {
            let queue = player.queue();
            imp.vinyl_view.set_queue(Some(queue));
        } else {
            imp.vinyl_view.set_queue(None);
        }
    }
}


