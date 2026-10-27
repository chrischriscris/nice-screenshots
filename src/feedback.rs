//! "The screenshot was taken" cues: a white flash over the captured area and a shutter sound.

use std::cell::Cell;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::geometry::Rect;
use crate::{ui, util};

const FLASH: Duration = Duration::from_millis(320);
const SHUTTER: &str = "/usr/share/sounds/freedesktop/stereo/camera-shutter.oga";

/// Flashes `rect` (global logical coordinates) on `output`, or the whole output when `rect` is None.
pub fn flash(app: &gtk::Application, output: &str, rect: Option<Rect>) {
    let Some(monitor) = find_monitor(output) else {
        return;
    };
    let g = monitor.geometry();
    let mon = Rect::new(
        g.x() as f64,
        g.y() as f64,
        g.width() as f64,
        g.height() as f64,
    );
    let area_rect = rect.unwrap_or(mon).relative_to(&mon);

    let win = ui::layer_window(app, "pinhole-flash");
    win.set_monitor(Some(&monitor));
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        win.set_anchor(edge, true);
    }
    win.set_exclusive_zone(-1);
    // Clicks go straight through to whatever is underneath.
    win.connect_realize(|win| {
        if let Some(surface) = win.surface() {
            surface.set_input_region(&cairo::Region::create());
        }
    });

    let area = gtk::DrawingArea::new();
    let start = Rc::new(Cell::new(None::<i64>));
    let alpha = Rc::new(Cell::new(0.0f64));
    area.set_draw_func({
        let alpha = alpha.clone();
        move |_, cr, _, _| {
            cr.set_source_rgba(1.0, 1.0, 1.0, alpha.get());
            cr.rectangle(area_rect.x, area_rect.y, area_rect.w, area_rect.h);
            let _ = cr.fill();
        }
    });
    area.add_tick_callback({
        let win = win.downgrade();
        move |area, clock| {
            let now = clock.frame_time();
            let t0 = start.get().unwrap_or(now);
            start.set(Some(t0));
            let t = (now - t0) as f64 / FLASH.as_micros() as f64;
            if t >= 1.0 {
                if let Some(win) = win.upgrade() {
                    win.close();
                }
                return glib::ControlFlow::Break;
            }
            // Bright at once, then ease out.
            alpha.set(0.8 * (1.0 - t).powi(2));
            area.queue_draw();
            glib::ControlFlow::Continue
        }
    });
    win.set_child(Some(&area));
    win.present();
}

/// Plays the camera shutter sound, if anything can play it.
pub fn shutter() {
    if std::path::Path::new(SHUTTER).exists() {
        for player in ["pw-play", "paplay"] {
            if util::which(player) {
                util::spawn_detached(Command::new(player).arg(SHUTTER));
                return;
            }
        }
    }
    if util::which("canberra-gtk-play") {
        util::spawn_detached(Command::new("canberra-gtk-play").args(["-i", "camera-shutter"]));
    }
}

fn find_monitor(name: &str) -> Option<gdk::Monitor> {
    let monitors = gdk::Display::default()?.monitors();
    let all =
        (0..monitors.n_items()).filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>());
    let mut first = None;
    for m in all {
        if m.connector().is_some_and(|c| c == name) {
            return Some(m);
        }
        first.get_or_insert(m);
    }
    first
}
