//! Ours (self-logging, not upstream): EffectCraft's own fields of the diagnostic report, shared by
//! the window app and `effectcraft-cli` (which includes this file by path, so the two programs say
//! the same things the same way). craft-log gathers the fields every app shares; this adds the
//! folders, how the previous window run ended, and where the logs are.

use std::path::{Path, PathBuf};

use craft_log::crash::is_crash_file;
use craft_log::report::{Report, created, last_line_with, mask_home, modified, newest_file};

/// The window app's log file and its previous run's, after a start rotated it (upstream's names,
/// in `logging::log_dir`).
pub const LOG_FILE: &str = effectcraft_engine::logging::SESSION_LOG;
pub const PREVIOUS_LOG_FILE: &str = "effectcraft.1.log";
/// The command-line programs' shared log (craft-log's name).
pub const CLI_LOG_FILE: &str = "effectcraft-cli.log";
/// Environment switches EffectCraft reads besides craft-log's own: its own, wgpu's and winit's.
pub const SWITCH_PREFIXES: &[&str] = &["EFFECTCRAFT_", "WGPU_", "WINIT_"];
/// Upstream's last line of a window run that ended normally (`main.rs`); the next start looks for it.
pub const END_MARK: &str = "the window closed";
/// How much of the end of the previous log is searched for [`END_MARK`].
const TAIL_BYTES: u64 = 64 * 1024;

/// The settings folder (`EFFECTCRAFT_CONFIG_DIR` or the platform's).
pub fn settings_dir() -> Option<PathBuf> {
    effectcraft_host::config_dir()
}

/// The `Logs` folder (upstream's `logging::log_dir`: the temporary folder without settings).
pub fn log_dir(settings: Option<&Path>) -> PathBuf {
    effectcraft_engine::logging::log_dir(settings)
}

fn show(p: &Path) -> String {
    mask_home(&p.display().to_string())
}

/// Folders: settings (with whether the preferences and saved window exist), plug-ins.
pub fn folders(r: &mut Report, settings: Option<&Path>) {
    let Some(dir) = settings else {
        r.add("folders", "settings", "none (no settings folder: HOME and EFFECTCRAFT_CONFIG_DIR unset)");
        return;
    };
    let exists = |name: &str| if dir.join(name).is_file() { "present" } else { "not written yet" };
    r.add(
        "folders",
        "settings",
        format!("{} (preferences {}, saved window {})", show(dir), exists(effectcraft_engine::prefs::PREFS_FILE), exists("window.json")),
    );
    let plugins = dir.join("Plug-ins");
    r.add("folders", "plug-ins", if plugins.is_dir() { show(&plugins) } else { format!("{} (none)", show(&plugins)) });
}

/// How the previous window run ended, from its log (`previous_log`) and the crash files.
pub fn previous_run(r: &mut Report, logs: &Path, previous_log: &Path) {
    let ended = if !previous_log.is_file() {
        "no earlier window log".to_owned()
    } else if last_line_with(previous_log, END_MARK, TAIL_BYTES).is_some() {
        format!("yes (\"{END_MARK}\" in {})", show(previous_log))
    } else {
        format!("no \"{END_MARK}\" line in {} (killed, crashed, failed to start, or still running)", show(previous_log))
    };
    r.add("previous run", "ended cleanly", ended);
    let crash = newest_file(logs, &is_crash_file);
    let during = match (created(previous_log), crash.as_deref().and_then(modified)) {
        (_, None) => "no crash file".to_owned(),
        (Some(start), Some(at)) if at >= start => format!("yes: {}", crash.as_deref().map(show).unwrap_or_default()),
        (Some(_), Some(_)) => "no (the newest crash file is older)".to_owned(),
        (None, Some(_)) => format!("a crash file exists: {}", crash.as_deref().map(show).unwrap_or_default()),
    };
    r.add("previous run", "crash file written during it", during);
}

/// The logs, last: the folder, the window log, the command-line log, the newest crash file.
pub fn logs(r: &mut Report, logs: &Path) {
    let existing = |name: &str| Some(logs.join(name)).filter(|p| p.is_file());
    let window = existing(LOG_FILE);
    let cli = existing(CLI_LOG_FILE);
    let crash = newest_file(logs, &is_crash_file);
    r.add_logs(Some(logs), &[("window log", window.as_deref()), ("command-line log", cli.as_deref())], crash.as_deref());
}
