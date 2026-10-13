//! Ours (self-logging, not upstream): what `window::restore` decided, in one line. A child module
//! of `window` (declared there with one line), so it reads the monitors as `place` saw them
//! without upstream's fields being made public.

use winit::monitor::MonitorHandle;

use super::{DEFAULT_SIZE, Monitor, SCREEN_ROOM, WindowGeometry};

/// After `place`: each monitor as reported (position, size, scale), the usable area `place`
/// computed on the home one, the size asked, maximized or not, and the saved geometry used or
/// not. `decision: window: …` when the window was fitted, clamped, moved or maximized; else info.
pub(super) fn placed(handles: &[MonitorHandle], monitors: &[Monitor], home: Option<usize>, saved: Option<WindowGeometry>, g: &WindowGeometry) {
    let mut screens = Vec::new();
    for (n, (h, m)) in handles.iter().zip(monitors).enumerate() {
        let usable = m.usable();
        screens.push(format!(
            "screen {n} \"{}\" at ({}, {}), {}x{} pixels, scale {}{} (usable {:.0}x{:.0} points: its size less {}x{} for panels)",
            h.name().unwrap_or_else(|| "unnamed".into()),
            m.pos[0],
            m.pos[1],
            m.size[0],
            m.size[1],
            m.scale,
            if home == Some(n) { ", home" } else { "" },
            usable[0],
            usable[1],
            SCREEN_ROOM[0],
            SCREEN_ROOM[1],
        ));
    }
    if screens.is_empty() {
        screens.push("no screens reported by the window system".to_owned());
    }
    if home.is_none() && !monitors.is_empty() {
        screens.push("the window system named no home screen (the first was used)".to_owned());
    }
    let pos = g.pos.map_or_else(|| "no position (the window system places it)".to_owned(), |[x, y]| format!("at ({x}, {y})"));
    let asked = format!("asked {:.0}x{:.0} points, {pos}, maximized {}", g.size[0], g.size[1], g.maximized);
    let (from, changed) = match saved {
        None => {
            let fitted = g.maximized || g.size != DEFAULT_SIZE;
            (format!("no saved geometry: the default {:.0}x{:.0}", DEFAULT_SIZE[0], DEFAULT_SIZE[1]), fitted.then_some("the default size does not fit: maximized"))
        }
        Some(s) => {
            let was = format!("saved geometry used ({:.0}x{:.0}, {}, maximized {})", s.size[0], s.size[1], s.pos.map_or_else(|| "no position".to_owned(), |[x, y]| format!("at ({x}, {y})")), s.maximized);
            let why = if g.size != s.size {
                Some("fitted to its screen")
            } else if g.maximized && !s.maximized {
                Some("too big for its screen: maximized")
            } else if g.pos != s.pos {
                Some("moved onto a connected screen")
            } else {
                None
            };
            (was, why)
        }
    };
    let text = format!("window: {}; {from}; {asked}", screens.join("; "));
    match changed {
        Some(why) => log::warn!("decision: {text}: {why}"),
        None => log::info!("{text}"),
    }
}
