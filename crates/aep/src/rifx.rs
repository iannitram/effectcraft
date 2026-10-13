//! The RIFX container of an `.aep` file: big-endian RIFF.
//!
//! ```text
//! file  = "RIFX" u32 size "Egg!" chunk*        (an XMP packet may follow the RIFX body)
//! chunk = id[4] u32 size data[size] pad-to-even
//! LIST  = "LIST" u32 size type[4] chunk*
//! ```
//!
//! Every size comes from the file, so every read is bounds-checked and the tree is limited in
//! depth and in chunk count.

use crate::AepError;

/// Deepest `LIST` nesting followed. Deeper lists are kept as opaque data.
const MAX_DEPTH: usize = 64;
/// Most chunks read from one file (a 5 MB project of ours has about 60 000).
const MAX_CHUNKS: usize = 4_000_000;

/// One chunk. A `LIST` has `list` set and its sub-chunks in `kids`; `data` is then empty,
/// except for lists the reader keeps opaque (`btdk`, or anything past the depth limit).
#[derive(Clone, Debug)]
pub struct Chunk<'a> {
    pub id: [u8; 4],
    pub list: Option<[u8; 4]>,
    pub data: &'a [u8],
    pub kids: Vec<Chunk<'a>>,
}

impl<'a> Chunk<'a> {
    /// The first direct child chunk with this id (not a `LIST`).
    pub fn first(&self, id: &[u8; 4]) -> Option<&Chunk<'a>> {
        self.kids.iter().find(|k| k.list.is_none() && &k.id == id)
    }
    /// The first direct child `LIST` of this type.
    pub fn list(&self, ty: &[u8; 4]) -> Option<&Chunk<'a>> {
        self.kids.iter().find(|k| k.list.as_ref() == Some(ty))
    }
    /// Every direct child `LIST` of this type.
    pub fn lists<'s>(&'s self, ty: &'s [u8; 4]) -> impl Iterator<Item = &'s Chunk<'a>> + 's {
        self.kids.iter().filter(move |k| k.list.as_ref() == Some(ty))
    }
    /// The first `LIST` of this type anywhere below (depth-first).
    pub fn find_list(&self, ty: &[u8; 4]) -> Option<&Chunk<'a>> {
        for k in &self.kids {
            if k.list.as_ref() == Some(ty) {
                return Some(k);
            }
            if let Some(f) = k.find_list(ty) {
                return Some(f);
            }
        }
        None
    }
    pub fn is(&self, id: &[u8; 4]) -> bool {
        self.list.is_none() && &self.id == id
    }
    pub fn is_list(&self, ty: &[u8; 4]) -> bool {
        self.list.as_ref() == Some(ty)
    }
}

fn four(b: &[u8], at: usize) -> Option<[u8; 4]> {
    b.get(at..at.checked_add(4)?)?.try_into().ok()
}

fn parse_chunks<'a>(buf: &'a [u8], depth: usize, budget: &mut usize) -> Vec<Chunk<'a>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let (Some(id), Some(size)) = (four(buf, pos), four(buf, pos.saturating_add(4))) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let start = pos.saturating_add(8);
        // A size running past the end is clamped: the lenient reading of a truncated file.
        let size = (u32::from_be_bytes(size) as usize).min(buf.len().saturating_sub(start));
        let data = buf.get(start..start.saturating_add(size)).unwrap_or(&[]);
        let list = if &id == b"LIST" { four(data, 0) } else { None };
        let opaque = depth >= MAX_DEPTH || list.as_ref() == Some(b"btdk");
        match list {
            Some(ty) if !opaque => {
                out.push(Chunk { id, list: Some(ty), data: &[], kids: parse_chunks(data.get(4..).unwrap_or(&[]), depth + 1, budget) });
            }
            Some(ty) => out.push(Chunk { id, list: Some(ty), data: data.get(4..).unwrap_or(&[]), kids: Vec::new() }),
            None => out.push(Chunk { id, list: None, data, kids: Vec::new() }),
        }
        pos = start.saturating_add(size).saturating_add(size & 1);
    }
    out
}

/// The chunk tree of an `.aep` file and whatever follows the RIFX body (the XMP packet).
pub fn parse(bytes: &[u8]) -> Result<(Chunk<'_>, &[u8]), AepError> {
    if bytes.get(..4) != Some(b"RIFX") {
        return Err(AepError::NotAep("the file does not start with RIFX".into()));
    }
    let size = four(bytes, 4).map(u32::from_be_bytes).ok_or_else(|| AepError::NotAep("the file is shorter than a RIFX header".into()))? as usize;
    let form = four(bytes, 8).ok_or_else(|| AepError::NotAep("the file is shorter than a RIFX header".into()))?;
    if &form != b"Egg!" {
        return Err(AepError::NotAep("the RIFX file is not an After Effects project".into()));
    }
    let end = size.saturating_add(8).min(bytes.len());
    let body = bytes.get(12..end).unwrap_or(&[]);
    let mut budget = MAX_CHUNKS;
    let kids = parse_chunks(body, 0, &mut budget);
    Ok((Chunk { id: *b"RIFX", list: Some(form), data: &[], kids }, bytes.get(end..).unwrap_or(&[])))
}

// Big-endian field readers: a field past the end of its chunk reads as zero, which the
// importer treats as "absent" (older saves have shorter chunks).

pub fn u8_at(d: &[u8], o: usize) -> u8 {
    d.get(o).copied().unwrap_or(0)
}
pub fn u16_at(d: &[u8], o: usize) -> u16 {
    o.checked_add(2).and_then(|e| d.get(o..e)).and_then(|s| s.try_into().ok()).map(u16::from_be_bytes).unwrap_or(0)
}
pub fn i16_at(d: &[u8], o: usize) -> i16 {
    u16_at(d, o) as i16
}
pub fn u32_at(d: &[u8], o: usize) -> u32 {
    four(d, o).map(u32::from_be_bytes).unwrap_or(0)
}
pub fn i32_at(d: &[u8], o: usize) -> i32 {
    u32_at(d, o) as i32
}
pub fn f32_at(d: &[u8], o: usize) -> f64 {
    let v = four(d, o).map(f32::from_be_bytes).unwrap_or(0.0);
    if v.is_finite() { v as f64 } else { 0.0 }
}
pub fn f64_at(d: &[u8], o: usize) -> f64 {
    let v = o.checked_add(8).and_then(|e| d.get(o..e)).and_then(|s| s.try_into().ok()).map(f64::from_be_bytes).unwrap_or(0.0);
    if v.is_finite() { v } else { 0.0 }
}

/// A NUL-terminated string field, or (newer saves) a nested `Utf8` chunk.
pub fn text_field(d: &[u8]) -> String {
    if d.get(..4) == Some(b"Utf8") {
        let n = u32_at(d, 4) as usize;
        return String::from_utf8_lossy(d.get(8..8usize.saturating_add(n).min(d.len())).unwrap_or(&[])).into_owned();
    }
    let end = d.iter().position(|b| *b == 0).unwrap_or(d.len());
    String::from_utf8_lossy(d.get(..end).unwrap_or(&[])).into_owned()
}
