//! Opening and importing an After Effects project through the command registry
//! (`effectcraft-aep-native`), on a small synthetic file built here.

use serde_json::json;

use crate::Session;

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("effectcraft-aep-native-tests-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name).to_string_lossy().to_string()
}

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

fn put(d: &mut [u8], at: usize, bytes: &[u8]) {
    d[at..at + bytes.len()].copy_from_slice(bytes);
}

/// A project with one 640x360, 30 fps, 2 s composition "Titles" holding one red solid layer
/// that fades (opacity 0% at 0 s, 100% at 1 s) and a link to a movie that does not exist.
fn sample() -> Vec<u8> {
    let idta = |ty: u16, id: u32| {
        let mut d = vec![0u8; 84];
        put(&mut d, 0, &ty.to_be_bytes());
        put(&mut d, 16, &id.to_be_bytes());
        chunk(b"idta", &d)
    };
    let mut sspc = vec![0u8; 286];
    put(&mut sspc, 30, &640u32.to_be_bytes());
    put(&mut sspc, 34, &360u32.to_be_bytes());
    let mut opti = b"Soli".to_vec();
    opti.extend([0u8; 6]);
    for c in [1.0f32, 1.0, 0.0, 0.0] {
        opti.extend(c.to_be_bytes());
    }
    opti.extend(b"Red Solid 1\0");
    let solid = list(b"Item", &[idta(7, 11), chunk(b"Utf8", b""), list(b"Pin ", &[chunk(b"sspc", &sspc), chunk(b"opti", &opti)])]);
    let movie = list(
        b"Item",
        &[
            idta(7, 12),
            chunk(b"Utf8", b""),
            list(b"Pin ", &[chunk(b"sspc", &sspc), list(b"Als2", &[chunk(b"alas", br#"{"fullpath":"Z:\\nowhere\\clip.mov"}"#)]), chunk(b"opti", b"MooV")]),
        ],
    );
    let mut cdta = vec![0u8; 204];
    put(&mut cdta, 4, &1000u32.to_be_bytes());
    put(&mut cdta, 8, &30_000u32.to_be_bytes());
    put(&mut cdta, 44, &60_000u32.to_be_bytes());
    put(&mut cdta, 48, &30_000u32.to_be_bytes());
    put(&mut cdta, 140, &640u16.to_be_bytes());
    put(&mut cdta, 142, &360u16.to_be_bytes());
    let mut ldta = vec![0u8; 160];
    put(&mut ldta, 0, &1u32.to_be_bytes());
    put(&mut ldta, 10, &1u16.to_be_bytes());
    for at in [12, 20, 28] {
        put(&mut ldta, at + 4, &30_000u32.to_be_bytes());
    }
    put(&mut ldta, 28, &60_000u32.to_be_bytes());
    put(&mut ldta, 39, &[0x07]);
    put(&mut ldta, 40, &11u32.to_be_bytes());
    put(&mut ldta, 96, &2u32.to_be_bytes());
    let tdmn = |name: &str| {
        let mut d = vec![0u8; 40];
        put(&mut d, 0, name.as_bytes());
        chunk(b"tdmn", &d)
    };
    let mut tdb4 = vec![0u8; 124];
    put(&mut tdb4, 2, &1u16.to_be_bytes());
    put(&mut tdb4, 12, &30_000u32.to_be_bytes());
    let key = |time: i32, v: f64| {
        let mut k = time.to_be_bytes().to_vec();
        k.extend([1, 1, 0, 0]);
        for x in [v, 0.0, 0.0, 0.0, 0.0] {
            k.extend(x.to_be_bytes());
        }
        k
    };
    let mut lhd3 = vec![0u8; 52];
    put(&mut lhd3, 8, &2u32.to_be_bytes());
    put(&mut lhd3, 16, &48u32.to_be_bytes());
    let opacity = list(b"tdbs", &[chunk(b"tdb4", &tdb4), list(b"list", &[chunk(b"lhd3", &lhd3), chunk(b"ldat", &[key(0, 0.0), key(30_000, 1.0)].concat())])]);
    let transform = list(b"tdgp", &[tdmn("ADBE Opacity"), opacity, tdmn("ADBE Group End")]);
    let props = list(b"tdgp", &[tdmn("ADBE Transform Group"), transform, tdmn("ADBE Group End")]);
    let layer = list(b"Layr", &[chunk(b"ldta", &ldta), chunk(b"Utf8", b"Fade"), props]);
    let comp = list(b"Item", &[idta(4, 20), chunk(b"Utf8", b"Titles"), chunk(b"cdta", &cdta), layer]);
    let mut body = b"Egg!".to_vec();
    body.extend(list(b"Fold", &[solid, movie, comp]));
    let mut v = b"RIFX".to_vec();
    v.extend((body.len() as u32).to_be_bytes());
    v.extend(body);
    v
}

#[test]
fn file_open_reads_an_after_effects_project_as_a_new_untitled_project() {
    let path = tmp("titles.aep");
    let bytes = sample();
    std::fs::write(&path, &bytes).unwrap();
    let mut s = Session::default();
    let r = s.execute_checked("file.open", json!({"path": path})).unwrap();
    assert_eq!(r["importedFrom"], json!("aep"));
    assert_eq!((r["layers"].as_u64(), r["missingFootage"].as_u64()), (Some(1), Some(1)));
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("linked file not found")));
    // Untitled: saving asks for a new file, so the .aep is never written over.
    assert_eq!(s.path, None);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let comp = s.active_comp().unwrap();
    assert_eq!((comp.width, comp.height, comp.layers.len()), (640, 360, 1));
    assert_eq!(comp.layers[0].name, "Fade");
    let op = comp.layers[0].props.prop("transform/opacity").unwrap();
    assert_eq!(op.keys.len(), 2);
    assert_eq!(op.value_at(effectcraft_time::Tick::from_units(1, 2)), effectcraft_keyframe::Value::Scalar(50.0));
    assert!(s.project.find_by_name("Red Solid 1").is_some());
    assert!(s.project.find_by_name("clip.mov").is_some());
}

#[test]
fn file_import_adds_an_after_effects_project_to_the_open_one_and_undoes() {
    let path = tmp("titles-import.aep");
    std::fs::write(&path, sample()).unwrap();
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Mine", "width": 320, "height": 180, "frameRate": 30, "duration": 2})).unwrap();
    let before = s.project.items.len();
    let r = s.execute_checked("file.import", json!({"paths": [path]})).unwrap();
    assert_eq!(s.project.items.len(), before + 3);
    assert!(s.project.find_by_name("Titles").is_some() && s.project.find_by_name("Mine").is_some());
    assert_eq!(r["comps"].as_array().map(Vec::len), Some(1));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.project.items.len(), before);
}

#[test]
fn a_damaged_after_effects_project_is_an_error_not_a_crash() {
    let path = tmp("broken.aep");
    std::fs::write(&path, b"RIFX\0\0\0\x08Egg!junk").unwrap();
    let mut s = Session::default();
    let e = s.execute("file.open", json!({"path": path})).unwrap_err().to_string();
    assert!(e.contains("not an After Effects project"), "{e}");
}
