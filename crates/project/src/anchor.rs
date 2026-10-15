//! Ours (not upstream): where a project file was saved, so footage can follow a moved project
//! folder (`effectcraft_engine::media_anchor`).
//!
//! Written at the top of the project file beside `savedBy` (`savedDir`, `savedSizes`), like the
//! saved view: outside the schema (no version bump); older versions ignore it, files without it
//! read as an empty anchor.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The folder a project file was saved in, and the size of each footage file then (by path; the
/// first frame of a sequence), to tell the same file from another at the same place.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_dir: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub saved_sizes: BTreeMap<String, u64>,
    /// A content fingerprint of each of those files then (hex; size, first and last 64 KiB), so a
    /// file of the same size at the same place is not taken for it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub saved_prints: BTreeMap<String, String>,
}

/// The anchor a project file was saved with; empty for older files and anything unreadable.
pub fn saved_anchor(text: &str) -> Anchor {
    serde_json::from_str(text).unwrap_or_default()
}
