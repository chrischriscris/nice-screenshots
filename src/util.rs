use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

pub fn which(bin: &str) -> bool {
    glib::find_program_in_path(bin).is_some()
}

/// Spawns a command without blocking; a thread reaps it when it exits.
pub fn spawn_detached(cmd: &mut Command) {
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("pinhole: failed to run {:?}: {e}", cmd.get_program()),
    }
}

pub fn notify(summary: &str, body: &str) {
    eprintln!("pinhole: {summary}: {body}");
    spawn_detached(Command::new("notify-send").args(["-a", "Pinhole", summary, body]));
}

pub fn open_path(path: &Path) {
    let uri = gio::File::for_path(path).uri();
    if gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>).is_err() {
        spawn_detached(Command::new("xdg-open").arg(path));
    }
}

pub fn runtime_dir() -> PathBuf {
    glib::user_runtime_dir().join("pinhole")
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local/state"))
        .join("pinhole")
}

pub fn timestamp(format: &str) -> String {
    glib::DateTime::now_local()
        .and_then(|d| d.format(format))
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "capture".into())
}

/// Returns `dir/name.ext`, appending " (n)" if that file already exists.
pub fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{name} ({n}).{ext}"));
        n += 1;
    }
    path
}

pub fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
