//! Big-endian byte reader.

use crate::{Error, Result};

#[derive(Clone)]
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Reader { data, pos }
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::Truncated);
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64> {
        let b = self.bytes(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_be_bytes(a))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
    /// A big-endian double where NaN and ±infinity read as 0 (they are never meaningful in a file).
    pub fn f64_finite(&mut self) -> Result<f64> {
        let v = self.f64()?;
        Ok(if v.is_finite() { v } else { 0.0 })
    }
    pub fn tag(&mut self) -> Result<[u8; 4]> {
        let b = self.bytes(4)?;
        Ok([b[0], b[1], b[2], b[3]])
    }
    /// A length field: 4 bytes in PSD, 8 bytes in PSB (`wide`).
    pub fn length(&mut self, wide: bool) -> Result<usize> {
        let v = if wide { self.u64()? } else { self.u32()? as u64 };
        usize::try_from(v).map_err(|_| Error::Invalid("length".into()))
    }
    /// A Pascal string padded so that length byte + text is a multiple of `pad`.
    pub fn pascal(&mut self, pad: usize) -> Result<String> {
        let n = self.u8()? as usize;
        let s = self.bytes(n)?;
        let total = 1 + n;
        let padded = total.div_ceil(pad) * pad;
        self.skip(padded - total)?;
        // Pascal strings are in the system (Mac Roman) encoding; ASCII covers the common case.
        Ok(s.iter().map(|&b| if b < 0x80 { b as char } else { mac_roman(b) }).collect())
    }
}

/// Mac OS Roman upper half (0x80..=0xFF), as published by the Unicode Consortium mapping table.
/// Byte 0xCA was corrected to U+00A0 (no-break space) from U+0020 (space).
fn mac_roman(b: u8) -> char {
    const MAC_ROMAN: [char; 128] = [
        '\u{00C4}', '\u{00C5}', '\u{00C7}', '\u{00C9}', '\u{00D1}', '\u{00D6}', '\u{00DC}', '\u{00E1}', '\u{00E0}', '\u{00E2}', '\u{00E4}', '\u{00E3}',
        '\u{00E5}', '\u{00E7}', '\u{00E9}', '\u{00E8}', '\u{00EA}', '\u{00EB}', '\u{00ED}', '\u{00EC}', '\u{00EE}', '\u{00EF}', '\u{00F1}', '\u{00F3}',
        '\u{00F2}', '\u{00F4}', '\u{00F6}', '\u{00F5}', '\u{00FA}', '\u{00F9}', '\u{00FB}', '\u{00FC}', '\u{2020}', '\u{00B0}', '\u{00A2}', '\u{00A3}',
        '\u{00A7}', '\u{2022}', '\u{00B6}', '\u{00DF}', '\u{00AE}', '\u{00A9}', '\u{2122}', '\u{00B4}', '\u{00A8}', '\u{2260}', '\u{00C6}', '\u{00D8}',
        '\u{221E}', '\u{00B1}', '\u{2264}', '\u{2265}', '\u{00A5}', '\u{00B5}', '\u{2202}', '\u{2211}', '\u{220F}', '\u{03C0}', '\u{222B}', '\u{00AA}',
        '\u{00BA}', '\u{03A9}', '\u{00E6}', '\u{00F8}', '\u{00BF}', '\u{00A1}', '\u{00AC}', '\u{221A}', '\u{0192}', '\u{2248}', '\u{2206}', '\u{00AB}',
        '\u{00BB}', '\u{2026}', '\u{00A0}', '\u{00C0}', '\u{00C3}', '\u{00D5}', '\u{0152}', '\u{0153}', '\u{2013}', '\u{2014}', '\u{201C}', '\u{201D}',
        '\u{2018}', '\u{2019}', '\u{00F7}', '\u{25CA}', '\u{00FF}', '\u{0178}', '\u{2044}', '\u{20AC}', '\u{2039}', '\u{203A}', '\u{FB01}', '\u{FB02}',
        '\u{2021}', '\u{00B7}', '\u{201A}', '\u{201E}', '\u{2030}', '\u{00C2}', '\u{00CA}', '\u{00C1}', '\u{00CB}', '\u{00C8}', '\u{00CD}', '\u{00CE}',
        '\u{00CF}', '\u{00CC}', '\u{00D3}', '\u{00D4}', '\u{F8FF}', '\u{00D2}', '\u{00DA}', '\u{00DB}', '\u{00D9}', '\u{0131}', '\u{02C6}', '\u{02DC}',
        '\u{00AF}', '\u{02D8}', '\u{02D9}', '\u{02DA}', '\u{00B8}', '\u{02DD}', '\u{02DB}', '\u{02C7}',
    ];
    MAC_ROMAN[(b - 0x80) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_roman_matches_unicode_table() {
        const KEY: [u32; 128] = [
            0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1, 0x00E0, 0x00E2, 0x00E4, 0x00E3, 0x00E5, 0x00E7, 0x00E9, 0x00E8, 0x00EA, 0x00EB,
            0x00ED, 0x00EC, 0x00EE, 0x00EF, 0x00F1, 0x00F3, 0x00F2, 0x00F4, 0x00F6, 0x00F5, 0x00FA, 0x00F9, 0x00FB, 0x00FC, 0x2020, 0x00B0, 0x00A2, 0x00A3,
            0x00A7, 0x2022, 0x00B6, 0x00DF, 0x00AE, 0x00A9, 0x2122, 0x00B4, 0x00A8, 0x2260, 0x00C6, 0x00D8, 0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5,
            0x2202, 0x2211, 0x220F, 0x03C0, 0x222B, 0x00AA, 0x00BA, 0x03A9, 0x00E6, 0x00F8, 0x00BF, 0x00A1, 0x00AC, 0x221A, 0x0192, 0x2248, 0x2206, 0x00AB,
            0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3, 0x00D5, 0x0152, 0x0153, 0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019, 0x00F7, 0x25CA, 0x00FF, 0x0178,
            0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02, 0x2021, 0x00B7, 0x201A, 0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1, 0x00CB, 0x00C8, 0x00CD, 0x00CE,
            0x00CF, 0x00CC, 0x00D3, 0x00D4, 0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC, 0x00AF, 0x02D8, 0x02D9, 0x02DA, 0x00B8, 0x02DD,
            0x02DB, 0x02C7,
        ];
        for i in 0..128 {
            assert_eq!(mac_roman(0x80 + i as u8) as u32, KEY[i], "byte 0x{:02X}", 0x80 + i as u8);
        }
    }

    #[test]
    fn mac_roman_spot_values() {
        assert_eq!(mac_roman(0x80), 'Ä');
        assert_eq!(mac_roman(0x8E), 'é');
        assert_eq!(mac_roman(0xCA), '\u{A0}');
        assert_eq!(mac_roman(0xDB), '€');
        assert_eq!(mac_roman(0xF0), '\u{F8FF}');
        assert_eq!(mac_roman(0xFF), '\u{02C7}');
    }

    #[test]
    fn pascal_pad_2() {
        let mut r = Reader::new(&[3, b'a', b'b', b'c']);
        assert_eq!(r.pascal(2), Ok("abc".to_string()));
        assert_eq!(r.pos, 4);
        let mut r = Reader::new(&[2, b'a', b'b', 0]);
        assert_eq!(r.pascal(2), Ok("ab".to_string()));
        assert_eq!(r.pos, 4);
    }

    #[test]
    fn pascal_pad_4_and_empty() {
        let mut r = Reader::new(&[1, b'x', 0, 0]);
        assert_eq!(r.pascal(4), Ok("x".to_string()));
        assert_eq!(r.pos, 4);
        let mut r = Reader::new(&[0, 0, 0, 0]);
        assert_eq!(r.pascal(4), Ok("".to_string()));
        assert_eq!(r.pos, 4);
        let mut r = Reader::new(&[2, b'h', b'i']);
        assert_eq!(r.pascal(1), Ok("hi".to_string()));
        assert_eq!(r.pos, 3);
    }

    #[test]
    fn pascal_mac_roman_text() {
        let mut r = Reader::new(&[2, 0x8E, 0xCA]);
        assert_eq!(r.pascal(1), Ok("é\u{A0}".to_string()));
    }

    #[test]
    fn pascal_truncated() {
        let mut r = Reader::new(&[5, b'a']);
        assert_eq!(r.pascal(1), Err(Error::Truncated));
        let mut r = Reader::new(&[1, b'x']);
        assert_eq!(r.pascal(4), Err(Error::Truncated));
    }

    #[test]
    fn length_psd_and_psb() {
        let mut r = Reader::new(&[0, 0, 1, 0]);
        assert_eq!(r.length(false), Ok(256));
        assert_eq!(r.pos, 4);
        let mut r = Reader::new(&[0, 0, 0, 0, 0, 0, 1, 0]);
        assert_eq!(r.length(true), Ok(256));
        assert_eq!(r.pos, 8);
    }

    #[test]
    fn integers_big_endian() {
        let d = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let mut r = Reader::new(&d);
        assert_eq!(r.u8(), Ok(0x12));
        let mut r = Reader::new(&d);
        assert_eq!(r.u16(), Ok(0x1234));
        let mut r = Reader::new(&d);
        assert_eq!(r.u32(), Ok(0x1234_5678));
        let mut r = Reader::new(&d);
        assert_eq!(r.u64(), Ok(0x1234_5678_9ABC_DEF0));
        let mut r = Reader::new(&[0xFF, 0xFE]);
        assert_eq!(r.i16(), Ok(-2));
        let mut r = Reader::new(&[0xFF; 4]);
        assert_eq!(r.i32(), Ok(-1));
    }

    #[test]
    fn floats_and_f64_finite() {
        let f32_bytes = 1.5f32.to_be_bytes();
        let mut r = Reader::new(&f32_bytes);
        assert_eq!(r.f32(), Ok(1.5));
        let f64_bytes = 2.5f64.to_be_bytes();
        let mut r = Reader::new(&f64_bytes);
        assert_eq!(r.f64_finite(), Ok(2.5));
        let nan_bytes = f64::NAN.to_be_bytes();
        let mut r = Reader::new(&nan_bytes);
        assert_eq!(r.f64_finite(), Ok(0.0));
        let inf_bytes = f64::INFINITY.to_be_bytes();
        let mut r = Reader::new(&inf_bytes);
        assert_eq!(r.f64_finite(), Ok(0.0));
        let ninf_bytes = f64::NEG_INFINITY.to_be_bytes();
        let mut r = Reader::new(&ninf_bytes);
        assert_eq!(r.f64_finite(), Ok(0.0));
    }

    #[test]
    fn tag_and_remaining() {
        let mut r = Reader::new(b"8BPSxyz");
        assert_eq!(r.tag(), Ok(*b"8BPS"));
        assert_eq!(r.remaining(), 3);
        let r = Reader::at(b"abcdef", 2);
        assert_eq!(r.remaining(), 4);
    }

    #[test]
    fn truncated_does_not_advance() {
        let mut r = Reader::new(&[1, 2, 3]);
        assert_eq!(r.u32(), Err(Error::Truncated));
        assert_eq!(r.pos, 0);
        assert_eq!(r.u16(), Ok(0x0102));
        assert_eq!(r.skip(2), Err(Error::Truncated));
        assert_eq!(r.pos, 2);
    }
}
