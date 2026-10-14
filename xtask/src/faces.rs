//! `cargo xtask faces <Manrope-400.ttf> <Manrope-700.ttf>
//! <JetBrainsMono-400.ttf> <JetBrainsMono-700.ttf>`: renders the browser's
//! faces, smoothed, at every size pages ask for, into `crates/type/src/faces/`.
//!
//! Each face is one file, read by `hln-type` as it is (see its `Face` for
//! the format): a header, one record per glyph in character order, then
//! the glyphs' coverage, four bits a pixel. The fonts are the static files
//! of Google Fonts (one weight each).

use crate::{Result, root};
use ab_glyph_rasterizer::{Point, Rasterizer, point};
use std::fs;
use ttf_parser::{Face, OutlineBuilder};

/// The sizes rendered, in pixels: a page asking for another gets the
/// nearest one.
pub const SIZES: [u16; 12] = [11, 12, 13, 14, 15, 16, 18, 20, 24, 28, 32, 40];
/// The monospaced faces are for code, which is seldom large.
pub const MONO_SIZES: [u16; 4] = [12, 13, 14, 16];

/// Coverage above this is drawn a little heavier, as in the system's faces.
const GAMMA: f32 = 0.85;

/// The characters rendered, beyond printable ASCII, Latin-1 and Latin
/// Extended-A: the punctuation, arrows and signs pages use most.
const EXTRA: &str = "‐‑‒–—―‘’‚‛“”„‟†‡•…‰′″‹›⁄€™←↑→↓↔↗↘✓✔✗✘×−∞≈≠≤≥·★☆▲▶▼◀►◄■□●○◆◇♥♦♣♠ˆ˜ʼƒ№℃™";

pub fn generate(args: &[String]) -> Result {
    let [sans, sans_bold, mono, mono_bold] = args else {
        return Err(
            "usage: cargo xtask faces <Manrope-400.ttf> <Manrope-700.ttf> <JetBrainsMono-400.ttf> <JetBrainsMono-700.ttf>"
                .into(),
        );
    };
    let mut wanted: Vec<char> = (' '..='~').chain('\u{a0}'..='\u{17f}').chain(EXTRA.chars()).collect::<Vec<_>>();
    wanted.sort_unstable();
    wanted.dedup();
    let dir = root().join("crates/type/src/faces");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut total = 0;
    for (path, prefix, sizes) in [
        (sans, "sans", &SIZES[..]),
        (sans_bold, "sans-bold", &SIZES[..]),
        (mono, "mono", &MONO_SIZES[..]),
        (mono_bold, "mono-bold", &MONO_SIZES[..]),
    ] {
        let data = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let face = Face::parse(&data, 0).map_err(|e| format!("{path}: {e}"))?;
        for &size in sizes {
            let bytes = render(&face, size, &wanted);
            let file = format!("{prefix}-{size}.bin");
            fs::write(dir.join(&file), &bytes).map_err(|e| format!("{file}: {e}"))?;
            total += bytes.len();
        }
        println!("xtask: {prefix}: {} sizes", sizes.len());
    }
    println!("xtask: faces written to {} ({total} bytes)", dir.display());
    Ok(())
}

/// One face: its file's bytes.
fn render(face: &Face, size: u16, wanted: &[char]) -> Vec<u8> {
    let scale = f32::from(size) / f32::from(face.units_per_em());
    let ascent = (f32::from(face.ascender()) * scale).round() as i16;
    let height = (f32::from(face.ascender() - face.descender() + face.line_gap()) * scale).round() as u16;
    let mut records = Vec::new();
    let mut nibbles: Vec<u8> = Vec::new();
    let mut count = 0u16;
    for &c in wanted {
        let Some(id) = face.glyph_index(c) else { continue };
        let advance = f32::from(face.glyph_hor_advance(id).unwrap_or(0)) * scale;
        let mut path = Path::new(scale);
        face.outline_glyph(id, &mut path);
        let (x, y, w, h, coverage) = path.render();
        let at = nibbles.len() as u32;
        nibbles.extend(coverage.iter().map(|&a| ((f32::from(a) / 255.0).powf(GAMMA) * 15.0).round() as u8));
        records.extend_from_slice(&(c as u32).to_le_bytes());
        records.extend_from_slice(&at.to_le_bytes());
        records.extend_from_slice(&((advance * 16.0).round() as u16).to_le_bytes());
        records.extend_from_slice(&(x as i16).to_le_bytes());
        records.extend_from_slice(&((y as i16) + ascent).to_le_bytes());
        records.push(w as u8);
        records.push(h as u8);
        count += 1;
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"HLNF");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&(ascent as u16).to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&records);
    // Two pixels a byte, the first in the low nibble.
    out.extend(nibbles.chunks(2).map(|p| p[0] | p.get(1).copied().unwrap_or(0) << 4));
    out
}

enum Segment {
    Line(Point, Point),
    Quad(Point, Point, Point),
    Cubic(Point, Point, Point, Point),
}

/// A glyph's outline in pixels, y downward from the baseline.
struct Path {
    scale: f32,
    segments: Vec<Segment>,
    last: Point,
    first: Point,
}

impl Path {
    fn new(scale: f32) -> Self {
        Self { scale, segments: Vec::new(), last: point(0.0, 0.0), first: point(0.0, 0.0) }
    }

    fn at(&self, x: f32, y: f32) -> Point {
        point(x * self.scale, -y * self.scale)
    }

    /// Rasterizes the outline: its left edge and top from the pen on the
    /// baseline, its size, and its coverage row by row.
    fn render(&self) -> (i64, i64, usize, usize, Vec<u8>) {
        if self.segments.is_empty() {
            return (0, 0, 0, 0, Vec::new());
        }
        let points = self.segments.iter().flat_map(|s| match s {
            Segment::Line(a, b) => vec![*a, *b],
            Segment::Quad(a, b, c) => vec![*a, *b, *c],
            Segment::Cubic(a, b, c, d) => vec![*a, *b, *c, *d],
        });
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in points {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        let (left, top) = (x0.floor(), y0.floor());
        let (w, h) = ((x1.ceil() - left).max(1.0) as usize, (y1.ceil() - top).max(1.0) as usize);
        let shift = |p: &Point| point(p.x - left, p.y - top);
        let mut raster = Rasterizer::new(w, h);
        for s in &self.segments {
            match s {
                Segment::Line(a, b) => raster.draw_line(shift(a), shift(b)),
                Segment::Quad(a, b, c) => raster.draw_quad(shift(a), shift(b), shift(c)),
                Segment::Cubic(a, b, c, d) => raster.draw_cubic(shift(a), shift(b), shift(c), shift(d)),
            }
        }
        let mut coverage = vec![0u8; w * h];
        raster.for_each_pixel(|i, alpha| coverage[i] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
        (left as i64, top as i64, w, h, coverage)
    }
}

impl OutlineBuilder for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.last = self.at(x, y);
        self.first = self.last;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.segments.push(Segment::Line(self.last, p));
        self.last = p;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.at(x1, y1), self.at(x, y));
        self.segments.push(Segment::Quad(self.last, c, p));
        self.last = p;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.segments.push(Segment::Cubic(self.last, c1, c2, p));
        self.last = p;
    }

    fn close(&mut self) {
        if self.last != self.first {
            self.segments.push(Segment::Line(self.last, self.first));
        }
        self.last = self.first;
    }
}
