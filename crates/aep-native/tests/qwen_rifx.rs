use effectcraft_aep_native::rifx::{self, f32_at, f64_at, i16_at, i32_at, text_field, u8_at, u16_at, u32_at};

/// One chunk: id, big-endian size, data, and a pad byte when the size is odd.
fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend_from_slice(&(data.len() as u32).to_be_bytes());
    v.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        v.push(0);
    }
    v
}

/// A LIST chunk of type `ty` holding the given (already built) chunks.
fn list(ty: &[u8; 4], kids: &[Vec<u8>]) -> Vec<u8> {
    let mut body = ty.to_vec();
    for k in kids {
        body.extend_from_slice(k);
    }
    chunk(b"LIST", &body)
}

/// A whole .aep file: "RIFX", size, "Egg!", then the chunks.
fn rifx_file(kids: &[Vec<u8>]) -> Vec<u8> {
    let mut body = b"Egg!".to_vec();
    for k in kids {
        body.extend_from_slice(k);
    }
    let mut v = b"RIFX".to_vec();
    v.extend_from_slice(&(body.len() as u32).to_be_bytes());
    v.extend_from_slice(&body);
    v
}

#[test]
fn rejects_non_rifx() {
    let f = b"RIFF\0\0\0\x04Egg!".to_vec();
    assert!(rifx::parse(&f).is_err());
}

#[test]
fn rejects_short_header() {
    let f = b"RIFX\0\0".to_vec();
    assert!(rifx::parse(&f).is_err());
}

#[test]
fn rejects_wrong_form() {
    let f = b"RIFX\0\0\0\x04Egg?".to_vec();
    assert!(rifx::parse(&f).is_err());
}

#[test]
fn empty_project_has_no_chunks() {
    let f = rifx_file(&[]);
    let (root, rest) = rifx::parse(&f).unwrap();
    assert!(root.kids.is_empty());
    assert_eq!(root.id, *b"RIFX");
    assert_eq!(root.list, Some(*b"Egg!"));
    assert!(rest.is_empty());
}

#[test]
fn flat_chunks_and_odd_padding() {
    let f = rifx_file(&[chunk(b"abcd", b"xyz"), chunk(b"efgh", b"1234")]);
    let (root, _rest) = rifx::parse(&f).unwrap();
    assert_eq!(root.kids.len(), 2);
    let k0 = &root.kids[0];
    assert_eq!(k0.id, *b"abcd");
    assert_eq!(k0.data, b"xyz");
    assert!(k0.list.is_none());
    let k1 = &root.kids[1];
    assert_eq!(k1.id, *b"efgh");
    assert_eq!(k1.data, b"1234");
}

#[test]
fn nested_list_found() {
    let f = rifx_file(&[list(b"Fold", &[chunk(b"name", b"Comp 1\0")])]);
    let (root, _rest) = rifx::parse(&f).unwrap();
    let l = root.list(b"Fold").expect("Fold list should exist");
    assert!(l.data.is_empty());
    assert_eq!(l.first(b"name").unwrap().data, b"Comp 1\0");
    assert!(root.find_list(b"Fold").is_some());
    assert!(root.first(b"Fold").is_none());
    assert!(l.is_list(b"Fold"));
}

#[test]
fn find_list_searches_depth_first() {
    let f = rifx_file(&[list(b"Outr", &[list(b"Innr", &[chunk(b"data", b"ab")])])]);
    let (root, _rest) = rifx::parse(&f).unwrap();
    assert!(root.list(b"Innr").is_none());
    assert_eq!(root.find_list(b"Innr").unwrap().first(b"data").unwrap().data, b"ab");
}

#[test]
fn lists_returns_every_match() {
    let f = rifx_file(&[list(b"Item", &[]), chunk(b"sep_", b""), list(b"Item", &[])]);
    let (root, _rest) = rifx::parse(&f).unwrap();
    assert_eq!(root.lists(b"Item").count(), 2);
}

#[test]
fn btdk_list_kept_opaque() {
    let f = rifx_file(&[list(b"btdk", &[chunk(b"abcd", b"")])]);
    let (root, _rest) = rifx::parse(&f).unwrap();
    let l = root.list(b"btdk").expect("btdk list should exist");
    assert_eq!(l.data.len(), 8);
}

#[test]
fn oversize_length_clamped() {
    let mut f = rifx_file(&[chunk(b"abcd", b"12345")]);
    f.extend_from_slice(b"<x:xmpmeta/>");
    f[16..20].copy_from_slice(&1000u32.to_be_bytes());
    let (root, rest) = rifx::parse(&f).unwrap();
    assert_eq!(root.kids.len(), 1);
    assert_eq!(root.kids[0].data, b"12345\0");
    assert_eq!(rest, b"<x:xmpmeta/>");
}

#[test]
fn trailing_xmp_returned() {
    let mut f = rifx_file(&[chunk(b"abcd", b"12")]);
    f.extend_from_slice(b"<x:xmpmeta/>");
    let (root, rest) = rifx::parse(&f).unwrap();
    assert_eq!(rest, b"<x:xmpmeta/>");
    assert_eq!(root.kids.len(), 1);
}

#[test]
fn field_readers_big_endian() {
    let d = [0x12, 0x34, 0x56, 0x78, 0xFF, 0xFE];
    assert_eq!(u8_at(&d, 0), 0x12);
    assert_eq!(u16_at(&d, 0), 0x1234);
    assert_eq!(u32_at(&d, 0), 0x1234_5678);
    assert_eq!(i16_at(&d, 4), -2);
    assert_eq!(i32_at(&[0xFF, 0xFF, 0xFF, 0xFF], 0), -1);
    assert_eq!(f32_at(&1.5f32.to_be_bytes(), 0), 1.5);
    assert_eq!(f64_at(&(-2.25f64).to_be_bytes(), 0), -2.25);
}

#[test]
fn field_readers_zero_past_end() {
    let d = [0x12, 0x34];
    assert_eq!(u8_at(&d, 2), 0);
    assert_eq!(u16_at(&d, 1), 0);
    assert_eq!(u32_at(&d, 0), 0);
    assert_eq!(f64_at(&d, 0), 0.0);
    assert_eq!(u32_at(&d, usize::MAX), 0);
}

#[test]
fn non_finite_floats_read_as_zero() {
    assert_eq!(f32_at(&f32::NAN.to_be_bytes(), 0), 0.0);
    assert_eq!(f32_at(&f32::INFINITY.to_be_bytes(), 0), 0.0);
    assert_eq!(f64_at(&f64::NEG_INFINITY.to_be_bytes(), 0), 0.0);
}

#[test]
fn text_field_nul_terminated() {
    assert_eq!(text_field(b"abc\0xyz"), "abc");
    assert_eq!(text_field(b"abc"), "abc");
    assert_eq!(text_field(b""), "");
}

#[test]
fn text_field_utf8_chunk() {
    assert_eq!(text_field(b"Utf8\0\0\0\x05hello!!"), "hello");
    assert_eq!(text_field(b"Utf8\0\0\0\x09hi"), "hi");
}
