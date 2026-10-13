//! Effect settings: what an `.aep` stores for an effect → the parameters of the EffectCraft
//! effect that stands for it.
//!
//! A project describes every parameter of an effect once ([`RawParamDef`]: kind, name,
//! default) and then stores, per instance, only the parameters that were touched. This module
//! pairs the project's parameters with the EffectCraft effect's **by name, in order** (both
//! lists follow Effect Controls from top to bottom), converts each value by its kind, and
//! reports every setting it could not carry over. Nothing is guessed: a value that falls
//! outside what the EffectCraft parameter accepts is reported, not clamped.
//!
//! How a stored value becomes ours, per kind (measured on real projects, see
//! [`crate::read`]):
//!
//! - sliders and angles: the number as stored; a slider shown as a percentage of a 0..255
//!   or 0..1 range (Glow Threshold, A & B Midpoint) is turned into that percentage;
//! - checkboxes: 0 or 1;
//! - pop-ups: 1-based, paired with our options by their text;
//! - colours: alpha, red, green, blue as 0..255;
//! - points: fractions of the layer's size;
//! - layer choosers: the id of a layer of the same composition;
//! - Levels, Curves and Colorama keep their numbers in a private block ([`levels`],
//!   [`curves`], [`colorama`]).

use effectcraft_effects::{EffectSpec, ParamSpec};
use effectcraft_keyframe::{Interp, Keyframe, Value};
use effectcraft_project::{ParamUi, PropGroup, Property};

use crate::read::{RawKind, RawNode, RawParamDef, RawProp, param_kind};
use crate::rifx::{f32_at, u8_at, u16_at, u32_at};

/// What became of one effect's settings.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Settings the project changed that could not be carried over, by name. While this is
    /// not empty the effect would render wrong.
    pub unread: Vec<String>,
    /// Things approximated that do not stop the effect from being shown.
    pub notes: Vec<String>,
    /// Layer choosers: our parameter and the project's id of the layer it points at, for the
    /// caller to resolve once the composition's layers exist.
    pub layer_refs: Vec<(&'static str, u32)>,
}

/// A name as compared between the two parameter lists: case, spacing, a closing colon and the
/// kind of bracket around a unit ("Time Resolution [fps]") do not count.
fn norm(s: &str) -> String {
    let s = s.trim().trim_end_matches(':').replace('[', "(").replace(']', ")");
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A group's name as compared between the two lists: letters and digits only ("Sub Settings"
/// and `subSettings` are the same group).
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// One of our parameters as the pairing sees it.
struct Ours {
    name: String,
    /// How many twirl-down groups it sits in, and the innermost one.
    depth: usize,
    group: String,
}

/// How a parameter's stored numbers become one of our values.
enum Conv {
    Scalar { scale: f64, min: f64, max: f64 },
    Bool,
    /// Our option index for each of the project's (1-based) choices.
    Enum(Vec<Option<u32>>),
    Color,
    Point([f64; 2]),
}

impl Conv {
    fn value(&self, v: &[f64]) -> Option<Value> {
        let first = v.first().copied().filter(|x| x.is_finite())?;
        match self {
            Conv::Scalar { scale, min, max } => {
                let x = first * scale;
                // A hair of tolerance for values stored at the very end of a range.
                let slack = 1e-6 * (max - min).abs().max(1.0);
                (x >= min - slack && x <= max + slack).then_some(Value::Scalar(x))
            }
            Conv::Bool => Some(Value::Bool(first != 0.0)),
            Conv::Enum(map) => {
                let i = first.round();
                if i < 1.0 {
                    return None;
                }
                map.get(i as usize - 1).copied().flatten().map(Value::Enum)
            }
            Conv::Color => {
                let c = |i: usize| v.get(i).map(|x| (x / 255.0).clamp(0.0, 1.0));
                Some(Value::Color([c(1)?, c(2)?, c(3)?, c(0)?]))
            }
            Conv::Point(size) => Some(Value::Vec2([first * size[0], v.get(1).copied().filter(|x| x.is_finite())? * size[1]])),
        }
    }
}

/// Our option for each option of a project pop-up, paired by text. Separators (`(-`) keep
/// their place in the project's numbering but have no partner.
fn popup_map(theirs: &str, ours: &[String]) -> Vec<Option<u32>> {
    let ours: Vec<String> = ours.iter().map(|o| norm(o)).collect();
    theirs.split('|').map(|t| ours.iter().position(|o| *o == norm(t)).map(|i| i as u32)).collect()
}

fn conv_for(def: &RawParamDef, ps: &ParamSpec, size: [f64; 2]) -> Option<Conv> {
    use param_kind::*;
    match (def.kind, &ps.default, &ps.ui) {
        (INT_SLIDER | FIXED_SLIDER | FLOAT_SLIDER | ANGLE, Value::Scalar(_), ui) => {
            // A slider After Effects shows as a percentage of a 0..255 or 0..1 range.
            let scale = if def.percent && def.valid_max == 255.0 {
                100.0 / 255.0
            } else if def.percent && def.valid_max > 0.0 && def.valid_max <= 1.0 {
                100.0
            } else {
                1.0
            };
            let (min, max) = match ui {
                ParamUi::Slider { min, max, .. } => (*min, *max),
                ParamUi::Percent => (-1.0e6, 1.0e6),
                _ => (f64::NEG_INFINITY, f64::INFINITY),
            };
            Some(Conv::Scalar { scale, min, max })
        }
        (CHECKBOX, Value::Bool(_), _) => Some(Conv::Bool),
        (POPUP, Value::Enum(_), ParamUi::Popup { options }) => Some(Conv::Enum(popup_map(&def.label, options))),
        (COLOR, Value::Color(_), _) => Some(Conv::Color),
        (POINT, Value::Vec2(_), _) => Some(Conv::Point(size)),
        _ => None,
    }
}

/// Whether the project changed this parameter from the effect's default.
fn changed(def: &RawParamDef, p: &RawProp) -> bool {
    if !p.keys.is_empty() || p.expression.is_some() {
        return true;
    }
    if def.kind == param_kind::PRIVATE {
        return p.blobs.first().is_some_and(|b| *b != def.default_blob);
    }
    if def.default.is_empty() {
        // A kind this reader does not understand: stored is all it knows.
        return p.value.iter().any(|v| *v != 0.0);
    }
    def.default.iter().zip(&p.value).any(|(d, v)| (d - v).abs() > 1e-4 * d.abs().max(1.0))
}

/// The option a pop-up of this (normalised) name is set to, stored or default, normalised.
fn popup_choice(defs: &[RawParamDef], params: &[RawNode], name: &str) -> Option<String> {
    let def = defs.iter().find(|d| d.kind == param_kind::POPUP && norm(&d.name) == name)?;
    let stored = stored(params, &def.match_name).filter(|p| p.keys.is_empty()).and_then(|p| p.value.first().copied());
    let choice = stored.or(def.default.first().copied())?.round();
    if !(1.0..=4096.0).contains(&choice) {
        return None;
    }
    def.label.split('|').nth(choice as usize - 1).map(norm)
}

fn stored<'a>(params: &'a [RawNode], match_name: &str) -> Option<&'a RawProp> {
    params.iter().find(|n| n.match_name == match_name).and_then(|n| match &n.kind {
        RawKind::Prop(p) => Some(p),
        _ => None,
    })
}

/// Put a stored value or its keyframes on `prop`. `false` when any of it does not convert.
fn put(prop: &mut Property, raw: &RawProp, conv: &Conv) -> bool {
    if raw.keys.is_empty() {
        let Some(v) = conv.value(&raw.value) else { return false };
        prop.value = v;
        return true;
    }
    let mut keys: Vec<Keyframe> = Vec::with_capacity(raw.keys.len());
    for k in &raw.keys {
        let Some(v) = conv.value(&k.value) else { return false };
        let (scale, spatial) = match conv {
            Conv::Scalar { scale, .. } => (*scale, false),
            // A point's tangents are fractions of the layer too; its speed along the path is
            // scaled by the mean of the two sides (not verified against a rendered move).
            Conv::Point(size) => ((size[0] + size[1]) / 2.0, true),
            _ => (1.0, false),
        };
        let mut kf = crate::import::key(k, v, scale, false);
        if spatial && let Conv::Point(size) = conv {
            let t = |t: &[f64]| [t.first().copied().unwrap_or(0.0) * size[0], t.get(1).copied().unwrap_or(0.0) * size[1], 0.0];
            if !k.tan_in.is_empty() || !k.tan_out.is_empty() {
                kf.spatial_in = t(&k.tan_in);
                kf.spatial_out = t(&k.tan_out);
                kf.spatial_auto = false;
            }
        }
        if prop.hold_only {
            kf.in_interp = Interp::Hold;
            kf.out_interp = Interp::Hold;
            kf.in_ease.clear();
            kf.out_ease.clear();
        }
        keys.push(kf);
    }
    keys.sort_by_key(|k| k.time);
    keys.dedup_by_key(|k| k.time);
    let Some(first) = keys.first() else { return false };
    prop.value = first.value.clone();
    prop.keys = keys;
    true
}

/// Carry an effect's settings from the project onto `fg`, the freshly made EffectCraft effect.
///
/// `params` are the parameters this instance stores, `defs` the project's description of the
/// effect's parameters (empty when the project holds none), `size` the layer's size in pixels.
pub(crate) fn apply(spec: &EffectSpec, fg: &mut PropGroup, effect: &str, params: &[RawNode], defs: &[RawParamDef], size: (u32, u32)) -> Outcome {
    use param_kind::*;
    let mut out = Outcome::default();
    let size = [size.0 as f64, size.1 as f64];
    if defs.is_empty() {
        // No description of the parameters: only say whether anything was stored at all.
        if params.iter().any(|n| matches!(&n.kind, RawKind::Prop(p) if !p.keys.is_empty() || !p.blobs.is_empty() || p.value.iter().any(|v| *v != 0.0))) {
            out.unread.push("all of them (the project does not describe this effect's parameters)".into());
        }
        return out;
    }
    let ours: Vec<Ours> = spec
        .params
        .iter()
        .map(|p| {
            let mut path: Vec<&str> = p.id.split('/').collect();
            path.pop();
            Ours { name: norm(p.name), depth: path.len(), group: path.last().map(|g| squash(g)).unwrap_or_default() }
        })
        .collect();
    let count_ours = |name: &str| ours.iter().filter(|o| o.name == name).count();
    let count_theirs = |name: &str| defs.iter().filter(|d| !matches!(d.kind, GROUP_START | GROUP_END) && (norm(&d.name) == name || (d.kind == CHECKBOX && norm(&d.label) == name))).count();
    let mut used = vec![false; ours.len()];
    let mut cursor = 0usize;
    // The twirl-down groups the walk is inside, outermost first.
    let mut groups: Vec<&str> = Vec::new();
    for def in defs {
        match def.kind {
            GROUP_START => {
                if groups.len() < 32 {
                    groups.push(def.name.trim());
                }
                continue;
            }
            GROUP_END => {
                groups.pop();
                continue;
            }
            _ => {}
        }
        // Parameter 0 is the effect's input; the built-in options group and buttons carry no
        // value of their own.
        if def.match_name.ends_with("-0000") || matches!(def.kind, 9 | 15 | u32::MAX) {
            continue;
        }
        let raw = stored(params, &def.match_name);
        let shown = if def.name.trim().is_empty() { def.label.trim() } else { def.name.trim() };
        let shown = if shown.is_empty() { def.match_name.as_str() } else { shown }.trim_end_matches(':').to_string();
        if def.kind == PRIVATE {
            // After the plain parameters (below): Levels shows the same numbers as sliders
            // too, and the block is the one that counts.
            continue;
        }
        // Our parameter of the same name. Names repeat (Grid has two Widths, Selective Color
        // nine Cyans), so in order of trust: the name with its group in front ("Reds Cyan");
        // the same name in the same group, forward from the last pairing first; the same
        // name at the same depth further on; a name that occurs once on both sides.
        let names = [norm(&def.name), if def.kind == CHECKBOX { norm(&def.label) } else { String::new() }];
        let (depth, group) = (groups.len(), groups.last().map(|g| squash(g)).unwrap_or_default());
        let qualified = groups.last().map(|g| norm(&format!("{g} {}", def.name))).unwrap_or_default();
        let free = |i: usize| used.get(i) == Some(&false);
        let named = |i: usize| ours.get(i).is_some_and(|o| !o.name.is_empty() && names.contains(&o.name));
        let here = |i: usize| ours.get(i).is_some_and(|o| o.depth == depth && (depth == 0 || o.group.contains(&group) || group.contains(&o.group)));
        let level = |i: usize| ours.get(i).is_some_and(|o| o.depth == depth);
        let around = || (cursor..ours.len()).chain(0..cursor.min(ours.len()));
        let found = (0..ours.len())
            .find(|i| free(*i) && !qualified.is_empty() && ours.get(*i).is_some_and(|o| o.name == qualified))
            .or_else(|| around().find(|i| free(*i) && named(*i) && here(*i)))
            .or_else(|| (cursor..ours.len()).find(|i| free(*i) && named(*i) && level(*i)))
            .or_else(|| (0..ours.len()).find(|i| free(*i) && named(*i) && ours.get(*i).is_some_and(|o| count_ours(&o.name) == 1 && count_theirs(&o.name) == 1)));
        if def.kind == LAYER {
            // A layer chooser: the layer is named by its id, which the caller resolves.
            let target = raw.map(|p| p.layer_ref).filter(|r| *r != 0);
            let partner = found.and_then(|i| spec.params.get(i).map(|ps| (i, ps))).filter(|(_, ps)| matches!(ps.default, Value::Layer(_)));
            match (partner, target) {
                (Some((i, ps)), target) => {
                    if let Some(u) = used.get_mut(i) {
                        *u = true;
                    }
                    cursor = i + 1;
                    if let Some(t) = target {
                        out.layer_refs.push((ps.id, t));
                    }
                }
                (None, Some(_)) => out.unread.push(shown),
                (None, None) => {}
            }
            continue;
        }
        let pairing = found.and_then(|i| spec.params.get(i).map(|ps| (i, ps))).and_then(|(i, ps)| conv_for(def, ps, size).map(|c| (i, ps, c)));
        let Some((i, ps, conv)) = pairing else {
            if raw.is_some_and(|p| changed(def, p)) {
                out.unread.push(shown);
            }
            continue;
        };
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        cursor = i + 1;
        let Some(prop) = fg.prop_mut(ps.id) else { continue };
        match raw {
            Some(p) => {
                // A parameter stored at its default needs no reading, so one that does not
                // convert is only reported when it was changed.
                let mut trial = prop.clone();
                if put(&mut trial, p, &conv) {
                    *prop = trial;
                } else if changed(def, p) {
                    out.unread.push(shown);
                }
            }
            // Not stored: After Effects' default, which is not always ours.
            None => {
                if let Some(v) = conv.value(&def.default) {
                    prop.value = v;
                }
            }
        }
    }
    for def in defs.iter().filter(|d| d.kind == PRIVATE) {
        let Some(raw) = stored(params, &def.match_name) else { continue };
        let done = match effect {
            "ADBE Easy Levels2" | "ADBE Easy Levels" => Some(levels(fg, raw, &mut out)),
            "ADBE CurvesCustom" => Some(curves(fg, raw, &mut out)),
            "APC Colorama" if norm(&def.name) == "output cycle" => Some(colorama(fg, raw, &mut out)),
            _ => None,
        };
        let shown = if def.name.trim().is_empty() { def.match_name.as_str() } else { def.name.trim() };
        if done != Some(true) && changed(def, raw) {
            out.unread.push(shown.to_string());
        }
    }
    // CC Spotlight always stores a Gel Layer (its own layer when none was picked), and only
    // its "Gel …" render modes look at it.
    if effect == "CC Spotlight" && !popup_choice(defs, params, "render").is_some_and(|r| r.starts_with("gel")) {
        out.unread.retain(|u| norm(u) != "gel layer");
    }
    // Effect opacity and effect masks (After Effects CC 2014 and later).
    let options = params.iter().find(|n| n.match_name == "ADBE Effect Built In Params").and_then(|n| match &n.kind {
        RawKind::Group(g) => Some(g),
        _ => None,
    });
    if options.is_some_and(|g| g.iter().any(|n| matches!(&n.kind, RawKind::Prop(p) if !p.keys.is_empty() || n.match_name == "ADBE Effect Mask Parade") || matches!(&n.kind, RawKind::Group(g) if !g.is_empty()))) {
        out.notes.push("its Compositing Options (effect opacity, effect masks) are not imported".into());
    }
    out
}

const LEVELS_HEAD: usize = 8;
const LEVELS_LEN: usize = LEVELS_HEAD + 25 * 4;

/// Our Levels parameter for control `k` (input black, input white, gamma, output black,
/// output white) of channel `ch` (RGB, red, green, blue, alpha).
fn levels_id(ch: usize, k: usize) -> Option<String> {
    let plain = ["inBlack", "inWhite", "gamma", "outBlack", "outWhite"].get(k)?;
    let pre = ["", "red", "green", "blue", "alpha"].get(ch)?;
    if pre.is_empty() {
        return Some((*plain).to_string());
    }
    let mut c = plain.chars();
    let head = c.next()?.to_ascii_uppercase();
    Some(format!("{pre}{head}{}", c.as_str()))
}

/// Levels keeps its numbers in the Histogram parameter's private block: 108 bytes, an 8-byte
/// head and then, for RGB, red, green, blue and alpha in turn, five f32: input black, input
/// white, gamma, output black, output white, as fractions (1.0 = white). Measured on projects
/// saved by After Effects CS6 to 2025: an untouched Levels reads 0, 1, 1, 0, 1 five times,
/// and a keyframed one differs between its blocks only in the numbers that were animated.
fn levels(fg: &mut PropGroup, raw: &RawProp, out: &mut Outcome) -> bool {
    if raw.blobs.is_empty() || raw.blobs.iter().any(|b| b.len() != LEVELS_LEN) {
        return false;
    }
    let number = |b: &[u8], n: usize| f32_at(b, LEVELS_HEAD + 4 * n);
    let keyed = raw.keys.len() > 1 && raw.keys.len() == raw.blobs.len();
    if raw.keys.len() > 1 && !keyed {
        out.notes.push("its keyframes do not line up with their values: the first values are used".into());
    }
    for ch in 0..5 {
        for k in 0..5 {
            let n = ch * 5 + k;
            let Some(first) = raw.blobs.first().map(|b| number(b, n)) else { return false };
            let Some(prop) = levels_id(ch, k).and_then(|id| fg.prop_mut(&id)) else { return false };
            prop.value = Value::Scalar(first);
            if keyed && raw.blobs.iter().any(|b| number(b, n) != first) {
                let mut keys: Vec<Keyframe> = raw
                    .keys
                    .iter()
                    .zip(&raw.blobs)
                    .map(|(key, b)| {
                        let mut kf = Keyframe::new(crate::import::tick(key.time), Value::Scalar(number(b, n)));
                        // The block carries no ease of its own: hold stays hold, the rest
                        // runs straight from value to value.
                        let i = |n: u8| if n == 3 { Interp::Hold } else { Interp::Linear };
                        kf.in_interp = i(key.in_interp);
                        kf.out_interp = i(key.out_interp);
                        kf
                    })
                    .collect();
                keys.sort_by_key(|k| k.time);
                keys.dedup_by_key(|k| k.time);
                prop.keys = keys;
            }
        }
    }
    if keyed && raw.keys.iter().any(|k| k.in_interp == 2 || k.out_interp == 2) {
        out.notes.push("its keyframes' easing is not imported (the values change at an even rate)".into());
    }
    true
}

const CURVE_TABLES: usize = 4 + 5 * 256;
const CURVE_BLOCK: usize = 16 * 4 + 8;
const CURVES_LEN: usize = CURVE_TABLES + 5 * CURVE_BLOCK;

/// Curves keeps its curves in a private block of 1 644 bytes: a 4-byte head, five 256-entry
/// lookup tables (RGB, red, green, blue, alpha), then for each of the five a block of up to 16
/// points (x and y as u16, 0..255) followed by the number of points as u32 and one more u32.
/// Measured on projects saved by After Effects CS6 to 2025: an untouched curve is the two
/// points 0,0 and 255,255 over an identity table, and a curve with one point pulled up shows
/// that point between them. Our Curves draws its own curve through the same points.
fn curves(fg: &mut PropGroup, raw: &RawProp, out: &mut Outcome) -> bool {
    let Some(b) = raw.blobs.first().filter(|b| b.len() == CURVES_LEN) else { return false };
    let mut all = Vec::with_capacity(5);
    for ch in 0..5 {
        let at = CURVE_TABLES + ch * CURVE_BLOCK;
        let count = u32_at(b, at + 64) as usize;
        if !(2..=16).contains(&count) {
            return false;
        }
        let pts: Vec<String> = (0..count).map(|i| format!("{},{}", fmt(u16_at(b, at + 4 * i)), fmt(u16_at(b, at + 4 * i + 2)))).collect();
        all.push(pts.join(" "));
    }
    for (id, pts) in ["rgb", "red", "green", "blue", "alpha"].iter().zip(all) {
        let Some(prop) = fg.prop_mut(id) else { return false };
        prop.value = Value::Str(pts);
    }
    if raw.blobs.len() > 1 {
        out.notes.push("its curve is keyframed: the first curve is used throughout".into());
    }
    true
}

/// A 0..255 curve coordinate as a fraction, written short.
fn fmt(v: u16) -> String {
    short(v.min(255) as f64 / 255.0)
}

/// A number to four decimals, without trailing zeros.
fn short(v: f64) -> String {
    let x = (v * 10_000.0).round() / 10_000.0;
    format!("{x}")
}

const CYCLE_STOPS: usize = 64;
const CYCLE_LEN: usize = CYCLE_STOPS * 8 + 8;

/// Colorama keeps its Output Cycle in a private block of 520 bytes: room for 64 colour stops
/// of 8 bytes (the position around the wheel as u32, 0x10000 = once around; then alpha, red,
/// green, blue as bytes), and the number of stops in use as u32 @512. Stops past that number
/// hold stale bytes. Measured on projects saved by After Effects CS6 to 2025: an untouched
/// Colorama holds the six stops of the hue wheel at sixths of the circle, and a layer named
/// for taking the colour out holds two, black at 0 and white at the end. The cycle arrives as
/// our Custom palette.
fn colorama(fg: &mut PropGroup, raw: &RawProp, out: &mut Outcome) -> bool {
    let Some(b) = raw.blobs.first().filter(|b| b.len() == CYCLE_LEN) else { return false };
    let count = u32_at(b, CYCLE_STOPS * 8) as usize;
    if !(1..=CYCLE_STOPS).contains(&count) {
        return false;
    }
    let stops: Vec<String> = (0..count)
        .map(|i| {
            let at = 8 * i;
            // The wheel closes on itself, so a stop at the very end sits just before it.
            let pos = (u32_at(b, at).min(0x1_0000) as f64 / 65_536.0).min(0.999);
            let c = |k: usize| short(u8_at(b, at + 4 + k) as f64 / 255.0);
            if u8_at(b, at + 4) == 255 { format!("{}:{},{},{}", short(pos), c(1), c(2), c(3)) } else { format!("{}:{},{},{},{}", short(pos), c(1), c(2), c(3), c(0)) }
        })
        .collect();
    let custom = match fg.prop("outputCycle/usePresetPalette").map(|p| &p.ui) {
        Some(ParamUi::Popup { options }) => options.iter().position(|o| o == "Custom"),
        _ => None,
    };
    let Some(custom) = custom else { return false };
    let Some(preset) = fg.prop_mut("outputCycle/usePresetPalette") else { return false };
    preset.value = Value::Enum(custom as u32);
    let Some(palette) = fg.prop_mut("outputCycle/palette") else { return false };
    palette.value = Value::Str(stops.join(" "));
    if raw.blobs.len() > 1 {
        out.notes.push("its Output Cycle is keyframed: the first cycle is used throughout".into());
    }
    true
}
