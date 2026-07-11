// SPDX-License-Identifier: GPL-3.0-or-later
// Ember — Circular progress arc with cyan glow effect
// Uses GTK4's Snapshot API (no deprecated Cairo paint calls).

use gtk::subclass::prelude::*;
use gtk::{glib, graphene, prelude::*};
use std::f64::consts::PI;

mod imp {
    use super::*;
    use std::cell::Cell;
    use glib::ParamSpec;
    use once_cell::sync::Lazy;

    #[derive(Default)]
    pub struct CircularProgress {
        pub value: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CircularProgress {
        const NAME: &'static str = "EmberCircularProgress";
        type Type = super::CircularProgress;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for CircularProgress {
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    glib::ParamSpecDouble::builder("value")
                        .minimum(0.0)
                        .maximum(1.0)
                        .default_value(0.0)
                        .build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &ParamSpec) {
            match pspec.name() {
                "value" => {
                    let v: f64 = value.get().unwrap();
                    self.value.set(v.clamp(0.0, 1.0));
                    self.obj().queue_draw();
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> glib::Value {
            match pspec.name() {
                "value" => self.value.get().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl WidgetImpl for CircularProgress {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let width = widget.width() as f64;
            let height = widget.height() as f64;
            if width <= 0.0 || height <= 0.0 {
                return;
            }

            let size = width.min(height);
            let cx = width / 2.0;
            let cy = height / 2.0;

            // Outer glow ring sits 6px outside the cover edge.
            // The cover is 380px wide, our widget is 460px — so we center
            // the arc at radius = 190 + 10 = 200px from the widget centre.
            let ring_radius = size / 2.0 - 12.0;

            let cr = snapshot.append_cairo(&graphene::Rect::new(
                0.0, 0.0, width as f32, height as f32,
            ));

            cr.set_line_cap(gtk::cairo::LineCap::Round);

            // ── Background track (very dim cyan ring) ──────────────────
            cr.set_line_width(3.0);
            cr.set_source_rgba(0.0, 1.0, 1.0, 0.08);
            cr.arc(cx, cy, ring_radius, 0.0, 2.0 * PI);
            cr.stroke().ok();

            // ── Progress arc ───────────────────────────────────────────
            let val = self.value.get();
            if val > 0.001 {
                let start_angle = -PI / 2.0; // 12 o'clock
                let end_angle = start_angle + val * 2.0 * PI;

                // Layer 1: wide soft outer glow (transparent cyan, thick)
                cr.set_line_width(18.0);
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.06);
                cr.arc(cx, cy, ring_radius, start_angle, end_angle);
                cr.stroke().ok();

                // Layer 2: mid glow
                cr.set_line_width(10.0);
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.12);
                cr.arc(cx, cy, ring_radius, start_angle, end_angle);
                cr.stroke().ok();

                // Layer 3: the crisp bright line
                cr.set_line_width(3.5);
                cr.set_source_rgba(0.0, 1.0, 1.0, 1.0);
                cr.arc(cx, cy, ring_radius, start_angle, end_angle);
                cr.stroke().ok();

                // ── Trailing dot at the arc tip ────────────────────────
                let tip_x = cx + ring_radius * end_angle.cos();
                let tip_y = cy + ring_radius * end_angle.sin();
                // Dot glow
                cr.set_source_rgba(0.0, 1.0, 1.0, 0.4);
                cr.arc(tip_x, tip_y, 8.0, 0.0, 2.0 * PI);
                cr.fill().ok();
                // Dot core
                cr.set_source_rgba(0.0, 1.0, 1.0, 1.0);
                cr.arc(tip_x, tip_y, 4.5, 0.0, 2.0 * PI);
                cr.fill().ok();
            }
        }
    }
}

glib::wrapper! {
    pub struct CircularProgress(ObjectSubclass<imp::CircularProgress>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CircularProgress {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for CircularProgress {
    fn default() -> Self {
        Self::new()
    }
}
