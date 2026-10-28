# AGENTS.md

Notes for AI coding agents working on this repository. To help someone install and configure
nice-screenshots instead, follow [docs/agent-setup.md](docs/agent-setup.md).

## Project

A single Rust binary (GTK 4 + gtk4-layer-shell) for Hyprland and sway. Every invocation is
forwarded to one running instance through `gtk::Application` (see `src/app.rs`).

- `src/app.rs`: command dispatch, capture and recording flow.
- `src/overlay.rs`: frozen-screen selection, window picking and the toolbar.
- `src/record.rs`: recorder backends (wl-screenrec, wf-recorder), audio mixing, MP4 finalizing,
  `status.json`.
- `src/capture.rs`, `src/desktop.rs`: grim capture and compositor IPC (Hyprland, sway).
- `src/thumbnail.rs`, `src/indicator.rs`, `src/feedback.rs`: floating thumbnail, recording pill
  and countdown, capture flash and sound.
- `src/config.rs`: `config.toml` and the persisted toolbar options.
- `src/style.css`: all styling.

## Checks

CI runs these; run them before you finish:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Conventions

- CSS classes are prefixed `ns-`, and the stylesheet loads at `STYLE_PROVIDER_PRIORITY_USER + 1`.
  User GTK themes restyle generic classes like `.card` or `.toolbar`, so don't rely on them.
- Layer-shell namespaces (`nice-screenshots`, `nice-screenshots-flash`,
  `nice-screenshots-thumbnail`, `nice-screenshots-indicator`, `nice-screenshots-countdown`) are
  public: people match them in compositor rules. Changing them is a breaking change.
- The same goes for the commands, the config keys and the `status.json` format. Document changes
  in the README and `CHANGELOG.md`.
- Never change the person's default audio sink; the recording mix sink is temporary.

## Testing the UI

The overlay freezes the screen and grabs the keyboard, so don't open it on someone's live
desktop without asking. Test in a nested headless sway instead:

```sh
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c <test config> &
```

Then run the binary with that session's `WAYLAND_DISPLAY` and `SWAYSOCK`, separate
`XDG_CONFIG_HOME` and `XDG_STATE_HOME` directories, and capture the result with `grim`. Copy the
real `~/.config/gtk-4.0` into the test config directory so theme clashes show up.
