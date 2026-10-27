//! The floating preview card shown in the corner after a capture, like macOS.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk_pixbuf::Pixbuf;
use gtk::prelude::*;
use gtk::{cairo, gdk, gio, glib};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::config::Config;
use crate::icons::{self, Icon};
use crate::{capture, ui, util};

pub enum Kind {
    Image,
    Video { duration: Duration },
}

pub struct Spec {
    /// The captured file.
    pub path: PathBuf,
    /// Image to preview; the capture itself for screenshots, a still frame for recordings.
    pub preview: Option<PathBuf>,
    pub kind: Kind,
    /// Whether `path` lives in a user-visible folder (as opposed to a scratch file).
    pub in_folder: bool,
}

const WIDTH: i32 = 264;
const FADE_MS: u32 = 180;

pub fn show(app: &gtk::Application, spec: Spec, cfg: &Config) -> gtk::Window {
    let win = ui::layer_window(app, "pinhole-thumbnail");
    win.set_anchor(Edge::Bottom, true);
    win.set_anchor(Edge::Right, true);

    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    revealer.set_transition_duration(FADE_MS);

    let dismiss = {
        let win = win.downgrade();
        let revealer = revealer.downgrade();
        Rc::new(move || {
            if let (Some(win), Some(revealer)) = (win.upgrade(), revealer.upgrade()) {
                revealer.set_reveal_child(false);
                ui::close_after(&win, FADE_MS as u64);
            }
        })
    };

    let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
    card.add_css_class("ph-card");

    let stack = gtk::Overlay::new();
    stack.set_child(Some(&preview_widget(&spec)));

    if let Kind::Video { duration } = spec.kind {
        let badge = gtk::Label::new(Some(&util::format_duration(duration)));
        badge.add_css_class("ph-badge");
        badge.set_halign(gtk::Align::Start);
        badge.set_valign(gtk::Align::End);
        stack.add_overlay(&badge);
    }

    let close = ui::icon_button(Icon::Close, 18, "Dismiss");
    close.add_css_class("ph-close");
    close.set_halign(gtk::Align::End);
    close.set_valign(gtk::Align::Start);
    close.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });
    stack.add_overlay(&close);
    card.append(&stack);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    actions.add_css_class("ph-actions");
    actions.set_homogeneous(true);
    let path = Rc::new(spec.path.clone());

    let add = |icon: Icon, tooltip: &str, f: Box<dyn Fn(&Path)>| {
        let button = ui::icon_button(icon, 18, tooltip);
        let path = path.clone();
        let dismiss = dismiss.clone();
        button.connect_clicked(move |_| {
            f(&path);
            dismiss();
        });
        actions.append(&button);
    };

    match spec.kind {
        Kind::Image => {
            add(Icon::Copy, "Copy", Box::new(capture::copy_png));
            let editor = cfg.editor.clone();
            add(
                Icon::Edit,
                "Mark up",
                Box::new(move |p| open_editor(&editor, p)),
            );
        }
        Kind::Video { .. } => {
            add(Icon::Play, "Play", Box::new(util::open_path));
            add(Icon::Copy, "Copy file", Box::new(capture::copy_file_uri));
        }
    }
    if spec.in_folder {
        add(Icon::Folder, "Show in folder", Box::new(show_in_folder));
    }
    add(Icon::Trash, "Delete", Box::new(trash));
    card.append(&actions);

    // Click the preview to open the file; drag it into other apps like a file.
    let click = gtk::GestureClick::new();
    click.connect_released({
        let path = path.clone();
        let dismiss = dismiss.clone();
        move |_, _, _, _| {
            util::open_path(&path);
            dismiss();
        }
    });
    stack.add_controller(click);

    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    drag.connect_prepare({
        let path = path.clone();
        move |_, _, _| {
            let files = gdk::FileList::from_array(&[gio::File::for_path(path.as_path())]);
            Some(gdk::ContentProvider::for_value(&files.to_value()))
        }
    });
    stack.add_controller(drag);

    // Auto-dismiss, paused while the pointer is over the card.
    let generation = Rc::new(Cell::new(0u32));
    let schedule = {
        let generation = generation.clone();
        let dismiss = dismiss.clone();
        let seconds = cfg.thumbnail_seconds;
        Rc::new(move || {
            let current = generation.get().wrapping_add(1);
            generation.set(current);
            if seconds == 0 {
                return;
            }
            let generation = generation.clone();
            let dismiss = dismiss.clone();
            glib::timeout_add_local_once(Duration::from_secs(seconds as u64), move || {
                if generation.get() == current {
                    dismiss();
                }
            });
        })
    };
    let hover = gtk::EventControllerMotion::new();
    hover.connect_enter({
        let generation = generation.clone();
        move |_, _, _| generation.set(generation.get().wrapping_add(1))
    });
    hover.connect_leave({
        let schedule = schedule.clone();
        move |_| schedule()
    });
    card.add_controller(hover);

    revealer.set_child(Some(&card));
    win.set_child(Some(&revealer));
    win.present();
    glib::idle_add_local_once({
        let revealer = revealer.downgrade();
        move || {
            if let Some(r) = revealer.upgrade() {
                r.set_reveal_child(true);
            }
        }
    });
    schedule();
    win
}

fn preview_widget(spec: &Spec) -> gtk::Widget {
    // Decode at twice the card width so the preview stays sharp on HiDPI outputs.
    let image = spec
        .preview
        .as_ref()
        .and_then(|p| Pixbuf::from_file_at_scale(p, WIDTH * 2, -1, true).ok())
        .and_then(|pix| capture::to_surface(&pix).ok());
    let Some(image) = image else {
        let placeholder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        placeholder.add_css_class("ph-preview");
        placeholder.set_size_request(WIDTH, WIDTH * 9 / 16);
        let icon = icons::widget(Icon::Play, 40);
        icon.set_vexpand(true);
        placeholder.append(&icon);
        return placeholder.upcast();
    };
    let aspect = image.height() as f64 / image.width().max(1) as f64;
    let height = ((WIDTH as f64 * aspect) as i32).clamp(72, 220);

    // A drawing area has exactly the size we ask for, unlike GtkPicture which
    // reports the image's natural size and would stretch the layer surface.
    let area = gtk::DrawingArea::new();
    area.set_content_width(WIDTH);
    area.set_content_height(height);
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let (iw, ih) = (image.width() as f64, image.height() as f64);
        // Cover: fill the area, cropping the overflow from the centre.
        let scale = (w / iw).max(h / ih);
        icons::rounded_rect(cr, 0.0, 0.0, w, h, 9.0);
        cr.clip();
        cr.translate((w - iw * scale) / 2.0, (h - ih * scale) / 2.0);
        cr.scale(scale, scale);
        let _ = cr.set_source_surface(&image, 0.0, 0.0);
        cr.source().set_filter(cairo::Filter::Good);
        let _ = cr.paint();
    });
    area.upcast()
}

fn open_editor(editor: &str, path: &Path) {
    let p = path.to_string_lossy().to_string();
    let argv: Vec<String> = if editor != "auto" && !editor.trim().is_empty() {
        let mut parts: Vec<String> = editor
            .split_whitespace()
            .map(|s| s.replace("{}", &p))
            .collect();
        if !editor.contains("{}") {
            parts.push(p.clone());
        }
        parts
    } else if util::which("satty") {
        [
            "satty",
            "--filename",
            &p,
            "--output-filename",
            &p,
            "--copy-command",
            "wl-copy",
        ]
        .map(String::from)
        .to_vec()
    } else if util::which("swappy") {
        ["swappy", "-f", &p, "-o", &p].map(String::from).to_vec()
    } else {
        util::open_path(path);
        return;
    };
    util::spawn_detached(Command::new(&argv[0]).args(&argv[1..]));
}

fn show_in_folder(path: &Path) {
    // Ask the file manager to highlight the file; fall back to opening the folder.
    let uri = gio::File::for_path(path).uri();
    let shown = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("array:string:{uri}"),
            "string:",
        ])
        .status()
        .is_ok_and(|s| s.success());
    if !shown {
        if let Some(dir) = path.parent() {
            util::open_path(dir);
        }
    }
}

fn trash(path: &Path) {
    if let Err(e) = gio::File::for_path(path).trash(gio::Cancellable::NONE) {
        util::notify("Could not delete", &e.to_string());
    }
}
