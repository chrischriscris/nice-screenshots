# pinhole

macOS-style screenshots and screen recordings for Hyprland and sway.

pinhole brings the Cmd+Shift+3/4/5 workflow to wlroots-style Wayland compositors: a capture
toolbar, a frozen-screen selection with a size readout and magnifier, window picking, a floating
thumbnail after each capture, and screen recording that stops from your status bar.

![Selecting a region: live size, handles and the capture toolbar](docs/region.png)

## Features

- **Capture toolbar** (like Cmd+Shift+5): entire screen, a window, or a selected portion;
  record the screen or a portion; options; Capture/Record.
- **Frozen-frame selection**: the screen freezes while you choose, so menus and tooltips stay
  put. Crosshair guides with coordinates, resize handles, a live `W × H` readout, arrow-key
  nudging, and it remembers your last selection. An optional pixel magnifier helps with
  pixel-exact picks.
- **Window picking**: hover to highlight a window, click to capture it. Press Space to switch
  between selecting a region and picking a window, as on macOS.
- **Capture feedback**: the captured area flashes and a shutter sound plays.
- **Floating thumbnail**: copy, mark up (Satty or swappy), show in folder, or delete. Drag it
  straight into other apps.
- **Screen recording**: GPU-encoded H.264 by default, microphone and/or system audio, and files
  that play everywhere (WhatsApp, Slack, Discord, browsers). Stop it from your status bar, with a
  shortcut, or from the floating pill.

| Window picking | Options |
| --- | --- |
| ![Window mode highlights the window under the pointer](docs/window.png) | ![Options menu in recording mode](docs/options.png) |

![Floating thumbnail after a capture](docs/thumbnail.png)

## Install

### Arch Linux (AUR)

```sh
paru -S pinhole        # or pinhole-git for the latest commit
```

### Cargo

pinhole needs the GTK 4 and gtk4-layer-shell development files, plus Rust 1.87 or newer.

```sh
cargo install pinhole
```

### From source

```sh
git clone https://github.com/chrischriscris/pinhole
cd pinhole
cargo build --release
install -Dm755 target/release/pinhole ~/.local/bin/pinhole
```

### Runtime dependencies

| Needed for | Packages (Arch names) |
| --- | --- |
| Everything | `gtk4`, `gtk4-layer-shell`, `grim`, `wl-clipboard` |
| Recording | `wl-screenrec` (GPU encoding, needs a VA-API driver such as `intel-media-driver`) or `wf-recorder` |
| Share-friendly MP4s, video previews | `ffmpeg` |
| Mark up | `satty` or `swappy` |
| Microphone list, mixing mic + system audio | `libpulse` (`pactl`, works with PipeWire) |
| Shutter sound | `pipewire` (`pw-play`) and `sound-theme-freedesktop` |
| Error notifications | `libnotify` |

## Usage

```
pinhole                 open the toolbar (press again to stop a recording)
pinhole screen          capture the focused screen immediately
pinhole region          drag to capture; Space switches to window mode
pinhole window          click a window to capture it
pinhole record-region   toolbar in region-recording mode
pinhole record-screen   toolbar in screen-recording mode
pinhole stop            stop recording
```

In the overlay you can:

- Drag to select, then drag the handles to resize or drag inside to move.
- Nudge the selection with the arrow keys (Shift moves 10 px, Alt resizes instead).
- Press Space to switch between selecting a region and picking a window.
- Press Enter or double-click to capture.
- Press Esc or right-click to cancel.

Screenshots go to `$XDG_PICTURES_DIR/Screenshots` and the clipboard; recordings go to
`$XDG_VIDEOS_DIR/Screen Recordings`. Both are configurable.

### Hyprland

```ini
bind = , Print, exec, pinhole region                # like Cmd+Shift+4 (Space: window mode)
bind = CONTROL, Print, exec, pinhole                # like Cmd+Shift+5 (toolbar)
bind = SUPER, Print, exec, pinhole screen           # like Cmd+Shift+3
bind = SUPER CONTROL, Escape, exec, pinhole stop    # like Cmd+Ctrl+Esc (stop recording)

# Optional: no fade on the frozen overlay and the flash, blur behind the floating UI.
layerrule {
    name = pinhole-overlay
    match:namespace = ^(pinhole|pinhole-flash)$
    no_anim = on
}

layerrule {
    name = pinhole-chrome
    match:namespace = ^(pinhole-(thumbnail|indicator|countdown))$
    blur = on
    ignore_alpha = 0.3
}
```

The `layerrule` blocks use the block syntax of recent Hyprland releases (checked with 0.56).

### sway

```
bindsym Print exec pinhole region
bindsym Ctrl+Print exec pinhole
bindsym Mod4+Print exec pinhole screen
bindsym Mod4+Ctrl+Escape exec pinhole stop
```

## Recording

The Options menu changes with the mode, as on macOS. For recordings it offers a microphone,
**Include System Audio**, **Show Mouse Pointer**, and the quality:

- **Standard (Most compatible)**: H.264 at a bitrate that keeps text sharp and files small.
- **High Quality (Larger files)**: 2.5× the bitrate, for fast motion and smooth gradients.

After a recording stops, pinhole moves the MP4 index to the front and corrects the H.264 level
(without re-encoding), so phones and chat apps play it reliably.

`wl-screenrec` encodes on the GPU and is used automatically when a VA-API driver is installed;
otherwise pinhole falls back to `wf-recorder` on the CPU. Recording the microphone and system
audio together uses a temporary PipeWire/PulseAudio mix sink that is removed afterwards.

### Stopping from a status bar

While recording, pinhole writes `$XDG_RUNTIME_DIR/pinhole/status.json`:

```json
{"recording": true, "started": 1793036189, "path": "/home/you/Videos/Screen Recordings/…"}
```

A bar can show a stop button that runs `pinhole stop`, like the one in the macOS menu bar. Once
it does, set `recording_pill = false` to hide pinhole's floating pill.

- **Quickshell**: copy [`extras/quickshell`](extras/quickshell) into your config and add
  `ScreenRecording {}` to your bar.
- **Waybar**: a custom module, for example:

  ```json
  "custom/pinhole": {
      "exec": "jq -r 'if .recording then \"⏹\" else \"\" end' $XDG_RUNTIME_DIR/pinhole/status.json 2>/dev/null",
      "interval": 1,
      "on-click": "pinhole stop",
      "tooltip": false
  }
  ```

## Configuration

`~/.config/pinhole/config.toml`; every key is optional.

```toml
screenshot_dir = "~/Pictures/Screenshots"         # default: $XDG_PICTURES_DIR/Screenshots
recording_dir = "~/Videos/Screen Recordings"      # default: $XDG_VIDEOS_DIR/Screen Recordings
screenshot_name = "Screenshot %Y-%m-%d at %H.%M.%S"
recording_name = "Screen Recording %Y-%m-%d at %H.%M.%S"
editor = "auto"            # satty, then swappy, then your image viewer; or a command with {}
recorder = "auto"          # "wl-screenrec" or "wf-recorder"
thumbnail_seconds = 6      # 0 keeps the thumbnail until dismissed
magnifier = false          # pixel magnifier next to the pointer while selecting
flash = true               # flash the captured area
sound = true               # play the camera shutter sound

recording_codec = "h264"   # "h264" (plays everywhere), "hevc" (smaller), "vp9" or "av1"
recording_format = "mp4"   # "mp4", "mkv" (survives crashes) or "webm" (VP9/AV1 only)
recording_fps = 0          # cap the frame rate, e.g. 30; 0 = up to the display's refresh rate
recording_pill = true      # floating timer and stop button while recording
```

H.264 and HEVC are encoded on the GPU. VP9 and AV1 are encoded on the CPU through
`wf-recorder`, since few GPUs can encode them. Combinations that can't work, such as WebM with
H.264, are corrected with a notification.

The toolbar's options persist in `~/.local/state/pinhole/options.json`.

## Notes

- pinhole is tested on Hyprland and sway. Other compositors with `wlr-layer-shell` and screen
  capture support (river, Wayfire, …) should work for screenshots and recording, but window
  picking needs Hyprland or sway.
- GTK themes can't restyle pinhole; it always uses its own dark look.
- Wayland doesn't let apps see clicks outside their own windows, so there is no "Show Mouse
  Clicks" option.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
