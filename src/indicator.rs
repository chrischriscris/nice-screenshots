//! Countdown before timed captures, and the floating "recording" pill.

use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, LayerShell};

use crate::icons::Icon;
use crate::{ui, util};

pub async fn countdown(app: &gtk::Application, seconds: u32) {
    let win = ui::layer_window(app, "pinhole-countdown");
    let label = gtk::Label::new(None);
    label.add_css_class("ph-countdown");
    win.set_child(Some(&label));
    win.present();
    for n in (1..=seconds).rev() {
        label.set_text(&n.to_string());
        glib::timeout_future(Duration::from_secs(1)).await;
    }
    win.close();
    // Let the compositor finish fading the countdown out before anything is captured.
    glib::timeout_future(Duration::from_millis(350)).await;
}

pub fn recording(
    app: &gtk::Application,
    started: Instant,
    on_stop: impl Fn() + 'static,
) -> gtk::Window {
    let win = ui::layer_window(app, "pinhole-indicator");
    win.set_anchor(Edge::Top, true);
    win.set_margin(Edge::Top, 8);

    let pill = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    pill.add_css_class("ph-pill");

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("ph-rec-dot");
    dot.set_valign(gtk::Align::Center);

    let time = gtk::Label::new(Some("0:00"));
    time.add_css_class("ph-rec-time");

    let stop = ui::icon_button(Icon::Stop, 16, "Stop recording");
    stop.add_css_class("ph-rec-stop");
    stop.connect_clicked(move |_| on_stop());

    pill.append(&dot);
    pill.append(&time);
    pill.append(&stop);
    win.set_child(Some(&pill));

    let weak = time.downgrade();
    glib::timeout_add_local(Duration::from_millis(500), move || match weak.upgrade() {
        Some(label) => {
            label.set_text(&util::format_duration(started.elapsed()));
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });

    win.present();
    win
}
