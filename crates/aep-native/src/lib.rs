//! After Effects project (`.aep`) import.
//!
//! Where the format knowledge comes from, stated exactly: the RIFF container rules; the chunk
//! names and the broad layout of the property tree as the community has publicly described
//! them (After Effects publishes no specification); After Effects' documented match names; and
//! measurements on projects saved by After Effects 6.5 through 2025, which is where every
//! field position used here was checked. An effect's parameter definitions (`pard`) are laid
//! out the way After Effects' published plug-in programming interface describes a parameter;
//! that description was recalled, not opened, and each position was then checked on files
//! against defaults known from using After Effects. No Adobe code was read, and no other
//! reader's source code (py-aep and the like) was read or ported. Field positions are written down where they
//! are read ([`read`]); a field this crate does not understand is reported, not guessed.
//!
//! **What is imported** ([`import`]): folders, compositions (size, frame rate, duration, work
//! area, pixel aspect), solids, linked files (found by the caller, or kept as missing footage
//! with the recorded size and duration), and layers: source, name, label, timing (start, in,
//! out, whole-number stretch), switches, blend mode, parent, track matte, the transform group
//! with keyframes (interpolation kind, ease, spatial tangents), time remapping, audio levels
//! with keyframes, masks (mode, inverted, path with keyframes, feather, opacity, expansion),
//! and effects with their settings: sliders, angles, checkboxes, pop-ups, colours, points and
//! layer choosers with their keyframes, plus the private data of Levels, Curves and Colorama
//! (`settings`). An effect with a setting that could not be carried over is added switched
//! off, and the setting is named. Text layers bring their text, fonts, sizes, fill colours,
//! tracking, leading, scale and alignment (`text`).
//!
//! **What is not, yet** (each is listed in [`ImportResult::warnings`] when a project uses it):
//! other effects' private data and colour keyframes, text stroke, baseline shift, box text
//! and animators, shape layer contents, camera and light options, layer styles, expressions,
//! markers, separated position dimensions, orientation, fractional time stretch.
//!
//! Keyframe times in the file are layer time, which is EffectCraft's convention too.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod effects_table;
mod import;
pub mod read;
pub mod rifx;
mod settings;
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
