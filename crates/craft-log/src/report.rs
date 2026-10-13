//! The diagnostic report (contract B): ordered `(section, key, value)` lines, rendered as text and
//! as JSON, written at the top of a log and printed by `<app>-cli diagnostics`.
//!
//! craft-log fills the fields every app shares, from what Linux shows in `/proc`, `/etc` and the
//! environment (no dependencies, nothing run but `ffmpeg -version`); the app adds its own sections
//! (graphics, video, screens, folders, the previous run) in between. Every value is a string, and
//! a field nobody could read says "unknown" rather than going missing. The home folder is shown as
//! `~` everywhere ([`mask_home`]). On other systems the Linux-only fields read "unknown".
//!
//! The order, which the app keeps by calling these in turn:
//! [`Report::new`] (app, system, processor, memory) → the app's graphics, video, screens and
//! folders → [`Report::add_switches`] → [`Report::add_helpers`] → the app's previous run →
//! [`Report::add_logs`], last, so the report ends with where the logs are.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use crate::App;
use crate::line::escape;

/// What a field says when it could not be read.
pub const UNKNOWN: &str = "unknown";

/// How long `ffmpeg -version` may take before it is given up on.
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(3);

/// One field: its section, its name and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub section: String,
    pub key: String,
    pub value: String,
}

/// The report: fields in the order they were added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    fields: Vec<Field>,
}

impl Report {
    /// An empty report (the app's tests use this; programs use [`Report::new`]).
    pub fn empty() -> Report {
        Report::default()
    }

    /// A report holding the fields craft-log gathers first: the app and its build, the system,
    /// the processor and memory.
    pub fn new(app: &App) -> Report {
        let mut r = Report::empty();
        r.add("app", "name", app.name);
        r.add("app", "version", app.version);
        r.add("app", "commit", crate::BUILD_COMMIT);
        r.add("app", "build profile", crate::BUILD_PROFILE);
        r.add("app", "built on", crate::BUILD_DATE);
        r.add("app", "runs from", std::env::current_exe().map(|p| mask_home(&p.display().to_string())).unwrap_or_default());
        r.add("system", "operating system", read("/etc/os-release").map(|t| os_name(&t)).unwrap_or_default());
        r.add("system", "kernel", read("/proc/sys/kernel/osrelease").map(|t| t.trim().to_owned()).unwrap_or_default());
        r.add("system", "architecture", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH));
        r.add("system", "session", session(&|k| std::env::var(k).ok()));
        r.add("system", "locale", locale(&|k| std::env::var(k).ok()));
        r.add("processor", "model", read("/proc/cpuinfo").map(|t| cpu_model(&t)).unwrap_or_default());
        r.add("processor", "threads", std::thread::available_parallelism().map(|n| n.to_string()).unwrap_or_default());
        let mem = read("/proc/meminfo").map(|t| meminfo(&t)).unwrap_or_default();
        r.add("memory", "total", mem.0);
        r.add("memory", "available", mem.1);
        r
    }

    /// Add a field. An empty value reads "unknown"; a key already in the section gets " 2", " 3"…
    pub fn add(&mut self, section: &str, key: &str, value: impl Into<String>) {
        let value = value.into();
        let value = if value.trim().is_empty() { UNKNOWN.to_owned() } else { value };
        let taken = |k: &str| self.fields.iter().any(|f| f.section == section && f.key == k);
        let mut unique = key.to_owned();
        let mut n = 1;
        while taken(&unique) {
            n += 1;
            unique = format!("{key} {n}");
        }
        self.fields.push(Field { section: section.to_owned(), key: unique, value });
    }

    /// The fields, in order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The value of `section`/`key`, if present.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.fields.iter().find(|f| f.section == section && f.key == key).map(|f| f.value.as_str())
    }

    /// The switches in force: every environment variable starting with `CRAFT_`, `RUST_LOG`, and
    /// those starting with one of the app's `prefixes` (`FILMCRAFT_`, `WGPU_`), values masked.
    pub fn add_switches(&mut self, prefixes: &[&str]) {
        let vars: Vec<(String, String)> = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.to_string_lossy().into_owned()))).collect();
        let found = switches(&vars, prefixes);
        if found.is_empty() {
            self.add("switches", "environment", "none");
        }
        for (k, v) in found {
            self.add("switches", &k, mask_home(&v));
        }
    }

    /// Helper programs: ffmpeg (where it is on `PATH`, and its first version line).
    pub fn add_helpers(&mut self) {
        match find_on_path("ffmpeg") {
            None => self.add("helpers", "ffmpeg", "not found on PATH"),
            Some(p) => {
                let version = first_line_of(&p, &["-version"], FFMPEG_TIMEOUT).unwrap_or_else(|why| format!("{UNKNOWN} ({why})"));
                self.add("helpers", "ffmpeg", format!("{} ({version})", mask_home(&p.display().to_string())));
            }
        }
    }

    /// Where the logs are: the folder, each named log (`("window log", path)`), and the newest
    /// crash file. Last, by the contract, so whoever reads the report's end finds them.
    pub fn add_logs(&mut self, folder: Option<&Path>, logs: &[(&str, Option<&Path>)], newest_crash: Option<&Path>) {
        let show = |p: Option<&Path>, none: &str| p.map_or_else(|| none.to_owned(), |p| mask_home(&p.display().to_string()));
        self.add("logs", "folder", show(folder, UNKNOWN));
        for (name, path) in logs {
            self.add("logs", name, show(*path, "none"));
        }
        self.add("logs", "newest crash file", show(newest_crash, "none"));
    }

    /// The report as text: a title, then each section's fields.
    pub fn text(&self) -> String {
        let mut s = String::from("diagnostic report\n");
        let mut section = "";
        for f in &self.fields {
            if f.section != section {
                section = &f.section;
                let _ = writeln!(s, "[{section}]");
            }
            let _ = writeln!(s, "  {}: {}", f.key, f.value);
        }
        s
    }

    /// The report as one JSON object: `{"<section>": {"<key>": "<value>", …}, …}`, in order.
    pub fn json(&self) -> String {
        let mut s = String::from("{");
        let mut section: Option<&str> = None;
        for f in &self.fields {
            if section != Some(f.section.as_str()) {
                if section.is_some() {
                    s.push_str("},");
                }
                section = Some(&f.section);
                let _ = write!(s, "\"{}\":{{", escape(&f.section));
            } else {
                s.push(',');
            }
            let _ = write!(s, "\"{}\":\"{}\"", escape(&f.key), escape(&f.value));
        }
        if section.is_some() {
            s.push('}');
        }
        s.push('}');
        s
    }

    /// Write the report into the log, one info line per field (`report: [section] key: value`),
    /// under `target`.
    pub fn log(&self, target: &str) {
        for f in &self.fields {
            log::info!(target: target, "report: [{}] {}: {}", f.section, f.key, f.value);
        }
    }
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// `~` in place of the home folder (`$HOME`, or `%USERPROFILE%` on Windows), wherever it starts a
/// path in `text`.
pub fn mask_home(text: &str) -> String {
    let home = std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok());
    mask_with(text, home.as_deref())
}

/// [`mask_home`] with the home folder given: each occurrence followed by a path separator, the
/// end of the text, or a character that cannot be in a name (space, quote, `:`, `;`), becomes `~`.
/// `/home/deckard` is left alone when the home folder is `/home/deck`.
pub fn mask_with(text: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])).filter(|h| h.len() > 1) else { return text.to_owned() };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(home) {
        let after = &rest[i + home.len()..];
        let boundary = after.chars().next().is_none_or(|c| matches!(c, '/' | '\\' | ' ' | '"' | '\'' | ':' | ';' | ',' | ')'));
        out.push_str(&rest[..i]);
        out.push_str(if boundary { "~" } else { home });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `PRETTY_NAME` from `/etc/os-release` text, else `NAME VERSION_ID`, with `BUILD_ID` and
/// `VARIANT_ID` added when present (SteamOS puts its build there).
pub fn os_name(os_release: &str) -> String {
    let field = |k: &str| {
        os_release.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_owned()).filter(|v| !v.is_empty())
    };
    let mut name = field("PRETTY_NAME").or_else(|| Some(format!("{} {}", field("NAME")?, field("VERSION_ID").unwrap_or_default()).trim().to_owned()));
    if let Some(n) = &mut name {
        if let Some(v) = field("VERSION_ID").filter(|v| !n.contains(v.as_str())) {
            let _ = write!(n, " {v}");
        }
        if let Some(b) = field("BUILD_ID") {
            let _ = write!(n, " (build {b})");
        }
        if let Some(v) = field("VARIANT_ID") {
            let _ = write!(n, " [{v}]");
        }
    }
    name.unwrap_or_default()
}

/// The first `model name` (x86) or `Model`/`Hardware` (ARM) in `/proc/cpuinfo` text.
pub fn cpu_model(cpuinfo: &str) -> String {
    ["model name", "Model", "Hardware"]
        .iter()
        .find_map(|k| cpuinfo.lines().find_map(|l| l.split_once(':').filter(|(a, _)| a.trim() == *k).map(|(_, v)| v.trim().to_owned())))
        .unwrap_or_default()
}

/// (total, available) from `/proc/meminfo` text, as `"62.5 GiB"`.
pub fn meminfo(text: &str) -> (String, String) {
    let kib =
        |k: &str| -> Option<u64> { text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix(':')).and_then(|v| v.split_whitespace().next()?.parse().ok()) };
    let gib = |v: Option<u64>| v.map(|k| format!("{:.1} GiB", k as f64 / 1_048_576.0)).unwrap_or_default();
    (gib(kib("MemTotal")), gib(kib("MemAvailable")))
}

/// The graphical session from the environment `env` reads: Game Mode (gamescope) when Steam's
/// game UI is running, else the session type and the desktop (`wayland, KDE`), else "unknown".
pub fn session(env: &dyn Fn(&str) -> Option<String>) -> String {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    let desktop = set("XDG_CURRENT_DESKTOP");
    let gamescope = desktop.as_deref().is_some_and(|d| d.to_ascii_lowercase().contains("gamescope")) || set("GAMESCOPE_WAYLAND_DISPLAY").is_some();
    if set("SteamGamepadUI").is_some_and(|v| v != "0") || gamescope {
        return "SteamOS Game Mode (gamescope)".into();
    }
    let kind = set("XDG_SESSION_TYPE").or_else(|| set("WAYLAND_DISPLAY").map(|_| "wayland".into())).or_else(|| set("DISPLAY").map(|_| "x11".into()));
    match (kind, desktop) {
        (Some(k), Some(d)) => format!("{k}, {d}"),
        (Some(k), None) => k,
        (None, Some(d)) => d,
        (None, None) if cfg!(windows) => "Windows".into(),
        (None, None) => UNKNOWN.into(),
    }
}

/// `LC_ALL`, else `LANG`, else "unknown".
pub fn locale(env: &dyn Fn(&str) -> Option<String>) -> String {
    ["LC_ALL", "LANG"].iter().find_map(|k| env(k).filter(|v| !v.is_empty())).unwrap_or_else(|| UNKNOWN.into())
}

/// The switches among `vars`: `CRAFT_*`, `RUST_LOG`, and names starting with one of `prefixes`,
/// sorted by name.
pub fn switches(vars: &[(String, String)], prefixes: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        vars.iter().filter(|(k, _)| k.starts_with("CRAFT_") || k == "RUST_LOG" || prefixes.iter().any(|p| k.starts_with(p))).cloned().collect();
    out.sort();
    out
}

/// The first file named `program` in a folder on `PATH`.
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: Vec<String> = if cfg!(windows) { vec![format!("{program}.exe")] } else { vec![program.to_owned()] };
    std::env::split_paths(&path).flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// Run `program args` and return the first line it printed, killing it after `timeout`.
pub fn first_line_of(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no answer within {} s", timeout.as_secs()));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut text = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut out, &mut text);
    }
    text.lines().next().map(str::to_owned).filter(|l| !l.is_empty()).ok_or_else(|| "printed nothing".into())
}

/// The most recently modified file in `dir` whose name `wanted` accepts.
pub fn newest_file(dir: &Path, wanted: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(wanted) && e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// The last line of the file at `path` that contains `needle`, looking at its last `max_bytes`.
pub fn last_line_with(path: &Path, needle: &str, max_bytes: u64) -> Option<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max_bytes))).ok()?;
    let mut bytes = Vec::new();
    f.take(max_bytes).read_to_end(&mut bytes).ok()?;
    String::from_utf8_lossy(&bytes).lines().rev().find(|l| l.contains(needle)).map(str::to_owned)
}

/// When `path` was created, where the file system records it (a rename keeps it).
pub fn created(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.created().ok()
}

/// When `path` was last modified.
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_replaces_the_home_folder_only_where_a_path_continues() {
        let home = Some("/home/deck");
        assert_eq!(mask_with("/home/deck/dev/x and /home/deck", home), "~/dev/x and ~");
        assert_eq!(mask_with("/home/deckard/x", home), "/home/deckard/x");
        assert_eq!(mask_with("FILMCRAFT_DATA_DIR=/home/deck/a:/home/deck/b", home), "FILMCRAFT_DATA_DIR=~/a:~/b");
        assert_eq!(mask_with("C:\\Users\\ian\\AppData", Some("C:\\Users\\ian\\")), "~\\AppData");
        assert_eq!(mask_with("/x", None), "/x");
        assert_eq!(mask_with("/x", Some("/")), "/x", "a root home masks nothing");
    }

    #[test]
    fn os_release_names_steamos_with_its_build() {
        let steamos = "NAME=\"SteamOS\"\nPRETTY_NAME=\"SteamOS\"\nVERSION_ID=\"3.8\"\nBUILD_ID=\"20260930.1\"\nVARIANT_ID=steamdeck\nID=steamos\n";
        assert_eq!(os_name(steamos), "SteamOS 3.8 (build 20260930.1) [steamdeck]");
        assert_eq!(os_name("NAME=Fedora Linux\nVERSION_ID=41\n"), "Fedora Linux 41");
        assert_eq!(os_name("PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nVERSION_ID=\"24.04\"\n"), "Ubuntu 24.04.1 LTS");
        assert_eq!(os_name(""), "");
    }

    #[test]
    fn cpuinfo_and_meminfo() {
        let cpu = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD RYZEN AI MAX+ 395 w/ Radeon 8060S\nprocessor\t: 1\nmodel name\t: AMD RYZEN AI MAX+ 395 w/ Radeon 8060S\n";
        assert_eq!(cpu_model(cpu), "AMD RYZEN AI MAX+ 395 w/ Radeon 8060S");
        assert_eq!(cpu_model("processor : 0\nHardware : BCM2835\n"), "BCM2835");
        let mem = "MemTotal:       65536000 kB\nMemFree:         1000 kB\nMemAvailable:   32768000 kB\n";
        assert_eq!(meminfo(mem), ("62.5 GiB".into(), "31.2 GiB".into()));
        assert_eq!(meminfo("nothing"), (String::new(), String::new()));
    }

    #[test]
    fn session_and_locale_from_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(a, _)| *a == k).map(|(_, v)| (*v).to_owned());
        assert_eq!(session(&env(&[("XDG_SESSION_TYPE", "wayland"), ("XDG_CURRENT_DESKTOP", "KDE")])), "wayland, KDE");
        assert_eq!(session(&env(&[("XDG_SESSION_TYPE", "wayland"), ("SteamGamepadUI", "1")])), "SteamOS Game Mode (gamescope)");
        assert_eq!(session(&env(&[("XDG_CURRENT_DESKTOP", "gamescope")])), "SteamOS Game Mode (gamescope)");
        assert_eq!(session(&env(&[("DISPLAY", ":0")])), "x11");
        if !cfg!(windows) {
            assert_eq!(session(&env(&[])), "unknown");
        }
        assert_eq!(locale(&env(&[("LANG", "en_US.UTF-8")])), "en_US.UTF-8");
        assert_eq!(locale(&env(&[("LANG", "")])), "unknown");
    }

    #[test]
    fn switches_are_ours_and_sorted() {
        let vars: Vec<(String, String)> =
            [("PATH", "/bin"), ("FILMCRAFT_DATA_DIR", "/d"), ("CRAFT_LOG", "debug"), ("RUST_LOG", "info"), ("WGPU_BACKEND", "vulkan")]
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect();
        let got: Vec<String> = switches(&vars, &["FILMCRAFT_", "WGPU_"]).into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(got, ["CRAFT_LOG=debug", "FILMCRAFT_DATA_DIR=/d", "RUST_LOG=info", "WGPU_BACKEND=vulkan"]);
    }

    #[test]
    fn a_report_renders_in_order_as_text_and_json_with_unknowns() {
        let mut r = Report::empty();
        r.add("app", "name", "filmcraft");
        r.add("app", "commit", "");
        r.add("graphics", "adapter", "Radeon \"8060S\"");
        r.add("graphics", "adapter", "llvmpipe");
        r.add_logs(Some(Path::new("/tmp/Logs")), &[("newest log", None)], None);
        assert_eq!(r.get("app", "commit"), Some("unknown"));
        assert_eq!(r.get("graphics", "adapter 2"), Some("llvmpipe"));
        assert_eq!(
            r.text(),
            "diagnostic report\n[app]\n  name: filmcraft\n  commit: unknown\n[graphics]\n  adapter: Radeon \"8060S\"\n  adapter 2: llvmpipe\n[logs]\n  folder: /tmp/Logs\n  newest log: none\n  newest crash file: none\n"
        );
        assert_eq!(
            r.json(),
            r#"{"app":{"name":"filmcraft","commit":"unknown"},"graphics":{"adapter":"Radeon \"8060S\"","adapter 2":"llvmpipe"},"logs":{"folder":"/tmp/Logs","newest log":"none","newest crash file":"none"}}"#
        );
        assert_eq!(Report::empty().json(), "{}");
    }

    #[test]
    fn the_gathered_fields_are_never_missing() {
        let app = App { name: "testapp", version: "1.0.0", ours: "testapp", default_filter: "info" };
        let mut r = Report::new(&app);
        r.add_switches(&["TESTAPP_"]);
        r.add_helpers();
        for (s, k) in [("app", "commit"), ("system", "kernel"), ("system", "session"), ("processor", "threads"), ("memory", "total"), ("helpers", "ffmpeg")] {
            assert!(r.get(s, k).is_some_and(|v| !v.is_empty()), "{s}/{k} in {}", r.text());
        }
        if cfg!(target_os = "linux") {
            assert_ne!(r.get("system", "kernel"), Some(UNKNOWN));
            assert_ne!(r.get("memory", "total"), Some(UNKNOWN));
        }
    }

    #[test]
    fn a_program_that_does_not_answer_is_given_up_on() {
        if !cfg!(unix) || !Path::new("/bin/sleep").is_file() {
            return;
        }
        let t0 = Instant::now();
        let got = first_line_of(Path::new("/bin/sleep"), &["5"], Duration::from_millis(200));
        assert!(got.is_err_and(|e| e.contains("no answer")));
        assert!(t0.elapsed() < Duration::from_secs(3));
        assert_eq!(first_line_of(Path::new("/bin/echo"), &["hello"], FFMPEG_TIMEOUT).as_deref(), Ok("hello"));
    }

    #[test]
    fn the_newest_file_is_found_by_name() {
        let dir = std::env::temp_dir().join(format!("craft-log-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("crash-1.log"), "x").unwrap();
        std::fs::write(dir.join("app.log"), "x").unwrap();
        assert_eq!(newest_file(&dir, &|n| n.starts_with("crash-")), Some(dir.join("crash-1.log")));
        assert_eq!(newest_file(&dir, &|n| n.ends_with(".txt")), None);
        assert!(modified(&dir.join("app.log")).is_some());
        std::fs::write(dir.join("run.log"), "start\nrun ended: one\nmiddle\nrun ended: two\nlast\n").unwrap();
        assert_eq!(last_line_with(&dir.join("run.log"), "run ended:", 4096).as_deref(), Some("run ended: two"));
        assert_eq!(last_line_with(&dir.join("run.log"), "run ended:", 5), None, "only the tail is read");
        assert_eq!(last_line_with(&dir.join("missing.log"), "x", 10), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
