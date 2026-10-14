//! The browser's faces: Manrope for text and JetBrains Mono for code, each
//! regular and bold, smoothed in advance at every size pages use (see
//! `cargo xtask faces`). A page asking for another size gets the nearest
//! one; italics are slanted when drawn.
//!
//! A face's file: `HLNF`, then its size, line height and ascent in pixels
//! and its number of glyphs (four `u16`), then one record of 16 bytes per
//! glyph in character order (the character `u32`, where its coverage
//! starts in nibbles `u32`, its advance in sixteenths of a pixel `u16`,
//! where its grid starts from the pen at the line's top, x and y `i16`,
//! its width and height `u8`), then the coverage, four bits a pixel, the
//! first of each byte in its low nibble. Everything is little-endian.

#![no_std]

mod files;

use files::{MONO, MONO_BOLD, MONO_SIZES, SANS, SANS_BOLD, SIZES};

// Le P'tit Âne reads only large print.

const HEADER: usize = 12;
const RECORD: usize = 16;

/// A typeface at one size.
#[derive(Clone, Copy)]
pub struct Face {
    data: &'static [u8],
    /// The size it was drawn at, in pixels.
    pub size: i32,
    /// From one line's top to the next one's.
    pub height: i32,
    /// From a line's top to its baseline.
    pub ascent: i32,
    pub bold: bool,
    pub mono: bool,
    count: usize,
}

/// One character of a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub c: char,
    /// How far the pen moves, in sixteenths of a pixel.
    pub advance: i32,
    /// Where its grid starts, from the pen at the line's top.
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    at: usize,
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

impl Face {
    fn read(data: &'static [u8], bold: bool, mono: bool) -> Self {
        debug_assert_eq!(&data[..4], b"HLNF");
        Self {
            data,
            size: i32::from(u16_at(data, 4)),
            height: i32::from(u16_at(data, 6)),
            ascent: i32::from(u16_at(data, 8)),
            count: usize::from(u16_at(data, 10)),
            bold,
            mono,
        }
    }

    fn record(&self, i: usize) -> Glyph {
        let at = HEADER + i * RECORD;
        let d = self.data;
        Glyph {
            c: char::from_u32(u32_at(d, at)).unwrap_or('?'),
            at: u32_at(d, at + 4) as usize,
            advance: i32::from(u16_at(d, at + 8)),
            x: i32::from(u16_at(d, at + 10) as i16),
            y: i32::from(u16_at(d, at + 12) as i16),
            w: usize::from(d[at + 14]),
            h: usize::from(d[at + 15]),
        }
    }

    /// The glyph for `c`, if the face has one.
    pub fn glyph(&self, c: char) -> Option<Glyph> {
        let (mut lo, mut hi) = (0, self.count);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = u32_at(self.data, HEADER + mid * RECORD);
            match at.cmp(&(c as u32)) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(self.record(mid)),
            }
        }
        None
    }

    /// How far the pen moves after `c`, in sixteenths of a pixel; a
    /// character the face lacks is drawn as an empty box this wide.
    pub fn advance(&self, c: char) -> i32 {
        match c {
            '\u{200b}' | '\u{ad}' | '\u{feff}' | '\u{200c}' | '\u{200d}' => 0,
            // Icon fonts put their pictures here: drawn as nothing.
            '\u{e000}'..='\u{f8ff}' => self.size * 16,
            _ => self.glyph(c).map_or(self.size * 10, |g| g.advance),
        }
    }

    /// The coverage of the glyph's pixel (x, y) in its grid, 0 to 15.
    pub fn ink(&self, glyph: &Glyph, x: usize, y: usize) -> u8 {
        let n = glyph.at + y * glyph.w + x;
        let base = HEADER + self.count * RECORD;
        let byte = self.data[base + n / 2];
        if n % 2 == 0 { byte & 15 } else { byte >> 4 }
    }

    /// The width of `text`, in sixteenths of a pixel.
    pub fn width16(&self, text: &str) -> i32 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    /// The width of `text`, in pixels.
    pub fn width(&self, text: &str) -> i32 {
        (self.width16(text) + 8) / 16
    }

    /// Where a line's top goes for its capitals to sit centered on
    /// `middle` (in a button, a bar).
    pub fn top_for(&self, middle: i32) -> i32 {
        let cap = self.glyph('H').map_or(self.ascent * 7 / 10, |g| g.h as i32);
        middle - self.ascent + cap / 2
    }

    /// The longest start of `text` that fits in `width` pixels, ending with
    /// « … » when cut: the bytes kept, and whether the ellipsis is needed.
    pub fn fit(&self, text: &str, width: i32) -> (usize, bool) {
        if self.width(text) <= width {
            return (text.len(), false);
        }
        let room = width * 16 - self.advance('…');
        let mut pen = 0;
        for (i, c) in text.char_indices() {
            pen += self.advance(c);
            if pen > room {
                return (text[..i].trim_end().len(), true);
            }
        }
        (text.len(), true)
    }
}

/// The face nearest to `size` pixels: monospaced or not, bold or not.
pub fn face(mono: bool, bold: bool, size: i32) -> Face {
    let (sizes, files): (&[i32], &[&'static [u8]]) = match (mono, bold) {
        (false, false) => (&SIZES, &SANS),
        (false, true) => (&SIZES, &SANS_BOLD),
        (true, false) => (&MONO_SIZES, &MONO),
        (true, true) => (&MONO_SIZES, &MONO_BOLD),
    };
    let i = (0..sizes.len()).min_by_key(|&i| (sizes[i] - size).abs()).unwrap_or(0);
    Face::read(files[i], bold, mono)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_reads_and_has_french() {
        for mono in [false, true] {
            for bold in [false, true] {
                for size in [11, 16, 40] {
                    let f = face(mono, bold, size);
                    assert!(f.ascent > 0 && f.height > f.ascent);
                    for c in "Entrée Échap Âne œuvre « € » … ’ Œ ł".chars() {
                        assert_eq!(f.glyph(c).map(|g| g.c), Some(c), "{c} {mono} {bold} {size}");
                    }
                    assert!(f.glyph('\u{4e2d}').is_none());
                }
            }
        }
    }

    #[test]
    fn nearest_size_and_widths() {
        assert_eq!(face(false, false, 17).size, 16);
        assert_eq!(face(false, false, 100).size, 40);
        assert_eq!(face(true, false, 30).size, 16);
        let f = face(false, false, 16);
        let w = f.width("Bibliothèque des holons");
        assert!(w > 120 && w < 260, "{w}");
        let m = face(true, false, 13);
        assert_eq!(m.width16("iiii"), m.width16("MMMM"));
        let (kept, cut) = f.fit("Bibliothèque des holons", w / 2);
        assert!(cut && kept < 16);
    }

    #[test]
    fn ink_stays_in_range() {
        let f = face(false, true, 24);
        let g = f.glyph('W').unwrap();
        let mut any = false;
        for y in 0..g.h {
            for x in 0..g.w {
                let i = f.ink(&g, x, y);
                assert!(i <= 15);
                any |= i == 15;
            }
        }
        assert!(any);
    }
}
