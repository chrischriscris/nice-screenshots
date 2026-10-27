use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

use crate::icons::{self, Icon};

pub fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        // Above STYLE_PROVIDER_PRIORITY_USER, so ~/.config/gtk-4.0/gtk.css can't override us.
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
        );
    }
}

/// A borderless window placed on the overlay layer, above every regular window and panel.
pub fn layer_window(app: &gtk::Application, namespace: &str) -> gtk::Window {
    let win = gtk::Window::new();
    win.set_application(Some(app));
    win.set_decorated(false);
    win.add_css_class("nice-screenshots");
    win.init_layer_shell();
    win.set_layer(Layer::Overlay);
    win.set_namespace(Some(namespace));
    win.set_keyboard_mode(KeyboardMode::None);
    win
}

pub fn icon_button(icon: Icon, size: i32, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_child(Some(&icons::widget(icon, size)));
    button.set_tooltip_text(Some(tooltip));
    button.set_focusable(false);
    button
}

/// Closes a window after `ms` milliseconds, giving fade-out transitions time to run.
pub fn close_after(win: &gtk::Window, ms: u64) {
    let win = win.downgrade();
    glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
        if let Some(win) = win.upgrade() {
            win.close();
        }
    });
}
