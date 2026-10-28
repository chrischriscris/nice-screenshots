# Setting up Nice Screenshots: instructions for AI agents

You are helping a person install and configure Nice Screenshots (the command is
`nice-screenshots`), a macOS-style screenshot and screen recording tool for Hyprland and sway.
Work through the steps below in order. The person may not know Linux well: explain what you are
about to do in plain words, and keep questions short, with a recommended default they can
accept.

Ground rules:

- Ask before running anything with `sudo`, and before changing any config file. Make a copy of
  a config file (`<file>.bak`) before you edit it for the first time.
- Configs are often symlinks into a dotfiles repository. Edit the real file the symlink points
  to, and don't commit or push anything unless the person asks.
- Never run `nice-screenshots` without a command while the person is busy: it freezes the
  screen until they press Esc. Ask them to try the shortcuts themselves at the end.
- If a step fails, show the error and explain it. Don't keep retrying the same command.

## 1. Check that this desktop is supported

Run `echo $XDG_SESSION_TYPE $XDG_CURRENT_DESKTOP` and check `$HYPRLAND_INSTANCE_SIGNATURE` and
`$SWAYSOCK`.

- **Hyprland** or **sway** on Wayland: fully supported. Continue.
- **river, Wayfire, labwc, niri** and other wlroots-style compositors: screenshots and recording
  should work, but picking a window doesn't. Tell the person and ask whether to continue.
- **GNOME**, **KDE Plasma** or **X11**: not supported. Stop here and explain that the desktop
  doesn't let apps draw the capture overlay or capture the screen directly. On GNOME, its
  built-in screenshot tool (the Print key) already works much like macOS.

## 2. Install nice-screenshots

**Arch Linux and derivatives** (`/etc/arch-release` exists): use the AUR helper the person has
(`paru` or `yay`):

```sh
paru -S nice-screenshots
```

If the package isn't found, build from source as below. Without an AUR helper, ask whether to
build from source or install one.

**Other distributions:** build from source. It needs Rust 1.87 or newer (`rustup` is the easiest
way to get it) and the GTK 4 and gtk4-layer-shell development packages. Package names vary, for
example `libgtk-4-dev libgtk4-layer-shell-dev` on Debian and Ubuntu, `gtk4-devel
gtk4-layer-shell-devel` on Fedora. Look up the right names for the distribution rather than
guessing.

```sh
cargo install --locked --git https://github.com/chrischriscris/nice-screenshots
```

This installs to `~/.cargo/bin/nice-screenshots`. Remember the full path to the binary; you will
use it in the keybinds, because compositors often don't have `~/.cargo/bin` or `~/.local/bin` in
their `PATH`.

Then install the runtime tools (Arch names; find the equivalents elsewhere):

| Needed for | Packages |
| --- | --- |
| Everything | `grim`, `wl-clipboard` |
| Recording | `wl-screenrec` (GPU, preferred) or `wf-recorder` (CPU) |
| Share-friendly MP4s and previews | `ffmpeg` |
| Mark up | `satty` or `swappy` |
| Microphone and system audio | `libpulse` (provides `pactl`, works with PipeWire) |
| Shutter sound | `pipewire` and `sound-theme-freedesktop` |
| Error notifications | `libnotify` |

For GPU recording, check `vainfo` (from `libva-utils`). If it reports no working driver, install
the VA-API driver for the GPU: `intel-media-driver` for Intel from Broadwell on, Mesa for AMD.
If GPU encoding can't be made to work, `wf-recorder` is fine.

Confirm with `nice-screenshots --version`.

## 3. Ask about keyboard shortcuts

Suggest these macOS-like defaults:

| Shortcut | Command | Like on macOS |
| --- | --- | --- |
| Print | `nice-screenshots region` | Cmd+Shift+4 (Space switches to window mode) |
| Ctrl+Print | `nice-screenshots` | Cmd+Shift+5 (the toolbar) |
| Super+Print | `nice-screenshots screen` | Cmd+Shift+3 |
| Super+Ctrl+Escape | `nice-screenshots stop` | Cmd+Ctrl+Esc (stop recording) |

Before asking, check for conflicts: on Hyprland run `hyprctl binds`, on sway search the config
for `bindsym`. If Print is already bound (to grim, hyprshot, flameshot and so on), say so and ask
whether to replace that binding. Laptops without a Print key may prefer something like
Super+Shift+4. Use whatever the person picks.

Other commands they can bind: `window`, `record-region`, `record-screen`.

## 4. Add the shortcuts

**Hyprland:** find the config with `hyprctl -j instances` or `~/.config/hypr/hyprland.conf`
(it may `source` other files; put the binds next to the existing ones). Check what `$mod` is set
to before using it. Use the full path to the binary:

```ini
bind = , Print, exec, /full/path/to/nice-screenshots region
bind = CONTROL, Print, exec, /full/path/to/nice-screenshots
bind = SUPER, Print, exec, /full/path/to/nice-screenshots screen
bind = SUPER CONTROL, Escape, exec, /full/path/to/nice-screenshots stop
```

Also add these layer rules, so the frozen overlay appears without a fade and the floating UI gets
a blurred background:

```ini
layerrule {
    name = nice-screenshots-overlay
    match:namespace = ^(nice-screenshots|nice-screenshots-flash)$
    no_anim = on
}

layerrule {
    name = nice-screenshots-chrome
    match:namespace = ^(nice-screenshots-(thumbnail|indicator|countdown))$
    blur = on
    ignore_alpha = 0.3
}
```

This block syntax needs a recent Hyprland (it was checked with 0.56). Run `hyprctl reload`, then
`hyprctl configerrors`. If it reports errors about the layer rules, the Hyprland is older: rewrite
them in the one-line `layerrule = ...` syntax its version documents, or leave them out. They are
optional.

**sway:** add to `~/.config/sway/config` and run `swaymsg reload`:

```
bindsym Print exec /full/path/to/nice-screenshots region
bindsym Ctrl+Print exec /full/path/to/nice-screenshots
bindsym Mod4+Print exec /full/path/to/nice-screenshots screen
bindsym Mod4+Ctrl+Escape exec /full/path/to/nice-screenshots stop
```

## 5. Ask about preferences

Ask a few short questions. Only write `~/.config/nice-screenshots/config.toml` if something
differs from the defaults; every key is optional.

- Where to save screenshots and recordings? Defaults: `~/Pictures/Screenshots` and
  `~/Videos/Screen Recordings` (or the person's XDG folders).
  Keys: `screenshot_dir`, `recording_dir`.
- Shutter sound and flash? Default on. Keys: `sound`, `flash`.
- Pixel magnifier while selecting, for pixel-exact picks? Default off. Key: `magnifier`.
- How long the thumbnail stays? Default 6 seconds, 0 keeps it until dismissed.
  Key: `thumbnail_seconds`.

Recording quality, microphone, system audio and the mouse pointer are set from the toolbar's
Options menu, not the config file. Tell the person where to find them.

The full list of keys is in the "Configuration" section of the README:
https://raw.githubusercontent.com/chrischriscris/nice-screenshots/main/README.md

## 6. Offer a stop button in the status bar

While recording, a small floating pill shows the time and a stop button. If the person uses a
bar, offer a stop button there instead, like the one in the macOS menu bar:

- **Waybar:** add this module, put `"custom/nice-screenshots"` in one of the bar's module
  lists, then restart Waybar:

  ```json
  "custom/nice-screenshots": {
      "exec": "jq -r 'if .recording then \"⏹\" else \"\" end' $XDG_RUNTIME_DIR/nice-screenshots/status.json 2>/dev/null",
      "interval": 1,
      "on-click": "nice-screenshots stop",
      "tooltip": false
  }
  ```

  It needs `jq`.
- **Quickshell:** copy `extras/quickshell` from the repository into their config and add
  `ScreenRecording {}` to the bar. Match their bar's existing icon size and spacing.
- Other bars: they can read `$XDG_RUNTIME_DIR/nice-screenshots/status.json` and run
  `nice-screenshots stop` on click.

If you add a bar button, set `recording_pill = false` in the config.

## 7. Finish

Ask the person to try it:

1. Press the region shortcut, drag a selection and release. A thumbnail appears in the corner
   and the image is on the clipboard.
2. Press it again and tap Space to switch to picking a window.
3. Open the toolbar, pick a recording mode, record a few seconds and stop it.

If nothing happens, run the same command in a terminal and read its output. Then summarize what
you installed and which files you changed.
