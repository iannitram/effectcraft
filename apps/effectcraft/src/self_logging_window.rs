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
    let names: Vec<Option<String>> = handles.iter().map(MonitorHandle::name).collect();
    let keys: Vec<_> = names.iter().zip(monitors).map(|(name, m)| (name.clone(), m.pos, m.size)).collect();
    for (n, i) in unique(&keys).into_iter().enumerate() {
        let (h, m) = (&handles[i], &monitors[i]);
        let usable = m.usable();
        screens.push(format!(
            "screen {n} \"{}\" at ({}, {}), {}x{} pixels, scale {}{} (usable {:.0}x{:.0} points: its size less {}x{} for panels)",
            h.name().unwrap_or_else(|| "unnamed".into()),
            m.pos[0],
            m.pos[1],
            m.size[0],
            m.size[1],
            m.scale,
            if home.is_some_and(|h| keys.get(h) == Some(&keys[i])) { ", home" } else { "" },
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
            (
                format!("no saved geometry: the default {:.0}x{:.0}", DEFAULT_SIZE[0], DEFAULT_SIZE[1]),
                fitted.then_some("the default size does not fit: maximized"),
            )
        }
        Some(s) => {
            let was = format!(
                "saved geometry used ({:.0}x{:.0}, {}, maximized {})",
                s.size[0],
                s.size[1],
                s.pos.map_or_else(|| "no position".to_owned(), |[x, y]| format!("at ({x}, {y})")),
                s.maximized
            );
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

/// The first of each screen in `keys` (name, position, size): the window system may hand out one
/// screen twice (seen on the Z13 under KDE Wayland: "eDP-1" as screen 0 and again as screen 1).
fn unique<K: PartialEq>(keys: &[K]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        if !out.iter().any(|&j| keys[j] == *k) {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::unique;

    #[test]
    fn a_screen_handed_out_twice_is_listed_once() {
        let edp = (Some("eDP-1".to_owned()), [0, 0], [1920, 1200]);
        let hdmi = (Some("HDMI-A-1".to_owned()), [1920, 0], [2560, 1440]);
        assert_eq!(unique(&[edp.clone(), edp.clone()]), [0]);
        assert_eq!(unique(&[edp.clone(), hdmi.clone(), edp.clone()]), [0, 1]);
        // two screens of one model at different places are two screens
        let edp_moved = (Some("eDP-1".to_owned()), [1920, 0], [1920, 1200]);
        assert_eq!(unique(&[edp, edp_moved]), [0, 1]);
        assert_eq!(unique::<u8>(&[]), Vec::<usize>::new());
    }
}
