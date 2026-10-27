//! Focused output and visible windows, from Hyprland (`hyprctl -j`) or sway (`swaymsg`).

use std::process::Command;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::geometry::Rect;

#[derive(Debug, Clone)]
pub struct Window {
    pub rect: Rect,
    pub title: String,
    pub class: String,
}

#[derive(Debug, Default)]
pub struct Desktop {
    pub focused_output: Option<String>,
    /// Windows on visible workspaces, topmost first.
    pub windows: Vec<Window>,
}

impl Desktop {
    pub fn query() -> Desktop {
        let result = if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            hyprland()
        } else if std::env::var_os("SWAYSOCK").is_some() {
            sway()
        } else {
            return Desktop::default();
        };
        result.unwrap_or_else(|e| {
            eprintln!("nice-screenshots: {e}");
            Desktop::default()
        })
    }
}

fn json<T: DeserializeOwned>(program: &str, args: &[&str]) -> Result<T, String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    serde_json::from_slice(&out.stdout).map_err(|e| format!("{program} {}: {e}", args.join(" ")))
}

// ---------------------------------------------------------------------------
// Hyprland

#[derive(Debug, Deserialize)]
struct WorkspaceRef {
    id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HyprMonitor {
    name: String,
    focused: bool,
    active_workspace: WorkspaceRef,
    special_workspace: WorkspaceRef,
}

#[derive(Debug, Deserialize)]
struct HyprClient {
    at: [i32; 2],
    size: [i32; 2],
    workspace: WorkspaceRef,
    mapped: bool,
    hidden: bool,
    floating: bool,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    fullscreen: serde_json::Value,
    #[serde(rename = "focusHistoryID", default)]
    focus_history_id: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    class: String,
}

impl HyprClient {
    fn is_fullscreen(&self) -> bool {
        match &self.fullscreen {
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
            _ => false,
        }
    }
}

fn hyprland() -> Result<Desktop, String> {
    let monitors: Vec<HyprMonitor> = json("hyprctl", &["-j", "monitors"])?;
    let clients: Vec<HyprClient> = json("hyprctl", &["-j", "clients"])?;
    let regular: Vec<i64> = monitors.iter().map(|m| m.active_workspace.id).collect();
    let special: Vec<i64> = monitors
        .iter()
        .map(|m| m.special_workspace.id)
        .filter(|&id| id != 0)
        .collect();

    let mut ranked: Vec<(u8, i64, Window)> = clients
        .into_iter()
        .filter(|c| c.mapped && !c.hidden)
        .filter_map(|c| {
            let ws = c.workspace.id;
            // Approximate Hyprland's stacking: special workspace, pinned, fullscreen, floating, tiled.
            let layer = if special.contains(&ws) {
                0
            } else if c.pinned {
                1
            } else if !regular.contains(&ws) {
                return None;
            } else if c.is_fullscreen() {
                2
            } else if c.floating {
                3
            } else {
                4
            };
            let recency = if c.focus_history_id < 0 {
                i64::MAX
            } else {
                c.focus_history_id
            };
            let rect = Rect::new(
                c.at[0] as f64,
                c.at[1] as f64,
                c.size[0] as f64,
                c.size[1] as f64,
            );
            Some((
                layer,
                recency,
                Window {
                    rect,
                    title: c.title,
                    class: c.class,
                },
            ))
        })
        .collect();
    ranked.sort_by_key(|(layer, recency, _)| (*layer, *recency));

    Ok(Desktop {
        focused_output: monitors.into_iter().find(|m| m.focused).map(|m| m.name),
        windows: ranked.into_iter().map(|(_, _, w)| w).collect(),
    })
}

// ---------------------------------------------------------------------------
// sway

#[derive(Debug, Deserialize)]
struct SwayOutput {
    name: String,
    focused: bool,
}

#[derive(Debug, Deserialize)]
struct SwayRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Deserialize)]
struct SwayNode {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    window_properties: Option<serde_json::Value>,
    #[serde(default)]
    visible: Option<bool>,
    rect: SwayRect,
    deco_rect: SwayRect,
    #[serde(default)]
    nodes: Vec<SwayNode>,
    #[serde(default)]
    floating_nodes: Vec<SwayNode>,
}

fn sway() -> Result<Desktop, String> {
    let outputs: Vec<SwayOutput> = json("swaymsg", &["-r", "-t", "get_outputs"])?;
    let tree: SwayNode = json("swaymsg", &["-r", "-t", "get_tree"])?;

    fn walk(node: &SwayNode, floating: bool, out: &mut Vec<(bool, Window)>) {
        let is_view = node.app_id.is_some() || node.window_properties.is_some();
        if is_view && node.visible == Some(true) {
            // Include the title bar, which sway reports separately.
            let (r, d) = (&node.rect, &node.deco_rect);
            let rect = Rect::new(
                r.x as f64,
                (r.y - d.height) as f64,
                r.width as f64,
                (r.height + d.height) as f64,
            );
            let class = node
                .app_id
                .clone()
                .or_else(|| {
                    let props = node.window_properties.as_ref()?;
                    props.get("class")?.as_str().map(String::from)
                })
                .unwrap_or_default();
            out.push((
                floating,
                Window {
                    rect,
                    title: node.name.clone().unwrap_or_default(),
                    class,
                },
            ));
        }
        for child in &node.floating_nodes {
            walk(child, true, out);
        }
        for child in &node.nodes {
            walk(child, floating, out);
        }
    }
    let mut views = Vec::new();
    walk(&tree, false, &mut views);
    // Floating windows sit above tiled ones.
    views.sort_by_key(|(floating, _)| !floating);

    Ok(Desktop {
        focused_output: outputs.into_iter().find(|o| o.focused).map(|o| o.name),
        windows: views.into_iter().map(|(_, w)| w).collect(),
    })
}
