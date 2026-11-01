// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::rc::Rc;

use gtk::subclass::prelude::*;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use ashpd::desktop::background::Background;
use async_channel::Receiver;
use glib::clone;
use gtk::{gio, glib, prelude::*};
use log::debug;

use crate::{
    audio::AudioPlayer,
    config::{APPLICATION_ID, VERSION},
    i18n::i18n,
    utils,
    window::Window,
};

pub enum ApplicationAction {
    Present,
}

mod imp {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Debug)]
    pub struct Application {
        pub player: Rc<AudioPlayer>,
        pub receiver: RefCell<Option<Receiver<ApplicationAction>>>,
        pub background_hold: RefCell<Option<gio::ApplicationHoldGuard>>,
        pub settings: gio::Settings,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "AmberolApplication";
        type Type = super::Application;
        type ParentType = gtk::Application;

        fn new() -> Self {
            let (sender, r) = async_channel::unbounded();
            let receiver = RefCell::new(Some(r));

            Self {
                player: AudioPlayer::new(sender),
                receiver,
                background_hold: RefCell::default(),
                settings: utils::settings_manager(),
            }
        }
    }

    impl ObjectImpl for Application {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.setup_channel();
            obj.setup_gactions();
            obj.setup_settings();

            obj.set_accels_for_action("app.quit", &["<primary>q"]);

            obj.set_accels_for_action("queue.add-song", &["<primary>s"]);
            obj.set_accels_for_action("queue.add-folder", &["<primary>a"]);
            obj.set_accels_for_action("queue.clear", &["<primary>L"]);
            obj.set_accels_for_action("queue.toggle", &["F9"]);
            obj.set_accels_for_action("queue.search", &["<primary>F"]);
            obj.set_accels_for_action("queue.shuffle", &["<primary>r"]);

            obj.set_accels_for_action("win.seek-backwards", &["<primary>Left"]);
            obj.set_accels_for_action("win.seek-forward", &["<primary>Right"]);
            obj.set_accels_for_action("win.previous", &["<primary>b"]);
            obj.set_accels_for_action("win.next", &["<primary>n"]);
            obj.set_accels_for_action("win.play", &["<primary>p"]);
            obj.set_accels_for_action("win.copy", &["<primary>c"]);
        }
    }

    impl ApplicationImpl for Application {
        fn startup(&self) {
            self.parent_startup();

            gtk::Window::set_default_icon_name(APPLICATION_ID);

            // Force OLED Black & Cyan theme globally
            let provider = gtk::CssProvider::new();
            provider.load_from_resource("/io/bassi/Amberol/style.css");
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_USER,
                );
            }
        }

        fn activate(&self) {
            debug!("Application::activate");

            self.obj().present_main_window();
        }

        fn open(&self, files: &[gio::File], _hint: &str) {
            debug!("Application::open");

            let application = self.obj();
            application.present_main_window();
            if let Some(window) = application.active_window() {
                window.downcast_ref::<Window>().unwrap().open_files(files);
            }
        }
    }

    impl GtkApplicationImpl for Application {}
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends gio::Application, gtk::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Default for Application {
    fn default() -> Self {
        #[cfg(target_os = "windows")]
        let flags = gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::NON_UNIQUE;
        #[cfg(not(target_os = "windows"))]
        let flags = gio::ApplicationFlags::HANDLES_OPEN;

        glib::Object::builder::<Application>()
            .property("application-id", APPLICATION_ID)
            .property("flags", flags)
            .property("resource-base-path", "/io/bassi/Amberol")
            .build()
    }
}

impl Application {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn player(&self) -> Rc<AudioPlayer> {
        self.imp().player.clone()
    }

    fn setup_settings(&self) {
        self.imp().settings.connect_changed(
            Some("background-play"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |settings, _| {
                    let background_play = settings.boolean("background-play");
                    debug!("GSettings:background-play: {background_play}");
                    if background_play {
                        this.request_background();
                    } else {
                        debug!("Dropping background hold");
                        this.imp().background_hold.replace(None);
                    }
                }
            ),
        );

        let _dummy = self.imp().settings.boolean("background-play");
    }

    fn setup_channel(&self) {
        let receiver = self.imp().receiver.borrow_mut().take().unwrap();
        glib::MainContext::default().spawn_local(clone!(
            #[strong(rename_to = this)]
            self,
            async move {
                use futures::prelude::*;

                let mut receiver = std::pin::pin!(receiver);

                while let Some(action) = receiver.next().await {
                    this.process_action(action);
                }
            }
        ));
    }

    fn process_action(&self, action: ApplicationAction) -> glib::ControlFlow {
        match action {
            ApplicationAction::Present => self.present_main_window(),
            // _ => debug!("Received action {:?}", action),
        }

        glib::ControlFlow::Continue
    }

    fn present_main_window(&self) {
        let window = if let Some(window) = self.active_window() {
            window
        } else {
            let window = Window::new(self);
            window.upcast()
        };

        self.request_background();

        window.present();
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("quit")
                .activate(|app: &Application, _, _| {
                    app.quit();
                })
                .build(),
            gio::ActionEntry::builder("about")
                .activate(|app: &Application, _, _| {
                    app.show_about();
                })
                .build(),
        ]);

        let background_play = self.imp().settings.boolean("background-play");
        self.add_action_entries([gio::ActionEntry::builder("background-play")
            .state(background_play.to_variant())
            .activate(|this: &Application, action, _| {
                let state = action.state().unwrap();
                let action_state: bool = state.get().unwrap();
                let background_play = !action_state;
                action.set_state(&background_play.to_variant());

                this.imp()
                    .settings
                    .set_boolean("background-play", background_play)
                    .expect("Unable to store background-play setting");
            })
            .build()]);
    }

    fn show_about(&self) {
        let window = self.active_window().unwrap();
        let dialog = gtk::AboutDialog::builder()
            .logo_icon_name(APPLICATION_ID)
            .program_name("Amberol")
            .version(VERSION)
            .authors(vec!["Emmanuele Bassi"])
            .copyright("© 2022 Emmanuele Bassi")
            .website("https://apps.gnome.org/Amberol/")
            .license_type(gtk::License::Gpl30)
            // Translators: Replace "translator-credits" with your names, one name per line
            .translator_credits(i18n("translator-credits"))
            .transient_for(&window)
            .build();

        dialog.present();
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    async fn portal_request_background(&self) {
        if let Some(window) = self.active_window() {
            let root = window.native().unwrap();
            let identifier = ashpd::WindowIdentifier::from_native(&root).await;
            let request = Background::request().identifier(identifier).reason(&*i18n(
                "Amberol needs to run in the background to play music",
            ));

            match request.send().await.and_then(|r| r.response()) {
                Ok(response) => {
                    debug!("Background request successful: {:?}", response);
                    self.imp().background_hold.replace(Some(self.hold()));
                }
                Err(err) => {
                    warn!("Background request denied: {}", err);
                    self.imp()
                        .settings
                        .set_boolean("background-play", false)
                        .expect("Unable to set background-play settings key");
                }
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn request_background(&self) {
        let background_play = self.imp().settings.boolean("background-play");
        if background_play {
            let ctx = glib::MainContext::default();
            ctx.spawn_local(clone!(
                #[weak(rename_to = app)]
                self,
                async move { app.portal_request_background().await }
            ));
        }
    }

    #[cfg(target_os = "windows")]
    fn request_background(&self) {
        let background_play = self.imp().settings.boolean("background-play");
        if background_play {
            debug!("Acquiring application hold for background playback");
            self.imp().background_hold.replace(Some(self.hold()));
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "windows")))]
    fn request_background(&self) {}
}
