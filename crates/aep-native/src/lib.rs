//! After Effects project (`.aep`) import.
//!
//! Where the format knowledge comes from, stated exactly: the RIFF container rules; the chunk
//! names and the broad layout of the property tree as the community has publicly described
//! them (After Effects publishes no specification); After Effects' documented match names; and
//! measurements on projects saved by After Effects 6.5 through 2025, which is where every
//! field position used here was checked. No Adobe code was read, and no other reader's source
//! code (py-aep and the like) was read or ported. Field positions are written down where they
//! are read ([`read`]); a field this crate does not understand is reported, not guessed.
//!
//! **What is imported** ([`import`]): folders, compositions (size, frame rate, duration, work
//! area, pixel aspect), solids, linked files (found by the caller, or kept as missing footage
//! with the recorded size and duration), and layers: source, name, label, timing (start, in,
//! out, whole-number stretch), switches, blend mode, parent, track matte, the transform group
//! with keyframes (interpolation kind, ease, spatial tangents), time remapping, and masks
//! (mode, inverted, path with keyframes, feather, opacity, expansion).
//!
//! **What is not, yet** (each is listed in [`ImportResult::warnings`] when a project uses it):
//! effect settings (effects are added switched off), text settings beyond the text itself, its
//! fonts, sizes and fill colours, shape layer contents, camera and light options, layer styles, expressions, markers, audio levels,
//! separated position dimensions, orientation, fractional time stretch.
//!
//! Keyframe times in the file are layer time, which is EffectCraft's convention too.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod effects_table;
mod import;
pub mod read;
pub mod rifx;
mod text;

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
