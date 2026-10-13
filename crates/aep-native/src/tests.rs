//! Tests on synthetic `.aep` data built here chunk by chunk (no real project files).

use effectcraft_keyframe::{Interp, Value};
use effectcraft_project::{Footage, ItemKind, LayerSource, MaskMode, MatteKind, Project};
use effectcraft_time::Tick;

use crate::{FootageRef, import, is_aep};

#[path = "tests_settings.rs"]
mod settings;

fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend((data.len() as u32).to_be_bytes());
    v.extend(data);
    if data.len() % 2 == 1 {
        v.push(0);
    }
    v
}

fn list(ty: &[u8; 4], kids: &[Vec<u8>]) -> Vec<u8> {
    let mut d = ty.to_vec();
    for k in kids {
        d.extend(k);
    }
    chunk(b"LIST", &d)
}

fn file(kids: &[Vec<u8>]) -> Vec<u8> {
    let mut body = b"Egg!".to_vec();
    for k in kids {
        body.extend(k);
    }
    let mut v = b"RIFX".to_vec();
    v.extend((body.len() as u32).to_be_bytes());
    v.extend(body);
    v
}

fn put(d: &mut [u8], at: usize, bytes: &[u8]) {
    d[at..at + bytes.len()].copy_from_slice(bytes);
}

fn tdmn(name: &str) -> Vec<u8> {
    let mut d = vec![0u8; 40];
    put(&mut d, 0, name.as_bytes());
    chunk(b"tdmn", &d)
}

fn idta(ty: u16, id: u32) -> Vec<u8> {
    let mut d = vec![0u8; 84];
    put(&mut d, 0, &ty.to_be_bytes());
    put(&mut d, 16, &id.to_be_bytes());
    chunk(b"idta", &d)
}

/// A composition at 30 fps (time scale 30 000) lasting `frames` frames.
fn cdta(w: u16, h: u16, frames: i32) -> Vec<u8> {
    cdta_scale(w, h, frames, 30_000)
}

/// A composition whose frame lasts 1000 units of `scale` (23 976 for 23.976 fps), `frames` long.
fn cdta_scale(w: u16, h: u16, frames: i32, scale: u32) -> Vec<u8> {
    let mut d = vec![0u8; 204];
    put(&mut d, 4, &1000u32.to_be_bytes());
    put(&mut d, 8, &scale.to_be_bytes());
    put(&mut d, 36, &(-1i32).to_be_bytes());
    put(&mut d, 40, &600u32.to_be_bytes());
    put(&mut d, 44, &(frames * 1000).to_be_bytes());
    put(&mut d, 48, &scale.to_be_bytes());
    put(&mut d, 140, &w.to_be_bytes());
    put(&mut d, 142, &h.to_be_bytes());
    put(&mut d, 144, &1u32.to_be_bytes());
    put(&mut d, 148, &1u32.to_be_bytes());
    chunk(b"cdta", &d)
}

struct L {
    id: u32,
    source: u32,
    start: i32,
    inp: i32,
    out: i32,
    stretch: (i32, u32),
    flags: [u8; 3],
    blend: u32,
    matte: u32,
    ty: u32,
    parent: u32,
    /// The time scale of start, in and out.
    scale: u32,
}

impl Default for L {
    fn default() -> Self {
        L { id: 1, source: 0, start: 0, inp: 0, out: 30_000, stretch: (1, 1), flags: [0, 0, 0x07], blend: 2, matte: 0, ty: 0, parent: 0, scale: 30_000 }
    }
}

/// Layer data with times in a 30 000 time scale (or `scale`).
fn ldta(l: &L) -> Vec<u8> {
    let mut d = vec![0u8; 160];
    put(&mut d, 0, &l.id.to_be_bytes());
    put(&mut d, 4, &2u16.to_be_bytes());
    put(&mut d, 8, &l.stretch.0.to_be_bytes());
    put(&mut d, 108, &l.stretch.1.to_be_bytes());
    for (at, v) in [(12, l.start), (20, l.inp), (28, l.out)] {
        put(&mut d, at, &v.to_be_bytes());
        put(&mut d, at + 4, &l.scale.to_be_bytes());
    }
    put(&mut d, 37, &l.flags);
    put(&mut d, 40, &l.source.to_be_bytes());
    put(&mut d, 61, &[9]);
    put(&mut d, 96, &l.blend.to_be_bytes());
    put(&mut d, 104, &l.matte.to_be_bytes());
    put(&mut d, 128, &l.ty.to_be_bytes());
    put(&mut d, 132, &l.parent.to_be_bytes());
    chunk(b"ldta", &d)
}

fn tdb4(dims: u16, spatial: bool) -> Vec<u8> {
    let mut d = vec![0u8; 124];
    put(&mut d, 0, &[0xdb, 0x99]);
    put(&mut d, 2, &dims.to_be_bytes());
    put(&mut d, 5, &[if spatial { 0x0e } else { 0x00 }]);
    put(&mut d, 12, &30_000u32.to_be_bytes());
    chunk(b"tdb4", &d)
}

fn f64s(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

/// A property with a fixed value.
fn fixed(dims: u16, value: &[f64]) -> Vec<u8> {
    list(b"tdbs", &[tdb4(dims, false), chunk(b"cdat", &f64s(value))])
}

fn key_list(size: u32, keys: &[Vec<u8>]) -> Vec<u8> {
    let mut head = vec![0u8; 52];
    put(&mut head, 8, &(keys.len() as u32).to_be_bytes());
    put(&mut head, 16, &size.to_be_bytes());
    list(b"list", &[chunk(b"lhd3", &head), chunk(b"ldat", &keys.concat())])
}

fn key_head(time: i32, interp: u8) -> Vec<u8> {
    let mut k = time.to_be_bytes().to_vec();
    k.extend([interp, interp, 0, 0]);
    k
}

/// A one-dimensional key: value, in speed, in influence, out speed, out influence.
fn key1(time: i32, interp: u8, v: f64) -> Vec<u8> {
    let mut k = key_head(time, interp);
    k.extend(f64s(&[v, 0.0, 1.0 / 3.0, 0.0, 1.0 / 3.0]));
    k
}

/// A three-dimensional spatial key with its tangents.
fn key_pos(time: i32, v: [f64; 3], tan_out: [f64; 3]) -> Vec<u8> {
    let mut k = key_head(time, 1);
    k.extend([0u8; 16]);
    k.extend(f64s(&[0.0, 0.0, 0.0, 0.0]));
    k.extend(f64s(&v));
    k.extend(f64s(&[0.0, 0.0, 0.0]));
    k.extend(f64s(&tan_out));
    k
}

fn group(kids: &[Vec<u8>]) -> Vec<u8> {
    let mut all = kids.to_vec();
    all.push(tdmn("ADBE Group End"));
    list(b"tdgp", &all)
}

fn solid_item(id: u32, w: u32, h: u32, argb: [f32; 4], name: &str) -> Vec<u8> {
    let mut sspc = vec![0u8; 286];
    put(&mut sspc, 30, &w.to_be_bytes());
    put(&mut sspc, 34, &h.to_be_bytes());
    let mut opti = b"Soli".to_vec();
    opti.extend([0u8; 6]);
    for c in argb {
        opti.extend(c.to_be_bytes());
    }
    opti.extend(name.as_bytes());
    opti.push(0);
    list(b"Item", &[idta(7, id), chunk(b"Utf8", b""), list(b"Pin ", &[chunk(b"sspc", &sspc), chunk(b"opti", &opti)])])
}

fn footage_item(id: u32, w: u32, h: u32, path: &str) -> Vec<u8> {
    let mut sspc = vec![0u8; 286];
    put(&mut sspc, 30, &w.to_be_bytes());
    put(&mut sspc, 34, &h.to_be_bytes());
    put(&mut sspc, 38, &90u32.to_be_bytes());
    put(&mut sspc, 42, &30u32.to_be_bytes());
    put(&mut sspc, 58, &(30u32 << 16).to_be_bytes());
    let alias = format!("{{\"fullpath\":\"{}\",\"os\":\"win\"}}", path.replace('\\', "\\\\"));
    let opti = b"MooV".to_vec();
    list(
        b"Item",
        &[idta(7, id), chunk(b"Utf8", b""), list(b"Pin ", &[chunk(b"sspc", &sspc), list(b"Als2", &[chunk(b"alas", alias.as_bytes())]), chunk(b"opti", &opti)])],
    )
}

fn mask(points: &[[f32; 2]]) -> Vec<Vec<u8>> {
    let mut mkif = vec![0u8; 48];
    put(&mut mkif, 0, &[1]);
    put(&mut mkif, 6, &2u16.to_be_bytes());
    let mut shph = vec![0u8; 24];
    put(&mut shph, 3, &[1]);
    for (i, v) in [0.25f32, 0.25, 0.75, 0.75].iter().enumerate() {
        put(&mut shph, 4 + 4 * i, &v.to_be_bytes());
    }
    let pts: Vec<Vec<u8>> = points.iter().map(|p| [p[0].to_be_bytes(), p[1].to_be_bytes()].concat()).collect();
    let shape = list(b"shap", &[chunk(b"shph", &shph), key_list(8, &pts)]);
    let path = list(b"om-s", &[list(b"tdbs", &[tdb4(1, false)]), list(b"omks", &[shape])]);
    let inner = {
        let mut kids = vec![chunk(b"tdsn", b"Vignette\0")];
        kids.extend([tdmn("ADBE Mask Shape"), path, tdmn("ADBE Mask Feather"), fixed(2, &[12.0, 12.0]), tdmn("ADBE Mask Opacity"), fixed(1, &[0.5])]);
        group(&kids)
    };
    vec![tdmn("ADBE Mask Atom"), chunk(b"mkif", &mkif), inner]
}

/// A project: folder "Solids" (id 10) holding a white-ish solid (11); a movie link (12); a
/// precomp (20) with one solid layer; a main comp (30) with a movie layer matted by a precomp
/// layer, and a solid layer with keyframes, a mask and an effect, parented to the movie.
fn sample() -> Vec<u8> {
    sample_stretched((2, 1))
}

/// [`sample`] with the movie layer's time stretch as given.
fn sample_stretched(stretch: (i32, u32)) -> Vec<u8> {
    let solids = list(b"Item", &[idta(1, 10), chunk(b"Utf8", b"Solids"), list(b"Sfdr", &[solid_item(11, 640, 360, [1.0, 1.0, 0.5, 0.25], "Warm Solid 1")])]);
    let movie = footage_item(12, 1280, 720, "C:\\shots\\plate.mov");
    let pre_layer = list(b"Layr", &[ldta(&L { id: 1, source: 11, ..Default::default() }), chunk(b"Utf8", b""), group(&[])]);
    let precomp = list(b"Item", &[idta(4, 20), chunk(b"Utf8", b"Matte"), cdta(640, 360, 60), pre_layer]);

    let matte_layer = list(b"Layr", &[ldta(&L { id: 5, source: 20, flags: [0, 0, 0x06], ..Default::default() }), chunk(b"Utf8", b"Matte layer"), group(&[])]);
    let movie_layer = list(
        b"Layr",
        &[
            ldta(&L { id: 6, source: 12, start: 15_000, inp: 3_000, out: 33_000, stretch, flags: [0, 0, 0x17], blend: 6, matte: 3, ..Default::default() }),
            chunk(b"Utf8", b""),
            group(&[]),
        ],
    );
    let transform = group(&[
        tdmn("ADBE Anchor Point"),
        fixed(3, &[320.0, 180.0, 0.0]),
        tdmn("ADBE Position"),
        list(b"tdbs", &[tdb4(3, true), key_list(128, &[key_pos(0, [100.0, 50.0, 0.0], [10.0, -5.0, 0.0]), key_pos(30_000, [400.0, 300.0, 0.0], [0.0; 3])])]),
        tdmn("ADBE Scale"),
        fixed(3, &[0.5, 0.5, 1.0]),
        tdmn("ADBE Rotate Z"),
        fixed(1, &[33.0]),
        tdmn("ADBE Opacity"),
        list(b"tdbs", &[tdb4(1, false), key_list(48, &[key1(0, 2, 0.0), key1(15_000, 3, 1.0)])]),
    ]);
    let fx = {
        let mut name = vec![0u8; 48];
        put(&mut name, 0, b"Invert");
        list(b"sspc", &[chunk(b"fnam", &name), group(&[tdmn("ADBE Invert-0001"), fixed(1, &[0.0])])])
    };
    let unknown_fx = {
        let mut name = vec![0u8; 48];
        put(&mut name, 0, b"Made Up");
        list(b"sspc", &[chunk(b"fnam", &name), group(&[])])
    };
    let solid_layer = list(
        b"Layr",
        &[
            ldta(&L { id: 7, source: 11, flags: [0, 0x02, 0x07], blend: 8, parent: 6, ..Default::default() }),
            chunk(b"Utf8", b"Grade"),
            group(&[
                tdmn("ADBE Mask Parade"),
                group(&mask(&[[0.0, 0.0], [0.25, 0.0], [0.75, 0.0], [1.0, 0.0], [1.0, 0.25], [1.0, 0.75], [1.0, 1.0], [0.75, 1.0], [0.25, 1.0]])),
                tdmn("ADBE Effect Parade"),
                group(&[tdmn("ADBE Invert"), fx, tdmn("ACME Made Up"), unknown_fx]),
                tdmn("ADBE Transform Group"),
                transform,
            ]),
        ],
    );
    let main = list(b"Item", &[idta(4, 30), chunk(b"Utf8", b"Main"), cdta(1280, 720, 90), matte_layer, movie_layer, solid_layer]);
    file(&[chunk(b"head", &[0; 20]), list(b"Fold", &[solids, movie, precomp, main])])
}

fn run(bytes: &[u8]) -> (Project, crate::ImportResult) {
    let mut p = Project::default();
    let mut none = |_: &FootageRef| -> Option<Footage> { None };
    let r = import(&mut p, bytes, &mut none).unwrap();
    (p, r)
}

#[test]
fn recognises_the_container() {
    assert!(is_aep(&sample()));
    assert!(!is_aep(b"RIFF\0\0\0\0WAVE"));
    let mut p = Project::default();
    let mut none = |_: &FootageRef| -> Option<Footage> { None };
    assert!(import(&mut p, b"{\"not\": \"an aep\"}", &mut none).is_err());
    assert!(import(&mut p, b"RIFX\0\0\0\x04WAVE", &mut none).is_err());
}

#[test]
fn items_and_compositions() {
    let (p, r) = run(&sample());
    assert_eq!(r.items.len(), 5);
    assert_eq!(r.comps.len(), 2);
    let folder = p.find_by_name("Solids").unwrap();
    assert!(folder.is_folder());
    let solid = p.find_by_name("Warm Solid 1").unwrap();
    assert_eq!(solid.parent, Some(folder.id));
    let ItemKind::Solid(s) = &solid.kind else { panic!("not a solid") };
    assert_eq!((s.width, s.height, s.color), (640, 360, [1.0, 0.5, 0.25]));
    // The link that was not found is kept as missing footage with the recorded facts.
    let movie = p.find_by_name("plate.mov").unwrap();
    let ItemKind::Footage(f) = &movie.kind else { panic!("not footage") };
    assert!(f.missing);
    assert_eq!((f.width, f.height, f.path.as_str()), (1280, 720, "C:\\shots\\plate.mov"));
    assert_eq!(f.duration, Tick::from_units(3, 1));
    assert_eq!(r.missing_footage, 1);
    assert!(r.warnings.iter().any(|w| w.contains("linked file not found") && w.contains("plate.mov")));
    let main = p.comp(r.main_comp.unwrap()).unwrap();
    assert_eq!((main.width, main.height), (1280, 720));
    assert!((main.frame_rate.as_f64() - 30.0).abs() < 1e-9);
    assert_eq!(main.duration, Tick::from_units(3, 1));
    assert_eq!(main.work_area, (Tick::ZERO, main.duration));
    assert_eq!(p.find_by_name("Main").unwrap().id, r.main_comp.unwrap());
}

#[test]
fn layers_timing_switches_and_links() {
    let (p, r) = run(&sample());
    let main = p.comp(r.main_comp.unwrap()).unwrap();
    assert_eq!(r.layers, 4);
    let names: Vec<&str> = main.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Matte layer", "plate.mov", "Grade"]);
    let (matte, movie, grade) = (&main.layers[0], &main.layers[1], &main.layers[2]);
    assert!(matches!(matte.source, LayerSource::Comp { .. }));
    assert!(!matte.switches.video);
    // Start 0.5 s; in 0.1 s and out 1.1 s of layer time at a 200% stretch.
    assert_eq!(movie.start_time, Tick::from_units(1, 2));
    assert_eq!(movie.in_point, Tick::from_units(7, 10));
    assert_eq!(movie.out_point, Tick::from_units(27, 10));
    assert_eq!(movie.stretch, 200.0);
    assert_eq!(movie.blend_mode, effectcraft_color::BlendMode::Screen);
    assert_ne!(movie.switches.frame_blend, effectcraft_project::FrameBlend::Off);
    let tm = movie.track_matte.unwrap();
    assert_eq!((tm.layer, tm.kind), (matte.id, MatteKind::Luma));
    assert!(grade.switches.adjustment);
    assert_eq!(grade.parent, Some(movie.id));
    assert_eq!(grade.blend_mode, effectcraft_color::BlendMode::SoftLight);
    assert_eq!(grade.label, effectcraft_color::Label::Green);
    assert!(!r.warnings.iter().any(|w| w.contains("time stretch")), "{:?}", r.warnings);
}

/// A 23.976 fps composition stores its times for 23976/1000 frames a second and runs here at
/// 24000/1001. Carried over by seconds, a layer that starts on frame 34 would start a hair
/// after it and show one frame late; carried over by frame count it starts on the frame.
#[test]
fn a_23_976_composition_keeps_its_times_on_its_frames() {
    let solids = list(b"Item", &[idta(1, 10), chunk(b"Utf8", b"Solids"), list(b"Sfdr", &[solid_item(11, 640, 360, [1.0, 1.0, 0.5, 0.25], "Solid")])]);
    let opacity = group(&[tdmn("ADBE Opacity"), list(b"tdbs", &[tdb4(1, false), key_list(48, &[key1(0, 2, 0.0), key1(30_000, 3, 1.0)])])]);
    let shot = L { id: 1, source: 11, start: 34_000, inp: 0, out: 48_000, scale: 23_976, ..Default::default() };
    let layer = list(b"Layr", &[ldta(&shot), chunk(b"Utf8", b"Shot"), group(&[tdmn("ADBE Transform Group"), opacity])]);
    let comp = list(b"Item", &[idta(4, 30), chunk(b"Utf8", b"Comp"), cdta_scale(1920, 1080, 504, 23_976), layer]);
    let (p, r) = run(&file(&[list(b"Fold", &[solids, comp])]));
    let c = p.comp(r.main_comp.unwrap()).unwrap();
    let rate = effectcraft_time::FrameRate::new(24_000, 1001);
    assert_eq!(c.frame_rate, rate);
    assert_eq!(c.duration, rate.tick_of(504));
    let l = &c.layers[0];
    assert_eq!((l.start_time, l.in_point, l.out_point), (rate.tick_of(34), rate.tick_of(34), rate.tick_of(82)));
    // A key one second into the layer on the file's clock is 23.976 frames in: just short of a second here.
    let keys = &l.props.prop("transform/opacity").unwrap().keys;
    assert_eq!(keys[0].time, Tick::ZERO);
    assert_eq!(keys[1].time, Tick((Tick::from_units(1, 1).0 as i128 * 23_999_976 / 24_000_000) as i64));
    // A 30 fps composition's times are left as they are (the sample project's tests).
}

/// Time stretch is a fraction (numerator @8, denominator @108 of the layer record): 70% and
/// 75% arrive as such, not as 700% and 300%, and the in and out points scale with it.
#[test]
fn time_stretch_is_a_fraction() {
    let movie = |stretch| {
        let (p, r) = run(&sample_stretched(stretch));
        let l = p.comp(r.main_comp.unwrap()).unwrap().layers[1].clone();
        (l.stretch, l.start_time, l.in_point, l.out_point, r.warnings.iter().any(|w| w.contains("time stretch")))
    };
    let near = |t: Tick, num: i64, den: i64| (t.0 - Tick::from_units(num, den).0).abs() <= 2;
    // Start 0.5 s; in 0.1 s and out 1.1 s of layer time.
    let (s, start, a, b, warned) = movie((7, 10));
    assert!((s - 70.0).abs() < 1e-9 && start == Tick::from_units(1, 2) && !warned, "{s}");
    assert!(near(a, 57, 100) && near(b, 127, 100), "70%: {a:?} to {b:?}");
    let (s, _, a, b, _) = movie((3, 4));
    assert!((s - 75.0).abs() < 1e-9 && near(a, 575, 1000) && near(b, 1325, 1000), "75%: {s} {a:?} to {b:?}");
    let (s, _, a, b, _) = movie((1, 2));
    assert!((s - 50.0).abs() < 1e-9 && near(a, 55, 100) && near(b, 105, 100), "50%: {s} {a:?} to {b:?}");
    // Reversed: the layer runs backwards from its start, so in and out swap sides.
    let (s, _, a, b, warned) = movie((-1, 1));
    assert!((s + 100.0).abs() < 1e-9 && near(a, -6, 10) && near(b, 4, 10) && !warned, "reversed: {s} {a:?} to {b:?}");
    // A zero on either side, or a factor no timeline could show, is named and read as 100%.
    for bad in [(0, 1), (3, 0), (i32::MAX, 1), (1, u32::MAX)] {
        let (s, _, a, b, warned) = movie(bad);
        assert!(s == 100.0 && near(a, 6, 10) && near(b, 16, 10) && warned, "{bad:?}: {s} {a:?} to {b:?}");
    }
}

#[test]
fn transform_values_and_keyframes() {
    let (p, r) = run(&sample());
    let grade = &p.comp(r.main_comp.unwrap()).unwrap().layers[2];
    let tr = grade.transform().unwrap();
    assert_eq!(tr.get("anchor").unwrap().value, Value::Vec3([320.0, 180.0, 0.0]));
    // Scale and opacity are fractions in the file, percent here.
    assert_eq!(tr.get("scale").unwrap().value, Value::Vec3([50.0, 50.0, 100.0]));
    assert_eq!(tr.get("rotation").unwrap().value, Value::Scalar(33.0));
    let pos = tr.get("position").unwrap();
    assert_eq!(pos.keys.len(), 2);
    assert_eq!(pos.keys[0].value, Value::Vec3([100.0, 50.0, 0.0]));
    assert_eq!(pos.keys[0].spatial_out, [10.0, -5.0, 0.0]);
    assert!(!pos.keys[0].spatial_auto);
    assert_eq!(pos.keys[1].time, Tick::from_units(1, 1));
    assert_eq!(pos.keys[1].value, Value::Vec3([400.0, 300.0, 0.0]));
    let op = tr.get("opacity").unwrap();
    assert_eq!(op.keys.len(), 2);
    assert_eq!((op.keys[0].out_interp, &op.keys[0].value), (Interp::Bezier, &Value::Scalar(0.0)));
    assert!((op.keys[0].out_ease[0].influence - 1.0 / 3.0).abs() < 1e-12);
    assert_eq!((op.keys[1].in_interp, &op.keys[1].value, op.keys[1].time), (Interp::Hold, &Value::Scalar(100.0), Tick::from_units(1, 2)));
    assert_eq!(op.value_at(Tick::ZERO), Value::Scalar(0.0));
}

#[test]
fn masks_and_effects() {
    let (p, r) = run(&sample());
    let grade = &p.comp(r.main_comp.unwrap()).unwrap().layers[2];
    let m = grade.masks().unwrap().groups().next().unwrap();
    assert_eq!(m.name, "Vignette");
    let effectcraft_project::GroupKind::Mask { mode, inverted, .. } = &m.kind else { panic!("not a mask") };
    assert_eq!((*mode, *inverted), (MaskMode::Subtract, true));
    let Value::Path(path) = &m.get("path").unwrap().value else { panic!("not a path") };
    // Three points per vertex inside the bounding box (0.25..0.75 of a 640×360 layer).
    assert!(path.closed);
    assert_eq!(path.vertices, vec![[160.0, 90.0], [480.0, 90.0], [480.0, 270.0]]);
    assert_eq!(path.out_tangents[0], [80.0, 0.0]);
    assert_eq!(path.in_tangents[1], [-80.0, 0.0]);
    assert_eq!(path.in_tangents[0], [80.0, 180.0]);
    // A feather of 12 in the file: scaled on the way in (EffectCraft's feather is the wider
    // one for the same number), and the import list says so.
    let Value::Vec2(feather) = m.get("feather").unwrap().value else { panic!("feather is not a pair") };
    assert!((feather[0] - 9.12).abs() < 1e-9 && (feather[1] - 9.12).abs() < 1e-9, "{feather:?}");
    assert!(r.warnings.iter().any(|w| w.starts_with("mask feather scaled to 76%")), "{:?}", r.warnings);
    assert_eq!(m.get("opacity").unwrap().value, Value::Scalar(50.0));
    // A known effect with nothing changed arrives switched on (settings: `tests_settings.rs`);
    // an unknown one is skipped and reported.
    let fx: Vec<_> = grade.effects().unwrap().groups().collect();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0].name, "Invert");
    assert!(fx[0].enabled);
    assert!(!r.warnings.iter().any(|w| w.contains("effect Invert")), "{:?}", r.warnings);
    assert!(r.warnings.iter().any(|w| w.contains("effect Made Up (ACME Made Up) has no EffectCraft equivalent")));
}

#[test]
fn the_caller_supplies_found_footage() {
    let mut p = Project::default();
    let mut asked = vec![];
    let mut locate = |f: &FootageRef| -> Option<Footage> {
        asked.push((f.path.map(str::to_string), f.width, f.height, f.fps));
        Some(Footage { path: "D:/copies/plate.mov".into(), width: 1280, height: 720, ..Default::default() })
    };
    let r = import(&mut p, &sample(), &mut locate).unwrap();
    assert_eq!(asked, vec![(Some("C:\\shots\\plate.mov".to_string()), 1280, 720, 30.0)]);
    assert_eq!(r.missing_footage, 0);
    let ItemKind::Footage(f) = &p.find_by_name("plate.mov").unwrap().kind else { panic!("not footage") };
    assert_eq!((f.path.as_str(), f.missing), ("D:/copies/plate.mov", false));
}

/// Audio Levels (decibels, left and right) arrive with their keyframes on a layer whose
/// source has sound; on one without, the import list says they were left out.
#[test]
fn audio_levels_and_their_keyframes() {
    let key = |t: i32, db: f64| {
        let mut k = key_head(t, 1);
        k.extend(f64s(&[db, db, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
        k
    };
    let audio = group(&[tdmn("ADBE Audio Levels"), list(b"tdbs", &[tdb4(2, false), key_list(88, &[key(0, -20.0), key(30_000, 0.0)])])]);
    let layer = list(
        b"Layr",
        &[ldta(&L { id: 1, source: 12, ..Default::default() }), chunk(b"Utf8", b"Music"), group(&[tdmn("ADBE Audio Group"), audio])],
    );
    let comp = list(b"Item", &[idta(4, 30), chunk(b"Utf8", b"Comp"), cdta(1280, 720, 90), layer]);
    let bytes = file(&[list(b"Fold", &[footage_item(12, 1280, 720, "C:\\media\\music.mov"), comp])]);
    let mut p = Project::default();
    let mut with_sound = |_: &FootageRef| -> Option<Footage> { Some(Footage { path: "D:/copies/music.mov".into(), width: 1280, height: 720, has_video: true, has_audio: true, ..Default::default() }) };
    let r = import(&mut p, &bytes, &mut with_sound).unwrap();
    let l = &p.comp(r.main_comp.unwrap()).unwrap().layers[0];
    let keys = &l.props.prop("audio/levels").unwrap().keys;
    assert_eq!(keys.len(), 2);
    assert_eq!((&keys[0].value, &keys[1].value, keys[1].time), (&Value::Vec2([-20.0, -20.0]), &Value::Vec2([0.0, 0.0]), Tick::from_units(1, 1)));
    assert!(!r.warnings.iter().any(|w| w.contains("audio")), "{:?}", r.warnings);
    // The same project with the file not found: no sound to put the levels on.
    let (_, r) = run(&bytes);
    assert!(r.warnings.iter().any(|w| w.starts_with("audio levels not imported")), "{:?}", r.warnings);
}

/// A damaged or hostile file must give an error or a partial import, never a panic.
#[test]
fn truncated_and_corrupted_files_never_panic() {
    let good = sample();
    let mut none = |_: &FootageRef| -> Option<Footage> { None };
    for n in 0..good.len() {
        let mut p = Project::default();
        let _ = import(&mut p, &good[..n], &mut none);
    }
    // Every byte in turn set to 0xff and to 0x00 (sizes, counts, offsets, flags).
    for i in 0..good.len() {
        for v in [0xffu8, 0x00, 0x7f] {
            let mut bad = good.clone();
            bad[i] = v;
            let mut p = Project::default();
            let _ = import(&mut p, &bad, &mut none);
        }
    }
}

#[test]
fn a_comp_that_contains_itself_is_refused() {
    let layer = list(b"Layr", &[ldta(&L { id: 1, source: 30, ..Default::default() }), chunk(b"Utf8", b"Loop"), group(&[])]);
    let comp = list(b"Item", &[idta(4, 30), chunk(b"Utf8", b"Loop comp"), cdta(100, 100, 30), layer]);
    let (p, r) = run(&file(&[list(b"Fold", &[comp])]));
    assert!(p.comp(r.main_comp.unwrap()).unwrap().layers.is_empty());
    assert!(r.warnings.iter().any(|w| w.contains("uses its own composition")));
}

/// Expressions are not run yet: each one is reported, in both ways the file can store it.
#[test]
fn expressions_are_reported() {
    let with = |chunk_id: &[u8; 4], text: &[u8]| list(b"tdbs", &[tdb4(1, false), chunk(b"cdat", &f64s(&[1.0])), chunk(chunk_id, text)]);
    let transform = group(&[tdmn("ADBE Opacity"), with(b"expr", b"wiggle(7, 100)\0"), tdmn("ADBE Rotate Z"), with(b"Utf8", b"time * 90")]);
    let layer =
        list(b"Layr", &[ldta(&L { id: 1, source: 11, ..Default::default() }), chunk(b"Utf8", b"Wobble"), group(&[tdmn("ADBE Transform Group"), transform])]);
    let comp = list(b"Item", &[idta(4, 30), chunk(b"Utf8", b"Comp"), cdta(100, 100, 30), layer]);
    let (p, r) = run(&file(&[list(b"Fold", &[solid_item(11, 100, 100, [1.0, 0.0, 0.0, 0.0], "Black Solid 1"), comp])]));
    assert!(r.warnings.iter().any(|w| w.starts_with("expression not imported") && w.ends_with("(×2)")), "{:?}", r.warnings);
    // The values under the expressions still arrive.
    let tr = p.comp(r.main_comp.unwrap()).unwrap().layers[0].transform().unwrap().clone();
    assert_eq!(tr.get("opacity").unwrap().value, Value::Scalar(100.0));
}

/// A text layer arrives with its text, font, size and fill colour.
#[test]
fn text_layers_bring_their_text() {
    let blob = crate::text::tests::document("Episode\rTitle\r", &[(14, 0, 96.0, [1.0, 0.75, 0.5, 0.25])]);
    let btdk = chunk(b"LIST", &[b"btdk".as_slice(), blob.as_slice()].concat());
    let text = group(&[tdmn("ADBE Text Document"), list(b"btds", &[list(b"tdbs", &[tdb4(1, false)]), btdk])]);
    let layer = list(
        b"Layr",
        &[ldta(&L { id: 1, ty: 3, flags: [0, 0, 0x87], ..Default::default() }), chunk(b"Utf8", b"Title"), group(&[tdmn("ADBE Text Properties"), text])],
    );
    let comp = list(b"Item", &[idta(4, 30), chunk(b"Utf8", b"Comp"), cdta(1920, 1080, 30), layer]);
    let (p, r) = run(&file(&[list(b"Fold", &[comp])]));
    let l = &p.comp(r.main_comp.unwrap()).unwrap().layers[0];
    assert!(matches!(l.source, LayerSource::Text));
    assert!(l.switches.collapse);
    let Value::Text(doc) = &l.props.prop("text/sourceText").unwrap().value else { panic!("not text") };
    assert_eq!((doc.text.as_str(), doc.font.as_str(), doc.size), ("Episode\nTitle", "Title-Bold", 96.0));
    assert_eq!(doc.fill, [0.75, 0.5, 0.25, 1.0]);
    assert!(r.warnings.iter().any(|w| w.starts_with("text layer: its text, fonts, sizes, fill colours, tracking, leading, baseline shift, scale and alignment are imported")));
}
