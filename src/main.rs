mod app;
mod capture;
mod config;
mod desktop;
mod feedback;
mod geometry;
mod icons;
mod indicator;
mod overlay;
mod record;
mod thumbnail;
mod ui;
mod util;

use gtk::glib;

const USAGE: &str = "\
pinhole — screenshots and screen recordings for Hyprland and sway, the macOS way

Usage: pinhole [COMMAND]

Commands:
  (none), toolbar   Open the capture toolbar (like Cmd+Shift+5); stops a running recording
  screen            Capture the focused screen immediately (Cmd+Shift+3)
  region            Drag to capture a region; Space switches to window mode (Cmd+Shift+4)
  window            Click a window to capture it
  record-region     Open the toolbar in region recording mode
  record-screen     Open the toolbar in screen recording mode
  stop              Stop the current recording

In the overlay: drag to select, drag handles to resize, arrows to nudge (Shift ×10,
Alt resizes), Enter or double-click to capture, Esc or right click to cancel.

Config: ~/.config/pinhole/config.toml
Status: $XDG_RUNTIME_DIR/pinhole/status.json (for status bars)
";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    match cmd {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return glib::ExitCode::SUCCESS;
        }
        "-V" | "--version" => {
            println!("pinhole {}", env!("CARGO_PKG_VERSION"));
            return glib::ExitCode::SUCCESS;
        }
        _ => {}
    }
    if app::Command::parse(cmd).is_none() || args.len() > 2 {
        eprint!(
            "pinhole: unknown command '{}'\n\n{USAGE}",
            args[1..].join(" ")
        );
        return glib::ExitCode::FAILURE;
    }
    app::run(&args)
}
