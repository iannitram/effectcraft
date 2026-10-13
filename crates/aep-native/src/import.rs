//! [`crate::read::AepFile`] → project items, compositions and layers.

use std::collections::BTreeMap;
use std::sync::Arc;

use effectcraft_color::{BlendMode, Label};
use effectcraft_keyframe::{Ease, Interp, Keyframe, ShapePath, Value};
use effectcraft_project::build::{self, Ids};
use effectcraft_project::{
    Comp, Footage, FootageKind, FrameBlend, GroupKind, ItemId, ItemKind, Layer, LayerId, LayerSource, LightKind, MaskMode, MatteKind, Node, ParamUi, Project,
    PropGroup, Property, Quality, Solid, TrackMatte,
};
use effectcraft_time::{FrameRate, Tick};

use crate::read::{AepFile, Rational, RawComp, RawFootage, RawItemKind, RawKey, RawKind, RawLayer, RawLayerKind, RawNode, RawParamDef, RawProp, RawShape};
use crate::{AepError, effects_table, rifx, settings};

/// A linked file the project names, for the caller to find and describe.
#[derive(Clone, Debug)]
pub struct FootageRef<'a> {
    /// The item's name in the Project panel (often empty: After Effects shows the file name).
    pub name: &'a str,
    /// The file as the project recorded it: a path on the machine that saved it.
    pub path: Option<&'a str>,
    pub width: u32,
    pub height: u32,
    pub duration: Tick,
    /// Frames per second (0 for stills and audio).
    pub fps: f64,
    /// After Effects' four-character code for the file type.
    pub kind: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct ImportResult {
    /// Every item created, in creation order.
    pub items: Vec<ItemId>,
    pub comps: Vec<ItemId>,
    /// The composition no other composition uses, with the most layers: the one to show.
    pub main_comp: Option<ItemId>,
    /// What was dropped or approximated, most frequent first. Nothing is dropped silently.
    pub warnings: Vec<String>,
    pub layers: usize,
    pub missing_footage: usize,
}

#[derive(Default)]
struct Warnings(BTreeMap<String, usize>);

impl Warnings {
    fn add(&mut self, w: impl Into<String>) {
        *self.0.entry(w.into()).or_insert(0) += 1;
    }
    fn finish(self) -> Vec<String> {
        let mut v: Vec<(String, usize)> = self.0.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().map(|(w, n)| if n > 1 { format!("{w} (×{n})") } else { w }).collect()
    }
}

pub(crate) fn tick(r: Rational) -> Tick {
    Tick::from_units(r.value, r.scale.max(1))
}

fn label(n: u8) -> Label {
    Label::ALL.get(n as usize).copied().unwrap_or_default()
}

/// After Effects' blend mode numbers. Only the modes this reader has seen in real projects are
/// marked sure; the rest follow the order of the Mode menu and are reported when used.
fn blend(n: u32) -> Option<(BlendMode, bool)> {
    Some(match n {
        2 => (BlendMode::Normal, true),
        3 => (BlendMode::Dissolve, false),
        4 => (BlendMode::Add, true),
        5 => (BlendMode::Multiply, true),
        6 => (BlendMode::Screen, true),
        7 => (BlendMode::Overlay, true),
        8 => (BlendMode::SoftLight, true),
        9 => (BlendMode::HardLight, false),
        10 => (BlendMode::Darken, false),
        11 => (BlendMode::Lighten, false),
        12 => (BlendMode::ClassicDifference, false),
        13 => (BlendMode::Hue, false),
        14 => (BlendMode::Saturation, false),
        15 => (BlendMode::Color, true),
        16 => (BlendMode::Luminosity, false),
        17 => (BlendMode::StencilAlpha, false),
        18 => (BlendMode::StencilLuma, false),
        19 => (BlendMode::SilhouetteAlpha, false),
        20 => (BlendMode::SilhouetteLuma, false),
        21 => (BlendMode::LuminescentPremul, false),
        22 => (BlendMode::AlphaAdd, false),
        23 => (BlendMode::ClassicColorDodge, true),
        24 => (BlendMode::ClassicColorBurn, false),
        25 => (BlendMode::Exclusion, false),
        26 => (BlendMode::Difference, false),
        27 => (BlendMode::ColorDodge, false),
        28 => (BlendMode::ColorBurn, false),
        29 => (BlendMode::LinearDodge, true),
        30 => (BlendMode::LinearBurn, false),
        31 => (BlendMode::LinearLight, false),
        32 => (BlendMode::VividLight, false),
        33 => (BlendMode::PinLight, false),
        34 => (BlendMode::HardMix, false),
        35 => (BlendMode::LighterColor, false),
        36 => (BlendMode::DarkerColor, false),
        37 => (BlendMode::Subtract, false),
        38 => (BlendMode::Divide, false),
        _ => return None,
    })
}

fn interp(n: u8) -> Interp {
    match n {
        2 => Interp::Bezier,
        3 => Interp::Hold,
        _ => Interp::Linear,
    }
}

fn vec3(v: &[f64], k: f64) -> Option<Value> {
    let g = |i: usize| v.get(i).copied().unwrap_or(0.0) * k;
    (v.len() >= 2).then(|| Value::Vec3([g(0), g(1), if v.len() >= 3 { g(2) } else { 0.0 }]))
}

fn vec2(v: &[f64], k: f64) -> Option<Value> {
    Some(Value::Vec2([v.first()? * k, v.get(1).copied().unwrap_or(*v.first()?) * k]))
}

fn scalar(v: &[f64], k: f64) -> Option<Value> {
    v.first().map(|x| Value::Scalar(x * k))
}

/// A keyframe's value and, scaled the same way, its speeds and spatial tangents.
pub(crate) fn key(k: &RawKey, value: Value, scale: f64, spatial: bool) -> Keyframe {
    let mut kf = Keyframe::new(tick(k.time), value);
    kf.in_interp = interp(k.in_interp);
    kf.out_interp = interp(k.out_interp);
    let eases = |speed: &[f64], influence: &[f64]| -> Vec<Ease> {
        speed.iter().zip(influence).map(|(s, i)| Ease { speed: s * scale, influence: i.clamp(0.0, 1.0) }).collect()
    };
    if kf.in_interp == Interp::Bezier {
        kf.in_ease = eases(&k.in_speed, &k.in_influence);
    }
    if kf.out_interp == Interp::Bezier {
        kf.out_ease = eases(&k.out_speed, &k.out_influence);
    }
    if spatial && (!k.tan_in.is_empty() || !k.tan_out.is_empty()) {
        let t3 = |t: &[f64]| [t.first().copied().unwrap_or(0.0) * scale, t.get(1).copied().unwrap_or(0.0) * scale, t.get(2).copied().unwrap_or(0.0) * scale];
        kf.spatial_in = t3(&k.tan_in);
        kf.spatial_out = t3(&k.tan_out);
        kf.spatial_auto = false;
    }
    kf
}

/// What an After Effects mask feather is multiplied by on its way in. EffectCraft blurs a
/// mask's edge with a Gaussian whose sigma is half the feather; After Effects' edge is
/// narrower for the same number. Measured on one mask only, a full-frame vignette (feather
/// 546 px, three expansions) against its export at four times: the pictures agree best with
/// the feather between 73% and 79% of its number, and differ three times as much at 100%.
/// One mask is thin evidence: the import list names the scaling, and this constant is the
/// only place to undo it.
const MASK_FEATHER_SCALE: f64 = 0.76;

/// Put a file property's value or keyframes on `prop`. `scale` converts the file's unit to
/// ours (After Effects stores percentages as fractions).
fn apply(prop: &mut Property, raw: &RawProp, scale: f64, conv: &dyn Fn(&[f64], f64) -> Option<Value>) -> bool {
    if raw.keys.is_empty() {
        let Some(v) = conv(&raw.value, scale) else { return false };
        prop.value = v;
        return true;
    }
    let mut keys: Vec<Keyframe> = raw.keys.iter().filter_map(|k| conv(&k.value, scale).map(|v| key(k, v, scale, raw.spatial))).collect();
    if keys.is_empty() {
        return false;
    }
    keys.sort_by_key(|k| k.time);
    keys.dedup_by_key(|k| k.time);
    if let Some(first) = keys.first() {
        prop.value = first.value.clone();
    }
    prop.keys = keys;
    true
}

struct Ctx<'a, 'f> {
    warn: Warnings,
    /// The project's description of each effect's parameters, by the effect's match name. A
    /// project stores it with the first instance of an effect only.
    defs: BTreeMap<&'f str, &'f [RawParamDef]>,
    /// After Effects item id → ours.
    items: BTreeMap<u32, ItemId>,
    created: Vec<ItemId>,
    locate: &'a mut dyn FnMut(&FootageRef) -> Option<Footage>,
    layers: usize,
    missing: usize,
    /// Comps used as a layer of another comp.
    nested: Vec<ItemId>,
}

/// Read an `.aep` file into `project`: every folder, composition, solid and linked file of the
/// After Effects project becomes a project item. `locate` finds a linked file on this machine
/// and describes it; when it returns `None` the item is created as missing footage with the
/// size and duration the project recorded.
pub fn import(project: &mut Project, bytes: &[u8], locate: &mut dyn FnMut(&FootageRef) -> Option<Footage>) -> Result<ImportResult, AepError> {
    let (root, _xmp) = rifx::parse(bytes)?;
    let file: AepFile = crate::read::read(&root);
    if file.items.is_empty() && root.list(b"Fold").is_none() {
        return Err(AepError::NotAep("no project folder found in the file".into()));
    }
    let mut defs = BTreeMap::new();
    for it in &file.items {
        if let RawItemKind::Comp(c) = &it.kind {
            for l in &c.layers {
                effect_defs(&l.props, 0, &mut defs);
            }
        }
    }
    let mut cx = Ctx { warn: Warnings::default(), defs, items: BTreeMap::new(), created: vec![], locate, layers: 0, missing: 0, nested: vec![] };
    // Pass 1: every item, compositions still empty so layers can refer to any of them.
    for it in &file.items {
        let parent = cx.items.get(&it.parent).copied();
        let id = match &it.kind {
            RawItemKind::Folder => project.add_item(&it.name, Label::Yellow, parent, ItemKind::Folder),
            RawItemKind::Comp(c) => project.add_item(&it.name, Label::Sandstone, parent, ItemKind::Comp(Arc::new(empty_comp(c)))),
            RawItemKind::Footage(f) => match &f.solid {
                Some(s) => {
                    let name = if it.name.is_empty() { s.name.as_str() } else { it.name.as_str() };
                    let solid = Solid { color: s.color, width: f.width.max(1), height: f.height.max(1), pixel_aspect: 1.0 };
                    project.add_item(name, Label::Red, parent, ItemKind::Solid(solid))
                }
                None => {
                    let (name, footage) = footage(&mut cx, &it.name, f);
                    project.add_item(&name, Label::Aqua, parent, ItemKind::Footage(footage))
                }
            },
            RawItemKind::Unknown(ty) => {
                cx.warn.add(format!("project item of unknown type {ty} skipped"));
                continue;
            }
        };
        cx.items.insert(it.id, id);
        cx.created.push(id);
    }
    // Pass 2: the layers of each composition.
    let mut comps = vec![];
    for it in &file.items {
        let RawItemKind::Comp(raw) = &it.kind else { continue };
        let Some(id) = cx.items.get(&it.id).copied() else { continue };
        let Some(mut comp) = project.comp(id).cloned() else { continue };
        build_layers(&mut cx, project, &mut comp, raw, id);
        if let Some(item) = project.item_mut(id) {
            item.kind = ItemKind::Comp(Arc::new(comp));
        }
        comps.push(id);
    }
    project.fix_next_id();
    let main_comp = comps
        .iter()
        .copied()
        .filter(|c| !cx.nested.contains(c))
        .max_by_key(|c| project.comp(*c).map(|c| (c.layers.len(), c.duration)).unwrap_or_default())
        .or(comps.first().copied());
    Ok(ImportResult { items: cx.created, comps, main_comp, warnings: cx.warn.finish(), layers: cx.layers, missing_footage: cx.missing })
}

/// Every effect's parameter description found below `nodes`, the first one per effect.
fn effect_defs<'f>(nodes: &'f [RawNode], depth: usize, out: &mut BTreeMap<&'f str, &'f [RawParamDef]>) {
    if depth > 64 {
        return;
    }
    for n in nodes {
        match &n.kind {
            RawKind::Effect { defs, .. } if !defs.is_empty() => {
                out.entry(n.match_name.as_str()).or_insert(defs.as_slice());
            }
            RawKind::Group(g) => effect_defs(g, depth + 1, out),
            _ => {}
        }
    }
}

fn empty_comp(c: &RawComp) -> Comp {
    let fps = if c.frame.value > 0 && c.frame.scale > 0 { c.frame.scale as f64 / c.frame.value as f64 } else { 30.0 };
    let duration = tick(c.duration).max(Tick::from_units(1, 1000));
    let mut comp = Comp::new(c.width.clamp(1, 30_000), c.height.clamp(1, 30_000), FrameRate::from_f64(fps.clamp(1.0, 999.0)), duration);
    if c.pixel_aspect.0 > 0 && c.pixel_aspect.1 > 0 {
        comp.pixel_aspect = c.pixel_aspect.0 as f64 / c.pixel_aspect.1 as f64;
    }
    let (wi, wo) = (tick(c.work_in), tick(c.work_out));
    // An out point before the in point means "to the end of the composition".
    comp.work_area = (wi.clamp(Tick::ZERO, duration), if wo > wi { wo.min(duration) } else { duration });
    comp
}

/// The project's own description of a linked file, used when the file is not found.
fn footage(cx: &mut Ctx, item_name: &str, f: &RawFootage) -> (String, Footage) {
    let file_name = f.path.as_deref().map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()).unwrap_or_default();
    let name = if !item_name.is_empty() {
        item_name.to_string()
    } else if !file_name.is_empty() {
        file_name.clone()
    } else {
        "Missing footage".to_string()
    };
    let duration = tick(f.duration);
    let r = FootageRef { name: &name, path: f.path.as_deref(), width: f.width, height: f.height, duration, fps: f.fps, kind: f.kind };
    if let Some(found) = (cx.locate)(&r) {
        return (name, found);
    }
    cx.missing += 1;
    cx.warn.add(format!("linked file not found: {}", f.path.as_deref().unwrap_or("(the project names no file)")));
    let audio_only = f.width == 0 || f.height == 0;
    let kind = if audio_only {
        FootageKind::Audio
    } else if f.fps <= 0.0 || duration == Tick::ZERO {
        FootageKind::Still
    } else {
        FootageKind::Video
    };
    let placeholder = Footage {
        path: f.path.clone().unwrap_or_default(),
        kind,
        width: f.width.max(1),
        height: f.height.max(1),
        frame_rate: if f.fps > 0.0 { FrameRate::from_f64(f.fps.clamp(1.0, 999.0)) } else { FrameRate::default() },
        duration,
        has_video: !audio_only,
        has_audio: audio_only,
        missing: true,
        ..Default::default()
    };
    (name, placeholder)
}

fn source(cx: &mut Ctx, project: &Project, comp: &Comp, l: &RawLayer) -> (LayerSource, (u32, u32)) {
    let full = (comp.width, comp.height);
    match l.kind {
        RawLayerKind::Text => return (LayerSource::Text, full),
        RawLayerKind::Shape => return (LayerSource::Shape, full),
        RawLayerKind::Camera => return (LayerSource::Camera, full),
        RawLayerKind::Light => return (LayerSource::Light { kind: LightKind::default() }, full),
        RawLayerKind::Unknown(n) => {
            cx.warn.add(format!("layer of unknown type {n} imported as a null"));
            return (LayerSource::Null, (100, 100));
        }
        RawLayerKind::Av => {}
    }
    let Some(item) = cx.items.get(&l.source).copied() else {
        if !l.null {
            cx.warn.add("layer whose source is not in the project imported as a null");
        }
        return (LayerSource::Null, (100, 100));
    };
    match project.item(item).map(|i| &i.kind) {
        Some(ItemKind::Comp(c)) => {
            cx.nested.push(item);
            (LayerSource::Comp { item }, (c.width, c.height))
        }
        Some(ItemKind::Solid(s)) if l.null => (LayerSource::Null, (s.width, s.height)),
        Some(ItemKind::Solid(s)) => (LayerSource::Solid { item }, (s.width, s.height)),
        Some(ItemKind::Footage(f)) => (LayerSource::Footage { item }, (f.width, f.height)),
        _ => (LayerSource::Null, (100, 100)),
    }
}

/// How many properties below carry an expression.
fn expressions(nodes: &[RawNode], depth: usize) -> usize {
    if depth > 64 {
        return 0;
    }
    nodes
        .iter()
        .map(|n| match &n.kind {
            RawKind::Prop(p) | RawKind::Shape { prop: p, .. } => usize::from(p.expression.is_some()),
            RawKind::Group(g) | RawKind::Effect { params: g, .. } | RawKind::Mask { props: g, .. } => expressions(g, depth + 1),
            _ => 0,
        })
        .sum()
}

fn group<'a>(nodes: &'a [RawNode], match_name: &str) -> Option<&'a [RawNode]> {
    nodes.iter().find(|n| n.match_name == match_name).and_then(|n| match &n.kind {
        RawKind::Group(g) => Some(g.as_slice()),
        _ => None,
    })
}

fn transform(cx: &mut Ctx, raw: &[RawNode], tr: &mut PropGroup, three_d: bool) {
    let mut separated = false;
    let mut has_position = false;
    for n in raw {
        let RawKind::Prop(p) = &n.kind else {
            if n.match_name == "ADBE Orientation" && three_d {
                cx.warn.add("3D layer: Orientation not imported");
            }
            continue;
        };
        let (id, scale, conv): (&str, f64, &dyn Fn(&[f64], f64) -> Option<Value>) = match n.match_name.as_str() {
            "ADBE Anchor Point" => ("anchor", 1.0, &vec3),
            "ADBE Position" => ("position", 1.0, &vec3),
            "ADBE Scale" => ("scale", 100.0, &vec3),
            "ADBE Rotate Z" => ("rotation", 1.0, &scalar),
            "ADBE Rotate X" => ("rotationX", 1.0, &scalar),
            "ADBE Rotate Y" => ("rotationY", 1.0, &scalar),
            "ADBE Opacity" => ("opacity", 100.0, &scalar),
            "ADBE Position_0" | "ADBE Position_1" | "ADBE Position_2" => {
                separated |= !p.keys.is_empty() || p.value.first().is_some_and(|v| *v != 0.0);
                continue;
            }
            _ => continue,
        };
        has_position |= id == "position";
        // Cameras and lights keep the point of interest where other layers keep the anchor.
        let target = if id == "anchor" && tr.get("anchor").is_none() { "poi" } else { id };
        if let Some(prop) = tr.get_mut(target) {
            apply(prop, p, scale, conv);
        }
    }
    if separated && !has_position {
        cx.warn.add("separated X/Y/Z position not imported (the layer keeps the default position)");
    } else if separated {
        cx.warn.add("separated X/Y/Z position values ignored (Position used)");
    }
}

fn shape_path(s: &RawShape, size: (u32, u32)) -> ShapePath {
    let (w, h) = (size.0 as f64, size.1 as f64);
    let at = |p: &[f64; 2]| -> [f64; 2] {
        [(s.top_left[0] + p[0] * (s.bottom_right[0] - s.top_left[0])) * w, (s.top_left[1] + p[1] * (s.bottom_right[1] - s.top_left[1])) * h]
    };
    let pts: Vec<[f64; 2]> = s.points.iter().map(at).collect();
    let n = pts.len().div_ceil(3);
    let mut path = ShapePath { closed: s.closed, ..Default::default() };
    for i in 0..n {
        let Some(v) = pts.get(3 * i).copied() else { break };
        path.vertices.push(v);
        path.in_tangents.push([0.0, 0.0]);
        path.out_tangents.push([0.0, 0.0]);
    }
    for i in 0..n {
        let Some(v) = path.vertices.get(i).copied() else { break };
        if let (Some(c), Some(o)) = (pts.get(3 * i + 1), path.out_tangents.get_mut(i)) {
            *o = [c[0] - v[0], c[1] - v[1]];
        }
        let next = (i + 1) % n.max(1);
        if let (Some(c), Some(nv)) = (pts.get(3 * i + 2), path.vertices.get(next).copied())
            && let Some(t) = path.in_tangents.get_mut(next)
        {
            *t = [c[0] - nv[0], c[1] - nv[1]];
        }
    }
    path
}

fn masks(cx: &mut Ctx, ids: &mut Ids, raw: &[RawNode], g: &mut PropGroup, size: (u32, u32)) {
    for (i, n) in raw.iter().enumerate() {
        let RawKind::Mask { inverted, locked, mode, props } = &n.kind else { continue };
        let mode = match mode {
            0 => MaskMode::None,
            2 => MaskMode::Subtract,
            3 => MaskMode::Intersect,
            4 => MaskMode::Darken,
            5 => MaskMode::Lighten,
            6 => MaskMode::Difference,
            _ => MaskMode::Add,
        };
        let name = if n.name.is_empty() { format!("Mask {}", i + 1) } else { n.name.clone() };
        let color = build::MASK_COLORS.get(i % build::MASK_COLORS.len()).copied().unwrap_or([0xb1, 0xb4, 0x4f]);
        let mut mg = build::mask(ids, &name, ShapePath::default(), mode, color);
        if let GroupKind::Mask { inverted: inv, locked: lock, .. } = &mut mg.kind {
            *inv = *inverted;
            *lock = *locked;
        }
        for p in props {
            match (&p.kind, p.match_name.as_str()) {
                (RawKind::Shape { prop, shapes }, "ADBE Mask Shape") => {
                    let Some(target) = mg.get_mut("path") else { continue };
                    if prop.keys.len() > 1 && prop.keys.len() == shapes.len() {
                        let mut keys: Vec<Keyframe> = prop.keys.iter().zip(shapes).map(|(k, s)| key(k, Value::Path(shape_path(s, size)), 1.0, false)).collect();
                        keys.sort_by_key(|k| k.time);
                        keys.dedup_by_key(|k| k.time);
                        if let Some(first) = keys.first() {
                            target.value = first.value.clone();
                        }
                        target.keys = keys;
                    } else if let Some(s) = shapes.first() {
                        target.value = Value::Path(shape_path(s, size));
                        if prop.keys.len() > 1 {
                            cx.warn.add("animated mask path imported as its first shape only");
                        }
                    }
                }
                (RawKind::Prop(rp), "ADBE Mask Feather") => {
                    if let Some(t) = mg.get_mut("feather") {
                        apply(t, rp, MASK_FEATHER_SCALE, &vec2);
                        let feathered = |v: &Value| matches!(v, Value::Vec2(f) if f[0] != 0.0 || f[1] != 0.0);
                        if feathered(&t.value) || t.keys.iter().any(|k| feathered(&k.value)) {
                            cx.warn.add(format!(
                                "mask feather scaled to {:.0}% of its number: EffectCraft's feather falls off wider than After Effects' for the same number",
                                MASK_FEATHER_SCALE * 100.0
                            ));
                        }
                    }
                }
                (RawKind::Prop(rp), "ADBE Mask Opacity") => {
                    if let Some(t) = mg.get_mut("opacity") {
                        apply(t, rp, 100.0, &scalar);
                    }
                }
                (RawKind::Prop(rp), "ADBE Mask Offset") => {
                    if let Some(t) = mg.get_mut("expansion") {
                        apply(t, rp, 1.0, &scalar);
                    }
                }
                _ => {}
            }
        }
        g.children.push(Node::Group(mg));
    }
}

/// A layer chooser of an imported effect, to point at its layer once all layers exist: the
/// effect's group, the parameter, the project's id of the layer, the effect's name.
type LayerChoice = (u64, &'static str, u32, String);

fn effects(cx: &mut Ctx, ids: &mut Ids, raw: &[RawNode], g: &mut PropGroup, size: (u32, u32), choices: &mut Vec<LayerChoice>) {
    for n in raw {
        let RawKind::Effect { display, enabled, params, defs } = &n.kind else { continue };
        let shown = if display.is_empty() { n.match_name.as_str() } else { display.as_str() };
        let stand_in = effects_table::stand_in(&n.match_name);
        let spec = effects_table::effect_id(&n.match_name)
            .or(stand_in.map(|s| s.0))
            .and_then(effectcraft_effects::find)
            .or_else(|| effectcraft_effects::all().into_iter().find(|s| !display.is_empty() && s.name.eq_ignore_ascii_case(display)));
        let Some(spec) = spec else {
            cx.warn.add(format!("effect {shown} ({}) has no EffectCraft equivalent: skipped", n.match_name));
            continue;
        };
        let name = if n.name.is_empty() { shown } else { n.name.as_str() };
        let mut fg = effectcraft_effects::instantiate(spec, ids, name, [size.0 as f64, size.1 as f64]);
        let defs: &[RawParamDef] = if defs.is_empty() { cx.defs.get(n.match_name.as_str()).copied().unwrap_or(&[]) } else { defs };
        let read = settings::apply(spec, &mut fg, &n.match_name, params, defs, size);
        // An effect with a setting that could not be carried over would render wrong: it is
        // added switched off rather than shown wrong, and the settings are named.
        fg.enabled = *enabled && read.unread.is_empty();
        if !read.unread.is_empty() {
            cx.warn.add(format!("effect {shown}: added switched off, these settings could not be carried over: {}", read.unread.join(", ")));
        } else if !*enabled {
            cx.warn.add(format!("effect {shown}: switched off in the project, kept off"));
        }
        for note in read.notes {
            cx.warn.add(format!("effect {shown}: {note}"));
        }
        if let Some((_, what)) = stand_in {
            cx.warn.add(format!("effect {shown}: imported as {what}"));
        }
        choices.extend(read.layer_refs.iter().map(|(param, layer)| (fg.uid, *param, *layer, shown.to_string())));
        g.children.push(Node::Group(fg));
    }
}

fn build_layers(cx: &mut Ctx, project: &mut Project, comp: &mut Comp, raw: &RawComp, comp_id: ItemId) {
    let mut by_id: BTreeMap<u32, LayerId> = BTreeMap::new();
    // (our layer, After Effects parent id, matte mode, the layer above).
    let mut pending: Vec<(LayerId, u32, u32, Option<LayerId>)> = vec![];
    let mut above: Option<LayerId> = None;
    let mut choices: Vec<LayerChoice> = vec![];
    for rl in &raw.layers {
        let (src, size) = source(cx, project, comp, rl);
        if matches!(src, LayerSource::Comp { item } if project.comp_contains(item, comp_id)) {
            cx.warn.add("layer that uses its own composition (directly or through another) skipped");
            continue;
        }
        let name = if !rl.name.is_empty() {
            rl.name.clone()
        } else {
            src.item().and_then(|i| project.item(i)).map(|i| i.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| src.type_name().to_string())
        };
        let mut l: Layer = build::layer(project, comp, &name, src.clone(), size, None);
        l.name = name;
        by_id.insert(rl.id, l.id);
        cx.layers += 1;
        // In and out points are stored in layer time, before the stretch.
        let stretch = if rl.stretch == 0 { 1.0 } else { rl.stretch as f64 };
        let start = tick(rl.start);
        let (a, b) = (Tick::from_seconds_f64(rl.in_point.seconds() * stretch), Tick::from_seconds_f64(rl.out_point.seconds() * stretch));
        l.start_time = start;
        l.in_point = Tick(start.0.saturating_add(a.0.min(b.0)));
        l.out_point = Tick(start.0.saturating_add(a.0.max(b.0)));
        l.stretch = stretch * 100.0;
        if rl.stretch != 1 && rl.stretch != 0 {
            cx.warn.add(format!("time stretch read as {}% (only whole factors are understood so far)", rl.stretch as i32 * 100));
        }
        l.label = label(rl.label);
        l.switches.video = rl.video;
        l.switches.audio = rl.audio;
        l.switches.effects = rl.effects_on;
        l.switches.motion_blur = rl.motion_blur;
        l.switches.frame_blend = if rl.frame_blend { FrameBlend::FrameMix } else { FrameBlend::Off };
        l.switches.locked = rl.locked;
        l.switches.shy = rl.shy;
        l.switches.collapse = rl.collapse;
        l.switches.adjustment = rl.adjustment;
        l.switches.three_d = rl.three_d || l.switches.three_d;
        l.switches.solo = rl.solo;
        l.switches.guide = rl.guide;
        l.switches.quality = if rl.quality == 0 {
            Quality::Wireframe
        } else if rl.quality == 1 {
            Quality::Draft
        } else {
            Quality::Best
        };
        if src.is_av() || matches!(src, LayerSource::Text | LayerSource::Shape) {
            match blend(rl.blend) {
                Some((mode, sure)) => {
                    l.blend_mode = mode;
                    if !sure {
                        cx.warn.add(format!("blend mode number {} read as {mode:?}: check it", rl.blend));
                    }
                }
                None => cx.warn.add(format!("blend mode number {} is not known: Normal used", rl.blend)),
            }
        }
        match src {
            LayerSource::Text => {
                let read = group(&rl.props, "ADBE Text Properties").and_then(|g| {
                    g.iter().find_map(|n| match &n.kind {
                        RawKind::Text(blob) if n.match_name == "ADBE Text Document" => crate::text::text_doc(blob),
                        _ => None,
                    })
                });
                match (read, l.props.sub_mut("text").and_then(|t| t.get_mut("sourceText"))) {
                    (Some(read), Some(st)) => {
                        st.value = Value::Text(Box::new(read.doc));
                        if read.partial {
                            cx.warn.add("text layer: its text, fonts, sizes, fill colours, tracking, leading, scale and alignment are imported; stroke, baseline shift, box text and animators are not yet");
                        }
                    }
                    _ => cx.warn.add("text layer: its text could not be read (placeholder text)"),
                }
            }
            LayerSource::Shape => cx.warn.add("shape layer: its contents are not imported yet"),
            LayerSource::Camera => cx.warn.add("camera: its options are not imported yet"),
            LayerSource::Light { .. } => cx.warn.add("light: its type and options are not imported yet"),
            _ => {}
        }
        let mut next = project.next_id;
        if let (Some(raw_tr), Some(tr)) = (group(&rl.props, "ADBE Transform Group"), l.props.sub_mut("transform")) {
            transform(cx, raw_tr, tr, rl.three_d);
        }
        let mut ids = Ids(&mut next);
        for n in &rl.props {
            match (n.match_name.as_str(), &n.kind) {
                ("ADBE Mask Parade", RawKind::Group(raw_masks)) => {
                    if let Some(g) = l.props.sub_mut("masks") {
                        masks(cx, &mut ids, raw_masks, g, size);
                    } else if !raw_masks.is_empty() {
                        cx.warn.add("masks on a layer type that has none here: skipped");
                    }
                }
                ("ADBE Effect Parade", RawKind::Group(raw_fx)) => {
                    if let Some(g) = l.props.sub_mut("effects") {
                        effects(cx, &mut ids, raw_fx, g, size, &mut choices);
                    } else if !raw_fx.is_empty() {
                        cx.warn.add("effects on a layer type that has none here: skipped");
                    }
                }
                ("ADBE Time Remapping", RawKind::Prop(p)) if !p.keys.is_empty() => {
                    let mut pr = ids.prop("timeRemap", "Time Remap", Value::Scalar(0.0)).with_ui(ParamUi::Number);
                    apply(&mut pr, p, 1.0, &scalar);
                    l.props.children.insert(0, Node::Prop(pr));
                }
                ("ADBE Audio Group", RawKind::Group(a)) => {
                    // Audio Levels: left and right in decibels, here as there.
                    let levels = a.iter().find_map(|p| match &p.kind {
                        RawKind::Prop(rp) if p.match_name == "ADBE Audio Levels" => Some(rp),
                        _ => None,
                    });
                    if let Some(rp) = levels {
                        match l.props.sub_mut("audio").and_then(|g| g.get_mut("levels")) {
                            Some(target) => {
                                apply(target, rp, 1.0, &vec2);
                            }
                            None if !rp.keys.is_empty() || rp.value.iter().any(|v| *v != 0.0) => {
                                cx.warn.add("audio levels not imported (the layer's source has no sound here, or was not found)");
                            }
                            None => {}
                        }
                    }
                }
                ("ADBE Layer Styles", RawKind::Group(_)) | ("ADBE Transform Group", _) => {}
                ("ADBE Marker", RawKind::Prop(p)) if !p.keys.is_empty() => cx.warn.add("layer markers not imported"),
                _ => {}
            }
        }
        for _ in 0..expressions(&rl.props, 0) {
            cx.warn.add("expression not imported (the property keeps its value or keyframes)");
        }
        project.next_id = next;
        pending.push((l.id, rl.parent, rl.matte, above));
        above = Some(l.id);
        comp.layers.push(l);
    }
    // Layer choosers of effects, now that every layer of the composition has its id.
    for (group, param, layer, shown) in choices {
        let target = by_id.get(&layer).copied();
        let Some(fg) = comp.layers.iter_mut().find_map(|l| l.props.find_group_mut(group)) else { continue };
        match (target, fg.prop_mut(param)) {
            (Some(target), Some(p)) => p.value = Value::Layer(Some(target.0)),
            _ => {
                fg.enabled = false;
                cx.warn.add(format!("effect {shown}: added switched off, the layer it uses was not found in its composition"));
            }
        }
    }
    for (id, parent, matte, above) in pending {
        let parent_id = by_id.get(&parent).copied().filter(|p| *p != id);
        let kind = match matte {
            1 => Some(MatteKind::Alpha),
            2 => Some(MatteKind::AlphaInverted),
            3 => Some(MatteKind::Luma),
            4 => Some(MatteKind::LumaInverted),
            _ => None,
        };
        if let Some(l) = comp.layer_mut(id) {
            l.parent = parent_id;
            // Before After Effects 2023 a track matte is always the layer directly above.
            l.track_matte = kind.zip(above).map(|(kind, layer)| TrackMatte { layer, kind });
        }
    }
}
