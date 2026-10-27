//! Grabbing pixels with grim, cropping, and handing results to the clipboard.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

use gtk::gdk_pixbuf::Pixbuf;
use gtk::prelude::*;
use gtk::{cairo, gio, glib};

use crate::config::Config;
use crate::geometry::Rect;
use crate::util;

fn grim(args: &[&str]) -> Result<Pixbuf, String> {
    let out = Command::new("grim")
        .args(args)
        .arg("-")
        .output()
        .map_err(|e| format!("could not run grim: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(out.stdout));
    Pixbuf::from_stream(&stream, gio::Cancellable::NONE).map_err(|e| e.to_string())
}

/// Captures a whole output at its native resolution.
pub fn grab_output(name: &str, cursor: bool) -> Result<Pixbuf, String> {
    let mut args = vec!["-l", "0"];
    if cursor {
        args.push("-c");
    }
    if !name.is_empty() {
        args.extend(["-o", name]);
    }
    grim(&args)
}

/// Captures a region given in global logical coordinates.
pub fn grab_region(rect: &Rect, cursor: bool) -> Result<Pixbuf, String> {
    let geometry = rect.geometry();
    let mut args = vec!["-l", "0", "-g", geometry.as_str()];
    if cursor {
        args.push("-c");
    }
    grim(&args)
}

/// Crops `sel` (global logical coordinates) out of a frozen capture of the output at `mon`.
pub fn crop(frozen: &Pixbuf, mon: &Rect, sel: &Rect) -> Pixbuf {
    let fx = frozen.width() as f64 / mon.w;
    let fy = frozen.height() as f64 / mon.h;
    let x = (((sel.x - mon.x) * fx).round() as i32).clamp(0, frozen.width() - 1);
    let y = (((sel.y - mon.y) * fy).round() as i32).clamp(0, frozen.height() - 1);
    let w = ((sel.w * fx).round() as i32).clamp(1, frozen.width() - x);
    let h = ((sel.h * fy).round() as i32).clamp(1, frozen.height() - y);
    frozen.new_subpixbuf(x, y, w, h)
}

/// Converts a pixbuf to a cairo surface once so the overlay can repaint it cheaply.
pub fn to_surface(pix: &Pixbuf) -> Result<cairo::ImageSurface, String> {
    let (w, h) = (pix.width(), pix.height());
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).map_err(|e| e.to_string())?;
    let stride = surface.stride() as usize;
    let channels = pix.n_channels() as usize;
    let rowstride = pix.rowstride() as usize;
    let src = pix.read_pixel_bytes();
    {
        let mut dst = surface.data().map_err(|e| e.to_string())?;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let s = y * rowstride + x * channels;
                let a = if channels == 4 {
                    src[s + 3] as u32
                } else {
                    255
                };
                let pre = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                let d = y * stride + x * 4;
                // CAIRO_FORMAT_ARGB32 is native-endian premultiplied BGRA on little-endian.
                dst[d] = pre(src[s + 2]);
                dst[d + 1] = pre(src[s + 1]);
                dst[d + 2] = pre(src[s]);
                dst[d + 3] = a as u8;
            }
        }
    }
    surface.mark_dirty();
    Ok(surface)
}

/// Saves a screenshot, either into the screenshots folder or a scratch file for clipboard-only use.
pub fn save(pix: &Pixbuf, cfg: &Config, in_folder: bool) -> Result<PathBuf, String> {
    let dir = if in_folder {
        cfg.screenshot_dir()
    } else {
        util::runtime_dir()
    };
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = util::unique_path(&dir, &util::timestamp(&cfg.screenshot_name), "png");
    pix.savev(&path, "png", &[]).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn copy_png(path: &Path) {
    match File::open(path) {
        Ok(file) => util::spawn_detached(
            Command::new("wl-copy")
                .args(["--type", "image/png"])
                .stdin(file),
        ),
        Err(e) => util::notify("Copy failed", &e.to_string()),
    }
}

pub fn copy_file_uri(path: &Path) {
    let uri = gio::File::for_path(path).uri();
    util::spawn_detached(Command::new("wl-copy").args(["--type", "text/uri-list", uri.as_str()]));
}
