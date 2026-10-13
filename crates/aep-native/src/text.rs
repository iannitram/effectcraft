//! The text document of a text layer.
//!
//! The file stores it as text in a PostScript-like syntax with numbered keys: dictionaries
//! `<< /0 value /1 value >>`, arrays `[ … ]`, names `/Name`, numbers, `true` / `false`, and
//! strings `( … )` that are UTF-16BE when they start with FE FF. What this reader takes from it
//! (key paths, arrays written `#n`):
//!
//! - the fonts: `/0 /1 /0 #n /0 /0 /0`, PostScript names;
//! - the text: `/1 /1 #0 /0 /0`, lines separated by carriage returns, one more at the end;
//! - the style runs: `/1 /1 #0 /0 /6 /0 #n`, each `/1` a length in UTF-16 units and
//!   `/0 /0 /6` its character style, of which `/0` is the font's index, `/1` the size in
//!   pixels, `/2` and `/3` faux bold and italic, `/4` whether leading is automatic and `/5` the
//!   leading in pixels, `/6` and `/7` the horizontal and vertical scale (1.0 = 100%), `/8` the
//!   tracking, `/9` the baseline shift in pixels (positive is up; measured: a title with 62.0
//!   here sits that much higher in its reference export than without it), and `/53 /0 /1` the
//!   fill colour as alpha, red, green, blue;
//! - the paragraph runs: `/1 /1 #0 /0 /5 /0 #n`, of whose paragraph style `/0 /0 /5` the key
//!   `/0` is the alignment: 0 left, 1 right, 2 centre (across 39 paragraphs measured: every
//!   title that sits centred in its reference export has 2).
//!
//! Everything else in the document (stroke, box text, the other paragraph
//! settings) is left at EffectCraft's defaults for now; the importer says so.

use effectcraft_keyframe::{Justify, StyleRun, TextDoc};

/// Deepest nesting followed and most values read: the document comes from the file.
const MAX_DEPTH: usize = 64;
const MAX_VALUES: usize = 400_000;

#[derive(Clone, Debug, PartialEq)]
enum Cos {
    Dict(Vec<(String, Cos)>),
    Array(Vec<Cos>),
    Num(f64),
    Bool(bool),
    Str(String),
    Name,
}

impl Cos {
    fn get(&self, key: &str) -> Option<&Cos> {
        match self {
            Cos::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn at(&self, i: usize) -> Option<&Cos> {
        match self {
            Cos::Array(a) => a.get(i),
            _ => None,
        }
    }
    fn path(&self, keys: &[&str]) -> Option<&Cos> {
        keys.iter().try_fold(self, |v, k| match k.strip_prefix('#') {
            Some(i) => v.at(i.parse().ok()?),
            None => v.get(k),
        })
    }
    fn items(&self) -> &[Cos] {
        match self {
            Cos::Array(a) => a,
            _ => &[],
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Cos::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Cos::Str(s) => Some(s),
            _ => None,
        }
    }
    fn bool(&self) -> Option<bool> {
        match self {
            Cos::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    budget: usize,
}

fn is_delim(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0 | b'<' | b'>' | b'[' | b']' | b'(' | b')' | b'/')
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.b.get(self.pos).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0)) {
            self.pos += 1;
        }
    }
    fn starts(&self, s: &[u8]) -> bool {
        self.b.get(self.pos..self.pos.saturating_add(s.len())) == Some(s)
    }
    fn word(&mut self) -> &[u8] {
        let start = self.pos;
        while self.b.get(self.pos).is_some_and(|c| !is_delim(*c)) {
            self.pos += 1;
        }
        self.b.get(start..self.pos).unwrap_or(&[])
    }
    /// The keys and values of a dictionary body, up to `>>` or the end of the data.
    fn dict(&mut self, depth: usize) -> Cos {
        let mut out = Vec::new();
        loop {
            self.skip_space();
            if self.pos >= self.b.len() || self.budget == 0 {
                break;
            }
            if self.starts(b">>") {
                self.pos += 2;
                break;
            }
            if self.b.get(self.pos) != Some(&b'/') {
                // Not a key: skip one byte and resynchronise.
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            let key = String::from_utf8_lossy(self.word()).into_owned();
            match self.value(depth) {
                Some(v) => out.push((key, v)),
                None => break,
            }
        }
        Cos::Dict(out)
    }
    fn value(&mut self, depth: usize) -> Option<Cos> {
        self.skip_space();
        if self.budget == 0 || depth > MAX_DEPTH {
            return None;
        }
        self.budget -= 1;
        let c = *self.b.get(self.pos)?;
        if self.starts(b"<<") {
            self.pos += 2;
            return Some(self.dict(depth + 1));
        }
        match c {
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    match self.b.get(self.pos) {
                        None => break,
                        Some(b']') => {
                            self.pos += 1;
                            break;
                        }
                        // A stray closer inside an array: stop rather than loop.
                        Some(b'>') | Some(b')') => break,
                        Some(_) => match self.value(depth + 1) {
                            Some(v) => items.push(v),
                            None => break,
                        },
                    }
                }
                Some(Cos::Array(items))
            }
            b'(' => {
                self.pos += 1;
                let mut raw = Vec::new();
                while let Some(&ch) = self.b.get(self.pos) {
                    self.pos += 1;
                    match ch {
                        b')' => break,
                        b'\\' => {
                            if let Some(&esc) = self.b.get(self.pos) {
                                raw.push(esc);
                                self.pos += 1;
                            }
                        }
                        ch => raw.push(ch),
                    }
                }
                Some(Cos::Str(match raw.strip_prefix(&[0xfe, 0xff]) {
                    Some(utf16) => {
                        let units: Vec<u16> = utf16.chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
                        String::from_utf16_lossy(&units)
                    }
                    None => raw.iter().map(|b| *b as char).collect(),
                }))
            }
            b'/' => {
                self.pos += 1;
                self.word();
                Some(Cos::Name)
            }
            _ => {
                let w = self.word();
                if w.is_empty() {
                    // A delimiter that starts no value here (a lone `>` or `)`).
                    self.pos += 1;
                    return Some(Cos::Name);
                }
                Some(match w {
                    b"true" => Cos::Bool(true),
                    b"false" => Cos::Bool(false),
                    _ => std::str::from_utf8(w).ok().and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite()).map(Cos::Num).unwrap_or(Cos::Name),
                })
            }
        }
    }
}

/// What a text document says, as far as this reader understands it.
#[derive(Clone, Debug)]
pub(crate) struct ReadText {
    pub doc: TextDoc,
    /// The document has settings this reader left at their defaults (always true for now:
    /// tracking, leading, stroke and paragraph settings are not read).
    pub partial: bool,
}

/// Read a text layer's document. `None` when no text can be found in it.
pub(crate) fn text_doc(blob: &[u8]) -> Option<ReadText> {
    let mut p = Parser { b: blob, pos: 0, budget: MAX_VALUES };
    let root = p.dict(0);
    let story = root.path(&["1", "1", "#0", "0"])?;
    let raw = story.get("0")?.str()?;
    let fonts: Vec<&str> =
        root.path(&["0", "1", "0"]).map(|a| a.items().iter().filter_map(|f| f.path(&["0", "0", "0"]).and_then(Cos::str)).collect()).unwrap_or_default();
    // After Effects ends the text with one carriage return and separates lines with them.
    let units_total = raw.encode_utf16().count();
    let body = raw.strip_suffix('\r').unwrap_or(raw);
    let mut doc = TextDoc::plain(&body.replace('\r', "\n"));
    let mut runs: Vec<StyleRun> = Vec::new();
    let base = doc.base_style();
    let mut units_seen = 0usize;
    let chars: Vec<char> = raw.chars().collect();
    let mut char_at = 0usize;
    for run in story.path(&["6", "0"]).map(Cos::items).unwrap_or(&[]) {
        let units = run.get("1").and_then(Cos::num).filter(|n| *n >= 0.0).map(|n| n as usize).unwrap_or(0).min(units_total.saturating_sub(units_seen));
        units_seen += units;
        // Lengths are in UTF-16 units; ours are in characters.
        let mut len = 0usize;
        let mut left = units;
        while left > 0 {
            let Some(c) = chars.get(char_at) else { break };
            left = left.saturating_sub(c.len_utf16());
            char_at += 1;
            len += 1;
        }
        let Some(feat) = run.path(&["0", "0", "6"]) else { continue };
        let mut style = base.clone();
        if let Some(name) = feat.get("0").and_then(Cos::num).and_then(|i| fonts.get(i as usize)) {
            style.font = (*name).to_string();
        }
        if let Some(size) = feat.get("1").and_then(Cos::num).filter(|s| *s > 0.0) {
            style.size = size.min(100_000.0);
        }
        if let Some(b) = feat.get("2").and_then(Cos::bool) {
            style.faux_bold = b;
        }
        if let Some(i) = feat.get("3").and_then(Cos::bool) {
            style.faux_italic = i;
        }
        // Leading: a number only counts when automatic leading is off.
        if feat.get("4").and_then(Cos::bool) == Some(false)
            && let Some(leading) = feat.get("5").and_then(Cos::num).filter(|l| *l > 0.0)
        {
            style.leading = Some(leading.min(100_000.0));
        }
        if let Some(h) = feat.get("6").and_then(Cos::num).filter(|s| *s > 0.0) {
            style.h_scale = (h * 100.0).min(100_000.0);
        }
        if let Some(v) = feat.get("7").and_then(Cos::num).filter(|s| *s > 0.0) {
            style.v_scale = (v * 100.0).min(100_000.0);
        }
        if let Some(t) = feat.get("8").and_then(Cos::num) {
            style.tracking = t.clamp(-100_000.0, 100_000.0);
        }
        // Baseline shift in pixels, positive raises the text.
        if let Some(s) = feat.get("9").and_then(Cos::num).filter(|s| s.is_finite()) {
            style.baseline_shift = s.clamp(-100_000.0, 100_000.0);
        }
        if let Some(c) = feat.path(&["53", "0", "1"]) {
            let ch = |i: usize| c.at(i).and_then(Cos::num).unwrap_or(1.0).clamp(0.0, 1.0) as f32;
            if c.items().len() >= 4 {
                style.fill = [ch(1), ch(2), ch(3), ch(0)];
            }
        }
        if len > 0 {
            runs.push(StyleRun { len, style });
        }
    }
    // The closing carriage return belongs to the last run and is not part of our text.
    if raw.ends_with('\r')
        && let Some(last) = runs.last_mut()
    {
        last.len = last.len.saturating_sub(1);
    }
    runs.retain(|r| r.len > 0);
    if !runs.is_empty() {
        doc.set_runs(runs);
    }
    // Alignment: the first paragraph's (the titles this was measured on are one paragraph, or
    // several set alike).
    let align = story.path(&["5", "0", "#0", "0", "0", "5", "0"]).and_then(Cos::num);
    doc.justify = match align.map(|a| a.round() as i64) {
        Some(1) => Justify::Right,
        Some(2) => Justify::Center,
        _ => Justify::Left,
    };
    Some(ReadText { doc, partial: true })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        let mut v = vec![b'(', 0xfe, 0xff];
        for u in s.encode_utf16() {
            for b in u.to_be_bytes() {
                // Parentheses and backslashes inside a string are escaped.
                if matches!(b, b'(' | b')' | b'\\') {
                    v.push(b'\\');
                }
                v.push(b);
            }
        }
        v.push(b')');
        v
    }

    /// A text document with two fonts and the given (length, font index, size, fill) runs.
    pub(crate) fn document(text: &str, runs: &[(usize, usize, f64, [f64; 4])]) -> Vec<u8> {
        let mut d = b"/98 << /0 7 >> /0 << /1 << /0 [ ".to_vec();
        for name in ["Title-Bold", "Body-Regular"] {
            d.extend(b"<< /0 << /99 /CoolTypeFont /0 << /0 ");
            d.extend(utf16(name));
            d.extend(b" /2 1 >> >> >> ");
        }
        d.extend(b"] >> >> /1 << /1 [ << /0 << /0 ");
        d.extend(utf16(text));
        d.extend(b" /5 << /0 [ ] >> /6 << /0 [ ");
        for (len, font, size, fill) in runs {
            d.extend(
                format!(
                    "<< /0 << /0 << /0 (Normal RGB) /6 << /0 {font} /1 {size:?} /2 false /3 true /53 << /99 /SimplePaint /0 << /0 1 /1 [ {:?} {:?} {:?} {:?} ] >> >> >> >> >> /1 {len} >> ",
                    fill[0], fill[1], fill[2], fill[3]
                )
                .bytes(),
            );
        }
        // Beside the story, the frame's mark as every point-text title has it.
        d.extend(b"] >> >> /1 << /0 [ << /0 0 >> ] >> >> ] >>");
        d
    }

    #[test]
    fn text_fonts_sizes_and_colours() {
        // "Hi (you)" on one line, "naïve 😀" on the next, then After Effects' closing return.
        let text = "Hi (you)\rna\u{ef}ve \u{1f600}\r";
        let blob = document(text, &[(2, 0, 120.0, [1.0, 1.0, 0.5, 0.25]), (text.encode_utf16().count() - 2, 1, 48.5, [1.0, 0.0, 0.0, 1.0])]);
        let t = text_doc(&blob).unwrap();
        assert_eq!(t.doc.text, "Hi (you)\nna\u{ef}ve \u{1f600}");
        let runs = t.doc.runs();
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].len, runs[0].style.font.as_str(), runs[0].style.size), (2, "Title-Bold", 120.0));
        assert_eq!(runs[0].style.fill, [1.0, 0.5, 0.25, 1.0]);
        assert!(runs[0].style.faux_italic && !runs[0].style.faux_bold);
        // The emoji is two UTF-16 units and one character; the closing return is dropped.
        assert_eq!((runs[1].len, runs[1].style.font.as_str(), runs[1].style.size), (t.doc.char_len() - 2, "Body-Regular", 48.5));
        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), t.doc.char_len());
    }

    fn replace(d: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
        let at = d.windows(from.len()).position(|w| w == from).unwrap();
        [&d[..at], to, &d[at + from.len()..]].concat()
    }

    #[test]
    fn alignment_tracking_leading_and_scale() {
        let plain = document("Episode\rTitle\r", &[(14, 0, 96.0, [1.0, 1.0, 1.0, 1.0])]);
        // As written: left-aligned, automatic leading, no tracking, scale 100%.
        let t = text_doc(&plain).unwrap();
        assert_eq!((t.doc.justify, t.doc.leading, t.doc.tracking, t.doc.h_scale, t.doc.v_scale), (Justify::Left, None, 0.0, 100.0, 100.0));
        let styled = replace(&plain, b"/5 << /0 [ ] >>", b"/5 << /0 [ << /0 << /0 << /0 (Normal RGB) /5 << /0 2 /1 0.0 >> >> >> /1 14 >> ] >>");
        let styled = replace(&styled, b"/2 false /3 true", b"/2 false /3 true /4 false /5 90.0 /6 1.5 /7 0.5 /8 100");
        let t = text_doc(&styled).unwrap();
        assert_eq!(t.doc.text, "Episode\nTitle");
        assert_eq!((t.doc.justify, t.doc.leading, t.doc.tracking, t.doc.h_scale, t.doc.v_scale), (Justify::Center, Some(90.0), 100.0, 150.0, 50.0));
        // Baseline shift: none as written, and the document's number when it has one.
        assert_eq!(t.doc.baseline_shift, 0.0);
        let raised = replace(&styled, b"/8 100", b"/8 100 /9 62.0");
        assert_eq!(text_doc(&raised).unwrap().doc.baseline_shift, 62.0);
        let lowered = replace(&styled, b"/8 100", b"/8 100 /9 -12.5");
        assert_eq!(text_doc(&lowered).unwrap().doc.baseline_shift, -12.5);
        // Right alignment, and a leading number that does not count while leading is automatic.
        let right = replace(&styled, b"/5 << /0 2 /1 0.0 >>", b"/5 << /0 1 /1 0.0 >>");
        let right = replace(&right, b"/4 false /5 90.0", b"/4 true /5 0.01");
        let t = text_doc(&right).unwrap();
        assert_eq!((t.doc.justify, t.doc.leading), (Justify::Right, None));
    }

    #[test]
    fn a_document_without_text_is_none_and_damage_never_panics() {
        assert!(text_doc(b"").is_none());
        assert!(text_doc(b"/0 << /1 << >> >>").is_none());
        let good = document("Title\r", &[(6, 0, 72.0, [1.0, 1.0, 1.0, 1.0])]);
        for n in 0..good.len() {
            let _ = text_doc(&good[..n]);
        }
        for i in 0..good.len() {
            for v in [b'(', b')', b'<', b'>', b'[', b']', b'/', 0xff, 0] {
                let mut bad = good.clone();
                bad[i] = v;
                let _ = text_doc(&bad);
            }
        }
        // Unbalanced nesting deeper than the limit.
        let deep = b"/0 ".iter().copied().chain(std::iter::repeat_n(*b"<< /0 ", 5000).flatten()).collect::<Vec<u8>>();
        let _ = text_doc(&deep);
        let deep = b"/0 ".iter().copied().chain(std::iter::repeat_n(b'[', 100_000)).collect::<Vec<u8>>();
        let _ = text_doc(&deep);
    }
}
