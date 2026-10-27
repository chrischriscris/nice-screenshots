//! Single-instance application: every `nice-screenshots …` invocation is forwarded to the running
//! instance over D-Bus, which is how a second key press can stop a recording.

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk_pixbuf::Pixbuf;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::config::{Config, Mode, Options, SavedRegion};
use crate::desktop::Desktop;
use crate::geometry::Rect;
use crate::overlay::{Overlay, Selection};
use crate::record::{self, Recording};
use crate::{capture, feedback, indicator, thumbnail, ui, util};

pub const APP_ID: &str = "io.github.chrischriscris.NiceScreenshots";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Toolbar,
    Screen,
    Window,
    Region,
    RecordScreen,
    RecordRegion,
    Stop,
}

impl Command {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "" | "toolbar" => Self::Toolbar,
            "screen" => Self::Screen,
            "window" => Self::Window,
            "region" => Self::Region,
            "record" | "record-region" => Self::RecordRegion,
            "record-screen" => Self::RecordScreen,
            "stop" => Self::Stop,
            _ => return None,
        })
    }
}

pub fn run(args: &[String]) -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_startup(|_| ui::load_css());

    let instance: RefCell<Option<Rc<App>>> = RefCell::default();
    app.connect_command_line(move |app, cmdline| {
        let app_state = instance
            .borrow_mut()
            .get_or_insert_with(|| App::new(app))
            .clone();
        let args = cmdline.arguments();
        let arg = args.get(1).and_then(|a| a.to_str()).unwrap_or("");
        if let Some(cmd) = Command::parse(arg) {
            app_state.dispatch(cmd);
        }
        glib::ExitCode::SUCCESS
    });
    app.run_with_args(args)
}

struct App {
    app: gtk::Application,
    cfg: Config,
    opts: Rc<RefCell<Options>>,
    overlay: RefCell<Option<Overlay>>,
    recording: RefCell<Option<Recording>>,
    indicator: RefCell<Option<gtk::Window>>,
    thumbnail: RefCell<Option<gtk::Window>>,
    /// A countdown or recorder start-up is in progress.
    busy: Cell<bool>,
}

impl App {
    fn new(app: &gtk::Application) -> Rc<Self> {
        record::write_status(None);
        if util::which("pactl") {
            record::remove_stale_mix();
        }
        Rc::new(Self {
            app: app.clone(),
            cfg: Config::load(),
            opts: Rc::new(RefCell::new(Options::load())),
            overlay: RefCell::default(),
            recording: RefCell::default(),
            indicator: RefCell::default(),
            thumbnail: RefCell::default(),
            busy: Cell::new(false),
        })
    }

    fn dispatch(self: &Rc<Self>, cmd: Command) {
        let recording = self.recording.borrow().is_some();
        let overlay_open = self.overlay.borrow().is_some();
        match cmd {
            Command::Stop => self.stop_recording(),
            Command::Toolbar | Command::RecordScreen | Command::RecordRegion if recording => {
                self.stop_recording()
            }
            _ if self.busy.get() => {}
            // Pressing the shortcut again while the overlay is up dismisses it.
            _ if overlay_open => self.overlay_done(None),
            Command::Toolbar => {
                let mode = self.opts.borrow().last_mode;
                self.open_overlay(mode, true);
            }
            Command::Region => self.open_overlay(Mode::Region, false),
            Command::Window => self.open_overlay(Mode::Window, false),
            Command::RecordRegion => self.open_overlay(Mode::RecordRegion, true),
            Command::RecordScreen => self.open_overlay(Mode::RecordScreen, true),
            Command::Screen => self.capture_focused_output(),
        }
    }

    fn open_overlay(self: &Rc<Self>, mode: Mode, toolbar: bool) {
        // A stale thumbnail would end up in the frozen frame.
        self.dismiss_thumbnail();
        let this = self.clone();
        match Overlay::open(
            &self.app,
            mode,
            toolbar,
            self.cfg.magnifier,
            self.opts.clone(),
            move |sel| this.overlay_done(sel),
        ) {
            Ok(overlay) => *self.overlay.borrow_mut() = Some(overlay),
            Err(e) => util::notify("Screenshot failed", &e),
        }
    }

    fn overlay_done(self: &Rc<Self>, selection: Option<Selection>) {
        if let Some(overlay) = self.overlay.borrow_mut().take() {
            overlay.close();
        }
        let Some(sel) = selection else { return };
        {
            let mut o = self.opts.borrow_mut();
            if sel.from_toolbar {
                o.last_mode = sel.mode;
                if sel.mode.is_region() {
                    o.last_region = Some(SavedRegion {
                        output: sel.output.clone(),
                        rect: sel.rect.relative_to(&sel.output_rect),
                    });
                }
            }
            o.save();
        }
        if sel.mode.is_record() {
            self.start_recording(sel);
        } else {
            self.take_screenshot(sel);
        }
    }

    fn capture_focused_output(self: &Rc<Self>) {
        let name = Desktop::query().focused_output.unwrap_or_default();
        self.dismiss_thumbnail();
        let cursor = self.opts.borrow().show_pointer;
        let this = self.clone();
        let guard = self.app.hold();
        // Give a dismissed thumbnail a moment to fade out first.
        glib::timeout_add_local_once(Duration::from_millis(80), move || {
            match capture::grab_output(&name, cursor) {
                Ok(pix) => this.present_screenshot(&pix, &name, None),
                Err(e) => util::notify("Screenshot failed", &e),
            }
            drop(guard);
        });
    }

    fn take_screenshot(self: &Rc<Self>, sel: Selection) {
        let (timer, cursor) = {
            let o = self.opts.borrow();
            (o.timer, o.show_pointer)
        };
        if timer == 0 {
            let pix = capture::crop(&sel.frozen, &sel.output_rect, &sel.rect);
            self.present_screenshot(&pix, &sel.output, Some(sel.rect));
            return;
        }
        // Timed capture: grab a fresh frame of the same area once the countdown ends.
        self.busy.set(true);
        let guard = self.app.hold();
        let this = self.clone();
        glib::spawn_future_local(async move {
            indicator::countdown(&this.app, timer).await;
            this.busy.set(false);
            match capture::grab_region(&sel.rect, cursor) {
                Ok(pix) => this.present_screenshot(&pix, &sel.output, Some(sel.rect)),
                Err(e) => util::notify("Screenshot failed", &e),
            }
            drop(guard);
        });
    }

    /// `output` and `area` say where the capture came from, for the flash.
    fn present_screenshot(self: &Rc<Self>, pix: &Pixbuf, output: &str, area: Option<Rect>) {
        if self.cfg.sound {
            feedback::shutter();
        }
        if self.cfg.flash {
            feedback::flash(&self.app, output, area);
        }
        let o = self.opts.borrow().clone();
        let path = match capture::save(pix, &self.cfg, o.save_to_folder) {
            Ok(path) => path,
            Err(e) => return util::notify("Could not save screenshot", &e),
        };
        if o.copy_to_clipboard || !o.save_to_folder {
            capture::copy_png(&path);
        }
        if o.show_thumbnail {
            self.show_thumbnail(thumbnail::Spec {
                preview: Some(path.clone()),
                path,
                kind: thumbnail::Kind::Image,
                in_folder: o.save_to_folder,
            });
        } else if o.save_to_folder {
            util::notify("Screenshot saved", &path.to_string_lossy());
        }
    }

    fn start_recording(self: &Rc<Self>, sel: Selection) {
        let opts = self.opts.borrow().clone();
        let target = record::Target {
            output: sel.output.clone(),
            rect: sel.rect,
            full_output: sel.mode == Mode::RecordScreen,
        };
        self.busy.set(true);
        let guard = self.app.hold();
        let this = self.clone();
        glib::spawn_future_local(async move {
            if opts.timer > 0 {
                indicator::countdown(&this.app, opts.timer).await;
            } else {
                // Wait for the overlay's fade-out so it isn't in the first frames.
                glib::timeout_future(Duration::from_millis(300)).await;
            }
            this.busy.set(false);
            let rec = match record::start(&this.cfg, &opts, &target) {
                Ok(rec) => rec,
                Err(e) => return util::notify("Recording failed", &e),
            };
            let process = rec.process();
            let started = rec.started;
            record::write_status(Some(&rec));
            *this.recording.borrow_mut() = Some(rec);
            if this.cfg.recording_pill {
                let stopper = Rc::downgrade(&this);
                let pill = indicator::recording(&this.app, started, move || {
                    if let Some(ph) = stopper.upgrade() {
                        ph.stop_recording();
                    }
                });
                *this.indicator.borrow_mut() = Some(pill);
            }

            let _ = process.wait_future().await;
            this.recording_finished().await;
            drop(guard);
        });
    }

    fn stop_recording(&self) {
        if let Some(rec) = self.recording.borrow().as_ref() {
            rec.stop();
        }
    }

    async fn recording_finished(self: &Rc<Self>) {
        let Some(rec) = self.recording.borrow_mut().take() else {
            return;
        };
        if let Some(pill) = self.indicator.borrow_mut().take() {
            pill.close();
        }
        record::write_status(None);

        let size = std::fs::metadata(&rec.path).map(|m| m.len()).unwrap_or(0);
        if size < 1024 {
            let _ = std::fs::remove_file(&rec.path);
            let details = record::log_tail(&rec.log);
            let body = if details.is_empty() {
                "The recorder exited without writing a video.".into()
            } else {
                details
            };
            return util::notify("Recording failed", &body);
        }

        let duration = rec.started.elapsed();
        record::finalize(&rec.path, rec.encoding).await;
        let preview = video_still(&rec.path).await;
        if self.opts.borrow().copy_to_clipboard {
            capture::copy_file_uri(&rec.path);
        }
        self.show_thumbnail(thumbnail::Spec {
            path: rec.path,
            preview,
            kind: thumbnail::Kind::Video { duration },
            in_folder: true,
        });
    }

    fn show_thumbnail(&self, spec: thumbnail::Spec) {
        self.dismiss_thumbnail();
        let win = thumbnail::show(&self.app, spec, &self.cfg);
        *self.thumbnail.borrow_mut() = Some(win);
    }

    fn dismiss_thumbnail(&self) {
        if let Some(win) = self.thumbnail.borrow_mut().take() {
            win.close();
        }
    }
}

/// Extracts a still frame from a recording with ffmpeg, if it is installed.
async fn video_still(video: &std::path::Path) -> Option<std::path::PathBuf> {
    if !util::which("ffmpeg") {
        return None;
    }
    let dir = util::runtime_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let still = dir.join("recording-preview.png");
    let argv = [
        OsStr::new("ffmpeg"),
        OsStr::new("-y"),
        OsStr::new("-loglevel"),
        OsStr::new("error"),
        OsStr::new("-i"),
        video.as_os_str(),
        OsStr::new("-frames:v"),
        OsStr::new("1"),
        OsStr::new("-vf"),
        OsStr::new("scale=560:-2"),
        still.as_os_str(),
    ];
    let process = gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDERR_SILENCE).ok()?;
    process.wait_future().await.ok()?;
    process.is_successful().then_some(still)
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn parses_commands() {
        assert_eq!(Command::parse(""), Some(Command::Toolbar));
        assert_eq!(Command::parse("record"), Some(Command::RecordRegion));
        assert_eq!(Command::parse("record-screen"), Some(Command::RecordScreen));
        assert_eq!(Command::parse("stop"), Some(Command::Stop));
        assert_eq!(Command::parse("nope"), None);
    }
}
