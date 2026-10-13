//! `.aep` chunks → a plain description of the project ([`AepFile`]), before anything is mapped
//! onto EffectCraft's model.
//!
//! Field positions are noted where they are read. They were worked out from the container
//! rules and from projects saved by After Effects 6.5 to 2025; a field this reader does not
//! understand is left alone and reported by the importer, never guessed silently.

use crate::rifx::{Chunk, f32_at, f64_at, i16_at, i32_at, text_field, u8_at, u16_at, u32_at};

/// Deepest property-group nesting followed.
const MAX_GROUP_DEPTH: usize = 48;
/// Most keyframes read for one property.
const MAX_KEYS: usize = 200_000;
/// Most points read for one mask shape.
const MAX_POINTS: usize = 100_000;
/// Most parameter definitions read for one effect.
const MAX_PARAM_DEFS: usize = 4_096;
/// Largest block of an effect's private data kept (Curves keeps 1 644 bytes, Levels 108).
const MAX_BLOB: usize = 1 << 20;
/// Most private data blocks kept for one property (one per keyframe).
const MAX_BLOBS: usize = 20_000;

/// A time as the file stores it: `value / scale` seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rational {
    pub value: i64,
    pub scale: i64,
}

impl Rational {
    pub fn seconds(self) -> f64 {
        if self.scale == 0 { 0.0 } else { self.value as f64 / self.scale as f64 }
    }
    fn at(d: &[u8], o: usize) -> Rational {
        Rational { value: i32_at(d, o) as i64, scale: u32_at(d, o.saturating_add(4)) as i64 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AepFile {
    /// Items in file order: a folder comes before its contents.
    pub items: Vec<RawItem>,
}

#[derive(Clone, Debug)]
pub struct RawItem {
    pub id: u32,
    /// Id of the containing folder, 0 for the project root.
    pub parent: u32,
    pub name: String,
    pub kind: RawItemKind,
}

#[derive(Clone, Debug)]
pub enum RawItemKind {
    Folder,
    Comp(Box<RawComp>),
    Footage(Box<RawFootage>),
    /// An item type this reader does not know.
    Unknown(u16),
}

#[derive(Clone, Debug, Default)]
pub struct RawComp {
    pub width: u32,
    pub height: u32,
    /// Seconds per frame.
    pub frame: Rational,
    pub duration: Rational,
    pub work_in: Rational,
    pub work_out: Rational,
    pub pixel_aspect: (u32, u32),
    /// Top layer first.
    pub layers: Vec<RawLayer>,
}

#[derive(Clone, Debug, Default)]
pub struct RawFootage {
    pub width: u32,
    pub height: u32,
    pub duration: Rational,
    /// Frames per second (0 for stills and audio).
    pub fps: f64,
    /// The importer's four-character code for the file type (`png!`, `MooV`, `Soli`…).
    pub kind: [u8; 4],
    /// The linked file as the project recorded it (a path on the machine that saved it).
    pub path: Option<String>,
    pub solid: Option<RawSolid>,
}

#[derive(Clone, Debug, Default)]
pub struct RawSolid {
    pub color: [f32; 3],
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RawLayerKind {
    #[default]
    Av,
    Light,
    Camera,
    Text,
    Shape,
    Unknown(u32),
}

#[derive(Clone, Debug, Default)]
pub struct RawLayer {
    pub id: u32,
    pub name: String,
    pub kind: RawLayerKind,
    /// Item id of the source (0 for none).
    pub source: u32,
    /// Comp time of the layer's time 0.
    pub start: Rational,
    /// In and out points in layer time (before the stretch).
    pub in_point: Rational,
    pub out_point: Rational,
    /// Time stretch as a fraction, numerator and denominator (1/1 = 100%, 7/10 = 70%, 3/4 =
    /// 75%, -1/1 = reversed). Measured: with the fraction, a stretched layer ends on the
    /// frame the next layer of the same cut starts on.
    pub stretch: (i32, u32),
    pub label: u8,
    pub blend: u32,
    pub matte: u32,
    pub parent: u32,
    pub video: bool,
    pub audio: bool,
    pub effects_on: bool,
    pub motion_blur: bool,
    pub frame_blend: bool,
    pub locked: bool,
    pub shy: bool,
    pub collapse: bool,
    pub adjustment: bool,
    pub three_d: bool,
    pub solo: bool,
    pub null: bool,
    pub guide: bool,
    pub quality: u16,
    pub props: Vec<RawNode>,
}

/// One entry of a property group: its match name and what follows it.
#[derive(Clone, Debug)]
pub struct RawNode {
    pub match_name: String,
    /// The name the user gave it (empty = the default name).
    pub name: String,
    pub kind: RawKind,
}

#[derive(Clone, Debug)]
pub enum RawKind {
    Group(Vec<RawNode>),
    /// An effect: display name and parameters (match names end in `-NNNN`, the parameter index).
    Effect {
        display: String,
        /// The effect's own switch (the "fx" box), taken from bit 0 of the group's `tdsb`.
        /// That reading is an assumption: see where it is read.
        enabled: bool,
        params: Vec<RawNode>,
        /// The effect's parameter list with kinds and defaults. A project stores it with the
        /// first instance of each effect only; later instances have it empty.
        defs: Vec<RawParamDef>,
    },
    /// A mask: its settings, then the same children as a group.
    Mask {
        inverted: bool,
        locked: bool,
        mode: u16,
        props: Vec<RawNode>,
    },
    Prop(RawProp),
    /// Mask path: one shape, or one per keyframe of `prop`.
    Shape {
        prop: RawProp,
        shapes: Vec<RawShape>,
    },
    /// A text document (the raw description; not decoded yet).
    Text(Vec<u8>),
    /// A list this reader does not decode (its four-character type).
    Other([u8; 4]),
}

#[derive(Clone, Debug, Default)]
pub struct RawProp {
    pub dims: usize,
    pub spatial: bool,
    /// The value when there are no keyframes (`dims` numbers; empty for valueless properties).
    pub value: Vec<f64>,
    pub keys: Vec<RawKey>,
    pub expression: Option<String>,
    /// An effect's private data for this parameter (Curves' curve, Levels' numbers): one
    /// block, or one per keyframe.
    pub blobs: Vec<Vec<u8>>,
    /// The layer a layer chooser points at, by its id in the composition (0 = none). An
    /// effect's hidden input (parameter 0) points at the effect's own layer.
    pub layer_ref: u32,
}

/// The kinds of effect parameter a project file names ([`RawParamDef::kind`]).
pub mod param_kind {
    /// A layer chooser (and the effect's hidden input, parameter 0).
    pub const LAYER: u32 = 0;
    /// A whole-number slider.
    pub const INT_SLIDER: u32 = 1;
    /// A slider stored as 16.16 fixed point.
    pub const FIXED_SLIDER: u32 = 2;
    pub const ANGLE: u32 = 3;
    pub const CHECKBOX: u32 = 4;
    pub const COLOR: u32 = 5;
    pub const POINT: u32 = 6;
    pub const POPUP: u32 = 7;
    /// A slider stored as floating point.
    pub const FLOAT_SLIDER: u32 = 10;
    /// Private data only the effect understands.
    pub const PRIVATE: u32 = 11;
    pub const GROUP_START: u32 = 13;
    pub const GROUP_END: u32 = 14;
}

/// One parameter of an effect as the project describes it (`LIST parT`: a `tdmn`, its `pard`
/// and, for pop-ups and checkboxes, a `pdnm`).
#[derive(Clone, Debug, Default)]
pub struct RawParamDef {
    pub match_name: String,
    /// See [`param_kind`].
    pub kind: u32,
    /// The parameter's name in Effect Controls (empty for most checkboxes, whose name is the
    /// label beside the box).
    pub name: String,
    /// A pop-up's options separated by `|`, or a checkbox's label.
    pub label: String,
    /// The default, in the units a property stores its value in (so comparable with
    /// [`RawProp::value`]): a slider's number, an angle in degrees, 0/1 for a checkbox, a
    /// pop-up's 1-based choice, a colour as alpha-red-green-blue 0..255, a point as fractions
    /// of the layer. Empty when this reader does not understand the kind.
    pub default: Vec<f64>,
    /// A slider's highest valid value (0 when not a slider).
    pub valid_max: f64,
    /// A slider shown as a percentage.
    pub percent: bool,
    /// The default of a [`param_kind::PRIVATE`] parameter.
    pub default_blob: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct RawKey {
    pub time: Rational,
    /// 1 linear, 2 Bezier, 3 hold.
    pub in_interp: u8,
    pub out_interp: u8,
    pub flags: u8,
    pub value: Vec<f64>,
    pub in_speed: Vec<f64>,
    pub in_influence: Vec<f64>,
    pub out_speed: Vec<f64>,
    pub out_influence: Vec<f64>,
    /// Spatial tangents relative to the value (spatial properties only).
    pub tan_in: Vec<f64>,
    pub tan_out: Vec<f64>,
}

/// A mask shape: cubic Bezier points (vertex, out handle, in handle of the next vertex, …) as
/// fractions of the bounding box, which is itself in fractions of the layer's size.
#[derive(Clone, Debug, Default)]
pub struct RawShape {
    pub closed: bool,
    pub top_left: [f64; 2],
    pub bottom_right: [f64; 2],
    pub points: Vec<[f64; 2]>,
}

pub fn read(root: &Chunk) -> AepFile {
    let mut items = Vec::new();
    if let Some(fold) = root.list(b"Fold") {
        for it in fold.lists(b"Item") {
            read_item(it, 0, 0, &mut items);
        }
    }
    AepFile { items }
}

fn read_item(item: &Chunk, parent: u32, depth: usize, out: &mut Vec<RawItem>) {
    let idta = item.first(b"idta").map(|c| c.data).unwrap_or(&[]);
    // idta: type u16 @0 (1 folder, 4 composition, 7 footage), item id u32 @16.
    let ty = u16_at(idta, 0);
    let id = u32_at(idta, 16);
    let name = item.first(b"Utf8").map(|c| String::from_utf8_lossy(c.data).into_owned()).unwrap_or_default();
    let kind = match ty {
        1 => RawItemKind::Folder,
        4 => RawItemKind::Comp(Box::new(read_comp(item))),
        7 => RawItemKind::Footage(Box::new(item.list(b"Pin ").map(read_footage).unwrap_or_default())),
        other => RawItemKind::Unknown(other),
    };
    out.push(RawItem { id, parent, name, kind });
    if depth < MAX_GROUP_DEPTH
        && let Some(sub) = item.list(b"Sfdr")
    {
        for it in sub.lists(b"Item") {
            read_item(it, id, depth + 1, out);
        }
    }
}

fn read_comp(item: &Chunk) -> RawComp {
    let c = item.first(b"cdta").map(|c| c.data).unwrap_or(&[]);
    // cdta: frame duration @4, then (value, scale) pairs: display start @12, current time @20,
    // work area in @28 and out @36, duration @44; width u16 @140, height u16 @142, pixel
    // aspect ratio as two u32 @144.
    RawComp {
        width: u16_at(c, 140) as u32,
        height: u16_at(c, 142) as u32,
        frame: Rational { value: u32_at(c, 4) as i64, scale: u32_at(c, 8) as i64 },
        duration: Rational::at(c, 44),
        work_in: Rational::at(c, 28),
        work_out: Rational::at(c, 36),
        pixel_aspect: (u32_at(c, 144), u32_at(c, 148)),
        layers: item.lists(b"Layr").map(read_layer).collect(),
    }
}

fn read_footage(pin: &Chunk) -> RawFootage {
    let s = pin.first(b"sspc").map(|c| c.data).unwrap_or(&[]);
    let opti = pin.first(b"opti").map(|c| c.data).unwrap_or(&[]);
    let kind: [u8; 4] = opti.get(..4).and_then(|k| k.try_into().ok()).unwrap_or([0; 4]);
    // sspc: width u32 @30, height u32 @34, duration (value, scale) @38, frame rate 16.16 @58.
    let mut f = RawFootage {
        width: u32_at(s, 30),
        height: u32_at(s, 34),
        duration: Rational { value: u32_at(s, 38) as i64, scale: u32_at(s, 42) as i64 },
        fps: u32_at(s, 58) as f64 / 65536.0,
        kind,
        path: None,
        solid: None,
    };
    if &kind == b"Soli" {
        // opti of a solid: colour as four f32 (alpha, red, green, blue) @10, name @26.
        let ch = |i: usize| f32_at(opti, 10 + 4 * i).clamp(0.0, 1.0) as f32;
        f.solid = Some(RawSolid { color: [ch(1), ch(2), ch(3)], name: text_field(opti.get(26..).unwrap_or(&[])) });
        return f;
    }
    f.path = pin
        .list(b"Als2")
        .and_then(|l| l.first(b"alas"))
        .and_then(|a| json_string(a.data, "fullpath"))
        .or_else(|| pin.list(b"Alas").and_then(|l| l.first(b"alas")).and_then(|a| legacy_alias_path(a.data)));
    f
}

/// The string value of `"key":"…"` in a small JSON object (the file link of newer saves).
fn json_string(d: &[u8], key: &str) -> Option<String> {
    let text = String::from_utf8_lossy(d);
    let at = text.find(&format!("\"{key}\""))?;
    let rest = text.get(at + key.len() + 2..)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// The path inside a pre-2014 binary file alias: its longest run of printable text that looks
/// like a path.
fn legacy_alias_path(d: &[u8]) -> Option<String> {
    let mut best: Option<&[u8]> = None;
    for run in d.split(|b| !(0x20..0x7f).contains(b)) {
        let pathlike = run.len() >= 4 && run.iter().any(|b| matches!(b, b'\\' | b'/' | b':'));
        if pathlike && best.is_none_or(|b| run.len() > b.len()) {
            best = Some(run);
        }
    }
    let s = String::from_utf8_lossy(best?).into_owned();
    // Windows aliases are written `\C:\folder\file`.
    Some(s.strip_prefix('\\').filter(|r| r.as_bytes().get(1) == Some(&b':')).map(str::to_string).unwrap_or(s))
}

fn read_layer(layr: &Chunk) -> RawLayer {
    let d = layr.first(b"ldta").map(|c| c.data).unwrap_or(&[]);
    // ldta: layer id u32 @0, quality u16 @4, the stretch's numerator i32 @8, start @12, in @20
    // and out @28 as (value, scale) pairs, three flag bytes @37, source item id u32 @40, label
    // u8 @61, blend mode u32 @96, track matte u32 @104, the stretch's denominator u32 @108,
    // layer type u32 @128, parent layer id u32 @132.
    let (f0, f1, f2) = (u8_at(d, 37), u8_at(d, 38), u8_at(d, 39));
    let bit = |b: u8, n: u8| b >> n & 1 == 1;
    RawLayer {
        id: u32_at(d, 0),
        name: layr.first(b"Utf8").map(|c| String::from_utf8_lossy(c.data).into_owned()).unwrap_or_default(),
        kind: match u32_at(d, 128) {
            0 => RawLayerKind::Av,
            1 => RawLayerKind::Light,
            2 => RawLayerKind::Camera,
            3 => RawLayerKind::Text,
            4 => RawLayerKind::Shape,
            other => RawLayerKind::Unknown(other),
        },
        source: u32_at(d, 40),
        start: Rational::at(d, 12),
        in_point: Rational::at(d, 20),
        out_point: Rational::at(d, 28),
        stretch: (i32_at(d, 8), u32_at(d, 108)),
        label: u8_at(d, 61),
        blend: u32_at(d, 96),
        matte: u32_at(d, 104),
        parent: u32_at(d, 132),
        video: bit(f2, 0),
        audio: bit(f2, 1),
        effects_on: bit(f2, 2),
        motion_blur: bit(f2, 3),
        frame_blend: bit(f2, 4),
        locked: bit(f2, 5),
        shy: bit(f2, 6),
        collapse: bit(f2, 7),
        adjustment: bit(f1, 1),
        three_d: bit(f1, 2),
        solo: bit(f1, 3),
        null: bit(f1, 7),
        guide: bit(f0, 1),
        quality: u16_at(d, 4),
        props: layr.list(b"tdgp").map(|g| read_group(g, 0)).unwrap_or_default(),
    }
}

/// The entries of a `tdgp` list: `tdmn` (a match name) followed by the list it names.
fn read_group(group: &Chunk, depth: usize) -> Vec<RawNode> {
    let mut out = Vec::new();
    if depth > MAX_GROUP_DEPTH {
        return out;
    }
    let mut match_name: Option<String> = None;
    let mut mkif: Option<&[u8]> = None;
    for k in &group.kids {
        if k.is(b"tdmn") {
            match_name = Some(text_field(k.data));
            mkif = None;
            continue;
        }
        if k.is(b"mkif") {
            mkif = Some(k.data);
            continue;
        }
        // An effect's private data follows the property it belongs to: `LIST aRbs` holding one
        // `aRbp`, or one per keyframe.
        if k.is_list(b"aRbs") && match_name.is_none() {
            if let Some(RawNode { kind: RawKind::Prop(p), .. }) = out.last_mut() {
                p.blobs = k.kids.iter().filter(|b| b.is(b"aRbp") && b.data.len() <= MAX_BLOB).take(MAX_BLOBS).map(|b| b.data.to_vec()).collect();
            }
            continue;
        }
        let (Some(ty), Some(mn)) = (k.list, match_name.as_ref()) else { continue };
        if mn == "ADBE Group End" {
            continue;
        }
        // After Effects CC writes `-_0_/-` where an instance was given no name of its own.
        let user_name = |c: &Chunk| c.first(b"tdsn").map(|n| text_field(n.data)).filter(|n| n != "-_0_/-").unwrap_or_default();
        let (name, kind) = match &ty {
            b"tdgp" => match mkif {
                // mkif: inverted u8 @0, locked u8 @1, mode u16 @6.
                Some(m) => {
                    (user_name(k), RawKind::Mask { inverted: u8_at(m, 0) != 0, locked: u8_at(m, 1) != 0, mode: u16_at(m, 6), props: read_group(k, depth + 1) })
                }
                None => (user_name(k), RawKind::Group(read_group(k, depth + 1))),
            },
            b"sspc" => {
                let inner = k.list(b"tdgp");
                (
                    inner.map(user_name).unwrap_or_default(),
                    RawKind::Effect {
                        display: k.first(b"fnam").map(|n| text_field(n.data)).unwrap_or_default(),
                        // tdsb of the effect's group is 1 or 9 on 427 of the 436 effects in
                        // the projects measured and 0 or 8 on the other nine (among them a
                        // slow denoiser, the usual effect to leave switched off). Bit 0 is
                        // taken as the effect's switch; not confirmed in After Effects.
                        enabled: inner.and_then(|g| g.first(b"tdsb")).is_none_or(|t| u32_at(t.data, 0) & 1 == 1),
                        params: inner.map(|g| read_group(g, depth + 1)).unwrap_or_default(),
                        defs: k.list(b"parT").map(read_param_defs).unwrap_or_default(),
                    },
                )
            }
            b"tdbs" => (user_name(k), RawKind::Prop(read_prop(k))),
            b"om-s" => {
                let prop = k.list(b"tdbs").map(read_prop).unwrap_or_default();
                let shapes = k.list(b"omks").map(|o| o.lists(b"shap").map(read_shape).collect()).unwrap_or_default();
                (k.list(b"tdbs").map(user_name).unwrap_or_default(), RawKind::Shape { prop, shapes })
            }
            b"btds" => (String::new(), RawKind::Text(k.find_list(b"btdk").map(|b| b.data.to_vec()).unwrap_or_default())),
            other => (String::new(), RawKind::Other(*other)),
        };
        // "Utf8\0\0\0\0" (an empty nested name) and a lone NUL both mean "no name".
        out.push(RawNode { match_name: mn.clone(), name, kind });
        match_name = None;
        mkif = None;
    }
    out
}

fn read_prop(tdbs: &Chunk) -> RawProp {
    let b = tdbs.first(b"tdb4").map(|c| c.data).unwrap_or(&[]);
    // tdb4: dimensions u16 @2, flags u8 @5 (bit 3: spatial), time scale u32 @12.
    let dims = (u16_at(b, 2) as usize).min(8);
    let spatial = u8_at(b, 5) & 0x08 != 0;
    let scale = u32_at(b, 12) as i64;
    let value = tdbs.first(b"cdat").map(|c| (0..dims).filter(|i| c.data.len() >= (i + 1) * 8).map(|i| f64_at(c.data, i * 8)).collect()).unwrap_or_default();
    // An expression: an `expr` chunk in saves up to After Effects 7, a `Utf8` chunk since.
    let expression = tdbs
        .kids
        .iter()
        .filter_map(|k| {
            if k.is(b"expr") {
                Some(text_field(k.data))
            } else if k.is(b"Utf8") {
                Some(String::from_utf8_lossy(k.data).into_owned())
            } else {
                None
            }
        })
        .find(|s| !s.trim().is_empty());
    let mut keys = Vec::new();
    if let Some(list) = tdbs.list(b"list")
        && let (Some(head), Some(data)) = (list.first(b"lhd3"), list.first(b"ldat"))
    {
        // lhd3: key count u32 @8, bytes per key u32 @16.
        let count = (u32_at(head.data, 8) as usize).min(MAX_KEYS);
        let size = u32_at(head.data, 16) as usize;
        if size >= 8 {
            for kf in data.data.chunks_exact(size).take(count) {
                keys.push(read_key(kf, dims, spatial, scale));
            }
        }
    }
    // tdpi: the id of the layer a layer chooser points at. Checked on a Time Displacement whose
    // map is another layer of its composition, and on the hidden input of every effect looked
    // at, which holds the id of the effect's own layer.
    let layer_ref = tdbs.first(b"tdpi").map(|c| u32_at(c.data, 0)).unwrap_or(0);
    RawProp { dims, spatial, value, keys, expression, blobs: Vec::new(), layer_ref }
}

/// The parameter definitions of an effect (`LIST parT`): a `tdmn` (the parameter's match name)
/// followed by its `pard` and, for some kinds, a `pdnm` (pop-up options, checkbox label) or an
/// `aRbp` (the default of a private-data parameter).
///
/// `pard` is 148 bytes in every project measured (After Effects 6.5 to 2025): the kind u32
/// @12, the name as a C string @16 (32 bytes), then per kind:
///
/// | kind | default | also |
/// |---|---|---|
/// | whole-number slider | i32 @140 | highest valid value i32 @128 |
/// | fixed-point slider | 16.16 @140 | highest valid value 16.16 @128, display flags u16 @146 (bit 0: percent) |
/// | float slider | f32 @120 | highest valid value f32 @108, display flags u16 @126 |
/// | angle | 16.16 @60, degrees | |
/// | checkbox | u8 @60 | |
/// | colour | alpha, red, green, blue bytes @60 | |
/// | point | x 16.16 @68, y 16.16 @72, percent of the layer | |
/// | pop-up | i16 @62, 1-based | |
///
/// Checked against defaults known from using After Effects (Glow Threshold 60%, Black &
/// White's six sliders, Grid's white colour, Fractal Noise's centred offset).
fn read_param_defs(part: &Chunk) -> Vec<RawParamDef> {
    let mut out: Vec<RawParamDef> = Vec::new();
    let fixed = |d: &[u8], o: usize| i32_at(d, o) as f64 / 65536.0;
    for k in &part.kids {
        if k.is(b"tdmn") {
            if out.len() >= MAX_PARAM_DEFS {
                break;
            }
            out.push(RawParamDef { match_name: text_field(k.data), kind: u32::MAX, ..Default::default() });
            continue;
        }
        let Some(def) = out.last_mut() else { continue };
        if k.is(b"pdnm") {
            def.label = text_field(k.data).trim_end_matches('\0').to_string();
        } else if k.is(b"aRbp") && k.data.len() <= MAX_BLOB {
            def.default_blob = k.data.to_vec();
        } else if k.is(b"pard") {
            let d = k.data;
            def.kind = u32_at(d, 12);
            def.name = text_field(d.get(16..48).unwrap_or(&[]));
            match def.kind {
                param_kind::INT_SLIDER => {
                    def.default = vec![i32_at(d, 140) as f64];
                    def.valid_max = i32_at(d, 128) as f64;
                }
                param_kind::FIXED_SLIDER => {
                    def.default = vec![fixed(d, 140)];
                    def.valid_max = fixed(d, 128);
                    def.percent = u16_at(d, 146) & 1 == 1;
                }
                param_kind::FLOAT_SLIDER => {
                    def.default = vec![f32_at(d, 120)];
                    def.valid_max = f32_at(d, 108);
                    def.percent = u16_at(d, 126) & 1 == 1;
                }
                param_kind::ANGLE => def.default = vec![fixed(d, 60)],
                param_kind::CHECKBOX => def.default = vec![f64::from(u8::from(u8_at(d, 60) != 0))],
                param_kind::COLOR => def.default = (0..4).map(|i| u8_at(d, 60 + i) as f64).collect(),
                param_kind::POINT => def.default = vec![fixed(d, 68) / 100.0, fixed(d, 72) / 100.0],
                param_kind::POPUP => def.default = vec![i16_at(d, 62) as f64],
                _ => {}
            }
        }
    }
    out
}

/// One keyframe. The first 8 bytes are the same for every kind: time i32 @0 (in the property's
/// time scale), in and out interpolation u8 @4 and @5, flags u8 @7. What follows depends on
/// the property:
///
/// - one or more dimensions, not spatial: value, in speed, in influence, out speed, out
///   influence, each `dims` f64 (48, 88 or 128 bytes in all);
/// - spatial: 16 bytes this reader skips, then in speed, in influence, out speed, out
///   influence (one f64 each), then value, in tangent, out tangent, each `dims` f64 (104 or
///   128 bytes);
/// - anything else (mask paths, markers, text documents) carries no numbers here.
fn read_key(kf: &[u8], dims: usize, spatial: bool, scale: i64) -> RawKey {
    let mut k = RawKey {
        time: Rational { value: i32_at(kf, 0) as i64, scale },
        in_interp: u8_at(kf, 4),
        out_interp: u8_at(kf, 5),
        flags: u8_at(kf, 7),
        ..Default::default()
    };
    let run = |at: usize, n: usize| -> Vec<f64> { (0..n).map(|i| f64_at(kf, at + 8 * i)).collect() };
    if dims == 0 {
        return k;
    }
    if spatial && kf.len() == 56 + 24 * dims {
        k.in_speed = run(24, 1);
        k.in_influence = run(32, 1);
        k.out_speed = run(40, 1);
        k.out_influence = run(48, 1);
        k.value = run(56, dims);
        k.tan_in = run(56 + 8 * dims, dims);
        k.tan_out = run(56 + 16 * dims, dims);
    } else if kf.len() == 8 + 40 * dims {
        k.value = run(8, dims);
        k.in_speed = run(8 + 8 * dims, dims);
        k.in_influence = run(8 + 16 * dims, dims);
        k.out_speed = run(8 + 24 * dims, dims);
        k.out_influence = run(8 + 32 * dims, dims);
    }
    k
}

fn read_shape(shap: &Chunk) -> RawShape {
    let h = shap.first(b"shph").map(|c| c.data).unwrap_or(&[]);
    // shph: flags u8 @3 (bit 3: open path), bounding box as four f32 @4.
    let mut s =
        RawShape { closed: u8_at(h, 3) & 0x08 == 0, top_left: [f32_at(h, 4), f32_at(h, 8)], bottom_right: [f32_at(h, 12), f32_at(h, 16)], points: Vec::new() };
    if let Some(list) = shap.list(b"list")
        && let (Some(head), Some(data)) = (list.first(b"lhd3"), list.first(b"ldat"))
    {
        let count = (u32_at(head.data, 8) as usize).min(MAX_POINTS);
        let size = u32_at(head.data, 16) as usize;
        if size >= 8 {
            s.points = data.data.chunks_exact(size).take(count).map(|p| [f32_at(p, 0), f32_at(p, 4)]).collect();
        }
    }
    s
}
