//! Small vector icons drawn with cairo, so the app doesn't depend on an icon theme.

use std::f64::consts::PI;

use gtk::cairo::{Context, LineCap, LineJoin};
use gtk::prelude::*;

#[derive(Clone, Copy, Debug)]
pub enum Icon {
    Close,
    Screen,
    Window,
    Region,
    RecordScreen,
    RecordRegion,
    Copy,
    Edit,
    Folder,
    Trash,
    Stop,
    Play,
    Chevron,
}

const RED: (f64, f64, f64) = (1.0, 0.271, 0.227);

pub fn widget(icon: Icon, size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_can_target(false);
    area.set_valign(gtk::Align::Center);
    area.set_halign(gtk::Align::Center);
    area.set_draw_func(move |area, cr, w, h| {
        let c = area.color();
        let s = w.min(h) as f64 / 24.0;
        cr.translate((w as f64 - 24.0 * s) / 2.0, (h as f64 - 24.0 * s) / 2.0);
        cr.scale(s, s);
        cr.set_line_width(1.6);
        cr.set_line_cap(LineCap::Round);
        cr.set_line_join(LineJoin::Round);
        cr.set_source_rgba(
            c.red() as f64,
            c.green() as f64,
            c.blue() as f64,
            c.alpha() as f64,
        );
        draw(icon, cr);
    });
    area
}

pub fn rounded_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn record_dot(cr: &Context) {
    let _ = cr.save();
    cr.set_source_rgb(RED.0, RED.1, RED.2);
    cr.arc(18.0, 17.0, 4.0, 0.0, 2.0 * PI);
    let _ = cr.fill();
    let _ = cr.restore();
}

fn screen(cr: &Context) {
    rounded_rect(cr, 2.8, 4.8, 18.4, 12.4, 2.0);
    cr.move_to(12.0, 17.2);
    cr.line_to(12.0, 20.0);
    cr.move_to(8.5, 20.2);
    cr.line_to(15.5, 20.2);
    let _ = cr.stroke();
}

fn dashed_region(cr: &Context) {
    cr.set_dash(&[2.2, 2.0], 0.0);
    cr.rectangle(3.5, 5.5, 17.0, 13.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
}

fn draw(icon: Icon, cr: &Context) {
    match icon {
        Icon::Close => {
            cr.arc(12.0, 12.0, 8.5, 0.0, 2.0 * PI);
            cr.move_to(9.0, 9.0);
            cr.line_to(15.0, 15.0);
            cr.move_to(15.0, 9.0);
            cr.line_to(9.0, 15.0);
            let _ = cr.stroke();
        }
        Icon::Screen => screen(cr),
        Icon::Window => {
            rounded_rect(cr, 2.8, 4.3, 18.4, 15.4, 2.5);
            cr.move_to(2.8, 8.5);
            cr.line_to(21.2, 8.5);
            let _ = cr.stroke();
            for x in [5.4, 7.6] {
                cr.arc(x, 6.4, 0.7, 0.0, 2.0 * PI);
                let _ = cr.fill();
            }
        }
        Icon::Region => dashed_region(cr),
        Icon::RecordScreen => {
            screen(cr);
            record_dot(cr);
        }
        Icon::RecordRegion => {
            dashed_region(cr);
            record_dot(cr);
        }
        Icon::Copy => {
            rounded_rect(cr, 8.5, 8.5, 11.0, 12.0, 2.0);
            cr.move_to(5.0, 15.5);
            cr.line_to(5.0, 6.0);
            cr.arc(7.0, 5.5, 1.5, PI, 1.5 * PI);
            cr.line_to(15.0, 4.0);
            let _ = cr.stroke();
        }
        Icon::Edit => {
            cr.move_to(5.0, 19.0);
            cr.line_to(5.8, 15.2);
            cr.line_to(15.8, 5.2);
            cr.line_to(18.8, 8.2);
            cr.line_to(8.8, 18.2);
            cr.close_path();
            cr.move_to(13.8, 7.2);
            cr.line_to(16.8, 10.2);
            let _ = cr.stroke();
        }
        Icon::Folder => {
            cr.move_to(3.0, 17.5);
            cr.line_to(3.0, 6.5);
            cr.line_to(9.0, 6.5);
            cr.line_to(11.0, 8.5);
            cr.line_to(21.0, 8.5);
            cr.line_to(21.0, 17.5);
            cr.close_path();
            let _ = cr.stroke();
        }
        Icon::Trash => {
            cr.move_to(4.5, 7.0);
            cr.line_to(19.5, 7.0);
            cr.move_to(9.0, 7.0);
            cr.line_to(9.5, 4.5);
            cr.line_to(14.5, 4.5);
            cr.line_to(15.0, 7.0);
            cr.move_to(6.5, 7.0);
            cr.line_to(7.5, 20.0);
            cr.line_to(16.5, 20.0);
            cr.line_to(17.5, 7.0);
            cr.move_to(10.2, 10.5);
            cr.line_to(10.2, 16.5);
            cr.move_to(13.8, 10.5);
            cr.line_to(13.8, 16.5);
            let _ = cr.stroke();
        }
        Icon::Stop => {
            rounded_rect(cr, 6.5, 6.5, 11.0, 11.0, 2.0);
            let _ = cr.fill();
        }
        Icon::Play => {
            cr.move_to(8.0, 5.5);
            cr.line_to(18.5, 12.0);
            cr.line_to(8.0, 18.5);
            cr.close_path();
            let _ = cr.fill();
        }
        Icon::Chevron => {
            cr.move_to(8.0, 10.0);
            cr.line_to(12.0, 14.0);
            cr.line_to(16.0, 10.0);
            let _ = cr.stroke();
        }
    }
}
