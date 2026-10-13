//! Ours (self-logging, not upstream): the GPU compositor's hand-overs to the CPU, logged as
//! `decision: gpu: …` once per (layer kind, reason) per process (contract C). The walk runs every
//! frame, so a hand-over is said once, not per frame: craft-log's repeat collapse alone would not
//! hold it, because other lines interleave. The first occurrence carries the layer and the sizes.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};

use effectcraft_color::BlendMode;
use effectcraft_project::{Layer, LayerSource};

/// The reasons the walk names (one line per call site there).
pub(crate) const ADV_3D: &str = "Advanced 3D: the GPU renderer declined the run";
pub(crate) const SPLIT_3D: &str = "Advanced 3D with 2D-composited layers: the GPU split path declined the run";
pub(crate) const CLASSIC_3D: &str = "Classic 3D: the run cannot be prepared for the GPU kernel, or the kernel declined it";
pub(crate) const PRECOMP_MASKED: &str = "the precomp layer has masks or layer styles";
pub(crate) const PRECOMP_CPU_PATH: &str =
    "it needs the CPU path (a cycle, content times, a proxy, essential overrides, frame blending, or a time outside the nested comp)";
pub(crate) const PRECOMP_NOT_RENDERED: &str = "the GPU could not render the nested comp (the line before says why, when it is the size or the depth)";

static SAID: Mutex<Option<HashSet<(&'static str, &'static str)>>> = Mutex::new(None);

/// Whether (`kind`, `reason`) is new in this process (and remember it).
fn first(kind: &'static str, reason: &'static str) -> bool {
    SAID.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_with(HashSet::new).insert((kind, reason))
}

/// `kind` is drawn on the CPU (read back, drawn, uploaded) for `reason`; `detail` (the layer,
/// the sizes) is formatted only the first time.
pub(crate) fn on_cpu(kind: &'static str, reason: &'static str, detail: impl FnOnce() -> String) {
    if first(kind, reason) {
        log::warn!("decision: gpu: {kind} drawn on the CPU: {reason} ({}; said once per run)", detail());
    }
}

fn layer(l: &Layer) -> String {
    format!("first seen: layer \"{}\"", l.name)
}

/// The whole frame (or a nested comp) left to the CPU before the walk starts.
pub(crate) fn frame(depth: usize, w: u32, h: u32, max_dim: u32) {
    if depth > 16 {
        on_cpu("comp", "nested deeper than 16 comps", || format!("depth {depth}"));
    } else {
        on_cpu("comp", "bigger than the GPU's largest texture", || format!("{w}x{h} pixels, the limit {max_dim}x{max_dim}, nesting depth {depth}"));
    }
}

/// An adjustment layer `draw_adjustment` declined.
pub(crate) fn adjustment(l: &Layer) {
    if l.blend_mode != BlendMode::Normal {
        let why = "its blending mode is not Normal (the GPU's adjust mix is Normal only)";
        on_cpu("adjustment layer", why, || format!("{}, mode {:?}", layer(l), l.blend_mode));
    } else {
        on_cpu("adjustment layer", "a GPU step declined it", || layer(l));
    }
}

/// A run of 3D layers the GPU kernel declined (`what`: which 3D path).
pub(crate) fn run_3d(what: &'static str, run: &[&Layer], w: u32, h: u32) {
    on_cpu("3D run", what, || format!("{} layers from \"{}\", canvas {w}x{h}", run.len(), run.first().map_or("", |l| l.name.as_str())));
}

/// A precomp layer whose nested comp is rendered by the CPU compositor and uploaded every frame
/// instead of resident on the GPU. Only precomp layers are named (the walk asks every layer).
pub(crate) fn precomp(l: &Layer, reason: &'static str) {
    if matches!(l.source, LayerSource::Comp { .. }) && !l.switches.adjustment {
        on_cpu("precomp layer's nested comp", reason, || layer(l));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_and_reason_is_said_once() {
        assert!(first("test kind", "reason a"));
        assert!(!first("test kind", "reason a"));
        assert!(first("test kind", "reason b"));
        assert!(first("other kind", "reason a"));
        assert!(!first("other kind", "reason a"));
    }
}
