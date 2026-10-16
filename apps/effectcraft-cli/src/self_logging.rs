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

/// Arguments whose values are never logged: none today (no effectcraft-cli argument carries a
/// secret).
const SECRET_ARGS: &[&str] = &[];

/// Start this run's log and the crash file hook (after `--help` and `--version`, which leave no
/// trace).
pub fn install() {
    let dir = log_dir();
    let _ = craft_log::cli::install(&APP, Some(&dir), SECRET_ARGS);
    craft_log::crash::install_hook(APP, dir);
}

/// The run is about to exit with an error: log why, and point at the log.
pub fn failed(reason: &str) {
    craft_log::cli::failed(&APP, reason);
}

/// EffectCraft's own report fields, shared with the window app (its file).
#[path = "../../effectcraft/src/self_logging_report.rs"]
#[allow(dead_code)] // the window app's names; not all are used here
mod report;

/// `effectcraft-cli diagnostics [--json]`: the diagnostic report as text (or one JSON object),
/// also written into this run's log. Reuses Help ▸ System Compatibility Report's fields (a
/// headless session's), lists the graphics adapters without opening a device, and ends with the
/// log folder, the newest logs and the newest crash file.
pub fn diagnostics(json: bool) -> String {
    use craft_log::report::Report;
    let settings = report::settings_dir();
    let logs = report::log_dir(settings.as_deref());
    let mut r = Report::new(&APP);
    system_report(&mut r);
    graphics(&mut r);
    r.add("screens", "screens", "unknown (no window; the window app logs them at start)");
    report::folders(&mut r, settings.as_deref());
    r.add_switches(report::SWITCH_PREFIXES, report::SECRET_SWITCHES);
    r.add_helpers();
    // the window app's newest log is its last run (or the one running now)
    report::previous_run(&mut r, &logs, &logs.join(report::LOG_FILE));
    report::logs(&mut r, &logs);
    r.log(module_path!());
    if json { r.json() } else { r.text() }
}

/// Help ▸ System Compatibility Report's own fields, from a headless session (`help.systemReport`,
/// quiet): what EffectCraft itself sees, beside what craft-log read from the system.
fn system_report(r: &mut craft_log::report::Report) {
    let mut s = effectcraft_host::session();
    let v = match s.execute("help.systemReport", serde_json::json!({"quiet": true})) {
        Ok(v) => v,
        Err(e) => {
            r.add("effectcraft", "system report", format!("unknown ({e})"));
            return;
        }
    };
    let st = |k: &str| v.get(k).and_then(serde_json::Value::as_str).map(str::to_owned).unwrap_or_default();
    r.add("effectcraft", "operating system", st("os"));
    r.add("effectcraft", "processor", format!("{} ({} logical cores)", st("cpu"), v.get("cores").and_then(serde_json::Value::as_u64).unwrap_or(0)));
    let m = &v["memory"];
    r.add("effectcraft", "memory", format!("{} ({} available)", m["total"].as_str().unwrap_or(""), m["available"].as_str().unwrap_or("")));
    let gpu = match (v["gpu"].as_str(), v["gpuNote"].as_str()) {
        (Some(g), _) => g.to_owned(),
        (None, Some(why)) => format!("none here: {why}"),
        (None, None) => "none here".to_owned(),
    };
    r.add("effectcraft", "GPU compositor", gpu);
    let c = &v["caches"];
    r.add(
        "effectcraft",
        "cache budgets",
        format!(
            "layer {}, footage {}, preview {}",
            c["layer"].as_str().unwrap_or("?"),
            c["footage"].as_str().unwrap_or("?"),
            c["preview"].as_str().unwrap_or("?")
        ),
    );
    let export = v["export"].as_array().map_or(0, Vec::len);
    r.add("effectcraft", "export formats", export.to_string());
    let issues: Vec<&str> = v["issues"].as_array().map(|a| a.iter().filter_map(serde_json::Value::as_str).collect()).unwrap_or_default();
    r.add("effectcraft", "issues", if issues.is_empty() { "none".to_owned() } else { issues.join(" ") });
}

/// Graphics: every adapter Vulkan, Metal and DirectX 12 list (not OpenGL, whose drivers can crash
/// on creation). Opens no device.
fn graphics(r: &mut craft_log::report::Report) {
    if wgpu::Instance::enabled_backend_features().is_empty() {
        r.add("graphics", "adapters", "unknown: this build of effectcraft-cli has no wgpu backend");
        return;
    }
    let backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    if adapters.is_empty() {
        r.add("graphics", "adapters", "none listed by Vulkan, Metal or DirectX 12");
    }
    for a in &adapters {
        let i = a.get_info();
        r.add("graphics", "adapter", format!("\"{}\" ({:?}, {:?}, driver {} {})", i.name, i.backend, i.device_type, i.driver, i.driver_info));
    }
    r.add("graphics", "window would use", "the window's surface decides: the window app logs its \"graphics:\" line at start");
}
