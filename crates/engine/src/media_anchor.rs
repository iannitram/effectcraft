//! Ours (not upstream): footage follows a moved project folder. The twin of FilmCraft's
//! `media_anchor` (feat/relative-media-paths).
//!
//! A project file records the folder it was saved in and each footage file's size then
//! ([`effectcraft_project::Anchor`]: `savedDir`, `savedSizes`, beside `savedBy`). Footage paths
//! stay absolute, as before. When a project is opened from another folder, each footage item (a
//! sequence as one unit, with all its frames) and each proxy is looked for at the same place
//! relative to the project as when it was saved. Found there with the recorded size, it replaces
//! the absolute paths; otherwise they stay, and the background footage check (`find_moved`) and
//! Relink Missing Footage handle what is still missing, as before. Data footage (kept inside the
//! project) is skipped. Done in `file.open`, so the CLI and agents get it too.
//!
//! Relative first, and only when the project has moved: the copy beside the moved project is the
//! one meant, even when the original still exists. Paths are compared lexically on both
//! separators, so a project saved on Windows opens on Linux; footage on another drive than the
//! project (no shared folder) has no relative place and keeps its absolute path.

use effectcraft_project::{Anchor, Footage, FootageKind, ItemKind, Project};

use crate::{Services, Session};

/// The folder containing `path` (a project file), when `path` is absolute.
pub fn folder_of(path: &str) -> Option<String> {
    if !is_absolute(path) {
        return None;
    }
    let cut = path.rfind(['/', '\\'])?;
    let dir = &path[..cut];
    // a UNC path needs `\\server\share` before the file: `\\p.ecproj` has no folder
    if path.starts_with("\\\\") && !dir.get(2..).unwrap_or("").contains(['/', '\\']) {
        return None;
    }
    // the root itself: keep its separator ("/", "C:\")
    Some(if dir.is_empty() || dir.ends_with(':') { path[..=cut].to_string() } else { dir.to_string() })
}

/// Absolute on Unix (`/…`) or Windows (`C:\…`, `C:/…`, `\\server\…`).
fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/') || p.starts_with("\\\\") || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// The root (`/`, `C:`, `\\server`) and the folder names of an absolute path.
fn parts(p: &str) -> (String, Vec<&str>) {
    let root = if let Some(rest) = p.strip_prefix("\\\\") {
        format!("\\\\{}", rest.split(['/', '\\']).next().unwrap_or(""))
    } else if p.starts_with('/') {
        "/".to_string()
    } else {
        // `C:`; never slice past a short string (a lone `\` reached here and panicked)
        p.get(..2).unwrap_or(p).to_string()
    };
    let body = &p[root.len().min(p.len())..];
    let names = body.split(['/', '\\']).filter(|c| !c.is_empty() && *c != ".").collect();
    (root, names)
}

/// Where `file` is relative to the folder `dir` (both absolute): `..` steps up, then names.
/// `None` on different roots (drives), or when they share no folder below the root (`/home/…`
/// against `/run/media/…`): such media did not live with the project.
pub fn relative(file: &str, dir: &str) -> Option<Vec<String>> {
    if !is_absolute(file) || !is_absolute(dir) {
        return None;
    }
    let ((fr, f), (dr, d)) = (parts(file), parts(dir));
    if fr != dr || f.is_empty() {
        return None;
    }
    let common = f.iter().zip(&d).take_while(|(a, b)| a == b).count();
    if common == 0 || common == f.len() {
        return None;
    }
    let mut out: Vec<String> = std::iter::repeat_n("..".to_string(), d.len() - common).collect();
    out.extend(f[common..].iter().map(|s| s.to_string()));
    Some(out)
}

/// `rel` ([`relative`]) joined to the folder `dir`, written with `dir`'s separator.
pub fn resolve(rel: &[String], dir: &str) -> Option<String> {
    let sep = if dir.contains('\\') && !dir.contains('/') { '\\' } else { '/' };
    let (root, mut names) = parts(dir);
    let mut names: Vec<String> = names.drain(..).map(str::to_string).collect();
    for c in rel {
        if c == ".." {
            names.pop()?;
        } else {
            names.push(c.clone());
        }
    }
    let root = if root == "/" { String::new() } else { root };
    Some(format!("{root}{sep}{}", names.join(&sep.to_string())))
}

/// Whether two folders are the same place (separators, a trailing separator and, for drive
/// letters, case aside).
fn same_folder(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let t = s.replace('\\', "/");
        let t = t.trim_end_matches('/').to_string();
        if t.as_bytes().get(1) == Some(&b':') { t.to_lowercase() } else { t }
    };
    norm(a) == norm(b)
}

/// Bytes read from each end of a file for its fingerprint.
const PRINT_SPAN: usize = 64 * 1024;

/// A content fingerprint of the regular file `path` (FNV-1a over its size and its first and last
/// [`PRINT_SPAN`] bytes, hex); `None` for a folder or anything unreadable.
fn print(services: &dyn Services, path: &str) -> Option<String> {
    if services.is_dir(path) {
        return None;
    }
    let (size, _) = services.stamp(path)?;
    let head = services.read_range(path, 0, PRINT_SPAN).ok()?;
    let span = PRINT_SPAN as u64;
    let tail = if size > 2 * span { services.read_range(path, size - span, PRINT_SPAN).ok()? } else { vec![] };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in size.to_le_bytes().iter().chain(&head).chain(&tail) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    Some(format!("{h:016x}"))
}

/// The file whose size stands for `f`: the first frame of a sequence, else its path.
fn sized_path(f: &Footage) -> Option<&str> {
    let p = if f.kind == FootageKind::Sequence { f.sequence.first().map(String::as_str).unwrap_or(&f.path) } else { f.path.as_str() };
    (!p.is_empty() && f.kind != FootageKind::Data).then_some(p)
}

/// Every footage item's and proxy's sized file ([`sized_path`]).
fn footage_mut(p: &mut Project) -> impl Iterator<Item = &mut Footage> {
    p.items.values_mut().flat_map(|it| {
        let main = match &mut it.kind {
            ItemKind::Footage(f) => Some(f),
            _ => None,
        };
        main.into_iter().chain(it.proxy.as_mut().map(|x| &mut x.footage))
    })
}

/// What a save to `path` records: its folder, and each footage file's size now.
pub fn anchor_for(p: &Project, path: &str, services: &dyn Services) -> Anchor {
    let mut anchor = Anchor { saved_dir: folder_of(path), ..Anchor::default() };
    if anchor.saved_dir.is_none() {
        return anchor;
    }
    for it in p.items.values() {
        let main = if let ItemKind::Footage(f) = &it.kind { Some(f) } else { None };
        for f in main.into_iter().chain(it.proxy.as_ref().map(|x| &x.footage)) {
            if let Some(file) = sized_path(f)
                && let Some((size, _)) = services.stamp(file)
                && let Some(p) = print(services, file)
            {
                anchor.saved_sizes.insert(file.to_string(), size);
                anchor.saved_prints.insert(file.to_string(), p);
            }
        }
    }
    anchor
}

/// On open: point footage (and proxies) at their places beside the project, when the project was
/// saved in another folder than `project_path`'s and each is found there with its recorded size.
/// How many footage items (or proxies) changed.
pub fn follow(project: &mut Project, anchor: &Anchor, project_path: &str, services: &dyn Services) -> usize {
    let (Some(saved), Some(now)) = (anchor.saved_dir.as_deref(), folder_of(project_path)) else { return 0 };
    if same_folder(saved, &now) {
        return 0;
    }
    let moved = |path: &str| relative(path, saved).and_then(|rel| resolve(&rel, &now));
    let mut changed = 0;
    for f in footage_mut(project) {
        let Some(file) = sized_path(f).map(str::to_string) else { continue };
        let Some(cand) = moved(&file) else { continue };
        // The same file: a regular file with the recorded size AND fingerprint. Footage saved
        // without them (older files) never follows: nothing proves what is at the place.
        let (Some(&want_size), Some(want_print)) = (anchor.saved_sizes.get(&file), anchor.saved_prints.get(&file)) else { continue };
        if cand == file || services.stamp(&cand).map(|(size, _)| size) != Some(want_size) {
            continue;
        }
        if print(services, &cand).as_deref() != Some(want_print.as_str()) {
            log::info!("decision: footage: \"{cand}\" beside the moved project is another file (fingerprint differs); \"{file}\" kept");
            continue;
        }
        // a sequence moves as one unit: every frame by the same relation
        let frames: Option<Vec<String>> = f.sequence.iter().map(|q| moved(q)).collect();
        let Some(path) = (if f.path.is_empty() { Some(String::new()) } else { moved(&f.path) }) else { continue };
        let Some(frames) = frames else { continue };
        log::info!("decision: footage: \"{}\" followed the project folder to \"{path}\"", f.path);
        f.path = path;
        f.sequence = frames;
        changed += 1;
    }
    changed
}

/// Tell the user how many footage items followed the project folder (none: nothing).
pub fn report(s: &mut Session, followed: usize) {
    if followed > 0 {
        s.toast(format!("{followed} footage item{} followed the project folder", if followed == 1 { "" } else { "s" }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use effectcraft_project::Proxy;
    use serde_json::json;

    fn rel(f: &str, d: &str) -> Option<String> {
        relative(f, d).map(|v| v.join("/"))
    }

    #[test]
    fn relative_places_and_folders() {
        assert_eq!(rel("/shows/ep1/footage/a.mov", "/shows/ep1/project").as_deref(), Some("../footage/a.mov"));
        assert_eq!(rel(r"J:\Shows\Ep1\Footage\a.mov", r"J:\Shows\Ep1").as_deref(), Some("Footage/a.mov"));
        assert_eq!(rel(r"D:\Stock\a.mov", r"C:\Shows\Ep1"), None, "another drive");
        assert_eq!(rel("/run/media/x/a.mov", "/home/sam/ep1"), None, "only the root in common");
        let r = relative(r"J:\Shows\Ep1\Footage\a.mov", r"J:\Shows\Ep1\Project").unwrap();
        assert_eq!(resolve(&r, "/home/sam/Ep1/Project").as_deref(), Some("/home/sam/Ep1/Footage/a.mov"), "Windows to Linux");
        // more `..` than the folder has: refused, not stopped at the root
        assert_eq!(resolve(&["..".into(), "..".into(), "a.mov".into()], "/ep1"), None);
        assert_eq!(folder_of("/shows/ep1/p.ecproj").as_deref(), Some("/shows/ep1"));
        assert_eq!(folder_of("p.ecproj"), None);
        assert!(same_folder(r"J:\Shows\Ep1\", "j:/Shows/Ep1"));
    }

    /// `<root>/Show/Project/p.ecproj` with footage in `<root>/Show/Footage`: a movie, a 3-frame
    /// sequence and a proxy, all real files; saved through `file.save`. The session.
    fn saved_show(root: &std::path::Path) -> (Session, String) {
        let show = root.join("Show");
        let footage = show.join("Footage");
        std::fs::create_dir_all(footage.join("seq")).unwrap();
        std::fs::create_dir_all(show.join("Project")).unwrap();
        let file = |rel: &str, bytes: usize| {
            let p = footage.join(rel);
            std::fs::write(&p, vec![7u8; bytes]).unwrap();
            p.to_string_lossy().into_owned()
        };
        let movie = file("a.mov", 1000);
        let frames: Vec<String> = (1..=3).map(|n| file(&format!("seq/f_{n:04}.png"), 100 + n)).collect();
        let proxy = file("a_proxy.mp4", 300);
        let mut s = Session::default();
        let p = std::sync::Arc::make_mut(&mut s.project);
        let id = p.add_item("a", crate::color::Label::None, None, ItemKind::Footage(Footage { path: movie, has_video: true, ..Default::default() }));
        p.add_item(
            "seq",
            crate::color::Label::None,
            None,
            ItemKind::Footage(Footage { path: frames[0].clone(), kind: FootageKind::Sequence, sequence: frames, has_video: true, ..Default::default() }),
        );
        p.add_item(
            "data",
            crate::color::Label::None,
            None,
            ItemKind::Footage(Footage { path: "/elsewhere/table.csv".into(), kind: FootageKind::Data, ..Default::default() }),
        );
        if let Some(it) = p.items.get_mut(&id) {
            it.proxy = Some(Box::new(Proxy { footage: Footage { path: proxy, has_video: true, ..Default::default() }, enabled: true }));
        }
        let path = show.join("Project").join("p.ecproj").to_string_lossy().into_owned();
        s.execute("file.save", json!({"path": path})).unwrap();
        (s, path)
    }

    /// Every footage path, sequence frame and proxy path, sorted.
    fn all_paths(s: &Session) -> Vec<String> {
        let mut v = Vec::new();
        for it in s.project.items.values() {
            if let ItemKind::Footage(f) = &it.kind {
                v.push(f.path.clone());
                v.extend(f.sequence.iter().cloned());
            }
            if let Some(x) = &it.proxy {
                v.push(x.footage.path.clone());
            }
        }
        v.sort();
        v
    }

    fn open(path: &std::path::Path) -> Session {
        let mut s = Session::default();
        s.execute("file.open", json!({"path": path.to_string_lossy()})).unwrap();
        s
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ec-anchor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn footage_sequences_and_proxies_follow_a_moved_show_folder() {
        let root = tmp("moved");
        let (_, path) = saved_show(&root);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"savedDir\"") && text.contains("\"savedSizes\""), "the file records its folder and sizes");
        std::fs::rename(root.join("Show"), root.join("Moved")).unwrap();
        let s = open(&root.join("Moved").join("Project").join("p.ecproj"));
        let paths = all_paths(&s);
        let moved = root.join("Moved").to_string_lossy().into_owned();
        assert_eq!(paths.iter().filter(|p| p.starts_with(&moved)).count(), 1 + 1 + 3 + 1, "movie, sequence (path and 3 frames), proxy: {paths:?}");
        assert!(paths.contains(&"/elsewhere/table.csv".to_string()), "Data footage is left alone");
        assert!(!s.is_dirty(), "following the folder is not an edit");
    }

    #[test]
    fn short_and_odd_paths_do_not_panic() {
        // the Legion Go's crash: `\\p.ecproj` gave the folder `\`, and parts sliced 2 bytes of 1
        assert_eq!(folder_of(r"\\p.ecproj"), None);
        assert_eq!(folder_of(r"\\srv\share\p.ecproj").as_deref(), Some(r"\\srv\share"));
        for p in [r"\", "/", "C", "C:", r"\\", r"\\srv"] {
            let _ = parts(p);
            let _ = relative("/a/b.mov", p);
            let _ = resolve(&["..".to_string(), "b.mov".to_string()], p);
        }
    }

    #[test]
    fn a_same_size_file_at_the_moved_place_is_not_taken() {
        let root = tmp("same-size");
        let (_, _path) = saved_show(&root);
        std::fs::rename(root.join("Show"), root.join("Moved")).unwrap();
        // a.mov beside the moved project: same 1000 bytes, other content
        std::fs::write(root.join("Moved").join("Footage").join("a.mov"), vec![0u8; 1000]).unwrap();
        let s = open(&root.join("Moved").join("Project").join("p.ecproj"));
        let paths = all_paths(&s);
        let old_movie = root.join("Show").join("Footage").join("a.mov").to_string_lossy().into_owned();
        assert!(paths.contains(&old_movie), "a.mov keeps its old path: {paths:?}");
    }

    #[test]
    fn footage_without_recorded_sizes_does_not_follow_even_to_a_folder() {
        let root = tmp("no-sizes");
        let (_, path) = saved_show(&root);
        // an anchor with the folder but no sizes or fingerprints
        let mut doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let o = doc.as_object_mut().unwrap();
        assert!(o.remove("savedSizes").is_some() && o.remove("savedPrints").is_some(), "a save records both");
        std::fs::write(&path, serde_json::to_string(&doc).unwrap()).unwrap();
        std::fs::rename(root.join("Show"), root.join("Moved")).unwrap();
        let footage = root.join("Moved").join("Footage");
        std::fs::write(footage.join("a.mov"), b"four").unwrap();
        std::fs::remove_file(footage.join("a_proxy.mp4")).unwrap();
        std::fs::create_dir(footage.join("a_proxy.mp4")).unwrap();
        let s = open(&root.join("Moved").join("Project").join("p.ecproj"));
        let paths = all_paths(&s);
        assert!(paths.iter().all(|p| !p.contains("Moved")), "nothing followed: {paths:?}");
    }

    #[test]
    fn a_different_file_at_the_moved_place_is_not_taken() {
        let root = tmp("other");
        let (_, _path) = saved_show(&root);
        std::fs::rename(root.join("Show"), root.join("Moved")).unwrap();
        std::fs::write(root.join("Moved").join("Footage").join("a.mov"), b"another movie").unwrap();
        let s = open(&root.join("Moved").join("Project").join("p.ecproj"));
        let paths = all_paths(&s);
        let old_movie = root.join("Show").join("Footage").join("a.mov").to_string_lossy().into_owned();
        assert!(paths.contains(&old_movie), "a.mov keeps its old path: {paths:?}");
        let new_proxy = root.join("Moved").join("Footage").join("a_proxy.mp4").to_string_lossy().into_owned();
        assert!(paths.contains(&new_proxy), "its proxy (the right size) followed: {paths:?}");
    }

    #[test]
    fn an_older_file_without_the_anchor_opens_as_before() {
        let root = tmp("older");
        let (_, path) = saved_show(&root);
        let mut doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        doc.as_object_mut().unwrap().remove("savedDir");
        doc.as_object_mut().unwrap().remove("savedSizes");
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        let before = all_paths(&open(std::path::Path::new(&path)));
        std::fs::rename(root.join("Show"), root.join("Moved")).unwrap();
        let after = all_paths(&open(&root.join("Moved").join("Project").join("p.ecproj")));
        assert_eq!(after, before, "no anchor: paths unchanged");
    }
}
