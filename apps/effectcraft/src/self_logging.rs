//! Ours (self-logging, not upstream): the desktop app's logger, crash files, start-up report and
//! window checks, through craft-log.
//!
//! Upstream's logger (`effectcraft_engine::logging`) stays the logger that decides and writes
//! (same `Logs/effectcraft.log`, same rotation, format and size cap); craft-log wraps it to add
//! the build line, `CRAFT_LOG`, the repeat collapse, the ring of recent lines and the stall watch.

use craft_log::report::Report;
use effectcraft_engine::logging;

#[path = "self_logging_report.rs"]
pub mod report;

/// EffectCraft, as craft-log names it.
pub const APP: craft_log::App =
    craft_log::App { name: "effectcraft", version: env!("CARGO_PKG_VERSION"), ours: "effectcraft", default_filter: logging::DEFAULT_FILTER };

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
    r.add_switches(report::SWITCH_PREFIXES);
    r.add_helpers();
    report::previous_run(&mut r, &logs, &logs.join(report::PREVIOUS_LOG_FILE));
    report::logs(&mut r, &logs);
    r.log(module_path!());
}
