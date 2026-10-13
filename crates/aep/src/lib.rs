//! After Effects project (`.aep`) import.
//!
//! Worked out clean-room: from the public RIFF container rules, from what projects saved by
//! After Effects 6.5 through 2025 contain, and from After Effects' documented match names. No
//! Adobe code, no other reader's source. Field positions are written down where they are read
//! ([`read`]); a field this crate does not understand is reported, not guessed.
//!
//! **What is imported** ([`import`]): folders, compositions (size, frame rate, duration, work
//! area, pixel aspect), solids, linked files (found by the caller, or kept as missing footage
//! with the recorded size and duration), and layers: source, name, label, timing (start, in,
//! out, whole-number stretch), switches, blend mode, parent, track matte, the transform group
//! with keyframes (interpolation kind, ease, spatial tangents), time remapping, and masks
//! (mode, inverted, path with keyframes, feather, opacity, expansion).
//!
//! **What is not, yet** (each is listed in [`ImportResult::warnings`] when a project uses it):
//! effect settings (effects are added switched off), text content and style, shape layer
//! contents, camera and light options, layer styles, expressions, markers, audio levels,
//! separated position dimensions, orientation, fractional time stretch.
//!
//! Keyframe times in the file are layer time, which is EffectCraft's convention too.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod effects_table;
mod import;
pub mod read;
pub mod rifx;

pub use import::{FootageRef, ImportResult, import};

#[derive(Debug, thiserror::Error)]
pub enum AepError {
    #[error("not an After Effects project: {0}")]
    NotAep(String),
}

/// Whether the bytes start like an `.aep` file.
pub fn is_aep(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(b"RIFX") && bytes.get(8..12) == Some(b"Egg!")
}

#[cfg(test)]
mod tests;
