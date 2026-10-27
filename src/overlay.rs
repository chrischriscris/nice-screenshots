//! The full-screen selection overlay: a frozen frame of every output, the macOS-style
//! toolbar, and region/window/screen picking.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::cairo::{self, Context};
use gtk::gdk_pixbuf::Pixbuf;
use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, LayerShell};

use crate::config::{Mode, Options, Quality};
use crate::desktop::{self, Desktop};
use crate::geometry::Rect;
use crate::icons::{self, Icon};
use crate::{capture, record, ui};

/// What the user picked; handed back to the app when the overlay finishes.
pub struct Selection {
    pub mode: Mode,
    /// Global logical coordinates.
    pub rect: Rect,
    pub output: String,
    pub output_rect: Rect,
    pub frozen: Pixbuf,
    pub from_toolbar: bool,
}

struct Output {
    name: String,
    rect: Rect,
    frozen: Pixbuf,
    surface: cairo::ImageSurface,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    fn cursor(self) -> &'static str {
        match self {
            Handle::N | Handle::S => "ns-resize",
            Handle::E | Handle::W => "ew-resize",
            Handle::NW | Handle::SE => "nwse-resize",
            Handle::NE | Handle::SW => "nesw-resize",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    Creating { ax: f64, ay: f64 },
    Moving { orig: Rect },
    Resizing { handle: Handle, orig: Rect },
}

struct State {
    mode: Mode,
    outputs: Vec<Output>,
    windows: Vec<desktop::Window>,
    selection: Option<Rect>,
    selection_output: usize,
    drag: Drag,
    drag_start: (f64, f64),
    pointer: Option<(f64, f64)>,
    hover_output: usize,
    /// Show the pixel magnifier next to the pointer while selecting.
    magnifier: bool,
    done: bool,
}

impl State {
    fn output_at(&self, x: f64, y: f64) -> Option<usize> {
        self.outputs.iter().position(|o| o.rect.contains(x, y))
    }

    /// The topmost window under the point, clipped to the output it is on.
    fn window_at(&self, x: f64, y: f64) -> Option<(usize, Rect)> {
        let out = self.output_at(x, y)?;
        let win = self.windows.iter().position(|w| w.rect.contains(x, y))?;
        let rect = self.windows[win].rect.intersect(&self.outputs[out].rect)?;
        Some((win, rect))
    }
}

struct Ctx {
    state: RefCell<State>,
    toolbar: bool,
    opts: Rc<RefCell<Options>>,
    areas: RefCell<Vec<gtk::DrawingArea>>,
    mode_buttons: RefCell<Vec<(Mode, gtk::ToggleButton)>>,
    capture_button: RefCell<Option<gtk::Button>>,
    on_done: Box<dyn Fn(Option<Selection>)>,
}

pub struct Overlay {
    windows: Vec<gtk::Window>,
}

impl Overlay {
    pub fn close(&self) {
        for win in &self.windows {
            win.close();
        }
    }

    pub fn open(
        app: &gtk::Application,
        mode: Mode,
        toolbar: bool,
        magnifier: bool,
        opts: Rc<RefCell<Options>>,
        on_done: impl Fn(Option<Selection>) + 'static,
    ) -> Result<Overlay, String> {
        let display = gdk::Display::default().ok_or("no display")?;
        let desktop = Desktop::query();
        let cursor = opts.borrow().show_pointer;

        // Freeze every output before anything of ours is on screen.
        let mut outputs = Vec::new();
        let mut monitors = Vec::new();
        let model = display.monitors();
        for i in 0..model.n_items() {
            let Some(monitor) = model.item(i).and_downcast::<gdk::Monitor>() else {
                continue;
            };
            let name = monitor
                .connector()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let g = monitor.geometry();
            let rect = Rect::new(
                g.x() as f64,
                g.y() as f64,
                g.width() as f64,
                g.height() as f64,
            );
            let frozen = capture::grab_output(&name, cursor)?;
            let surface = capture::to_surface(&frozen)?;
            outputs.push(Output {
                name,
                rect,
                frozen,
                surface,
            });
            monitors.push(monitor);
        }
        if outputs.is_empty() {
            return Err("no outputs found".into());
        }

        let focused = desktop
            .focused_output
            .and_then(|name| outputs.iter().position(|o| o.name == name))
            .unwrap_or(0);
        let windows = desktop.windows;

        let (selection, selection_output) = {
            let o = opts.borrow();
            let saved = o
                .last_region
                .as_ref()
                .filter(|_| toolbar && o.remember_selection);
            saved
                .and_then(|s| {
                    let idx = outputs.iter().position(|out| out.name == s.output)?;
                    let r = s.rect.translate(outputs[idx].rect.x, outputs[idx].rect.y);
                    Some((Some(r.clamp_inside(&outputs[idx].rect)), idx))
                })
                .unwrap_or((None, focused))
        };

        let ctx = Rc::new(Ctx {
            state: RefCell::new(State {
                mode,
                outputs,
                windows,
                selection,
                selection_output,
                drag: Drag::None,
                drag_start: (0.0, 0.0),
                pointer: None,
                hover_output: focused,
                magnifier,
                done: false,
            }),
            toolbar,
            opts,
            areas: RefCell::default(),
            mode_buttons: RefCell::default(),
            capture_button: RefCell::default(),
            on_done: Box::new(on_done),
        });

        let mut windows = Vec::new();
        for (i, monitor) in monitors.iter().enumerate() {
            let win = ui::layer_window(app, "nice-screenshots");
            win.add_css_class("nice-screenshots-overlay");
            win.set_monitor(Some(monitor));
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                win.set_anchor(edge, true);
            }
            win.set_exclusive_zone(-1);
            win.set_keyboard_mode(if i == focused {
                KeyboardMode::Exclusive
            } else {
                KeyboardMode::None
            });

            let area = gtk::DrawingArea::new();
            area.set_hexpand(true);
            area.set_vexpand(true);
            area.set_cursor_from_name(Some("crosshair"));
            ctx.areas.borrow_mut().push(area.clone());
            connect_area(&ctx, &area, i);

            let stack = gtk::Overlay::new();
            stack.set_child(Some(&area));
            if toolbar && i == focused {
                stack.add_overlay(&build_toolbar(&ctx));
            }
            win.set_child(Some(&stack));
            connect_keys(&ctx, &win);
            windows.push(win);
        }
        ctx.refresh();
        for win in &windows {
            win.present();
        }
        Ok(Overlay { windows })
    }
}

impl Ctx {
    fn redraw(&self) {
        for area in self.areas.borrow().iter() {
            area.queue_draw();
        }
    }

    fn refresh(&self) {
        let st = self.state.borrow();
        if let Some(button) = self.capture_button.borrow().as_ref() {
            button.set_label(if st.mode.is_record() {
                "Record"
            } else {
                "Capture"
            });
            button.set_sensitive(match st.mode {
                Mode::Region | Mode::RecordRegion => st.selection.is_some_and(|r| r.is_usable()),
                Mode::Window => false,
                Mode::Screen | Mode::RecordScreen => true,
            });
        }
        drop(st);
        self.redraw();
    }

    fn set_mode(&self, mode: Mode) {
        {
            let mut st = self.state.borrow_mut();
            if st.mode == mode {
                return;
            }
            st.mode = mode;
            st.drag = Drag::None;
        }
        for (m, button) in self.mode_buttons.borrow().iter() {
            if *m == mode && !button.is_active() {
                button.set_active(true);
            }
        }
        self.update_cursors();
        self.refresh();
    }

    fn finish(self: &Rc<Self>, selection: Option<Selection>) {
        {
            let mut st = self.state.borrow_mut();
            if st.done {
                return;
            }
            st.done = true;
        }
        // Defer so the overlay can be torn down outside of its own event handlers.
        let ctx = self.clone();
        glib::idle_add_local_once(move || (ctx.on_done)(selection));
    }

    fn confirm(self: &Rc<Self>) {
        let st = self.state.borrow();
        let target = match st.mode {
            Mode::Region | Mode::RecordRegion => st
                .selection
                .filter(|r| r.is_usable())
                .map(|r| (st.selection_output, r)),
            Mode::Window => st.pointer.and_then(|(x, y)| {
                let (_, rect) = st.window_at(x, y)?;
                Some((st.output_at(x, y)?, rect))
            }),
            Mode::Screen | Mode::RecordScreen => {
                Some((st.hover_output, st.outputs[st.hover_output].rect))
            }
        };
        let Some((idx, rect)) = target else { return };
        let out = &st.outputs[idx];
        let selection = Selection {
            mode: st.mode,
            rect: rect.round(),
            output: out.name.clone(),
            output_rect: out.rect,
            frozen: out.frozen.clone(),
            from_toolbar: self.toolbar,
        };
        drop(st);
        self.finish(Some(selection));
    }

    fn update_cursors(&self) {
        let st = self.state.borrow();
        let name = match st.mode {
            Mode::Window | Mode::Screen | Mode::RecordScreen => "pointer",
            Mode::Region | Mode::RecordRegion => {
                let over = |r: &Rect, (x, y): (f64, f64)| {
                    hit_handle(r, x, y)
                        .map(Handle::cursor)
                        .or(r.contains(x, y).then_some("move"))
                };
                match st.drag {
                    Drag::Resizing { handle, .. } => handle.cursor(),
                    Drag::Moving { .. } => "move",
                    Drag::Creating { .. } => "crosshair",
                    Drag::None if self.toolbar => st
                        .selection
                        .zip(st.pointer)
                        .and_then(|(r, p)| over(&r, p))
                        .unwrap_or("crosshair"),
                    Drag::None => "crosshair",
                }
            }
        };
        for area in self.areas.borrow().iter() {
            area.set_cursor_from_name(Some(name));
        }
    }

    fn nudge(&self, dx: f64, dy: f64, resize: bool) {
        let mut st = self.state.borrow_mut();
        if !st.mode.is_region() {
            return;
        }
        let bounds = st.outputs[st.selection_output].rect;
        if let Some(r) = st.selection {
            st.selection = Some(if resize {
                Rect::new(r.x, r.y, (r.w + dx).max(2.0), (r.h + dy).max(2.0))
                    .intersect(&bounds)
                    .unwrap_or(r)
            } else {
                r.translate(dx, dy).clamp_inside(&bounds)
            });
        }
        drop(st);
        self.refresh();
    }
}

fn handles(r: &Rect) -> [(Handle, f64, f64); 8] {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    [
        (Handle::NW, r.x, r.y),
        (Handle::N, cx, r.y),
        (Handle::NE, r.right(), r.y),
        (Handle::E, r.right(), cy),
        (Handle::SE, r.right(), r.bottom()),
        (Handle::S, cx, r.bottom()),
        (Handle::SW, r.x, r.bottom()),
        (Handle::W, r.x, cy),
    ]
}

fn hit_handle(r: &Rect, x: f64, y: f64) -> Option<Handle> {
    handles(r)
        .into_iter()
        .find(|(_, hx, hy)| (x - hx).abs() <= 9.0 && (y - hy).abs() <= 9.0)
        .map(|(h, _, _)| h)
}

fn resize(orig: Rect, handle: Handle, dx: f64, dy: f64) -> Rect {
    let (mut l, mut t, mut r, mut b) = (orig.x, orig.y, orig.right(), orig.bottom());
    match handle {
        Handle::N => t += dy,
        Handle::S => b += dy,
        Handle::E => r += dx,
        Handle::W => l += dx,
        Handle::NE => (t, r) = (t + dy, r + dx),
        Handle::NW => (t, l) = (t + dy, l + dx),
        Handle::SE => (b, r) = (b + dy, r + dx),
        Handle::SW => (b, l) = (b + dy, l + dx),
    }
    Rect::from_points(l, t, r, b)
}

fn connect_area(ctx: &Rc<Ctx>, area: &gtk::DrawingArea, idx: usize) {
    area.set_draw_func({
        let ctx = ctx.clone();
        move |_, cr, w, h| {
            draw(
                &ctx.state.borrow(),
                ctx.toolbar,
                idx,
                cr,
                w as f64,
                h as f64,
            )
        }
    });

    let origin = move |ctx: &Ctx| {
        let r = ctx.state.borrow().outputs[idx].rect;
        (r.x, r.y)
    };

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion({
        let ctx = ctx.clone();
        move |_, x, y| {
            let (ox, oy) = origin(&ctx);
            {
                let mut st = ctx.state.borrow_mut();
                if st.drag != Drag::None {
                    return;
                }
                st.pointer = Some((x + ox, y + oy));
                st.hover_output = idx;
            }
            ctx.update_cursors();
            ctx.redraw();
        }
    });
    motion.connect_leave({
        let ctx = ctx.clone();
        move |_| {
            let mut st = ctx.state.borrow_mut();
            if st.drag == Drag::None && st.hover_output == idx {
                st.pointer = None;
            }
            drop(st);
            ctx.redraw();
        }
    });
    area.add_controller(motion);

    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.connect_drag_begin({
        let ctx = ctx.clone();
        move |_, x, y| {
            let (ox, oy) = origin(&ctx);
            let (gx, gy) = (x + ox, y + oy);
            let mut st = ctx.state.borrow_mut();
            st.drag_start = (gx, gy);
            st.pointer = Some((gx, gy));
            st.hover_output = idx;
            if !st.mode.is_region() {
                return;
            }
            let existing = st
                .selection
                .filter(|_| ctx.toolbar && st.selection_output == idx);
            st.drag = match existing {
                Some(r) => match hit_handle(&r, gx, gy) {
                    Some(handle) => Drag::Resizing { handle, orig: r },
                    None if r.contains(gx, gy) => Drag::Moving { orig: r },
                    None => Drag::Creating { ax: gx, ay: gy },
                },
                None => Drag::Creating { ax: gx, ay: gy },
            };
            if matches!(st.drag, Drag::Creating { .. }) {
                st.selection = None;
                st.selection_output = idx;
            }
            drop(st);
            ctx.update_cursors();
            ctx.refresh();
        }
    });
    drag.connect_drag_update({
        let ctx = ctx.clone();
        move |_, dx, dy| {
            let mut st = ctx.state.borrow_mut();
            let bounds = st.outputs[st.selection_output].rect;
            let (sx, sy) = st.drag_start;
            let cx = (sx + dx).clamp(bounds.x, bounds.right());
            let cy = (sy + dy).clamp(bounds.y, bounds.bottom());
            st.pointer = Some((cx, cy));
            match st.drag {
                Drag::None => {}
                Drag::Creating { ax, ay } => st.selection = Some(Rect::from_points(ax, ay, cx, cy)),
                Drag::Moving { orig } => {
                    st.selection = Some(orig.translate(dx, dy).clamp_inside(&bounds))
                }
                Drag::Resizing { handle, orig } => {
                    st.selection = Some(
                        resize(orig, handle, dx, dy)
                            .intersect(&bounds)
                            .unwrap_or(orig),
                    )
                }
            }
            drop(st);
            ctx.refresh();
        }
    });
    drag.connect_drag_end({
        let ctx = ctx.clone();
        move |_, dx, dy| {
            let clicked = dx.abs() < 3.0 && dy.abs() < 3.0;
            let mut st = ctx.state.borrow_mut();
            let was = std::mem::replace(&mut st.drag, Drag::None);
            let mode = st.mode;
            if matches!(was, Drag::Creating { .. })
                && !st.selection.is_some_and(|r| r.w >= 3.0 && r.h >= 3.0)
            {
                st.selection = None;
            }
            let has_selection = st.selection.is_some();
            drop(st);
            ctx.update_cursors();
            ctx.refresh();
            match mode {
                // Without the toolbar (the Cmd+Shift+4 flow) releasing the mouse takes the shot.
                Mode::Region | Mode::RecordRegion if !ctx.toolbar && has_selection => ctx.confirm(),
                Mode::Window | Mode::Screen | Mode::RecordScreen if clicked => ctx.confirm(),
                _ => {}
            }
        }
    });
    area.add_controller(drag);

    // Double-click inside the selection to confirm it.
    let double = gtk::GestureClick::new();
    double.connect_pressed({
        let ctx = ctx.clone();
        move |_, n, _, _| {
            if n == 2 && ctx.state.borrow().mode.is_region() {
                ctx.confirm();
            }
        }
    });
    area.add_controller(double);

    // Right click cancels, as in most selection tools.
    let secondary = gtk::GestureClick::new();
    secondary.set_button(gdk::BUTTON_SECONDARY);
    secondary.connect_pressed({
        let ctx = ctx.clone();
        move |_, _, _, _| ctx.finish(None)
    });
    area.add_controller(secondary);
}

fn connect_keys(ctx: &Rc<Ctx>, win: &gtk::Window) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let ctx = ctx.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let step = if mods.contains(gdk::ModifierType::SHIFT_MASK) {
            10.0
        } else {
            1.0
        };
        let resize = mods.contains(gdk::ModifierType::ALT_MASK);
        match key {
            gdk::Key::Escape => ctx.finish(None),
            gdk::Key::Return | gdk::Key::KP_Enter => ctx.confirm(),
            gdk::Key::space if matches!(ctx.state.borrow().mode, Mode::Region | Mode::Window) => {
                // Like macOS: Space toggles between region and window selection.
                let mode = ctx.state.borrow().mode;
                ctx.set_mode(if mode == Mode::Window {
                    Mode::Region
                } else {
                    Mode::Window
                });
            }
            gdk::Key::Left => ctx.nudge(-step, 0.0, resize),
            gdk::Key::Right => ctx.nudge(step, 0.0, resize),
            gdk::Key::Up => ctx.nudge(0.0, -step, resize),
            gdk::Key::Down => ctx.nudge(0.0, step, resize),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    win.add_controller(keys);
}

// ---------------------------------------------------------------------------
// Toolbar

fn build_toolbar(ctx: &Rc<Ctx>) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    bar.add_css_class("ns-toolbar");
    bar.set_halign(gtk::Align::Center);
    bar.set_valign(gtk::Align::End);
    bar.set_margin_bottom(56);

    let close = ui::icon_button(Icon::Close, 22, "Cancel (Esc)");
    close.connect_clicked({
        let ctx = ctx.clone();
        move |_| ctx.finish(None)
    });
    bar.append(&close);
    bar.append(&separator());

    let current = ctx.state.borrow().mode;
    let mut group: Option<gtk::ToggleButton> = None;
    let modes = [
        (Mode::Screen, Icon::Screen, "Capture Entire Screen"),
        (Mode::Window, Icon::Window, "Capture Selected Window"),
        (Mode::Region, Icon::Region, "Capture Selected Portion"),
        (
            Mode::RecordScreen,
            Icon::RecordScreen,
            "Record Entire Screen",
        ),
        (
            Mode::RecordRegion,
            Icon::RecordRegion,
            "Record Selected Portion",
        ),
    ];
    for (mode, icon, tooltip) in modes {
        if mode == Mode::RecordScreen {
            bar.append(&separator());
        }
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&icons::widget(icon, 26)));
        button.set_tooltip_text(Some(tooltip));
        button.set_focusable(false);
        button.add_css_class("ns-mode");
        button.set_group(group.as_ref());
        button.set_active(mode == current);
        button.connect_toggled({
            let ctx = ctx.clone();
            move |b| {
                if b.is_active() {
                    ctx.set_mode(mode);
                }
            }
        });
        group.get_or_insert(button.clone());
        ctx.mode_buttons.borrow_mut().push((mode, button.clone()));
        bar.append(&button);
    }
    bar.append(&separator());

    let options = gtk::MenuButton::new();
    let label = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    label.append(&gtk::Label::new(Some("Options")));
    label.append(&icons::widget(Icon::Chevron, 14));
    options.set_child(Some(&label));
    options.set_direction(gtk::ArrowType::Up);
    options.set_focusable(false);
    options.add_css_class("ns-options");
    options.set_popover(Some(&build_options(&ctx.opts, {
        let ctx = Rc::downgrade(ctx);
        move || {
            ctx.upgrade()
                .map(|c| c.state.borrow().mode)
                .unwrap_or_default()
        }
    })));
    bar.append(&options);

    let capture = gtk::Button::with_label("Capture");
    capture.add_css_class("ns-capture");
    capture.set_focusable(false);
    capture.connect_clicked({
        let ctx = ctx.clone();
        move |_| ctx.confirm()
    });
    *ctx.capture_button.borrow_mut() = Some(capture.clone());
    bar.append(&capture);
    bar
}

fn separator() -> gtk::Separator {
    let sep = gtk::Separator::new(gtk::Orientation::Vertical);
    sep.add_css_class("ns-sep");
    sep
}

/// The Options menu. Like macOS, it lists different choices for screenshots and recordings,
/// so it is rebuilt for the current mode each time it opens.
fn build_options(opts: &Rc<RefCell<Options>>, mode: impl Fn() -> Mode + 'static) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("ns-options-popover");
    popover.set_has_arrow(false);
    let opts = opts.clone();
    popover.connect_show(move |p| p.set_child(Some(&options_content(&opts, mode()))));
    popover
}

/// Applies the radio item at the given index to the options.
type SetOption = Box<dyn Fn(&mut Options, usize)>;

fn options_content(opts: &Rc<RefCell<Options>>, mode: Mode) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 2);

    let heading = |text: &str| {
        let l = gtk::Label::new(Some(text));
        l.set_xalign(0.0);
        l.add_css_class("ns-heading");
        col.append(&l);
    };
    let check = |label: &str, get: fn(&Options) -> bool, set: fn(&mut Options, bool)| {
        let b = gtk::CheckButton::with_label(label);
        b.set_active(get(&opts.borrow()));
        let opts = opts.clone();
        b.connect_toggled(move |b| {
            let mut o = opts.borrow_mut();
            set(&mut o, b.is_active());
            o.save();
        });
        col.append(&b);
    };
    let radios = |items: &[&str], selected: usize, set: SetOption| {
        let set = Rc::new(set);
        let mut first: Option<gtk::CheckButton> = None;
        for (i, label) in items.iter().enumerate() {
            let b = gtk::CheckButton::with_label(label);
            b.set_group(first.as_ref());
            b.set_active(i == selected);
            let opts = opts.clone();
            let set = set.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    let mut o = opts.borrow_mut();
                    set(&mut o, i);
                    o.save();
                }
            });
            first.get_or_insert(b.clone());
            col.append(&b);
        }
    };

    heading("Save to");
    if mode.is_record() {
        // Recordings are always written to the folder; it's listed so you know where.
        let folder = gtk::CheckButton::with_label("Screen Recordings Folder");
        folder.set_active(true);
        folder.set_sensitive(false);
        col.append(&folder);
    } else {
        check(
            "Screenshots Folder",
            |o| o.save_to_folder,
            |o, v| o.save_to_folder = v,
        );
    }
    check(
        "Clipboard",
        |o| o.copy_to_clipboard,
        |o, v| o.copy_to_clipboard = v,
    );

    heading("Timer");
    let timers = [0, 5, 10];
    let current = timers
        .iter()
        .position(|&t| t == opts.borrow().timer)
        .unwrap_or(0);
    radios(
        &["None", "5 Seconds", "10 Seconds"],
        current,
        Box::new(move |o, i| o.timer = timers[i]),
    );

    heading("Options");
    check(
        "Show Floating Thumbnail",
        |o| o.show_thumbnail,
        |o, v| o.show_thumbnail = v,
    );
    check(
        "Remember Last Selection",
        |o| o.remember_selection,
        |o, v| o.remember_selection = v,
    );
    if !mode.is_record() {
        check(
            "Show Mouse Pointer",
            |o| o.show_pointer,
            |o, v| o.show_pointer = v,
        );
        return col;
    }

    heading("Microphone");
    let mics = record::microphones();
    if mics.is_empty() {
        let none = gtk::Label::new(Some("No microphones found"));
        none.set_xalign(0.0);
        none.add_css_class("ns-empty");
        col.append(&none);
    }
    let saved = opts.borrow().microphone.clone();
    let current = mics
        .iter()
        .position(|(name, _)| Some(name) == saved.as_ref())
        .map_or(0, |i| i + 1);
    let mut labels = vec!["None"];
    labels.extend(mics.iter().map(|(_, description)| description.as_str()));
    let names: Vec<String> = mics.iter().map(|(name, _)| name.clone()).collect();
    if !names.is_empty() {
        radios(
            &labels,
            current,
            Box::new(move |o, i| o.microphone = i.checked_sub(1).map(|i| names[i].clone())),
        );
    }

    heading("Capture");
    let qualities = [Quality::Standard, Quality::High];
    let current = qualities
        .iter()
        .position(|&q| q == opts.borrow().quality)
        .unwrap_or(0);
    radios(
        &["Standard (Most compatible)", "High Quality (Larger files)"],
        current,
        Box::new(move |o, i| o.quality = qualities[i]),
    );
    check(
        "Show Mouse Pointer",
        |o| o.record_pointer,
        |o, v| o.record_pointer = v,
    );
    check(
        "Include System Audio",
        |o| o.system_audio,
        |o, v| o.system_audio = v,
    );
    col
}

// ---------------------------------------------------------------------------
// Drawing

const DIM: f64 = 0.38;
const ACCENT: (f64, f64, f64) = (0.039, 0.518, 1.0);

fn draw(st: &State, toolbar: bool, idx: usize, cr: &Context, w: f64, h: f64) {
    let out = &st.outputs[idx];
    let (ox, oy) = (out.rect.x, out.rect.y);
    let local = |r: &Rect| r.translate(-ox, -oy);

    // The frozen frame.
    let _ = cr.save();
    cr.scale(
        w / out.surface.width() as f64,
        h / out.surface.height() as f64,
    );
    let _ = cr.set_source_surface(&out.surface, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();

    let pointer_here = st
        .pointer
        .filter(|&(x, y)| out.rect.contains(x, y) || st.drag != Drag::None);

    match st.mode {
        Mode::Region | Mode::RecordRegion => {
            let sel = st
                .selection
                .filter(|_| st.selection_output == idx)
                .and_then(|r| r.intersect(&out.rect));
            dim_except(cr, w, h, sel.map(|r| local(&r)).as_ref());

            if let Some(r) = sel.map(|r| local(&r)) {
                cr.set_line_width(1.0);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
                cr.rectangle(r.x - 0.5, r.y - 0.5, r.w + 1.0, r.h + 1.0);
                let _ = cr.stroke();
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
                cr.rectangle(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0);
                let _ = cr.stroke();

                if toolbar && !matches!(st.drag, Drag::Creating { .. }) {
                    for (_, hx, hy) in handles(&r) {
                        cr.arc(hx, hy, 4.5, 0.0, 2.0 * PI);
                        cr.set_source_rgb(1.0, 1.0, 1.0);
                        let _ = cr.fill_preserve();
                        cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
                        let _ = cr.stroke();
                    }
                }

                let size = format!("{} × {}", r.w.round() as i64, r.h.round() as i64);
                match (st.drag, pointer_here) {
                    (Drag::Creating { .. } | Drag::Resizing { .. }, Some((px, py))) => {
                        label(cr, &size, px - ox + 16.0, py - oy + 16.0, (0.0, 0.0), w, h)
                    }
                    _ => {
                        let below = r.bottom() + 34.0 < h;
                        let y = if below {
                            r.bottom() + 8.0
                        } else {
                            r.bottom() - 30.0
                        };
                        label(cr, &size, r.x + r.w / 2.0, y, (0.5, 0.0), w, h);
                    }
                }
            }

            if let Some((px, py)) = pointer_here {
                let creating = matches!(st.drag, Drag::Creating { .. } | Drag::Resizing { .. });
                if sel.is_none() && st.drag == Drag::None {
                    // Guides and coordinates before a selection exists.
                    let (lx, ly) = (px - ox, py - oy);
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
                    cr.set_line_width(1.0);
                    cr.move_to(0.0, ly.floor() + 0.5);
                    cr.line_to(w, ly.floor() + 0.5);
                    cr.move_to(lx.floor() + 0.5, 0.0);
                    cr.line_to(lx.floor() + 0.5, h);
                    let _ = cr.stroke();
                    let coords = format!("{}, {}", lx.round() as i64, ly.round() as i64);
                    label(cr, &coords, lx + 16.0, ly + 16.0, (0.0, 0.0), w, h);
                }
                if st.magnifier && (creating || (sel.is_none() && st.drag == Drag::None)) {
                    magnifier(cr, out, px - ox, py - oy, w, h);
                }
            }
        }
        Mode::Window => {
            let hovered = st
                .pointer
                .and_then(|(x, y)| st.window_at(x, y))
                .and_then(|(i, r)| Some((i, r.intersect(&out.rect)?)));
            dim_except(cr, w, h, hovered.map(|(_, r)| local(&r)).as_ref());
            if let Some((i, r)) = hovered {
                let r = local(&r);
                cr.rectangle(r.x, r.y, r.w, r.h);
                cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.22);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.9);
                cr.set_line_width(2.0);
                let _ = cr.stroke();
                let win = &st.windows[i];
                let name = if win.class.is_empty() {
                    &win.title
                } else {
                    &win.class
                };
                let text = format!("{name}  ·  {} × {}", r.w as i64, r.h as i64);
                label(
                    cr,
                    &text,
                    r.x + r.w / 2.0,
                    r.y + r.h / 2.0,
                    (0.5, 0.5),
                    w,
                    h,
                );
            } else if st.pointer.is_some_and(|(x, y)| out.rect.contains(x, y)) {
                label(cr, "Click a window", w / 2.0, h / 2.0, (0.5, 0.5), w, h);
            }
        }
        Mode::Screen | Mode::RecordScreen => {
            if st.hover_output == idx {
                cr.rectangle(0.0, 0.0, w, h);
                cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.10);
                let _ = cr.fill();
                cr.rectangle(1.0, 1.0, w - 2.0, h - 2.0);
                cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.8);
                cr.set_line_width(2.0);
                let _ = cr.stroke();
                let verb = if st.mode.is_record() {
                    "record"
                } else {
                    "capture"
                };
                let text = format!("Click to {verb} {}", out.name);
                label(cr, &text, w / 2.0, h / 2.0, (0.5, 0.5), w, h);
            } else {
                dim_except(cr, w, h, None);
            }
        }
    }
}

fn dim_except(cr: &Context, w: f64, h: f64, hole: Option<&Rect>) {
    cr.set_fill_rule(cairo::FillRule::EvenOdd);
    cr.rectangle(0.0, 0.0, w, h);
    if let Some(r) = hole {
        cr.rectangle(r.x, r.y, r.w, r.h);
    }
    cr.set_source_rgba(0.0, 0.0, 0.0, DIM);
    let _ = cr.fill();
    cr.set_fill_rule(cairo::FillRule::Winding);
}

/// Draws a dark pill with white text, anchored at (x, y) by `align` and kept on screen.
fn label(cr: &Context, text: &str, x: f64, y: f64, align: (f64, f64), w: f64, h: f64) {
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(12.0);
    let Ok(ext) = cr.text_extents(text) else {
        return;
    };
    let (pw, ph) = (ext.x_advance() + 18.0, 24.0);
    let px = (x - pw * align.0).clamp(6.0, (w - pw - 6.0).max(6.0));
    let py = (y - ph * align.1).clamp(6.0, (h - ph - 6.0).max(6.0));
    icons::rounded_rect(cr, px, py, pw, ph, 7.0);
    cr.set_source_rgba(0.08, 0.08, 0.09, 0.82);
    let _ = cr.fill();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.96);
    cr.move_to(
        px + 9.0,
        py + ph / 2.0 - (ext.y_bearing() + ext.height() / 2.0),
    );
    let _ = cr.show_text(text);
}

/// A loupe showing the pixels around the pointer, for pixel-exact selections.
fn magnifier(cr: &Context, out: &Output, lx: f64, ly: f64, w: f64, h: f64) {
    const RADIUS: f64 = 56.0;
    const ZOOM: f64 = 8.0;
    let sx = out.surface.width() as f64 / w;
    let sy = out.surface.height() as f64 / h;
    let (px, py) = ((lx * sx).floor() + 0.5, (ly * sy).floor() + 0.5);

    // Sit below-right of the pointer, flipping near the edges.
    let mut cx = lx + 40.0 + RADIUS;
    let mut cy = ly + 56.0 + RADIUS;
    if cx + RADIUS > w {
        cx = lx - 40.0 - RADIUS;
    }
    if cy + RADIUS > h {
        cy = ly - 40.0 - RADIUS;
    }

    let _ = cr.save();
    cr.arc(cx, cy, RADIUS, 0.0, 2.0 * PI);
    cr.clip();
    cr.set_source_rgb(0.0, 0.0, 0.0);
    let _ = cr.paint();
    cr.translate(cx, cy);
    cr.scale(ZOOM, ZOOM);
    cr.translate(-px, -py);
    let _ = cr.set_source_surface(&out.surface, 0.0, 0.0);
    cr.source().set_filter(cairo::Filter::Nearest);
    let _ = cr.paint();
    let _ = cr.restore();

    cr.set_line_width(1.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    cr.rectangle(cx - ZOOM / 2.0, cy - ZOOM / 2.0, ZOOM, ZOOM);
    let _ = cr.stroke();
    cr.arc(cx, cy, RADIUS, 0.0, 2.0 * PI);
    cr.set_line_width(2.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    let _ = cr.stroke_preserve();
    cr.set_line_width(1.0);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.4);
    let _ = cr.stroke();
}
