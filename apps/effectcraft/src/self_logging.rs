//! Ours (self-logging, not upstream): the desktop app's logger, crash files, start-up report and
//! window checks, through craft-log.
//!
//! Upstream's logger (`effectcraft_engine::logging`) stays the logger that decides and writes
//! (same `Logs/effectcraft.log`, same rotation, format and size cap); craft-log wraps it to add
//! the build line, `CRAFT_LOG`, the repeat collapse, the ring of recent lines and the stall watch.

use std::sync::atomic::{AtomicBool, Ordering};

use craft_log::report::Report;
use effectcraft_engine::logging;

use crate::window::{self, WindowGeometry};

#[path = "self_logging_report.rs"]
pub mod report;

/// EffectCraft, as craft-log names it.
pub const APP: craft_log::App =
    craft_log::App { name: "effectcraft", version: env!("CARGO_PKG_VERSION"), ours: "effectcraft", default_filter: logging::DEFAULT_FILTER };

/// The frame at which the window's size is compared with the size asked for: by then the
/// compositor has configured the window (Wayland sends the size it decided after the first frame).
const SIZE_CHECK_FRAME: u64 = 3;

/// In place of `logging::install`: the same logger, wrapped, with the filter `CRAFT_LOG`,
/// `RUST_LOG` or the default gives (the build line and the filter are its first two records).
pub fn install() {
    let (spec, _) = APP.filter_spec();
    let (inner, max) = logging::unwrapped(&spec);
    // false only when another logger was installed first; nothing changes then
    let _ = craft_log::install_wrapped(&APP, inner, max);
}

/// After upstream's panic hook: a panic also leaves `Logs/crash-<UTC stamp>.txt` (the build line,
/// the panic, a backtrace, the last 200 log lines) and ends its output with `crash: <path>`.
pub fn crash_hook() {
    craft_log::crash::install_hook(APP, report::log_dir(report::settings_dir().as_deref()));
}

/// The session log is attached (or failed to be): record its path for the lines that point at it,
/// then write the diagnostic report into the log. `backends`: Window Graphics' choice this launch.
pub fn start(backends: Option<eframe::wgpu::Backends>) {
    craft_log::set_log_path(logging::session_file());
    let settings = report::settings_dir();
    let logs = report::log_dir(settings.as_deref());
    let mut r = Report::new(&APP);
    let window_graphics = match backends {
        Some(b) if b == eframe::wgpu::Backends::GL => "OpenGL (Settings ▸ Startup & Repair ▸ Window Graphics, or after a start that failed)".to_owned(),
        Some(b) => format!("{b:?}"),
        None if std::env::var_os("WGPU_BACKEND").is_some() => "WGPU_BACKEND decides".to_owned(),
        None => "eframe's default".to_owned(),
    };
    r.add("graphics", "window graphics", window_graphics);
    r.add("graphics", "adapter", "chosen when the window opens: the \"graphics:\" line after this report");
    r.add("screens", "screens and window", "known when the window opens: the \"decision: window:\" or \"window:\" lines after this report");
    report::folders(&mut r, settings.as_deref());
    r.add_switches(report::SWITCH_PREFIXES, report::SECRET_SWITCHES);
    r.add_helpers();
    report::previous_run(&mut r, &logs, &logs.join(report::PREVIOUS_LOG_FILE));
    report::logs(&mut r, &logs);
    r.log(module_path!());
}

/// The window exists (in eframe's app creator, after `window::restore`): after a few frames,
/// log the size the window system gave against the size asked for (`opened`, what `restore`
/// asked). The size comes from the winit window: egui's `inner_rect` stays empty on Wayland,
/// where a window has no position.
pub fn window_check(cc: &eframe::CreationContext<'_>, opened: Option<WindowGeometry>) {
    let asked = opened.map_or(window::DEFAULT_SIZE, |g| g.size);
    let asked_max = opened.is_some_and(|g| g.maximized);
    // held weakly: the closure lives as long as the egui context, and a strong reference to the
    // window kept it (and the program) alive after it was closed
    let window = cc.winit_window().map(std::sync::Arc::downgrade);
    if window.is_none() {
        log::info!("window: no window handle from eframe; the size it got is unknown");
    }
    let done = AtomicBool::new(false);
    cc.egui_ctx.on_end_pass(
        "self-logging-window-size",
        std::sync::Arc::new(move |ui| {
            let ctx = ui.ctx();
            if ctx.cumulative_frame_nr() < SIZE_CHECK_FRAME || done.swap(true, Ordering::Relaxed) {
                return;
            }
            // no handle, or the window is gone: nothing to say
            let Some(w) = window.as_ref().and_then(std::sync::Weak::upgrade) else { return };
            let (inner, scale) = (w.inner_size(), w.scale_factor());
            let l = inner.to_logical::<f64>(scale);
            let got = [l.width as f32, l.height as f32];
            let (maximized, full) = (w.is_maximized(), w.fullscreen().is_some());
            let ppp = ctx.input(|i| i.viewport().native_pixels_per_point);
            let monitor = match w.current_monitor() {
                Some(m) => format!("on \"{}\"", m.name().unwrap_or_else(|| "unnamed".into())),
                None => "the window system does not say which screen holds it".to_owned(),
            };
            let text = format!(
                "window: asked {}x{} points{}, got {:.0}x{:.0} points after {SIZE_CHECK_FRAME} frames ({}x{} pixels, scale {scale}, maximized {maximized}, full screen {full}, egui pixels per point {}; {monitor})",
                asked[0],
                asked[1],
                if asked_max { " maximized" } else { "" },
                got[0],
                got[1],
                inner.width,
                inner.height,
                ppp.map_or_else(|| "unknown".to_owned(), |p| p.to_string()),
            );
            let differs = (got[0] - asked[0]).abs() > 1.0 || (got[1] - asked[1]).abs() > 1.0;
            if asked_max != maximized || (!asked_max && differs) {
                log::warn!("decision: {text}: the window system fitted, clamped or (un)maximized it");
            } else {
                log::info!("{text}");
            }
        }),
    );
}
