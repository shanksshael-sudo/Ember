// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::subclass::prelude::*;
use gtk::{glib, prelude::*};

mod imp {
    use super::*;
    use std::cell::RefCell;
    use once_cell::sync::Lazy;

    #[derive(Debug, Default)]
    pub struct DragOverlay {
        pub overlay: gtk::Overlay,
        pub revealer: gtk::Revealer,
        pub label: gtk::Label,
        pub drop_target: RefCell<Option<gtk::DropTarget>>,
        pub handler_id: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DragOverlay {
        const NAME: &'static str = "DragOverlay";
        type Type = super::DragOverlay;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("dragoverlay");
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for DragOverlay {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
                vec![
                    glib::ParamSpecString::builder("title").build(),
                    glib::ParamSpecObject::builder::<gtk::Widget>("child").build(),
                    glib::ParamSpecObject::builder::<gtk::DropTarget>("drop-target")
                        .explicit_notify()
                        .build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "title" => self.label.label().to_value(),
                "child" => self.overlay.child().to_value(),
                "drop-target" => self.drop_target.borrow().to_value(),
                _ => unimplemented!(),
            }
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "title" => self.label.set_label(value.get().unwrap()),
                "child" => self
                    .overlay
                    .set_child(value.get::<gtk::Widget>().ok().as_ref()),
                "drop-target" => self
                    .obj()
                    .set_drop_target(&value.get::<gtk::DropTarget>().unwrap()),
                _ => unimplemented!(),
            };
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.overlay
                .set_parent(self.obj().upcast_ref::<gtk::Widget>());
            self.overlay.add_overlay(&self.revealer);

            self.revealer.set_can_target(false);
            self.revealer
                .set_transition_type(gtk::RevealerTransitionType::Crossfade);
            self.revealer.set_reveal_child(false);

            self.label.add_css_class("drag-overlay-label");
            self.label.set_halign(gtk::Align::Center);
            self.label.set_valign(gtk::Align::Center);

            self.revealer.set_child(Some(&self.label));
        }

        fn dispose(&self) {
            self.overlay.unparent();
        }
    }
    impl WidgetImpl for DragOverlay {}
}

glib::wrapper! {
    pub struct DragOverlay(ObjectSubclass<imp::DragOverlay>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for DragOverlay {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl DragOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_drop_target(&self, drop_target: &gtk::DropTarget) {
        let imp = self.imp();

        if let Some(id) = imp.handler_id.replace(None) {
            if let Some(ref old_drop_target) = *imp.drop_target.borrow() {
                old_drop_target.disconnect(id);
            }
        }

        let handler_id = drop_target.connect_drop(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            false,
            move |_, _, _, _| {
                this.imp().revealer.set_reveal_child(false);
                false
            }
        ));
        imp.handler_id.replace(Some(handler_id));

        self.add_controller(drop_target.clone());
        imp.drop_target.replace(Some(drop_target.clone()));
        
        // Disambiguate notify call
        gtk::prelude::ObjectExt::notify(self, "drop-target");
    }
}
