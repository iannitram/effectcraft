//! A monitor's true size on a Linux Wayland desktop (ours).
//!
//! winit 0.30.13 reports a Wayland monitor by its current mode and ignores the output's transform
//! (`platform_impl/linux/wayland/output.rs`), and its scale factor is the integer `wl_output.scale`
//! (a desktop at 1.25 reports 2). `window::place` divides the size by that scale, so a 1920x1200
//! laptop panel at 1.25 (desktop 1536x960 points) looked like 960x600 points, and a handheld's
//! sideways panel had the unturned shape.
//!
//! [`size`] asks the compositor instead, once, over a short-lived second connection with a hard
//! time limit ([`QUERY_LIMIT`]): `zxdg_output_v1.logical_size` is the screen in points with the
//! rotation and the fractional scale already applied. Without it, a screen turned a quarter is its
//! mode swapped and divided by the integer scale (right for a turned screen only). The size handed
//! back is that, in points, times winit's own scale factor, so `window::Monitor` (which divides by
//! the same factor) sees the true size in points. Otherwise, and on any error, timeout or odd
//! answer, winit's size is used as before and one info line says why.
//!
//! X11 (no `WAYLAND_DISPLAY`), a socket handed over in `WAYLAND_SOCKET` (that one is winit's) and
//! gamescope are left alone before anything connects. Screens plugged in or turned while the app
//! runs are not tracked. When winit applies the transform and the fractional scale itself, this
//! module can go.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::sync::mpsc;
use std::time::Duration;
use winit::dpi::PhysicalSize;

/// The longest the compositor query may delay the window.
pub const QUERY_LIMIT: Duration = Duration::from_millis(300);

/// A screen side, in points, outside this range is an odd answer and not used.
const SANE_SIDE: std::ops::RangeInclusive<f32> = 100.0..=16384.0;

/// One output as the compositor describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Connector name (`eDP-1`), from `wl_output.name` or `zxdg_output_v1.name`: what winit's
    /// `MonitorHandle::name` says.
    pub name: Option<String>,
    /// The current mode in pixels as the panel reports it (unturned): what winit calls its size.
    pub mode: Option<[i32; 2]>,
    /// Shown turned by 90 or 270 degrees.
    pub quarter_turned: bool,
    /// `wl_output.scale`, the integer winit reports.
    pub scale: i32,
    /// `zxdg_output_v1.logical_size`: the screen in points as the desktop uses it.
    pub logical: Option<[i32; 2]>,
}

impl Default for Output {
    fn default() -> Self {
        Self { name: None, mode: None, quarter_turned: false, scale: 1, logical: None }
    }
}

/// Where a screen size came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    XdgOutput,
    TurnedMode,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::XdgOutput => "xdg-output",
            Source::TurnedMode => "turned mode / integer scale",
        }
    }
}

fn sane(size: [f32; 2]) -> bool {
    size.iter().all(|s| s.is_finite() && SANE_SIDE.contains(s))
}

/// The screen's size in points, if it can be told: the xdg-output logical size, else (only for a
/// screen turned a quarter) the mode swapped and divided by the integer scale. `None` for an
/// upright screen without a logical size (winit's own figure is then all there is) and for zero,
/// negative or absurd figures.
pub fn screen_points(output: &Output) -> Option<([f32; 2], Source)> {
    if let Some([w, h]) = output.logical {
        let size = [w as f32, h as f32];
        if sane(size) {
            return Some((size, Source::XdgOutput));
        }
    }
    let [w, h] = output.mode?;
    if !output.quarter_turned || output.scale < 1 {
        return None;
    }
    let scale = output.scale as f32;
    let size = [h as f32 / scale, w as f32 / scale];
    sane(size).then_some((size, Source::TurnedMode))
}

/// The output winit calls `name`: the one with that name, else the only one.
pub fn find<'a>(outputs: &'a [Output], name: Option<&str>) -> Option<&'a Output> {
    if let Some(name) = name
        && let Some(found) = outputs.iter().find(|o| o.name.as_deref() == Some(name))
    {
        return Some(found);
    }
    match outputs {
        [only] => Some(only),
        _ => None,
    }
}

/// The size to give `window::Monitor` for a monitor winit reports as `winit_size` at
/// `winit_scale`: the true size in points times that scale, so dividing by it gives the points
/// back. `None` (keep winit's size) when the screen can't be told.
pub fn true_size(outputs: &[Output], name: Option<&str>, winit_scale: f64) -> Option<([u32; 2], [f32; 2], Source)> {
    let (points, source) = screen_points(find(outputs, name)?)?;
    let scale = if winit_scale.is_finite() && winit_scale > 0.0 { winit_scale as f32 } else { 1.0 };
    Some((points.map(|p| (p * scale).round() as u32), points, source))
}

/// Why the compositor is not asked; `None` when it may be. `var` reads an environment variable.
pub fn gate(var: impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return Some("not Linux");
    }
    if var("WAYLAND_DISPLAY").is_none_or(|d| d.is_empty()) {
        return Some("no WAYLAND_DISPLAY (X11 or XWayland), winit's monitor size is right there");
    }
    if var("WAYLAND_SOCKET").is_some() {
        return Some("WAYLAND_SOCKET is set (that socket is winit's)");
    }
    let gamescope = var("XDG_CURRENT_DESKTOP").is_some_and(|d| d.to_ascii_lowercase().contains("gamescope")) || var("GAMESCOPE_WAYLAND_DISPLAY").is_some();
    if gamescope {
        return Some("gamescope (Game Mode)");
    }
    None
}

/// Why a query gave no outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    TimedOut(Duration),
    Failed(String),
}

/// Runs `job` on its own thread and waits at most `limit` for it. A job still running then is
/// left to finish (or not) on its own; the caller goes on.
pub fn with_limit<T: Send + 'static>(limit: Duration, job: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, Failure> {
    let (tx, rx) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new().name("wayland-screen-query".into()).spawn(move || {
        let _ = tx.send(job());
    });
    if let Err(e) = spawned {
        return Err(Failure::Failed(format!("no thread for the query: {e}")));
    }
    match rx.recv_timeout(limit) {
        Ok(answer) => answer.map_err(Failure::Failed),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Failure::TimedOut(limit)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Failure::Failed("the query stopped without an answer".into())),
    }
}

/// What the query came back with.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// Not asked, and why.
    Gated(&'static str),
    Failed(Failure, Duration),
    Outputs(Vec<Output>, Duration),
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

/// The size for one monitor and the log line saying where it came from; pure.
pub fn decide(answer: &Answer, name: Option<&str>, winit_size: [u32; 2], winit_scale: f64) -> (Option<[u32; 2]>, String) {
    let who = name.unwrap_or("unnamed monitor");
    let head = format!("decision: window: monitor {who}: winit reports {}x{} at scale {winit_scale}; ", winit_size[0], winit_size[1]);
    let untouched = |reason: String| (None, format!("{head}untouched: {reason}"));
    match answer {
        Answer::Gated(why) => untouched((*why).into()),
        Answer::Failed(Failure::TimedOut(limit), _) => untouched(format!("the compositor did not answer within {} ms", ms(*limit))),
        Answer::Failed(Failure::Failed(e), took) => untouched(format!("compositor query failed after {} ms: {e}", ms(*took))),
        Answer::Outputs(outputs, took) => match true_size(outputs, name, winit_scale) {
            Some((size, _, source)) if size == winit_size => (None, format!("{head}{} agrees ({} ms), untouched", source.label(), ms(*took))),
            Some((size, points, source)) => {
                let line = format!(
                    "{head}screen {}x{} points ({}, {} ms); size used {}x{}",
                    points[0].round(),
                    points[1].round(),
                    source.label(),
                    ms(*took),
                    size[0],
                    size[1]
                );
                (Some(size), line)
            }
            None => untouched(format!("no xdg-output logical size and not turned a quarter, or no output by that name ({} ms)", ms(*took))),
        },
    }
}

#[cfg(target_os = "linux")]
mod query {
    use super::Output;
    use wayland_client::protocol::{wl_output, wl_registry};
    use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
    use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

    #[derive(Default)]
    struct State {
        outputs: Vec<Output>,
        wl_outputs: Vec<wl_output::WlOutput>,
        manager: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for State {
        fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, (): &(), _: &Connection, qh: &QueueHandle<Self>) {
            let wl_registry::Event::Global { name, interface, version } = event else { return };
            if interface == "wl_output" {
                let index = state.outputs.len();
                state.outputs.push(Output::default());
                // 4 brings the connector name, which winit's monitor is matched by.
                state.wl_outputs.push(registry.bind(name, version.min(4), qh, index));
            } else if interface == "zxdg_output_manager_v1" {
                state.manager = Some(registry.bind(name, version.min(3), qh, ()));
            }
        }
    }

    impl Dispatch<wl_output::WlOutput, usize> for State {
        fn event(state: &mut Self, _: &wl_output::WlOutput, event: wl_output::Event, index: &usize, _: &Connection, _: &QueueHandle<Self>) {
            let Some(output) = state.outputs.get_mut(*index) else { return };
            match event {
                wl_output::Event::Geometry { transform, .. } => {
                    use wl_output::Transform as T;
                    output.quarter_turned = matches!(transform, WEnum::Value(T::_90 | T::_270 | T::Flipped90 | T::Flipped270));
                }
                wl_output::Event::Mode { flags: WEnum::Value(flags), width, height, .. } if flags.contains(wl_output::Mode::Current) => {
                    output.mode = Some([width, height]);
                }
                wl_output::Event::Scale { factor } => output.scale = factor,
                wl_output::Event::Name { name } => output.name = Some(name),
                _ => {}
            }
        }
    }

    impl Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &zxdg_output_manager_v1::ZxdgOutputManagerV1,
            _: zxdg_output_manager_v1::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<zxdg_output_v1::ZxdgOutputV1, usize> for State {
        fn event(state: &mut Self, _: &zxdg_output_v1::ZxdgOutputV1, event: zxdg_output_v1::Event, index: &usize, _: &Connection, _: &QueueHandle<Self>) {
            let Some(output) = state.outputs.get_mut(*index) else { return };
            match event {
                zxdg_output_v1::Event::LogicalSize { width, height } => output.logical = Some([width, height]),
                zxdg_output_v1::Event::Name { name } if output.name.is_none() => output.name = Some(name),
                _ => {}
            }
        }
    }

    /// Every output the compositor advertises. The first round trip delivers the globals (and
    /// sends our binds), the second each output's events and its xdg-output's.
    pub fn outputs() -> Result<Vec<Output>, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
        let mut queue = conn.new_event_queue::<State>();
        let qh = queue.handle();
        let _registry = conn.display().get_registry(&qh, ());
        let mut state = State::default();
        queue.roundtrip(&mut state).map_err(|e| format!("roundtrip: {e}"))?;
        let mut xdg_outputs = Vec::new();
        if let Some(manager) = &state.manager {
            for (index, wl_output) in state.wl_outputs.iter().enumerate() {
                xdg_outputs.push(manager.get_xdg_output(wl_output, &qh, index));
            }
        }
        queue.roundtrip(&mut state).map_err(|e| format!("roundtrip: {e}"))?;
        for xdg_output in &xdg_outputs {
            xdg_output.destroy();
        }
        if let Some(manager) = &state.manager {
            manager.destroy();
        }
        let _ = conn.flush();
        Ok(state.outputs)
    }
}

/// Asks the compositor (unless [`gate`] says not to), within [`QUERY_LIMIT`]; once per process.
#[cfg(target_os = "linux")]
fn answer() -> &'static Answer {
    static ANSWER: std::sync::OnceLock<Answer> = std::sync::OnceLock::new();
    ANSWER.get_or_init(|| {
        if let Some(why) = gate(|name| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())) {
            return Answer::Gated(why);
        }
        let started = std::time::Instant::now();
        match with_limit(QUERY_LIMIT, query::outputs) {
            Ok(outputs) => Answer::Outputs(outputs, started.elapsed()),
            Err(failure) => Answer::Failed(failure, started.elapsed()),
        }
    })
}

/// The size of `monitor` for `window::Monitor`: on a Wayland desktop the compositor's logical size
/// times winit's scale factor (see the module), else winit's size unchanged. Logs what it did once
/// per monitor name.
pub fn size(monitor: &winit::monitor::MonitorHandle) -> PhysicalSize<u32> {
    let winit_size = monitor.size();
    #[cfg(target_os = "linux")]
    {
        let name = monitor.name();
        let (size, line) = decide(answer(), name.as_deref(), [winit_size.width, winit_size.height], monitor.scale_factor());
        static LOGGED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        if let Ok(mut logged) = LOGGED.lock()
            && !logged.contains(&line)
        {
            if size.is_some() {
                log::warn!("{line}");
            } else {
                log::info!("{line}");
            }
            logged.push(line);
        }
        if let Some([w, h]) = size {
            return PhysicalSize::new(w, h);
        }
    }
    winit_size
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS4: Duration = Duration::from_millis(4);

    fn z13(logical: bool) -> Output {
        Output { name: Some("eDP-1".into()), mode: Some([1920, 1200]), quarter_turned: false, scale: 2, logical: logical.then_some([1536, 960]) }
    }

    fn legion_go(logical: bool) -> Output {
        Output { name: Some("eDP-1".into()), mode: Some([1600, 2560]), quarter_turned: true, scale: 2, logical: logical.then_some([1280, 800]) }
    }

    #[test]
    fn z13_at_125_gets_its_logical_size_times_winits_scale() {
        let answer = Answer::Outputs(vec![z13(true)], MS4);
        let (size, line) = decide(&answer, Some("eDP-1"), [1920, 1200], 2.0);
        assert_eq!(size, Some([3072, 1920]));
        assert_eq!(line, "decision: window: monitor eDP-1: winit reports 1920x1200 at scale 2; screen 1536x960 points (xdg-output, 4 ms); size used 3072x1920");
    }

    #[test]
    fn z13_without_the_logical_size_keeps_winits_size() {
        let (size, line) = decide(&Answer::Outputs(vec![z13(false)], MS4), Some("eDP-1"), [1920, 1200], 2.0);
        assert_eq!(size, None);
        assert!(line.contains("untouched"), "{line}");
    }

    #[test]
    fn legion_go_turned_with_and_without_the_logical_size() {
        for logical in [true, false] {
            let (size, _) = decide(&Answer::Outputs(vec![legion_go(logical)], MS4), Some("eDP-1"), [1600, 2560], 2.0);
            assert_eq!(size, Some([2560, 1600]), "logical {logical}");
        }
    }

    #[test]
    fn agreeing_sizes_are_left_alone() {
        let big = Output { name: Some("DP-1".into()), mode: Some([2560, 1440]), quarter_turned: false, scale: 1, logical: Some([2560, 1440]) };
        let (size, line) = decide(&Answer::Outputs(vec![big], MS4), Some("DP-1"), [2560, 1440], 1.0);
        assert_eq!(size, None);
        assert!(line.ends_with("xdg-output agrees (4 ms), untouched"), "{line}");
    }

    #[test]
    fn the_monitor_is_found_by_name_or_as_the_only_one() {
        let big = Output { name: Some("DP-1".into()), mode: Some([2560, 1440]), quarter_turned: false, scale: 1, logical: Some([2560, 1440]) };
        let outputs = vec![big.clone(), z13(true)];
        assert_eq!(find(&outputs, Some("eDP-1")), Some(&outputs[1]));
        assert_eq!(find(&outputs, Some("HDMI-A-1")), None);
        assert_eq!(find(&outputs, None), None);
        assert_eq!(find(&[big.clone()], None), Some(&big));
        assert_eq!(find(&[big.clone()], Some("other")), Some(&big));
    }

    #[test]
    fn square_and_270_turned_screens() {
        let square = Output { name: None, mode: Some([2000, 2000]), quarter_turned: true, scale: 2, logical: None };
        assert_eq!(screen_points(&square), Some(([1000.0, 1000.0], Source::TurnedMode)));
        // 270 degrees sets the same flag as 90 (see `query`), and swaps the same way.
        assert_eq!(screen_points(&legion_go(false)), Some(([1280.0, 800.0], Source::TurnedMode)));
    }

    #[test]
    fn zero_negative_and_absurd_sizes_are_not_used() {
        let mut out = z13(true);
        for odd in [[0, 0], [1536, 0], [-1536, 960], [100_000, 960]] {
            out.logical = Some(odd);
            assert_eq!(screen_points(&out), None, "{odd:?}");
        }
        assert_eq!(screen_points(&Output { mode: Some([0, 0]), quarter_turned: true, scale: 2, ..Output::default() }), None);
        assert_eq!(screen_points(&Output { mode: Some([1600, 2560]), quarter_turned: true, scale: 0, ..Output::default() }), None);
        assert_eq!(true_size(&[], Some("eDP-1"), 2.0), None);
        // A broken winit scale is taken as 1.
        assert_eq!(true_size(&[z13(true)], Some("eDP-1"), f64::NAN), Some(([1536, 960], [1536.0, 960.0], Source::XdgOutput)));
    }

    #[test]
    fn gate_leaves_x11_handed_sockets_and_gamescope_alone() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string());
        assert_eq!(gate(env(&[("WAYLAND_DISPLAY", "wayland-0"), ("XDG_CURRENT_DESKTOP", "KDE")])), None);
        assert!(gate(env(&[])).is_some());
        assert!(gate(env(&[("WAYLAND_DISPLAY", "")])).is_some());
        assert!(gate(env(&[("WAYLAND_DISPLAY", "wayland-0"), ("WAYLAND_SOCKET", "5")])).is_some());
        assert!(gate(env(&[("WAYLAND_DISPLAY", "gamescope-0"), ("XDG_CURRENT_DESKTOP", "gamescope")])).is_some());
        assert!(gate(env(&[("WAYLAND_DISPLAY", "wayland-0"), ("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0")])).is_some());
        // SteamGamepadUI alone is not a gamescope sign (coordinator's ruling).
        assert_eq!(gate(env(&[("WAYLAND_DISPLAY", "wayland-0"), ("SteamGamepadUI", "1")])), None);
    }

    #[test]
    fn a_slow_or_failing_query_falls_back_to_winits_size_within_the_limit() {
        let started = std::time::Instant::now();
        let slow = with_limit(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(3));
            Ok(vec![z13(true)])
        });
        assert!(started.elapsed() < Duration::from_secs(1), "waited {:?}", started.elapsed());
        assert_eq!(slow, Err(Failure::TimedOut(Duration::from_millis(50))));
        let (size, line) = decide(&Answer::Failed(Failure::TimedOut(Duration::from_millis(50)), started.elapsed()), Some("eDP-1"), [1920, 1200], 2.0);
        assert_eq!(size, None);
        assert!(line.contains("did not answer within 50 ms"), "{line}");

        let failed = with_limit::<Vec<Output>>(QUERY_LIMIT, || Err("connect: no compositor".into()));
        assert_eq!(failed, Err(Failure::Failed("connect: no compositor".into())));
        let (size, line) = decide(&Answer::Gated("gamescope (Game Mode)"), Some("eDP-1"), [1920, 1200], 2.0);
        assert_eq!(size, None);
        assert!(line.ends_with("untouched: gamescope (Game Mode)"), "{line}");
    }

    #[test]
    fn place_opens_the_z13_maximized_at_1472x832_with_the_true_size() {
        use crate::window::{Monitor, place};
        use winit::dpi::{PhysicalPosition, PhysicalSize};
        let (size, _) = decide(&Answer::Outputs(vec![z13(true)], MS4), Some("eDP-1"), [1920, 1200], 2.0);
        let [w, h] = size.unwrap_or_default();
        let monitor = Monitor::new(PhysicalPosition::new(0, 0), PhysicalSize::new(w, h), 2.0);
        let g = place(None, &[monitor], Some(0));
        assert!(g.maximized);
        assert_eq!(g.size, [1472.0, 832.0]);
    }
}
