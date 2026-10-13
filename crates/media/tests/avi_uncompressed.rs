//! Uncompressed AVI footage: probed as a movie and decoded frame by frame, read in place.
//!
//! The file is built here byte by byte from Microsoft's AVI RIFF layout: a `hdrl` list with the
//! main header and one video stream (`BI_RGB`, 24 bits, so rows are stored bottom-up in
//! blue-green-red order, each padded to 4 bytes), a `movi` list with one `00db` chunk per frame,
//! and an `idx1` index. Before EffectCraft took FilmCraft's AVI reader, importing such a file
//! failed with "unrecognised format".

use effectcraft_media::{MediaPool, is_importable, probe};
use effectcraft_project::FootageKind;
use effectcraft_time::{FrameRate, Tick};

const W: u32 = 6;
const H: u32 = 4;
const FPS: u32 = 24;

fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend((data.len() as u32).to_le_bytes());
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

fn le(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// One frame: the top `H / 2` rows `top`, the rest `bottom` (red, green, blue), stored the way
/// `BI_RGB` wants it.
fn frame(top: [u8; 3], bottom: [u8; 3]) -> Vec<u8> {
    let stride = (W as usize * 3).next_multiple_of(4);
    let mut out = Vec::new();
    // Bottom row first.
    for y in (0..H).rev() {
        let c = if y < H / 2 { top } else { bottom };
        let mut row = Vec::with_capacity(stride);
        for _ in 0..W {
            row.extend([c[2], c[1], c[0]]);
        }
        row.resize(stride, 0);
        out.extend(row);
    }
    out
}

fn avi(frames: &[Vec<u8>]) -> Vec<u8> {
    let frame_bytes = frames.first().map_or(0, Vec::len) as u32;
    let n = frames.len() as u32;
    // avih: microseconds per frame, max bytes per second, padding, flags (has index), total
    // frames, initial frames, streams, suggested buffer, width, height, four reserved.
    let avih = le(&[1_000_000 / FPS, frame_bytes * FPS, 0, 0x10, n, 0, 1, frame_bytes, W, H, 0, 0, 0, 0]);
    // strh: type, handler, flags, priority + language, initial frames, scale, rate, start,
    // length, suggested buffer, quality, sample size, frame rectangle.
    let mut strh = b"vidsDIB ".to_vec();
    strh.extend(le(&[0, 0, 0, 1, FPS, 0, n, frame_bytes, 0xffff_ffff, 0]));
    strh.extend([0, 0, 0, 0, W as u8, 0, H as u8, 0]);
    // strf: BITMAPINFOHEADER with a positive height (bottom-up) and compression 0 (BI_RGB).
    let mut strf = le(&[40, W, H]);
    strf.extend([1, 0, 24, 0]);
    strf.extend(le(&[0, frame_bytes, 0, 0, 0, 0]));
    let hdrl = list(b"hdrl", &[chunk(b"avih", &avih), list(b"strl", &[chunk(b"strh", &strh), chunk(b"strf", &strf)])]);
    let mut movi_kids = Vec::new();
    let mut index = Vec::new();
    let mut offset = 4u32;
    for f in frames {
        index.extend(b"00db");
        index.extend(le(&[0x10, offset, f.len() as u32]));
        offset += 8 + f.len() as u32;
        movi_kids.push(chunk(b"00db", f));
    }
    let movi = list(b"movi", &movi_kids);
    let mut body = b"AVI ".to_vec();
    body.extend(hdrl);
    body.extend(movi);
    body.extend(chunk(b"idx1", &index));
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

fn near(px: [f32; 4], rgb: [u8; 3]) -> bool {
    (0..3).all(|i| (px[i] - rgb[i] as f32 / 255.0).abs() < 0.02) && (px[3] - 1.0).abs() < 0.02
}

#[test]
fn an_uncompressed_avi_is_a_movie_and_its_frames_decode_the_right_way_up() {
    let dir = std::env::temp_dir().join(format!("effectcraft-avi-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shot.avi");
    std::fs::write(&path, avi(&[frame(RED, BLUE), frame(GREEN, WHITE), frame(BLUE, RED)])).unwrap();
    assert!(is_importable(&path));
    let f = probe(&path).unwrap();
    assert_eq!(f.kind, FootageKind::Video);
    assert_eq!((f.width, f.height), (W, H));
    assert_eq!(f.frame_rate, FrameRate::new(FPS as i64, 1));
    assert_eq!(f.duration, FrameRate::new(FPS as i64, 1).tick_of(3));
    assert!(f.has_video && !f.has_audio && !f.missing);
    let pool = MediaPool::new();
    let rate = f.frame_rate;
    let at = |n: i64| pool.frame_at(&f, Tick(rate.tick_of(n).0 + rate.frame_duration().0 / 2)).unwrap();
    // Rows are stored bottom-up: the first colour must come out at the top.
    let first = at(0);
    assert_eq!((first.width, first.height), (W, H));
    assert!(near(first.get(0, 0), RED), "top of frame 0: {:?}", first.get(0, 0));
    assert!(near(first.get(W as i64 - 1, H as i64 - 1), BLUE), "bottom of frame 0: {:?}", first.get(W as i64 - 1, H as i64 - 1));
    let second = at(1);
    assert!(near(second.get(2, 0), GREEN) && near(second.get(2, H as i64 - 1), WHITE), "frame 1: {:?}", second.get(2, 0));
    // Out of order, as scrubbing does.
    let third = at(2);
    assert!(near(third.get(0, 0), BLUE) && near(third.get(0, H as i64 - 1), RED));
    assert!(near(at(0).get(0, 0), RED));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_truncated_avi_is_an_error_or_a_shorter_movie_never_a_panic() {
    let dir = std::env::temp_dir().join(format!("effectcraft-avi-cut-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let whole = avi(&[frame(RED, BLUE), frame(GREEN, WHITE)]);
    let pool = MediaPool::new();
    for cut in (0..whole.len()).step_by(7) {
        let path = dir.join(format!("cut-{cut}.avi"));
        std::fs::write(&path, &whole[..cut]).unwrap();
        if let Ok(f) = probe(&path) {
            let _ = pool.frame_at(&f, Tick::ZERO);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
