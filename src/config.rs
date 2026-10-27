use std::fs;
use std::path::PathBuf;

use gtk::glib;
use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::util;

/// User configuration, read from `~/.config/pinhole/config.toml`.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where screenshots are saved. Defaults to `$XDG_PICTURES_DIR/Screenshots`.
    pub screenshot_dir: String,
    /// Where recordings are saved. Defaults to `$XDG_VIDEOS_DIR/Screen Recordings`.
    pub recording_dir: String,
    /// strftime-style file name patterns (without extension).
    pub screenshot_name: String,
    pub recording_name: String,
    /// "auto" (satty, swappy, then the default image viewer) or a command; `{}` is replaced by the file path.
    pub editor: String,
    /// "auto", "wl-screenrec" or "wf-recorder".
    pub recorder: String,
    /// Seconds before the floating thumbnail disappears; 0 keeps it until dismissed.
    pub thumbnail_seconds: u32,
    /// Recording codec: "h264" (plays everywhere), "hevc", "vp9" or "av1".
    pub recording_codec: String,
    /// Recording container: "mp4", "mkv" (survives crashes) or "webm".
    pub recording_format: String,
    /// Frame rate cap for recordings; 0 records up to the display's refresh rate.
    pub recording_fps: u32,
    /// Show the floating pill with a timer and stop button while recording. Turn it off when
    /// a status bar shows a stop button instead (see status.json), as macOS does.
    pub recording_pill: bool,
    /// Show a pixel magnifier next to the pointer while selecting. Off by default, like macOS.
    pub magnifier: bool,
    /// Flash the captured area when a screenshot is taken.
    pub flash: bool,
    /// Play the camera shutter sound when a screenshot is taken.
    pub sound: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            screenshot_dir: String::new(),
            recording_dir: String::new(),
            screenshot_name: "Screenshot %Y-%m-%d at %H.%M.%S".into(),
            recording_name: "Screen Recording %Y-%m-%d at %H.%M.%S".into(),
            editor: "auto".into(),
            recorder: "auto".into(),
            thumbnail_seconds: 6,
            recording_codec: "h264".into(),
            recording_format: "mp4".into(),
            recording_fps: 0,
            recording_pill: true,
            magnifier: false,
            flash: true,
            sound: true,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = glib::user_config_dir().join("pinhole/config.toml");
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            util::notify("Pinhole config error", &format!("{}: {e}", path.display()));
            Self::default()
        })
    }

    pub fn screenshot_dir(&self) -> PathBuf {
        resolve_dir(
            &self.screenshot_dir,
            glib::UserDirectory::Pictures,
            "Pictures",
            "Screenshots",
        )
    }

    pub fn recording_dir(&self) -> PathBuf {
        resolve_dir(
            &self.recording_dir,
            glib::UserDirectory::Videos,
            "Videos",
            "Screen Recordings",
        )
    }
}

fn resolve_dir(
    configured: &str,
    special: glib::UserDirectory,
    fallback: &str,
    sub: &str,
) -> PathBuf {
    if let Some(rest) = configured.strip_prefix("~/") {
        return glib::home_dir().join(rest);
    }
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    glib::user_special_dir(special)
        .unwrap_or_else(|| glib::home_dir().join(fallback))
        .join(sub)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Screen,
    Window,
    #[default]
    Region,
    RecordScreen,
    RecordRegion,
}

impl Mode {
    pub fn is_record(self) -> bool {
        matches!(self, Mode::RecordScreen | Mode::RecordRegion)
    }

    pub fn is_region(self) -> bool {
        matches!(self, Mode::Region | Mode::RecordRegion)
    }
}

/// Recording quality, the toolbar's equivalent of macOS's HDR/SDR "Capture Format".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quality {
    /// Moderate bitrate: small files that play and upload everywhere.
    #[default]
    Standard,
    /// About 2.5x the bitrate, for fast motion and smooth gradients.
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedRegion {
    pub output: String,
    /// Relative to the output's top-left corner.
    pub rect: Rect,
}

/// Toolbar options, persisted between runs in `~/.local/state/pinhole/options.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub save_to_folder: bool,
    pub copy_to_clipboard: bool,
    pub timer: u32,
    pub show_pointer: bool,
    pub show_thumbnail: bool,
    pub remember_selection: bool,
    /// PulseAudio/PipeWire source name of the microphone to record, if any.
    pub microphone: Option<String>,
    /// Record what's playing (the default output's monitor).
    pub system_audio: bool,
    pub quality: Quality,
    /// Show the pointer in recordings (`show_pointer` covers screenshots).
    pub record_pointer: bool,
    pub last_mode: Mode,
    pub last_region: Option<SavedRegion>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            save_to_folder: true,
            copy_to_clipboard: true,
            timer: 0,
            show_pointer: false,
            show_thumbnail: true,
            remember_selection: true,
            microphone: None,
            system_audio: false,
            quality: Quality::Standard,
            record_pointer: true,
            last_mode: Mode::Region,
            last_region: None,
        }
    }
}

impl Options {
    fn path() -> PathBuf {
        util::state_dir().join("options.json")
    }

    pub fn load() -> Self {
        fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        let _ = fs::create_dir_all(path.parent().unwrap());
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = fs::write(path, json);
        }
    }
}
