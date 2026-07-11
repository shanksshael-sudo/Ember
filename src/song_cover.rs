// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::subclass::prelude::*;
use gtk::{glib, prelude::*, CompositeTemplate};

use crate::cover_picture::CoverPicture;

mod imp {
    use super::*;

    #[derive(Debug, Default, CompositeTemplate)]
    #[template(resource = "/io/bassi/Amberol/song-cover.ui")]
    pub struct SongCover {
        // Template widgets
        #[template_child]
        pub cover_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub album_image: TemplateChild<CoverPicture>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SongCover {
        const NAME: &'static str = "AmberolSongCover";
        type Type = super::SongCover;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_css_name("songcover");
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SongCover {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for SongCover {}
}

glib::wrapper! {
    pub struct SongCover(ObjectSubclass<imp::SongCover>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for SongCover {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl SongCover {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn album_image(&self) -> CoverPicture {
        self.imp().album_image.get()
    }

    pub fn show_cover_image(&self, show: bool) {
        if show {
            self.imp().cover_stack.set_visible_child_name("cover-image");
        } else {
            self.imp().cover_stack.set_visible_child_name("no-image");
        }
    }
}
