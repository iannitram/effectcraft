use effectcraft_time::{format_feet_frames, parse_feet_frames};

#[test]
fn format_basic_35mm() {
    assert_eq!(format_feet_frames(199, 16), "0012+07");
    assert_eq!(format_feet_frames(0, 16), "0000+00");
    assert_eq!(format_feet_frames(16, 16), "0001+00");
}

#[test]
fn format_negative() {
    assert_eq!(format_feet_frames(-20, 16), "-0001+04");
}

#[test]
fn format_16mm_40_per_foot() {
    assert_eq!(format_feet_frames(85, 40), "0002+05");
}

#[test]
fn format_wide_frames_over_100_per_foot() {
    assert_eq!(format_feet_frames(250, 120), "0002+010");
}

#[test]
fn format_per_foot_zero_treated_as_one() {
    assert_eq!(format_feet_frames(5, 0), "0005+00");
}

#[test]
fn parse_feet_and_frames() {
    assert_eq!(parse_feet_frames("12+07", 16, 0), Ok(199));
    assert_eq!(parse_feet_frames("0012+07", 16, 0), Ok(199));
    assert_eq!(parse_feet_frames(" 12+07 ", 16, 0), Ok(199));
}

#[test]
fn parse_frames_may_overflow_a_foot() {
    assert_eq!(parse_feet_frames("1+20", 16, 0), Ok(36));
}

#[test]
fn parse_negative_count() {
    assert_eq!(parse_feet_frames("-1+04", 16, 0), Ok(-20));
}

#[test]
fn parse_bare_frame_count() {
    assert_eq!(parse_feet_frames("250", 16, 999), Ok(250));
}

#[test]
fn parse_relative_moves() {
    assert_eq!(parse_feet_frames("+10", 16, 100), Ok(110));
    assert_eq!(parse_feet_frames("-10", 16, 100), Ok(90));
}

#[test]
fn parse_empty_parts_are_zero() {
    assert_eq!(parse_feet_frames("12+", 16, 0), Ok(192));
    assert_eq!(parse_feet_frames("+0", 16, 3), Ok(3));
}

#[test]
fn parse_rejects_garbage() {
    assert!(parse_feet_frames("", 16, 0).is_err());
    assert!(parse_feet_frames("   ", 16, 0).is_err());
    assert!(parse_feet_frames("abc", 16, 0).is_err());
    assert!(parse_feet_frames("1+x", 16, 0).is_err());
    assert!(parse_feet_frames("1.5", 16, 0).is_err());
}

#[test]
fn parse_rejects_too_large() {
    assert!(parse_feet_frames("99999999999999999999", 16, 0).is_err());
    assert!(parse_feet_frames("576460752303423488+0", 16, 0).is_err());
}

#[test]
fn roundtrip_format_then_parse() {
    for &per_foot in &[16, 40, 120] {
        for frame in -2000..=2000 {
            assert_eq!(parse_feet_frames(&format_feet_frames(frame, per_foot), per_foot, 0), Ok(frame));
        }
    }
}
