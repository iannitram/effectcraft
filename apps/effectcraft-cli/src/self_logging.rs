//! Ours (self-logging, not upstream): effectcraft-cli's log, through craft-log.
//!
//! Every run appends to `<settings folder>/Logs/effectcraft-cli.log` (`EFFECTCRAFT_CONFIG_DIR`
//! moves it, as it moves the window app's `effectcraft.log` beside it); a failing run ends with
//! `log: <path>` on standard error, and a panic leaves `Logs/crash-<UTC stamp>.txt` and ends with
//! `crash: <path>`. Standard output is unchanged.

/// EffectCraft, as craft-log names it.
pub const APP: craft_log::App = craft_log::App {
    name: "effectcraft",
    version: env!("CARGO_PKG_VERSION"),
    ours: "effectcraft",
    default_filter: effectcraft_engine::logging::DEFAULT_FILTER,
};

/// The `Logs` folder: the window app's own (`logging::log_dir`), in the settings folder.
fn log_dir() -> std::path::PathBuf {
    effectcraft_engine::logging::log_dir(effectcraft_host::config_dir().as_deref())
}

/// Start this run's log and the crash file hook (after `--help` and `--version`, which leave no
/// trace).
pub fn install() {
    let dir = log_dir();
    let _ = craft_log::cli::install(&APP, Some(&dir));
    craft_log::crash::install_hook(APP, dir);
}

/// The run is about to exit with an error: log why, and point at the log.
pub fn failed(reason: &str) {
    craft_log::cli::failed(&APP, reason);
}
