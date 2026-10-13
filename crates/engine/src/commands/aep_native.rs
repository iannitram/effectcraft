//! Opening and importing After Effects projects (`effectcraft-aep-native`, our own reader: no converter outside the app): File ▸ Open on an `.aep`,
//! an `.aep` among the files of File ▸ Import, and the command `file.importAepNative`.
//!
//! An `.aep` is only ever read. Opened, it becomes an untitled project, so saving asks for a
//! new `.ecproj` and can never write over the After Effects file.

use effectcraft_aep_native::{FootageRef, ImportResult};
use effectcraft_project::{Footage, FootageKind, Project};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, str_p};
use crate::{EngineError, Result, Session, cmd};

/// Folders below the project's folder searched for a linked file, and how many are looked at.
const SEARCH_DEPTH: usize = 3;
const SEARCH_FOLDERS: usize = 400;

/// Whether `path` names an After Effects project by its extension.
pub(crate) fn is_aep_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("aep") || e.eq_ignore_ascii_case("aet"))
}

/// The last component of a path written on any system (`C:\a\b.mov`, `/Users/x/b.mov`).
fn file_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The folders to search: the project's own, then the ones below it, breadth first.
fn search_folders(dir: &str) -> Vec<String> {
    let mut out = vec![dir.to_string()];
    let mut level = vec![dir.to_string()];
    for _ in 0..SEARCH_DEPTH {
        let mut next = vec![];
        for d in &level {
            let Ok(rd) = std::fs::read_dir(if d.is_empty() { "." } else { d }) else { continue };
            let mut subs: Vec<String> = rd
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();
            subs.sort();
            next.extend(subs);
        }
        out.extend(next.iter().cloned());
        if out.len() >= SEARCH_FOLDERS || next.is_empty() {
            break;
        }
        level = next;
    }
    out.truncate(SEARCH_FOLDERS);
    out
}

/// Where a linked file is on this machine: at the recorded path, or by its name in the
/// project's folder and the folders below it. A link to a folder (an image sequence) gives the
/// folder's first file.
fn find_file(s: &Session, folders: &[String], recorded: &str) -> Option<String> {
    let first_in = |dir: &str| -> Option<String> {
        let mut files = s.services.list_dir(dir);
        files.sort();
        files.into_iter().next()
    };
    if s.services.is_dir(recorded) {
        return first_in(recorded);
    }
    if s.services.exists(recorded) {
        return Some(recorded.to_string());
    }
    let name = file_name(recorded);
    if name.is_empty() {
        return None;
    }
    for f in folders {
        let cand = std::path::Path::new(f).join(name).to_string_lossy().to_string();
        if s.services.is_dir(&cand) {
            if let Some(first) = first_in(&cand) {
                return Some(first);
            }
        } else if s.services.exists(&cand) {
            return Some(cand);
        }
    }
    None
}

/// Import the bytes of an `.aep` into `proj`, finding linked files from the folder of `path`.
fn import_into(s: &Session, proj: &mut Project, path: &str, bytes: &[u8]) -> Result<ImportResult> {
    let dir = std::path::Path::new(path).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    let mut folders: Option<Vec<String>> = None;
    let mut unreadable: Vec<String> = vec![];
    let mut locate = |r: &FootageRef| -> Option<Footage> {
        let recorded = r.path?;
        let folders = folders.get_or_insert_with(|| search_folders(&dir));
        let found = find_file(s, folders, recorded)?;
        match s.probe_footage(&found) {
            Ok(f) => Some(f),
            Err(e) => {
                // The file is there but this build can't read it: keep the link, as missing.
                unreadable.push(format!("linked file found but not readable here: {found} ({e})"));
                let audio_only = r.width == 0 || r.height == 0;
                Some(Footage {
                    path: found,
                    kind: if audio_only { FootageKind::Audio } else { FootageKind::Video },
                    width: r.width.max(1),
                    height: r.height.max(1),
                    duration: r.duration,
                    has_video: !audio_only,
                    has_audio: audio_only,
                    missing: true,
                    ..Default::default()
                })
            }
        }
    };
    let mut res = effectcraft_aep_native::import(proj, bytes, &mut locate).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    unreadable.sort();
    unreadable.dedup();
    res.missing_footage += unreadable.len();
    res.warnings.extend(unreadable);
    Ok(res)
}

fn summary(path: &str, r: &ImportResult) -> Value {
    json!({
        "path": path,
        "importedFrom": "aep",
        "comp": r.main_comp.map(|c| c.0),
        "comps": r.comps.iter().map(|c| c.0).collect::<Vec<_>>(),
        "items": r.items.iter().map(|i| i.0).collect::<Vec<_>>(),
        "layers": r.layers,
        "missingFootage": r.missing_footage,
        "warnings": r.warnings,
    })
}

fn report(s: &mut Session, path: &str, r: &ImportResult) {
    let name = file_name(path);
    if r.warnings.is_empty() {
        s.toast(format!("Opened After Effects project {name}: {} compositions, {} layers.", r.comps.len(), r.layers));
    } else {
        for w in &r.warnings {
            log::warn!("{name}: {w}");
        }
        let shown = r.warnings.iter().take(3).cloned().collect::<Vec<_>>().join("; ");
        let more = r.warnings.len().saturating_sub(3);
        let more = if more > 0 { format!(" and {more} more (see the log)") } else { String::new() };
        s.toast(format!(
            "Opened After Effects project {name}: {} compositions, {} layers. Not everything came across: {shown}{more}.",
            r.comps.len(),
            r.layers
        ));
    }
}

/// File ▸ Open on an `.aep`: the After Effects project becomes a new untitled project.
pub(crate) fn open(s: &mut Session, path: &str, bytes: &[u8]) -> Result<Value> {
    let mut proj = Project::default();
    let res = import_into(s, &mut proj, path, bytes)?;
    s.replace_project(proj, None);
    if let Some(c) = res.main_comp {
        s.open_comp(c);
    }
    report(s, path, &res);
    Ok(summary(path, &res))
}

/// An `.aep` given to File ▸ Import: its items are added to the open project.
pub(crate) fn import_file(s: &mut Session, path: &str) -> Result<Value> {
    let bytes = s.services.read_file(path).map_err(|e| EngineError::Other(format!("cannot read {path}: {e}")))?;
    let mut proj = (*s.project).clone();
    let res = import_into(s, &mut proj, path, &bytes)?;
    let items = res.items.clone();
    s.edit("Import After Effects Project", None, |p, st| {
        *p = proj;
        st.project_selection = items;
        Ok(())
    })?;
    report(s, path, &res);
    Ok(summary(path, &res))
}

fn import_aep(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_p(p, "path").ok_or_else(|| bad("file.importAepNative", "missing `path`"))?.to_string();
    import_file(s, &path)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!("file.importAepNative", "Import After Effects Project (Built-in Reader)", [], None, "{path (.aep)}", always, import_aep)]
}
