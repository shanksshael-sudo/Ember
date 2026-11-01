// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::Cell;
use gtk::subclass::prelude::*;
use gtk::{gio, glib, prelude::*, CompositeTemplate};
use crate::audio::RepeatMode;

mod imp {
    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/bassi/Amberol/playback-control.ui")]
    pub struct PlaybackControl {
        #[template_child]
        pub center_box: TemplateChild<gtk::Box>,

        #[template_child]
        pub play_button: TemplateChild<gtk::Button>,

        #[template_child]
        pub previous_button: TemplateChild<gtk::Button>,

        #[template_child]
        pub next_button: TemplateChild<gtk::Button>,

        #[template_child]
        pub shuffle_button: TemplateChild<gtk::ToggleButton>,

        #[template_child]
        pub repeat_button: TemplateChild<gtk::Button>,

        pub compact_mode: Cell<bool>,
        pub playing: Cell<bool>,
        pub repeat_mode: Cell<RepeatMode>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PlaybackControl {
        const NAME: &'static str = "AmberolPlaybackControl";
        type Type = super::PlaybackControl;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_css_name("playbackcontrol");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PlaybackControl {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().update_buttons();
        }

        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for PlaybackControl {}
}

glib::wrapper! {
    pub struct PlaybackControl(ObjectSubclass<imp::PlaybackControl>)
        @extends gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible,
                    gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for PlaybackControl {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl PlaybackControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn play_button(&self) -> gtk::Button {
        self.imp().play_button.get()
    }

    pub fn set_compact_mode(&self, compact: bool) {
        let imp = self.imp();
        if imp.compact_mode.get() == compact {
            return;
        }
        imp.compact_mode.set(compact);
        self.update_buttons();
    }

    pub fn set_playing(&self, playing: bool) {
        let imp = self.imp();
        if imp.playing.get() == playing {
            return;
        }
        imp.playing.set(playing);
        self.update_buttons();
    }

    pub fn set_repeat_mode(&self, repeat_mode: RepeatMode) {
        let imp = self.imp();
        imp.repeat_mode.set(repeat_mode);
        self.update_buttons();
    }

    pub fn update_buttons(&self) {
        let imp = self.imp();
        let compact = imp.compact_mode.get();
        let playing = imp.playing.get();
        let repeat_mode = imp.repeat_mode.get();

        let shuffle_btn = imp.shuffle_button.get();
        let prev_btn = imp.previous_button.get();
        let play_btn = imp.play_button.get();
        let next_btn = imp.next_button.get();
        let repeat_btn = imp.repeat_button.get();

        if compact {
            self.add_css_class("compact");

            let img = gtk::Image::from_icon_name("media-playlist-shuffle-symbolic");
            img.set_icon_size(gtk::IconSize::Inherit);
            shuffle_btn.set_child(Some(&img));

            let img = gtk::Image::from_icon_name("media-skip-backward-symbolic");
            img.set_icon_size(gtk::IconSize::Inherit);
            prev_btn.set_child(Some(&img));

            let icon = if playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            };
            let img = gtk::Image::from_icon_name(icon);
            img.set_icon_size(gtk::IconSize::Inherit);
            play_btn.set_child(Some(&img));

            let img = gtk::Image::from_icon_name("media-skip-forward-symbolic");
            img.set_icon_size(gtk::IconSize::Inherit);
            next_btn.set_child(Some(&img));

            let (icon, active) = match repeat_mode {
                RepeatMode::Consecutive => ("media-playlist-repeat-symbolic", false),
                RepeatMode::RepeatAll => ("media-playlist-repeat-symbolic", true),
                RepeatMode::RepeatOne => ("media-playlist-repeat-song-symbolic", true),
            };
            let img = gtk::Image::from_icon_name(icon);
            img.set_icon_size(gtk::IconSize::Inherit);
            repeat_btn.set_child(Some(&img));
            if active {
                repeat_btn.add_css_class("active");
            } else {
                repeat_btn.remove_css_class("active");
            }
        } else {
            self.remove_css_class("compact");

            shuffle_btn.set_child(Some(&gtk::Label::new(Some("Shuffle"))));
            prev_btn.set_child(Some(&gtk::Label::new(Some("Backward"))));

            let play_label = if playing { "Pause" } else { "Play" };
            play_btn.set_child(Some(&gtk::Label::new(Some(play_label))));

            next_btn.set_child(Some(&gtk::Label::new(Some("Forward"))));

            let (label, active) = match repeat_mode {
                RepeatMode::Consecutive => ("Repeat OFF", false),
                RepeatMode::RepeatAll => ("Repeat ALL", true),
                RepeatMode::RepeatOne => ("Repeat ONE", true),
            };
            repeat_btn.set_child(Some(&gtk::Label::new(Some(label))));
            if active {
                repeat_btn.add_css_class("active");
            } else {
                repeat_btn.remove_css_class("active");
            }
        }
    }
}
