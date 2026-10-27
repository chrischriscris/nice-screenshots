//! Screen recording through wl-screenrec or wf-recorder.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use gtk::gio;

use crate::config::{Config, Options, Quality};
use crate::geometry::Rect;
use crate::util;

pub struct Target {
    pub output: String,
    pub rect: Rect,
    pub full_output: bool,
}

pub struct Recording {
    process: gio::Subprocess,
    pub path: PathBuf,
    pub log: PathBuf,
    pub started: Instant,
    pub encoding: Encoding,
    /// Set while the microphone and system audio are mixed into one source.
    _mix: Option<AudioMix>,
}

impl Recording {
    pub fn process(&self) -> gio::Subprocess {
        self.process.clone()
    }

    /// Asks the recorder to finish; it flushes the file and exits on SIGINT.
    pub fn stop(&self) {
        // GSubprocess ignores signals once the child has been reaped.
        self.process.send_signal(libc::SIGINT);
    }
}

/// PulseAudio/PipeWire name for the monitor of the default output device.
const SYSTEM_AUDIO: &str = "@DEFAULT_MONITOR@";

fn has_vaapi_driver() -> bool {
    [
        "/usr/lib/dri",
        "/usr/lib64/dri",
        "/usr/lib/x86_64-linux-gnu/dri",
    ]
    .iter()
    .filter_map(|d| fs::read_dir(d).ok())
    .flatten()
    .flatten()
    .any(|e| e.file_name().to_string_lossy().ends_with("_drv_video.so"))
}

/// Whether i915 loaded the GuC firmware (`enable_guc` bit 1), which Intel's low-power encoder needs.
fn low_power_encoder_available() -> bool {
    // Not an Intel GPU: let wl-screenrec decide.
    if !std::path::Path::new("/sys/module/i915").exists() {
        return true;
    }
    // The parameter is often root-only; it defaults to off before Gen12 (Tiger Lake).
    fs::read_to_string("/sys/module/i915/parameters/enable_guc")
        .is_ok_and(|v| v.trim().parse::<i32>().is_ok_and(|n| n & 2 != 0))
}

fn pick_backend(pref: &str, codec: Codec) -> Option<&'static str> {
    match pref {
        "wl-screenrec" | "wf-recorder" => {
            let name = if pref == "wl-screenrec" {
                "wl-screenrec"
            } else {
                "wf-recorder"
            };
            util::which(name).then_some(name)
        }
        // Few GPUs encode VP9/AV1 (Intel needs GuC firmware, AMD needs RDNA3), so encode
        // those on the CPU through wf-recorder unless the user asked for wl-screenrec.
        _ if matches!(codec, Codec::Vp9 | Codec::Av1) && util::which("wf-recorder") => {
            Some("wf-recorder")
        }
        // wl-screenrec is the lighter option but needs a working VA-API driver.
        _ if util::which("wl-screenrec") && has_vaapi_driver() => Some("wl-screenrec"),
        _ if util::which("wf-recorder") => Some("wf-recorder"),
        _ if util::which("wl-screenrec") => Some("wl-screenrec"),
        _ => None,
    }
}

/// Codec and container, after reconciling the config with what the container can hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Codec {
    H264,
    Hevc,
    Vp9,
    Av1,
}

#[derive(Clone, Copy, Debug)]
pub struct Encoding {
    pub codec: Codec,
    pub ext: &'static str,
}

impl Encoding {
    fn from_config(cfg: &Config) -> (Self, Option<String>) {
        let mut note = None;
        let mut codec = match cfg.recording_codec.to_ascii_lowercase().as_str() {
            "h264" | "avc" | "" | "auto" => Codec::H264,
            "hevc" | "h265" => Codec::Hevc,
            "vp9" => Codec::Vp9,
            "av1" => Codec::Av1,
            other => {
                note = Some(format!("Unknown recording_codec \"{other}\", using h264."));
                Codec::H264
            }
        };
        let ext = match cfg.recording_format.to_ascii_lowercase().as_str() {
            "mp4" | "" => "mp4",
            "mkv" => "mkv",
            "webm" => "webm",
            other => {
                note = Some(format!("Unknown recording_format \"{other}\", using mp4."));
                "mp4"
            }
        };
        // WebM only holds VP8/VP9/AV1.
        if ext == "webm" && matches!(codec, Codec::H264 | Codec::Hevc) {
            note = Some("WebM can't hold H.264 or HEVC, recording VP9 instead.".into());
            codec = Codec::Vp9;
        }
        (Self { codec, ext }, note)
    }
}

/// A temporary PipeWire/PulseAudio sink that mixes the microphone with system audio,
/// since both recorders take a single audio source. Unloaded when the recording ends.
struct AudioMix {
    modules: Vec<String>,
}

const MIX_SINK: &str = "nice_screenshots_mix";

impl AudioMix {
    fn new(mic: &str) -> Result<Self, String> {
        let default_sink = pactl(&["get-default-sink"])?;
        let mut mix = AudioMix {
            modules: Vec::new(),
        };
        // Lowest priority, so WirePlumber never prefers it over a real output. (Restoring the
        // default with `pactl set-default-sink` would save it as the user's preference.)
        mix.load(&[
            "module-null-sink",
            &format!("sink_name={MIX_SINK}"),
            "sink_properties=device.description=Screenshots-Recording-Mix priority.session=0 priority.driver=0 node.passive=true",
        ])?;
        mix.load(&[
            "module-loopback",
            &format!("source={mic}"),
            &format!("sink={MIX_SINK}"),
            "latency_msec=20",
        ])?;
        mix.load(&[
            "module-loopback",
            &format!("source={default_sink}.monitor"),
            &format!("sink={MIX_SINK}"),
            "latency_msec=20",
        ])?;
        // System audio comes from the previous default's monitor, never the mix's own, so
        // there is no feedback loop even if the mix briefly becomes the default output.
        Ok(mix)
    }

    fn load(&mut self, args: &[&str]) -> Result<(), String> {
        let mut full = vec!["load-module"];
        full.extend_from_slice(args);
        self.modules.push(pactl(&full)?);
        Ok(())
    }

    fn source() -> String {
        format!("{MIX_SINK}.monitor")
    }
}

impl Drop for AudioMix {
    fn drop(&mut self) {
        for id in self.modules.iter().rev() {
            let _ = pactl(&["unload-module", id]);
        }
    }
}

/// Unloads mix modules left behind if a previous nice-screenshots crashed mid-recording.
pub fn remove_stale_mix() {
    let Ok(modules) = pactl(&["list", "short", "modules"]) else {
        return;
    };
    for line in modules.lines().filter(|l| l.contains(MIX_SINK)) {
        if let Some(id) = line.split('\t').next() {
            let _ = pactl(&["unload-module", id]);
        }
    }
}

fn pactl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("pactl")
        .args(args)
        .output()
        .map_err(|e| format!("pactl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "pactl {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A microphone the user can pick in the toolbar: (source name, human-readable description).
pub fn microphones() -> Vec<(String, String)> {
    let Ok(json) = pactl(&["-f", "json", "list", "sources"]) else {
        return Vec::new();
    };
    let Ok(sources) = serde_json::from_str::<Vec<serde_json::Value>>(&json) else {
        return Vec::new();
    };
    sources
        .iter()
        .filter(|s| {
            s["properties"]["device.class"] != "monitor"
                && s["properties"]["media.class"] == "Audio/Source"
        })
        .filter_map(|s| {
            let name = s["name"].as_str()?;
            (!name.starts_with(MIX_SINK)).then(|| {
                (
                    name.to_string(),
                    s["description"].as_str().unwrap_or(name).to_string(),
                )
            })
        })
        .collect()
}

pub fn start(cfg: &Config, opts: &Options, target: &Target) -> Result<Recording, String> {
    let (encoding, note) = Encoding::from_config(cfg);
    let backend = pick_backend(&cfg.recorder, encoding.codec)
        .ok_or("No screen recorder found. Install wf-recorder or wl-screenrec.")?;
    if let Some(note) = note {
        util::notify("Recording settings", &note);
    }

    // A microphone that has since been unplugged is skipped rather than failing the recording.
    let mic = opts
        .microphone
        .clone()
        .filter(|m| microphones().iter().any(|(name, _)| name == m));
    let mut mix = None;
    let audio: Option<String> = match (mic, opts.system_audio) {
        (None, false) => None,
        (Some(mic), false) => Some(mic),
        (None, true) => Some(SYSTEM_AUDIO.into()),
        (Some(mic), true) => {
            mix = Some(
                AudioMix::new(&mic)
                    .map_err(|e| format!("Could not mix microphone and system audio: {e}"))?,
            );
            Some(AudioMix::source())
        }
    };

    let dir = cfg.recording_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = util::unique_path(&dir, &util::timestamp(&cfg.recording_name), encoding.ext);
    let geometry = target.rect.even().geometry();
    let high = opts.quality == Quality::High;

    let mut argv: Vec<String> = vec![backend.into()];
    let mut push = |args: &[&str]| argv.extend(args.iter().map(|s| s.to_string()));
    match backend {
        "wf-recorder" => {
            push(&["-y", "-f", &path.to_string_lossy(), "-x", "yuv420p"]);
            let crf = |standard: u32, better: u32| {
                format!("crf={}", if high { better } else { standard })
            };
            match encoding.codec {
                Codec::H264 => {
                    push(&["-c", "libx264", "-p", "preset=veryfast", "-p", &crf(23, 18)])
                }
                Codec::Hevc => {
                    push(&["-c", "libx265", "-p", "preset=veryfast", "-p", &crf(26, 21)])
                }
                Codec::Vp9 => push(&[
                    "-c",
                    "libvpx-vp9",
                    "-p",
                    "deadline=realtime",
                    "-p",
                    "cpu-used=8",
                    "-p",
                    "b=0",
                    "-p",
                    &crf(34, 28),
                ]),
                Codec::Av1 => push(&["-c", "libsvtav1", "-p", "preset=10", "-p", &crf(36, 30)]),
            }
            // SVT-AV1 rejects the variable frame rate wf-recorder reports, so give it a fixed one.
            let fps = match (cfg.recording_fps, encoding.codec) {
                (0, Codec::Av1) => 60,
                (fps, _) => fps,
            };
            if fps > 0 {
                push(&["-r", &fps.to_string()]);
            }
            if target.full_output {
                push(&["-o", &target.output]);
            } else {
                push(&["-g", &geometry]);
            }
            if let Some(device) = &audio {
                push(&[&format!("--audio={device}")]);
            }
        }
        _ => {
            push(&["-f", &path.to_string_lossy()]);
            // Intel's low-power encoder needs GuC firmware enabled; without it wl-screenrec
            // fails once and retries. Skip straight to the encoder that works everywhere.
            if !low_power_encoder_available() {
                push(&["--low-power=off"]);
            }
            let codec = match encoding.codec {
                Codec::H264 => "avc",
                Codec::Hevc => "hevc",
                Codec::Vp9 => "vp9",
                Codec::Av1 => "av1",
            };
            push(&["--codec", codec]);
            // wl-screenrec's default (40 Mbit/s) makes huge files; ~0.1 bit per pixel at 60 fps
            // still looks sharp for screen content and keeps clips easy to share.
            let (factor, max) = if high { (2.5, 7500.0) } else { (1.0, 3000.0) };
            let kbytes = (target.rect.w * target.rect.h * 60.0 * 0.1 / 8.0 / 1000.0 * factor)
                .clamp(500.0, max);
            push(&["-b", &format!("{} kB", kbytes as u32)]);
            if cfg.recording_fps > 0 {
                push(&["-m", &cfg.recording_fps.to_string()]);
            }
            if !opts.record_pointer {
                push(&["--no-cursor"]);
            }
            if target.full_output {
                push(&["-o", &target.output]);
            } else {
                push(&["-g", &geometry]);
            }
            if let Some(device) = &audio {
                push(&["--audio", "--audio-device", device]);
            }
        }
    }

    let log = util::state_dir().join("recorder.log");
    let _ = fs::create_dir_all(log.parent().unwrap());
    // GIO opens the file without truncating it, so clear out the previous run's output.
    let _ = fs::remove_file(&log);
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDERR_MERGE);
    launcher.set_stdout_file_path(Some(&log));
    let args: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
    let process = launcher
        .spawn(&args)
        .map_err(|e| format!("could not start {backend}: {e}"))?;

    Ok(Recording {
        process,
        path,
        log,
        started: Instant::now(),
        encoding,
        _mix: mix,
    })
}

/// Makes a finished MP4 friendly to phones and upload sites without re-encoding: moves the
/// index to the front (so it streams before fully downloading) and, for H.264, corrects the
/// level, which both recorders get wrong for variable-frame-rate screen captures.
pub async fn finalize(path: &Path, encoding: Encoding) {
    // MKV and WebM stream fine as they are; only MP4 keeps its index at the end.
    if encoding.ext != "mp4" || !util::which("ffmpeg") || !util::which("ffprobe") {
        return;
    }
    let Some(name) = path.file_name() else { return };
    let tmp = path.with_file_name(format!(".{}.part.mp4", name.to_string_lossy()));
    let mut argv: Vec<std::ffi::OsString> = ["ffmpeg", "-v", "error", "-y", "-i"]
        .map(Into::into)
        .to_vec();
    argv.push(path.into());
    argv.extend(["-map", "0", "-c", "copy", "-movflags", "+faststart"].map(Into::into));
    if encoding.codec == Codec::H264 {
        let Some((w, h)) = video_size(path).await else {
            return;
        };
        argv.push("-bsf:v".into());
        argv.push(format!("h264_metadata=level={}", h264_level(w, h)).into());
    }
    argv.push(tmp.clone().into());
    let argv: Vec<&OsStr> = argv.iter().map(|a| a.as_os_str()).collect();
    let ok = match gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDERR_SILENCE) {
        Ok(p) => p.wait_future().await.is_ok() && p.is_successful(),
        Err(_) => false,
    };
    if !(ok && fs::rename(&tmp, path).is_ok()) {
        let _ = fs::remove_file(&tmp);
    }
}

async fn video_size(path: &Path) -> Option<(u32, u32)> {
    let argv = [
        OsStr::new("ffprobe"),
        OsStr::new("-v"),
        OsStr::new("error"),
        OsStr::new("-select_streams"),
        OsStr::new("v:0"),
        OsStr::new("-show_entries"),
        OsStr::new("stream=width,height"),
        OsStr::new("-of"),
        OsStr::new("csv=p=0:s=x"),
        path.as_os_str(),
    ];
    let p = gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDOUT_PIPE).ok()?;
    let (out, _) = p.communicate_utf8_future(None).await.ok()?;
    let (w, h) = out?
        .trim()
        .split_once('x')
        .map(|(w, h)| (w.parse().ok(), h.parse().ok()))?;
    Some((w?, h?))
}

/// The lowest H.264 level that allows this frame size at 60 fps, as `level_idc`
/// (ffmpeg reads "4.0" as level 0.4, so use 40, 42, ... rather than dotted names).
fn h264_level(w: u32, h: u32) -> u32 {
    let macroblocks = w.div_ceil(16) * h.div_ceil(16);
    match macroblocks {
        0..=4096 => 40,
        4097..=8192 => 42,
        8193..=16384 => 51,
        16385..=34560 => 52,
        _ => 62,
    }
}

pub fn log_tail(log: &Path) -> String {
    let text = fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(4)..].join("\n")
}

/// Writes `$XDG_RUNTIME_DIR/nice-screenshots/status.json` so status bars can show recording state.
pub fn write_status(recording: Option<&Recording>) {
    let dir = util::runtime_dir();
    let _ = fs::create_dir_all(&dir);
    let started = recording.map(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    });
    let status = serde_json::json!({
        "recording": recording.is_some(),
        "started": started,
        "path": recording.map(|r| r.path.to_string_lossy().to_string()),
    });
    let _ = fs::write(dir.join("status.json"), status.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(codec: &str, format: &str) -> Config {
        Config {
            recording_codec: codec.into(),
            recording_format: format.into(),
            ..Config::default()
        }
    }

    #[test]
    fn h264_levels() {
        assert_eq!(h264_level(1280, 720), 40);
        assert_eq!(h264_level(1600, 900), 42);
        assert_eq!(h264_level(1920, 1080), 42);
        assert_eq!(h264_level(2560, 1440), 51);
        assert_eq!(h264_level(3840, 2160), 52);
    }

    #[test]
    fn encoding_defaults_to_h264_mp4() {
        let (e, note) = Encoding::from_config(&Config::default());
        assert_eq!((e.codec, e.ext, note), (Codec::H264, "mp4", None));
    }

    #[test]
    fn webm_switches_to_vp9() {
        let (e, note) = Encoding::from_config(&config("h264", "webm"));
        assert_eq!((e.codec, e.ext), (Codec::Vp9, "webm"));
        assert!(note.is_some());
    }

    #[test]
    fn unknown_values_fall_back() {
        let (e, note) = Encoding::from_config(&config("prores", "avi"));
        assert_eq!((e.codec, e.ext), (Codec::H264, "mp4"));
        assert!(note.is_some());
    }
}
