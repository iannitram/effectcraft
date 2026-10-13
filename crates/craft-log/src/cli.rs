//! The command-line programs' logger: one shared file per app, `<dir>/<app>-cli.log`, appended to
//! by every run (one check starts hundreds of short runs), with the process id in every line; a
//! 16 MiB cap with one earlier file kept. Standard error stays as quiet as it was unless
//! `CRAFT_LOG_STDERR=1`, so a command's output does not change.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

use log::LevelFilter;

use crate::App;
use crate::collapse::{self, Collapser};
use crate::filter::{self, Filter};
use crate::line::{Line, timestamp};

/// The command-line log stops growing past this size; at the next run it becomes `<app>-cli.1.log`.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// `<app>-cli.log`
pub fn file_name(app: &str) -> String {
    format!("{app}-cli.log")
}

/// Open `<dir>/<app>-cli.log` for appending, first moving a full one to `<app>-cli.1.log`
/// (replacing the older one). Returns the path, the file and the bytes already in it.
pub fn open(dir: &Path, app: &str) -> std::io::Result<(PathBuf, File, u64)> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(app));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= MAX_FILE_BYTES) {
        // Several runs may race here; whichever rename wins, a fresh file follows.
        let _ = std::fs::rename(&path, dir.join(format!("{app}-cli.1.log")));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    Ok((path, file, len))
}

struct Out {
    file: Option<File>,
    written: u64,
}

/// The installed command-line logger.
pub struct CliLogger {
    filter: Filter,
    json: bool,
    stderr: bool,
    path: Option<PathBuf>,
    out: Mutex<Out>,
    collapse: Mutex<Collapser>,
}

impl CliLogger {
    /// A logger writing to `file` (at `path`, already holding `written` bytes).
    pub fn new(filter: Filter, json: bool, stderr: bool, file: Option<(PathBuf, File, u64)>) -> CliLogger {
        let (path, file, written) = match file {
            Some((p, f, w)) => (Some(p), Some(f), w),
            None => (None, None, 0),
        };
        CliLogger { filter, json, stderr, path, out: Mutex::new(Out { file, written }), collapse: Mutex::new(Collapser::default()) }
    }

    /// The log file, when there is one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn write(&self, level: log::Level, target: &str, message: &str) {
        let thread = std::thread::current();
        let line = Line { ts: &timestamp(SystemTime::now()), level, pid: Some(std::process::id()), thread: thread.name().unwrap_or("?"), target, message };
        let text = line.text();
        collapse::remember(&text);
        let formatted = if self.json { line.json() } else { text };
        if self.stderr {
            let _ = std::io::stderr().write_all(formatted.as_bytes());
        }
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let len = u64::try_from(formatted.len()).unwrap_or(u64::MAX);
        let note =
            (out.written.saturating_add(len) > MAX_FILE_BYTES).then(|| format!("log file reached {MAX_FILE_BYTES} bytes; the next run starts a new one\n"));
        let Some(file) = out.file.as_mut() else { return };
        if let Some(n) = note {
            // capped: say so once and stop writing, never fail the command
            let _ = file.write_all(n.as_bytes());
            out.file = None;
            return;
        }
        // One write per line, so lines from concurrent runs appending to the same file stay whole.
        if file.write_all(formatted.as_bytes()).is_ok() {
            out.written = out.written.saturating_add(len);
        } else {
            // a full disk or a vanished file: stop writing, never fail the command
            out.file = None;
        }
    }

    fn note(&self, (times, level, target, so_far): (u64, log::Level, String, bool)) {
        self.write(level, &target, &collapse::note_text(times, so_far));
    }
}

impl log::Log for CliLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let action = self.collapse.lock().unwrap_or_else(PoisonError::into_inner).observe(record.level(), record.target(), &message);
        if let Some(note) = action.note {
            self.note(note);
        }
        if !action.swallow {
            self.write(record.level(), record.target(), &message);
        }
    }

    fn flush(&self) {
        let pending = self.collapse.lock().unwrap_or_else(PoisonError::into_inner).pending();
        if let Some(note) = pending {
            self.note(note);
        }
        if let Some(f) = self.out.lock().unwrap_or_else(PoisonError::into_inner).file.as_mut() {
            let _ = f.flush();
        }
    }
}

static CLI: OnceLock<CliLogger> = OnceLock::new();

/// Install the command-line logger for `app`, writing to `<log_dir>/<app>-cli.log` (no file when
/// `log_dir` is `None` or unusable). Logs the build line and the filter in force first. Returns
/// `None` when another logger was installed first.
pub fn install(app: &App, log_dir: Option<&Path>) -> Option<&'static CliLogger> {
    let (spec, source) = filter::spec_from_env(app.ours, app.default_filter);
    let filter = Filter::parse(&spec);
    let max = filter.max().max(LevelFilter::Info);
    let stderr = matches!(std::env::var("CRAFT_LOG_STDERR").ok().as_deref().map(str::trim), Some("1" | "true" | "yes"));
    let file = log_dir.and_then(|d| open(d, app.name).ok());
    let logger = CLI.get_or_init(|| CliLogger::new(filter, filter::json_from_env(), stderr, file));
    log::set_logger(logger).ok()?;
    log::set_max_level(max);
    logger.write(log::Level::Info, app.name, &app.build_line());
    logger.write(log::Level::Info, app.name, &format!("command line: {}; log filter \"{spec}\" from {}", args_masked(), source.name()));
    crate::set_log_path(logger.path().map(Path::to_path_buf));
    crate::stall::start(app.name);
    Some(logger)
}

/// The installed command-line logger, if any.
pub fn installed() -> Option<&'static CliLogger> {
    CLI.get()
}

/// The program's arguments with the home folder shown as `~`.
fn args_masked() -> String {
    let home = std::env::var("HOME").ok().filter(|h| h.len() > 1);
    std::env::args()
        .map(|a| match &home {
            Some(h) => a.replace(h.as_str(), "~"),
            None => a,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A command is about to exit with an error: log `reason` at error (so the log holds why), then
/// [`report_failure`].
pub fn failed(app: &App, reason: &str) {
    if installed().is_some() {
        log::error!(target: app.name, "{reason}");
    }
    report_failure();
}

/// For a command that is about to exit with an error: flush, and print `log: <path>` as the last
/// line on standard error, so whoever reads the failure knows where the reasons are. Prints
/// nothing when no log file is open.
pub fn report_failure() {
    let Some(logger) = installed() else { return };
    log::Log::flush(logger);
    if let Some(p) = logger.path() {
        eprintln!("log: {}", p.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("craft-log-cli-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn logger_in(dir: &Path, json: bool) -> CliLogger {
        CliLogger::new(Filter::parse("warn,app=debug"), json, false, open(dir, "app").ok())
    }

    fn rec(l: &CliLogger, level: log::Level, target: &str, msg: &str) {
        l.log(&log::Record::builder().level(level).target(target).args(format_args!("{msg}")).build());
    }

    #[test]
    fn every_run_appends_to_one_file_with_its_process_id() {
        let dir = temp_dir("append");
        for run in 0..2 {
            let l = logger_in(&dir, false);
            rec(&l, log::Level::Warn, "app::x", &format!("run {run}"));
            rec(&l, log::Level::Debug, "other", "third-party debug is filtered");
            l.flush();
        }
        let text = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        let pid = std::process::id();
        assert!(lines[0].contains(&format!(" WARN  [{pid} ")) && lines[0].ends_with("app::x: run 0"), "{}", lines[0]);
        assert!(lines[1].ends_with("run 1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_file_moves_aside_at_the_next_run() {
        let dir = temp_dir("rotate");
        std::fs::create_dir_all(&dir).unwrap();
        let full = dir.join("app-cli.log");
        std::fs::File::create(&full).unwrap().set_len(MAX_FILE_BYTES).unwrap();
        let (path, _, len) = open(&dir, "app").unwrap();
        assert_eq!((path, len), (full, 0));
        assert_eq!(std::fs::metadata(dir.join("app-cli.1.log")).unwrap().len(), MAX_FILE_BYTES);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_lines_and_collapsed_repeats() {
        let dir = temp_dir("json");
        let l = logger_in(&dir, true);
        for _ in 0..4 {
            rec(&l, log::Level::Warn, "app", "same");
        }
        rec(&l, log::Level::Info, "app", "next");
        l.flush();
        let text = std::fs::read_to_string(dir.join("app-cli.log")).unwrap();
        let msgs: Vec<_> = text.lines().map(|l| l.rsplit_once("\"msg\":").unwrap().1).collect();
        assert_eq!(msgs, ["\"same\"}", "\"previous line repeated 3 times\"}", "\"next\"}"]);
        assert!(text.lines().all(|l| l.starts_with("{\"ts\":\"") && l.contains(&format!("\"pid\":{}", std::process::id()))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_file_is_not_an_error() {
        let l = CliLogger::new(Filter::parse("info"), false, false, None);
        rec(&l, log::Level::Error, "app", "nowhere to go, and that is fine");
        l.flush();
        assert!(l.path().is_none());
    }
}
