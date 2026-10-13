//! Effect settings, on synthetic projects built chunk by chunk (no real project files).

use effectcraft_keyframe::{Interp, Value};
use effectcraft_project::{Footage, PropGroup};
use effectcraft_time::Tick;

use super::{L, cdta, chunk, f64s, file, fixed, group, idta, key_head, key_list, ldta, list, put, run, solid_item, tdb4, tdmn};
use crate::{FootageRef, import};

fn fx16(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

/// A parameter definition: its match name and a 148-byte `pard` of this kind and name.
fn def(match_name: &str, kind: u32, name: &str, fill: impl FnOnce(&mut Vec<u8>)) -> Vec<Vec<u8>> {
    let mut d = vec![0u8; 148];
    put(&mut d, 12, &kind.to_be_bytes());
    put(&mut d, 16, name.as_bytes());
    fill(&mut d);
    vec![tdmn(match_name), chunk(b"pard", &d)]
}

fn slider(match_name: &str, name: &str, default: f64, max: f64, percent: bool) -> Vec<Vec<u8>> {
    def(match_name, 2, name, |d| {
        put(d, 128, &fx16(max));
        put(d, 140, &fx16(default));
        put(d, 146, &u16::from(percent).to_be_bytes());
    })
}

fn float_slider(match_name: &str, name: &str, default: f32, max: f32) -> Vec<Vec<u8>> {
    def(match_name, 10, name, |d| {
        put(d, 108, &max.to_be_bytes());
        put(d, 120, &default.to_be_bytes());
    })
}

fn popup(match_name: &str, name: &str, options: &str, default: i16) -> Vec<Vec<u8>> {
    let mut v = def(match_name, 7, name, |d| put(d, 62, &default.to_be_bytes()));
    v.push(chunk(b"pdnm", options.as_bytes()));
    v
}

fn checkbox(match_name: &str, label: &str, default: bool) -> Vec<Vec<u8>> {
    let mut v = def(match_name, 4, " ", |d| put(d, 60, &[u8::from(default)]));
    v.push(chunk(b"pdnm", label.as_bytes()));
    v
}

fn color(match_name: &str, name: &str, argb: [u8; 4]) -> Vec<Vec<u8>> {
    def(match_name, 5, name, |d| put(d, 60, &argb))
}

fn point(match_name: &str, name: &str, percent: [f64; 2]) -> Vec<Vec<u8>> {
    def(match_name, 6, name, |d| {
        put(d, 68, &fx16(percent[0]));
        put(d, 72, &fx16(percent[1]));
    })
}

fn private(match_name: &str, name: &str, default: &[u8]) -> Vec<Vec<u8>> {
    let mut v = def(match_name, 11, name, |_| {});
    v.push(chunk(b"aRbp", default));
    v
}

/// An effect: its name, its parameter definitions (empty for a later instance), the group's
/// flags, and the parameters this instance stores.
fn effect(display: &str, defs: &[Vec<Vec<u8>>], flags: u32, stored: &[Vec<u8>]) -> Vec<u8> {
    let mut name = vec![0u8; 48];
    put(&mut name, 0, display.as_bytes());
    let part: Vec<Vec<u8>> = defs.iter().flatten().cloned().collect();
    let mut kids = vec![chunk(b"tdsb", &flags.to_be_bytes())];
    kids.extend(stored.iter().cloned());
    list(b"sspc", &[chunk(b"fnam", &name), list(b"parT", &part), group(&kids)])
}

/// Grid's parameters as After Effects lists them (its defaults are not EffectCraft's).
fn grid_defs() -> Vec<Vec<Vec<u8>>> {
    vec![
        def("ADBE Grid-0000", 0, "", |_| {}),
        point("ADBE Grid-0001", "Anchor", [50.0, 50.0]),
        popup("ADBE Grid-0002", "Size From", "Corner Point|Width Slider|Width & Height Sliders", 1),
        point("ADBE Grid-0003", "Corner", [60.0, 60.0]),
        slider("ADBE Grid-0004", "Width", 32.0, 4000.0, false),
        slider("ADBE Grid-0005", "Height", 24.0, 4000.0, false),
        slider("ADBE Grid-0006", "Border", 5.0, 400.0, false),
        def("ADBE Grid-0007", 13, "Feather", |_| {}),
        slider("ADBE Grid-0008", "Width", 0.0, 400.0, false),
        slider("ADBE Grid-0009", "Height", 0.0, 400.0, false),
        def("ADBE Grid-0010", 14, "", |_| {}),
        checkbox("ADBE Grid-0011", "Invert Grid", false),
        color("ADBE Grid-0012", "Color", [255, 255, 255, 255]),
        slider("ADBE Grid-0013", "Opacity", 100.0, 100.0, true),
        popup("ADBE Grid-0014", "Blending Mode", "None|Normal|Add", 1),
        def("ADBE Effect Built In Params", 9, "", |_| {}),
    ]
}

fn stored(match_name: &str, dims: u16, value: &[f64]) -> Vec<Vec<u8>> {
    vec![tdmn(match_name), fixed(dims, value)]
}

/// A 1920×1080 comp with one solid layer per entry of `effects` (each a list of effects).
fn project(layers: &[Vec<(&str, Vec<u8>)>]) -> Vec<u8> {
    let mut comp = vec![idta(4, 30), chunk(b"Utf8", b"Comp"), cdta(1920, 1080, 90)];
    for (i, fx) in layers.iter().enumerate() {
        let parade: Vec<Vec<u8>> = fx.iter().flat_map(|(mn, e)| [tdmn(mn), e.clone()]).collect();
        comp.push(list(
            b"Layr",
            &[ldta(&L { id: i as u32 + 1, source: 11, ..Default::default() }), chunk(b"Utf8", b"Layer"), group(&[tdmn("ADBE Effect Parade"), group(&parade)])],
        ));
    }
    file(&[list(b"Fold", &[solid_item(11, 1920, 1080, [1.0, 0.0, 0.0, 0.0], "Black Solid 1"), list(b"Item", &comp)])])
}

fn effect_of(p: &effectcraft_project::Project, r: &crate::ImportResult, layer: usize, n: usize) -> PropGroup {
    p.comp(r.main_comp.unwrap()).unwrap().layers[layer].effects().unwrap().groups().nth(n).unwrap().clone()
}

fn num(g: &PropGroup, path: &str) -> f64 {
    match g.prop(path).unwrap_or_else(|| panic!("no parameter {path}")).value {
        Value::Scalar(v) => v,
        ref other => panic!("{path} is {other:?}"),
    }
}

/// The letterbox case: a solid cut into bars by Grid. Every kind of plain parameter arrives,
/// and parameters the project left alone take After Effects' defaults, not ours.
#[test]
fn stored_values_and_after_effects_defaults_arrive() {
    let first = effect(
        "Grid",
        &grid_defs(),
        1,
        &[
            stored("ADBE Grid-0001", 2, &[0.25, 0.5]),
            stored("ADBE Grid-0002", 1, &[3.0]),
            stored("ADBE Grid-0004", 1, &[4000.0]),
            stored("ADBE Grid-0005", 1, &[1080.0]),
            stored("ADBE Grid-0006", 1, &[221.0]),
            stored("ADBE Grid-0007", 1, &[0.0]),
            stored("ADBE Grid-0012", 4, &[255.0, 0.0, 0.0, 0.0]),
        ]
        .concat(),
    );
    // A later instance carries no definitions of its own and stores one changed parameter.
    let second = effect("Grid", &[], 1, &stored("ADBE Grid-0011", 1, &[1.0]));
    let (p, r) = run(&project(&[vec![("ADBE Grid", first)], vec![("ADBE Grid", second)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!(g.prop("anchor").unwrap().value, Value::Vec2([480.0, 540.0]));
    assert_eq!(g.prop("sizeFrom").unwrap().value, Value::Enum(2));
    assert_eq!((num(&g, "width"), num(&g, "height"), num(&g, "border")), (4000.0, 1080.0, 221.0));
    assert_eq!(g.prop("color").unwrap().value, Value::Color([0.0, 0.0, 0.0, 1.0]));
    // Not stored: After Effects' defaults.
    assert_eq!(g.prop("corner").unwrap().value, Value::Vec2([1152.0, 648.0]));
    assert_eq!(g.prop("invertGrid").unwrap().value, Value::Bool(false));
    assert_eq!((num(&g, "opacity"), num(&g, "feather/featherWidth")), (100.0, 0.0));
    let g2 = effect_of(&p, &r, 1, 0);
    assert!(g2.enabled);
    assert_eq!(g2.prop("invertGrid").unwrap().value, Value::Bool(true));
    assert_eq!((num(&g2, "width"), num(&g2, "border")), (32.0, 5.0));
    assert_eq!(g2.prop("sizeFrom").unwrap().value, Value::Enum(0));
    assert!(!r.warnings.iter().any(|w| w.contains("effect Grid")), "{:?}", r.warnings);
}

/// A setting that cannot be carried over keeps the effect off and is named; so does a value
/// outside what our parameter accepts. A parameter stored at its default is no obstacle.
#[test]
fn what_cannot_be_read_is_named_and_keeps_the_effect_off() {
    let mut defs = grid_defs();
    defs.insert(7, slider("ADBE Grid-0020", "Made Up Setting", 1.0, 100.0, false));
    defs.insert(8, slider("ADBE Grid-0021", "Untouched Unknown", 7.0, 100.0, false));
    let unknown = effect("Grid", &defs, 1, &[stored("ADBE Grid-0020", 1, &[5.0]), stored("ADBE Grid-0021", 1, &[7.0])].concat());
    let (p, r) = run(&project(&[vec![("ADBE Grid", unknown)]]));
    assert!(!effect_of(&p, &r, 0, 0).enabled);
    assert!(r.warnings.iter().any(|w| w == "effect Grid: added switched off, these settings could not be carried over: Made Up Setting"), "{:?}", r.warnings);

    let too_big = effect("Grid", &grid_defs(), 1, &stored("ADBE Grid-0006", 1, &[9999.0]));
    let (p, r) = run(&project(&[vec![("ADBE Grid", too_big)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(!g.enabled);
    assert!(r.warnings.iter().any(|w| w.ends_with("could not be carried over: Border")), "{:?}", r.warnings);

    // A pop-up choice we have no option for.
    let mut defs = grid_defs();
    defs[14] = popup("ADBE Grid-0014", "Blending Mode", "None|Normal|No Such Mode", 1);
    let odd = effect("Grid", &defs, 1, &stored("ADBE Grid-0014", 1, &[3.0]));
    let (p, r) = run(&project(&[vec![("ADBE Grid", odd)]]));
    assert!(!effect_of(&p, &r, 0, 0).enabled);
    assert!(r.warnings.iter().any(|w| w.ends_with("could not be carried over: Blending Mode")), "{:?}", r.warnings);
}

/// An effect switched off in the project stays off, and says why.
#[test]
fn an_effect_switched_off_in_the_project_stays_off() {
    let off = effect("Grid", &grid_defs(), 0, &stored("ADBE Grid-0006", 1, &[10.0]));
    let (p, r) = run(&project(&[vec![("ADBE Grid", off)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(!g.enabled);
    assert_eq!(num(&g, "border"), 10.0);
    assert!(r.warnings.iter().any(|w| w == "effect Grid: switched off in the project, kept off"), "{:?}", r.warnings);
}

/// A slider After Effects shows as a percentage of 0..255 or of 0..1 arrives as the
/// percentage; keyframes are converted the same way.
#[test]
fn percent_sliders_and_keyframes() {
    let defs = vec![
        def("ADBE Glo2-0000", 0, "", |_| {}),
        popup("ADBE Glo2-0001", "Glow Based On", "Alpha Channel|Color Channels", 2),
        slider("ADBE Glo2-0002", "Glow Threshold", 153.0, 255.0, true),
        slider("ADBE Glo2-0003", "Glow Radius", 10.0, 1000.0, false),
        slider("ADBE Glo2-0011", "A & B Midpoint", 0.5, 0.99, true),
    ];
    let key = |t: i32, v: f64| {
        let mut k = key_head(t, 1);
        k.extend([v, 0.0, 0.0, 0.0, 0.0].iter().flat_map(|x| x.to_be_bytes()));
        k
    };
    let radius = list(b"tdbs", &[tdb4(1, false), key_list(48, &[key(0, 10.0), key(30_000, 40.0)])]);
    let glow = effect("Glow", &defs, 9, &[stored("ADBE Glo2-0002", 1, &[189.0]), vec![tdmn("ADBE Glo2-0003"), radius]].concat());
    let (p, r) = run(&project(&[vec![("ADBE Glo2", glow)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert!((num(&g, "threshold") - 189.0 / 2.55).abs() < 1e-9);
    assert_eq!(num(&g, "abMidpoint"), 50.0);
    assert_eq!(g.prop("based").unwrap().value, Value::Enum(1));
    let keys = &g.prop("radius").unwrap().keys;
    assert_eq!(keys.len(), 2);
    assert_eq!((keys[0].time, &keys[0].value, &keys[1].value), (Tick::ZERO, &Value::Scalar(10.0), &Value::Scalar(40.0)));
    assert_eq!(keys[1].time, Tick::from_units(1, 1));
}

fn levels_block(numbers: [f32; 25]) -> Vec<u8> {
    let mut b = vec![0x00, 0x0f, 0x10, 0xa7, 0, 0, 0, 1];
    b.extend(numbers.iter().flat_map(|v| v.to_be_bytes()));
    b
}

const LEVELS_UNTOUCHED: [f32; 25] = [0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];

fn levels_defs() -> Vec<Vec<Vec<u8>>> {
    vec![
        def("ADBE Easy Levels2-0000", 0, "", |_| {}),
        popup("ADBE Easy Levels2-0001", "Channel:", "RGB|Red|Green|Blue|Alpha", 1),
        private("ADBE Easy Levels2-0002", "Histogram", &levels_block(LEVELS_UNTOUCHED)),
        float_slider("ADBE Easy Levels2-0003", "Input Black", 0.0, 1.0),
        float_slider("ADBE Easy Levels2-0004", "Input White", 1.0, 1.0),
        float_slider("ADBE Easy Levels2-0005", "Gamma", 1.0, 10.0),
    ]
}

/// Levels' numbers live in the Histogram parameter's private block, also when keyframed.
#[test]
fn levels_reads_its_private_block() {
    let mut set = LEVELS_UNTOUCHED;
    set[0] = 0.125; // RGB input black
    set[2] = 1.5; // RGB gamma
    set[5 + 4] = 0.75; // red output white
    let block = |b: &[u8]| chunk(b"aRbp", b);
    let fixed_levels = effect(
        "Levels",
        &levels_defs(),
        1,
        &[tdmn("ADBE Easy Levels2-0002"), list(b"tdbs", &[tdb4(1, false)]), list(b"aRbs", &[block(&levels_block(set))])],
    );
    let mut later = set;
    later[0] = 0.25;
    let key = |t: i32| {
        let mut k = key_head(t, 1);
        k.extend([0u8; 56]);
        k
    };
    let keyed = effect(
        "Levels",
        &[],
        1,
        &[
            tdmn("ADBE Easy Levels2-0002"),
            list(b"tdbs", &[tdb4(1, false), key_list(64, &[key(0), key(15_000)])]),
            list(b"aRbs", &[block(&levels_block(set)), block(&levels_block(later))]),
        ],
    );
    let (p, r) = run(&project(&[vec![("ADBE Easy Levels2", fixed_levels), ("ADBE Easy Levels2", keyed)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!((num(&g, "inBlack"), num(&g, "inWhite"), num(&g, "gamma"), num(&g, "outWhite")), (0.125, 1.0, 1.5, 1.0));
    assert_eq!((num(&g, "redOutWhite"), num(&g, "redInBlack"), num(&g, "alphaGamma")), (0.75, 0.0, 1.0));
    let k = effect_of(&p, &r, 0, 1);
    assert!(k.enabled);
    let keys = &k.prop("inBlack").unwrap().keys;
    assert_eq!(keys.len(), 2);
    assert_eq!((&keys[0].value, &keys[1].value, keys[1].time), (&Value::Scalar(0.125), &Value::Scalar(0.25), Tick::from_units(1, 2)));
    assert_eq!(keys[0].out_interp, Interp::Linear);
    // Only the number that moves is keyframed.
    assert!(k.prop("gamma").unwrap().keys.is_empty());
    assert_eq!(num(&k, "gamma"), 1.5);

    // A block of another size is not guessed at.
    let odd = effect("Levels", &levels_defs(), 1, &[tdmn("ADBE Easy Levels2-0002"), list(b"tdbs", &[tdb4(1, false)]), list(b"aRbs", &[block(&[1, 2, 3, 4])])]);
    let (p, r) = run(&project(&[vec![("ADBE Easy Levels2", odd)]]));
    assert!(!effect_of(&p, &r, 0, 0).enabled);
    assert!(r.warnings.iter().any(|w| w.ends_with("could not be carried over: Histogram")), "{:?}", r.warnings);
}

/// Curves' private block: five lookup tables, then the points of each of the five curves.
fn curves_block(rgb: &[(u16, u16)]) -> Vec<u8> {
    let mut b = vec![0, 1, 0, 1];
    for _ in 0..5 {
        b.extend(0..=255u8);
    }
    for ch in 0..5 {
        let pts: &[(u16, u16)] = if ch == 0 { rgb } else { &[(0, 0), (255, 255)] };
        let mut block = vec![0u8; 64];
        for (i, (x, y)) in pts.iter().enumerate() {
            put(&mut block, 4 * i, &x.to_be_bytes());
            put(&mut block, 4 * i + 2, &y.to_be_bytes());
        }
        b.extend(block);
        b.extend((pts.len() as u32).to_be_bytes());
        b.extend([0xff; 4]);
    }
    b
}

#[test]
fn curves_reads_its_points() {
    let defs = vec![
        def("ADBE CurvesCustom-0000", 0, "", |_| {}),
        private("ADBE CurvesCustom-0001", "Curves", &curves_block(&[(0, 0), (255, 255)])),
    ];
    assert_eq!(curves_block(&[(0, 0), (255, 255)]).len(), 1644);
    let bent = curves_block(&[(0, 0), (51, 102), (255, 255)]);
    let fx = effect("Curves", &defs, 1, &[tdmn("ADBE CurvesCustom-0001"), list(b"tdbs", &[tdb4(1, false)]), list(b"aRbs", &[chunk(b"aRbp", &bent)])]);
    let (p, r) = run(&project(&[vec![("ADBE CurvesCustom", fx)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!(g.prop("rgb").unwrap().value, Value::Str("0,0 0.2,0.4 1,1".into()));
    assert_eq!(g.prop("red").unwrap().value, Value::Str("0,0 1,1".into()));
}

/// Names that repeat pair by their group: Selective Color has a Cyan for the colour on show
/// (which we do not have) and one in each of nine groups (which we call "Reds Cyan" …).
#[test]
fn repeated_names_pair_by_their_group() {
    let mut defs = vec![
        def("ADBE SelectiveColor-0000", 0, "", |_| {}),
        popup("ADBE SelectiveColor-0001", "Method:", "Relative|Absolute", 1),
        float_slider("ADBE SelectiveColor-0003", "Cyan", 0.0, 100.0),
        float_slider("ADBE SelectiveColor-0005", "Yellow", 0.0, 100.0),
        def("ADBE SelectiveColor-0007", 13, "Details", |_| {}),
    ];
    for (i, g) in ["Reds", "Yellows"].iter().enumerate() {
        let n = 8 + 6 * i;
        defs.push(def(&format!("ADBE SelectiveColor-{n:04}"), 13, g, |_| {}));
        defs.push(float_slider(&format!("ADBE SelectiveColor-{:04}", n + 1), "Cyan", 0.0, 100.0));
        defs.push(float_slider(&format!("ADBE SelectiveColor-{:04}", n + 3), "Yellow", 0.0, 100.0));
        defs.push(def(&format!("ADBE SelectiveColor-{:04}", n + 5), 14, "", |_| {}));
    }
    defs.push(def("ADBE SelectiveColor-0062", 14, "", |_| {}));
    let fx = effect("Selective Color", &defs, 1, &[stored("ADBE SelectiveColor-0015", 1, &[45.0]), stored("ADBE SelectiveColor-0017", 1, &[-65.0])].concat());
    let (p, r) = run(&project(&[vec![("ADBE SelectiveColor", fx)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!((num(&g, "details/yellows/yellowsCyan"), num(&g, "details/yellows/yellowsYellow")), (45.0, -65.0));
    assert_eq!((num(&g, "details/reds/redsCyan"), num(&g, "details/reds/redsYellow")), (0.0, 0.0));
}

/// Colorama's Output Cycle (here: black to white, the usual way to take the colour out)
/// arrives as our Custom palette.
#[test]
fn colorama_reads_its_output_cycle() {
    let cycle = |stops: &[(u32, [u8; 4])]| {
        let mut b = vec![0x5au8; 512];
        for (i, (pos, argb)) in stops.iter().enumerate() {
            put(&mut b, 8 * i, &pos.to_be_bytes());
            put(&mut b, 8 * i + 4, argb);
        }
        b.extend((stops.len() as u32).to_be_bytes());
        b.extend([0u8; 4]);
        b
    };
    let wheel = cycle(&[(0, [255, 255, 0, 0]), (0x2aaa, [255, 255, 255, 0]), (0x5555, [255, 0, 255, 0])]);
    let defs = vec![
        def("APC Colorama-0000", 0, "", |_| {}),
        popup("APC Colorama-0002", "Get Phase From", "Intensity|Red|Green|Blue|Hue|Lightness|Saturation|Value|Alpha|Zero", 1),
        private("APC Colorama-0012", "Output Cycle", &wheel),
    ];
    let ramp = cycle(&[(0, [255, 0, 0, 0]), (0xffff, [128, 255, 255, 255])]);
    let fx = effect("Colorama", &defs, 1, &[tdmn("APC Colorama-0012"), list(b"tdbs", &[tdb4(1, false)]), list(b"aRbs", &[chunk(b"aRbp", &ramp)])]);
    let (p, r) = run(&project(&[vec![("APC Colorama", fx)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!(g.prop("outputCycle/usePresetPalette").unwrap().value, Value::Enum(8));
    assert_eq!(g.prop("outputCycle/palette").unwrap().value, Value::Str("0:0,0,0 0.999984:1,1,1,0.502".into()));
    assert_eq!(g.prop("inputPhase/mode").unwrap().value, Value::Enum(0));

    // A count past the room for stops is not believed.
    let mut bad = ramp.clone();
    put(&mut bad, 512, &65u32.to_be_bytes());
    let fx = effect("Colorama", &defs, 1, &[tdmn("APC Colorama-0012"), list(b"tdbs", &[tdb4(1, false)]), list(b"aRbs", &[chunk(b"aRbp", &bad)])]);
    let (p, r) = run(&project(&[vec![("APC Colorama", fx)]]));
    assert!(!effect_of(&p, &r, 0, 0).enabled);
    assert!(r.warnings.iter().any(|w| w.ends_with("could not be carried over: Output Cycle")), "{:?}", r.warnings);
}

/// A layer chooser points at a layer of the same composition by its id; names that differ
/// only in their brackets still pair up; a checkbox pairs by the label beside its box.
#[test]
fn layer_choosers_point_at_their_layer() {
    let defs = vec![
        def("ADBE Time Displacement-0000", 0, "", |_| {}),
        def("ADBE Time Displacement-0001", 0, "Time Displacement Layer", |_| {}),
        float_slider("ADBE Time Displacement-0002", "Max Displacement Time [sec]", 1.0, 30.0),
        float_slider("ADBE Time Displacement-0003", "Time Resolution [fps]", 60.0, 999.0),
        {
            let mut v = def("ADBE Time Displacement-0004", 4, "If Layer Sizes Differ", |d| put(d, 60, &[1]));
            v.push(chunk(b"pdnm", b"Stretch Map to Fit"));
            v
        },
    ];
    let chooser = |layer: u32| vec![tdmn("ADBE Time Displacement-0001"), list(b"tdbs", &[tdb4(1, false), chunk(b"cdat", &f64s(&[0.0])), chunk(b"tdpi", &layer.to_be_bytes())])];
    let with = |layer: u32| {
        effect("Time Displacement", &defs, 1, &[chooser(layer), stored("ADBE Time Displacement-0003", 1, &[24.0]), stored("ADBE Time Displacement-0004", 1, &[0.0])].concat())
    };
    // Layer 1 uses layer 2 as its map; layer 2 has no effects.
    let (p, r) = run(&project(&[vec![("ADBE Time Displacement", with(2))], vec![]]));
    let comp = p.comp(r.main_comp.unwrap()).unwrap();
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!(g.prop("displacementMapLayer").unwrap().value, Value::Layer(Some(comp.layers[1].id.0)));
    assert_eq!((num(&g, "timeResolution"), num(&g, "maxDisplacementTime")), (24.0, 1.0));
    assert_eq!(g.prop("stretchMap").unwrap().value, Value::Bool(false));
    // A layer that is not in the composition: the effect stays off and says so.
    let (p, r) = run(&project(&[vec![("ADBE Time Displacement", with(99))]]));
    assert!(!effect_of(&p, &r, 0, 0).enabled);
    assert!(r.warnings.iter().any(|w| w.contains("the layer it uses was not found")), "{:?}", r.warnings);
}

/// CC Spotlight stores a Gel Layer we have no parameter for. It only matters in the "Gel"
/// render modes, so it keeps the effect off only then.
#[test]
fn a_gel_layer_only_counts_in_a_gel_render_mode() {
    let defs = vec![
        def("CC Spotlight-0000", 0, "", |_| {}),
        slider("CC Spotlight-0007", "Intensity", 100.0, 400.0, false),
        popup("CC Spotlight-0008", "Render", "Light Only|Light Add|Gel Only|Gel Add", 2),
        def("CC Spotlight-0009", 0, "Gel Layer", |_| {}),
    ];
    let gel = vec![tdmn("CC Spotlight-0009"), list(b"tdbs", &[tdb4(1, false), chunk(b"cdat", &f64s(&[0.0])), chunk(b"tdpi", &1u32.to_be_bytes())])];
    let light = effect("CC Spotlight", &defs, 1, &[stored("CC Spotlight-0007", 1, &[123.0]), gel.clone()].concat());
    let (p, r) = run(&project(&[vec![("CC Spotlight", light)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!(num(&g, "intensity"), 123.0);
    let with_gel = effect("CC Spotlight", &defs, 1, &[stored("CC Spotlight-0008", 1, &[3.0]), gel].concat());
    let (p, r) = run(&project(&[vec![("CC Spotlight", with_gel)]]));
    assert!(!effect_of(&p, &r, 0, 0).enabled);
    assert!(r.warnings.iter().any(|w| w.contains("could not be carried over") && w.contains("Gel Layer")), "{:?}", r.warnings);
}

/// CC Spotlight: a Cone Angle past our widest is held there and the effect stays on, and
/// Intensity over 100% is held at 100% in the Light Shadow mode, where After Effects does not
/// brighten the lit area. Each is said in the import list.
#[test]
fn cc_spotlight_cone_angle_and_light_shadow_intensity() {
    let defs = |render: i16| {
        vec![
            def("CC Spotlight-0000", 0, "", |_| {}),
            slider("CC Spotlight-0004", "Cone Angle", 10.0, 360.0, false),
            slider("CC Spotlight-0007", "Intensity", 100.0, 400.0, false),
            popup("CC Spotlight-0008", "Render", "Light Only|Light Add|Light Add+|Light Shadow|(-|Gel Only|Gel Add|Gel Add+|Gel Shadow", render),
        ]
    };
    let key = |t: i32, v: f64| {
        let mut k = key_head(t, 1);
        k.extend([v, 0.0, 0.0, 0.0, 0.0].iter().flat_map(|x| x.to_be_bytes()));
        k
    };
    let scale = crate::settings::SPOT_CONE_SCALE;
    let said = |r: &crate::ImportResult, part: &str| r.warnings.iter().any(|w| w.starts_with("effect CC Spotlight") && w.contains(part));
    // As a title has it: Light Shadow (the default), 123%, the cone opening from 12.8 to 143.9.
    let cone = list(b"tdbs", &[tdb4(1, false), key_list(48, &[key(0, 12.8), key(30_000, 143.9)])]);
    let wide = effect("CC Spotlight", &defs(4), 1, &[vec![tdmn("CC Spotlight-0004"), cone], stored("CC Spotlight-0007", 1, &[123.0])].concat());
    let (p, r) = run(&project(&[vec![("CC Spotlight", wide)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    let keys = &g.prop("coneAngle").unwrap().keys;
    assert_eq!(keys.len(), 2);
    assert_eq!((&keys[0].value, &keys[1].value), (&Value::Scalar((12.8 * scale).min(89.9)), &Value::Scalar(89.9)));
    assert_eq!(num(&g, "intensity"), 100.0);
    assert!(said(&r, "Cone Angle goes past") && said(&r, "Intensity above 100% is held at 100%"), "{:?}", r.warnings);
    // Left alone: After Effects' default cone, and nothing to say about holding anything.
    let plain = effect("CC Spotlight", &defs(4), 1, &[]);
    let (p, r) = run(&project(&[vec![("CC Spotlight", plain)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!((num(&g, "coneAngle"), num(&g, "intensity")), (10.0 * scale, 100.0));
    assert!(!said(&r, "held at"), "{:?}", r.warnings);
    // In another mode the light does brighten: the number stays.
    let add = effect("CC Spotlight", &defs(2), 1, &stored("CC Spotlight-0007", 1, &[123.0]));
    let (p, r) = run(&project(&[vec![("CC Spotlight", add)]]));
    assert_eq!(num(&effect_of(&p, &r, 0, 0), "intensity"), 123.0);
    assert!(!said(&r, "Intensity"), "{:?}", r.warnings);
}

/// Fast Blur has no twin here: Gaussian Blur stands in with the same Blurriness, and the
/// import list says so.
#[test]
fn fast_blur_comes_in_as_gaussian_blur() {
    let defs = vec![
        def("ADBE Fast Blur-0000", 0, "", |_| {}),
        slider("ADBE Fast Blur-0001", "Blurriness", 0.0, 32767.0, false),
        popup("ADBE Fast Blur-0002", "Blur Dimensions", "Horizontal and Vertical|Horizontal|Vertical", 1),
        checkbox("ADBE Fast Blur-0003", "Repeat Edge Pixels", false),
    ];
    let fx = effect("Fast Blur", &defs, 1, &[stored("ADBE Fast Blur-0001", 1, &[23.0]), stored("ADBE Fast Blur-0003", 1, &[1.0])].concat());
    let (p, r) = run(&project(&[vec![("ADBE Fast Blur", fx)]]));
    let g = effect_of(&p, &r, 0, 0);
    assert!(g.enabled, "{:?}", r.warnings);
    assert_eq!(num(&g, "blurriness"), 23.0);
    assert_eq!(g.prop("repeatEdge").unwrap().value, Value::Bool(true));
    assert!(r.warnings.iter().any(|w| w.starts_with("effect Fast Blur: imported as Gaussian Blur")), "{:?}", r.warnings);
}

/// Damaged effect data must give an error or a partial import, never a panic.
#[test]
fn truncated_and_corrupted_effect_data_never_panics() {
    let grid = effect("Grid", &grid_defs(), 1, &[stored("ADBE Grid-0002", 1, &[3.0]), stored("ADBE Grid-0012", 4, &[255.0, 0.0, 0.0, 0.0])].concat());
    let levels = effect(
        "Levels",
        &levels_defs(),
        1,
        &[tdmn("ADBE Easy Levels2-0002"), list(b"tdbs", &[tdb4(1, false)]), list(b"aRbs", &[chunk(b"aRbp", &levels_block(LEVELS_UNTOUCHED))])],
    );
    let good = project(&[vec![("ADBE Grid", grid), ("ADBE Easy Levels2", levels)]]);
    let mut none = |_: &FootageRef| -> Option<Footage> { None };
    for n in 0..good.len() {
        let mut p = effectcraft_project::Project::default();
        let _ = import(&mut p, &good[..n], &mut none);
    }
    for i in 0..good.len() {
        for v in [0xffu8, 0x00, 0x7f] {
            let mut bad = good.clone();
            bad[i] = v;
            let mut p = effectcraft_project::Project::default();
            let _ = import(&mut p, &bad, &mut none);
        }
    }
    // The curve block on its own: every count and coordinate hostile.
    let defs = vec![def("ADBE CurvesCustom-0000", 0, "", |_| {}), private("ADBE CurvesCustom-0001", "Curves", &[])];
    let block = curves_block(&[(0, 0), (255, 255)]);
    for i in (1284..block.len()).chain([0, 3, 700]) {
        let mut bad = block.clone();
        bad[i] = 0xff;
        let fx = effect("Curves", &defs, 1, &[tdmn("ADBE CurvesCustom-0001"), list(b"tdbs", &[tdb4(1, false)]), list(b"aRbs", &[chunk(b"aRbp", &bad)])]);
        let mut p = effectcraft_project::Project::default();
        let _ = import(&mut p, &project(&[vec![("ADBE CurvesCustom", fx)]]), &mut none);
    }
}
