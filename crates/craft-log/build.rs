//! Records the commit and the build date for `craft_log::BUILD_COMMIT` / `BUILD_DATE`.
//!
//! Reads `.git` directly (HEAD, the ref it points at, `packed-refs`), because builds run in
//! containers and sandboxes without a `git` program. A worktree's `.git` file is followed to its
//! git directory. Anything missing reads as "unknown"; the build never fails over it.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap_or_default();
    let (commit, watched) = commit(&manifest);
    for p in watched {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rustc-env=CRAFT_LOG_COMMIT={commit}");
    println!("cargo:rustc-env=CRAFT_LOG_BUILD_DATE={}", build_date());
}

/// The git directory for the repository holding `start`, and the common directory (they differ
/// for a worktree: refs live in the common one).
fn git_dirs(start: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let dot = d.join(".git");
        if dot.is_dir() {
            return Some((dot.clone(), dot));
        }
        if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            let gitdir = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
            let gitdir = d.join(gitdir);
            let common = std::fs::read_to_string(gitdir.join("commondir")).ok().map(|c| gitdir.join(c.trim())).unwrap_or_else(|| gitdir.clone());
            return Some((gitdir, common));
        }
        dir = d.parent();
    }
    None
}

fn commit(manifest: &Path) -> (String, Vec<PathBuf>) {
    let Some((gitdir, common)) = git_dirs(manifest) else { return ("unknown".into(), Vec::new()) };
    let head_path = gitdir.join("HEAD");
    let mut watched = vec![head_path.clone()];
    let Ok(head) = std::fs::read_to_string(&head_path) else { return ("unknown".into(), watched) };
    let head = head.trim();
    let Some(refname) = head.strip_prefix("ref:").map(str::trim) else {
        return (short(head), watched);
    };
    let loose = common.join(refname);
    watched.push(loose.clone());
    watched.push(common.join("packed-refs"));
    if let Ok(sha) = std::fs::read_to_string(&loose) {
        return (short(sha.trim()), watched);
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).unwrap_or_default();
    let sha = packed.lines().find_map(|l| l.strip_suffix(refname).map(str::trim)).unwrap_or("unknown");
    (short(sha), watched)
}

fn short(sha: &str) -> String {
    if sha.len() >= 8 && sha.bytes().all(|b| b.is_ascii_hexdigit()) { sha[..8].to_owned() } else { "unknown".into() }
}

/// UTC date of the build (`SOURCE_DATE_EPOCH` when set, for reproducible builds).
fn build_date() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    let z = secs / 86_400 + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = era * 400 + yoe + u64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
