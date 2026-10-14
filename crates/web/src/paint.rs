//! Drawing: a [`Canvas`] of pixels `0x00RRGGBB` (what `hln.present`
//! shows), shapes and text blended on it, and a laid out page drawn in a
//! viewport, scrolled.

use crate::dom::NodeId;
use crate::layout::{Frag, FragKind, Layout, Replaced, face_of};
use crate::style::{Color, LINE_THROUGH, OVERLINE, Position, Style, UNDERLINE};
use alloc::vec::Vec;
use hln_type::Face;

/// A rectangle: left, top, right, bottom (right and bottom excluded).
pub type Rect = (i32, i32, i32, i32);

pub struct Canvas {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
    clip: Rect,
}

fn mix(under: u32, over: u32, alpha: u32) -> u32 {
    if alpha >= 255 {
        return over & 0xff_ffff;
    }
    if alpha == 0 {
        return under;
    }
    let inv = 255 - alpha;
    let ch = |shift: u32| (((under >> shift & 255) * inv + (over >> shift & 255) * alpha + 127) / 255) << shift;
    ch(16) | ch(8) | ch(0)
}

/// The integer square root.
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> Canvas {
        let (width, height) = (width.max(1), height.max(1));
        Canvas { width, height, pixels: alloc::vec![0xffffff; (width * height) as usize], clip: (0, 0, width, height) }
    }

    pub fn clip(&self) -> Rect {
        self.clip
    }

    /// Restricts drawing to `rect` (within the canvas); returns the old clip.
    pub fn set_clip(&mut self, rect: Rect) -> Rect {
        let old = self.clip;
        self.clip = (rect.0.max(0), rect.1.max(0), rect.2.min(self.width), rect.3.min(self.height));
        old
    }

    pub fn restore_clip(&mut self, old: Rect) {
        self.clip = old;
    }

    /// Blends `color` at (x, y) with `cover` (0–255) more of transparency.
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, color: Color, cover: u32) {
        let (x0, y0, x1, y1) = self.clip;
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return;
        }
        let a = color.alpha() * cover / 255;
        if a == 0 {
            return;
        }
        let i = (y * self.width + x) as usize;
        self.pixels[i] = mix(self.pixels[i], color.0, a);
    }

    /// Fills a rectangle.
    pub fn fill(&mut self, (x0, y0, x1, y1): Rect, color: Color) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        let (x0, y0, x1, y1) = (x0.max(cx0), y0.max(cy0), x1.min(cx1), y1.min(cy1));
        if x0 >= x1 || y0 >= y1 || color.alpha() == 0 {
            return;
        }
        let a = color.alpha();
        for y in y0..y1 {
            let row = (y * self.width) as usize;
            let line = &mut self.pixels[row + x0 as usize..row + x1 as usize];
            if a == 255 {
                line.fill(color.0 & 0xff_ffff);
            } else {
                for p in line {
                    *p = mix(*p, color.0, a);
                }
            }
        }
    }

    /// How much of pixel (x, y) a rectangle with rounded corners covers,
    /// 0–255.
    fn rounded_cover(x: i32, y: i32, (x0, y0, x1, y1): Rect, r: i32) -> u32 {
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return 0;
        }
        let r = r.min((x1 - x0) / 2).min((y1 - y0) / 2);
        if r <= 0 {
            return 255;
        }
        // The corner's center, if the pixel is in a corner square.
        let cx = if x < x0 + r {
            x0 + r
        } else if x >= x1 - r {
            x1 - r
        } else {
            return 255;
        };
        let cy = if y < y0 + r {
            y0 + r
        } else if y >= y1 - r {
            y1 - r
        } else {
            return 255;
        };
        // Distance from the center to the pixel's center, in 16ths.
        let dx = i64::from((x - cx) * 16 + 8);
        let dy = i64::from((y - cy) * 16 + 8);
        let d = isqrt(dx * dx + dy * dy) as i32;
        let edge = r * 16;
        ((edge - d + 8).clamp(0, 16) * 255 / 16) as u32
    }

    /// Fills a rectangle with corners rounded by `r` pixels.
    pub fn fill_rounded(&mut self, rect: Rect, r: i32, color: Color) {
        if r <= 0 {
            self.fill(rect, color);
            return;
        }
        let (x0, y0, x1, y1) = rect;
        let r = r.min((x1 - x0) / 2).min((y1 - y0) / 2);
        // The middle band at once, the corner rows pixel by pixel.
        self.fill((x0, y0 + r, x1, y1 - r), color);
        for y in (y0..y0 + r).chain(y1 - r..y1) {
            if y < self.clip.1 || y >= self.clip.3 {
                continue;
            }
            for x in x0..x1 {
                let c = Self::rounded_cover(x, y, rect, r);
                if c > 0 {
                    self.blend(x, y, color, c);
                }
            }
        }
    }

    /// A ring: the rounded rectangle `outer` without the one `w` inside it.
    pub fn ring(&mut self, outer: Rect, r: i32, w: i32, color: Color) {
        let (x0, y0, x1, y1) = outer;
        if r <= 0 {
            self.fill((x0, y0, x1, y0 + w), color);
            self.fill((x0, y1 - w, x1, y1), color);
            self.fill((x0, y0 + w, x0 + w, y1 - w), color);
            self.fill((x1 - w, y0 + w, x1, y1 - w), color);
            return;
        }
        let inner = (x0 + w, y0 + w, x1 - w, y1 - w);
        let ri = (r - w).max(0);
        for y in y0..y1 {
            if y < self.clip.1 || y >= self.clip.3 {
                continue;
            }
            let corner_row = y < y0 + r || y >= y1 - r;
            if !corner_row {
                self.fill((x0, y, x0 + w, y + 1), color);
                self.fill((x1 - w, y, x1, y + 1), color);
                continue;
            }
            for x in x0..x1 {
                let o = Self::rounded_cover(x, y, outer, r);
                let i = Self::rounded_cover(x, y, inner, ri);
                let c = o.saturating_sub(i);
                if c > 0 {
                    self.blend(x, y, color, c);
                }
            }
        }
    }

    /// A filled disc of radius `r` (in 16ths of a pixel) at (`cx`, `cy`)
    /// (in 16ths).
    pub fn disc(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        let (px0, py0) = ((cx - r) / 16 - 1, (cy - r) / 16 - 1);
        let (px1, py1) = ((cx + r) / 16 + 2, (cy + r) / 16 + 2);
        for y in py0..py1 {
            for x in px0..px1 {
                let dx = i64::from(x * 16 + 8 - cx);
                let dy = i64::from(y * 16 + 8 - cy);
                let d = isqrt(dx * dx + dy * dy) as i32;
                let c = (r - d + 8).clamp(0, 16) as u32 * 255 / 16;
                if c > 0 {
                    self.blend(x, y, color, c);
                }
            }
        }
    }

    /// A line from (x0, y0) to (x1, y1), `w` pixels thick (squares along).
    pub fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), w: i32, color: Color) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for i in 0..=steps {
            let x = x0 + (x1 - x0) * i / steps;
            let y = y0 + (y1 - y0) * i / steps;
            self.fill((x - w / 2, y - w / 2, x - w / 2 + w, y - w / 2 + w), color);
        }
    }

    /// Writes `text` with its line's top at (x, y); slanted with `italic`.
    /// Returns the pen's end, in pixels.
    pub fn text(&mut self, x: i32, y: i32, text: &str, face: &Face, color: Color, italic: bool) -> i32 {
        let mut pen = x * 16;
        let baseline = y + face.ascent;
        let (cy0, cy1) = (self.clip.1, self.clip.3);
        if y > cy1 || y + face.height < cy0 {
            return x + face.width(text);
        }
        for c in text.chars() {
            let advance = face.advance(c);
            if pen / 16 > self.clip.2 {
                pen += advance;
                continue;
            }
            match face.glyph(c) {
                Some(g) => {
                    let gx = (pen + 8) / 16 + g.x;
                    let gy = y + g.y;
                    for row in 0..g.h {
                        let py = gy + row as i32;
                        if py < cy0 || py >= cy1 {
                            continue;
                        }
                        let slant = if italic { (baseline - py) * 13 / 64 } else { 0 };
                        for col in 0..g.w {
                            let ink = u32::from(face.ink(&g, col, row));
                            if ink > 0 {
                                self.blend(gx + col as i32 + slant, py, color, ink * 17);
                            }
                        }
                    }
                }
                None if c > ' ' && !('\u{e000}'..='\u{f8ff}').contains(&c) => {
                    // A character no face has: an empty box.
                    let w = advance / 16;
                    let top = y + face.ascent - face.size * 7 / 10;
                    let bx = (pen + 8) / 16 + 1;
                    let faint = color.with_alpha(color.alpha() * 2 / 3);
                    self.ring((bx, top, bx + (w - 2).max(3), y + face.ascent), 0, 1, faint);
                }
                None => {}
            }
            pen += advance;
        }
        (pen + 8) / 16
    }
}

// ---------------------------------------------------------------- page

/// What the browser wants shown on the page beyond the page itself.
#[derive(Clone, Default)]
pub struct Overlay {
    /// The form field being typed in, and where its caret is (in chars).
    pub focus: Option<(NodeId, usize)>,
    /// The element the keyboard is on (a link or a control): ringed.
    pub ring: Option<NodeId>,
    /// Text found by « Rechercher dans la page »: (text node, ...) drawn
    /// highlighted.
    pub found: Option<alloc::string::String>,
}

/// Draws a laid out page in `view` (a rectangle of the canvas), scrolled
/// down by `scroll` pixels.
pub fn paint(layout: &Layout, canvas: &mut Canvas, view: Rect, scroll: i32, overlay: &Overlay) {
    let old = canvas.set_clip(view);
    canvas.fill(view, layout.canvas);
    let dx = view.0;
    let dy = view.1 - scroll;
    let mut later: Vec<&Frag> = Vec::new();
    let page = Painter { dx, dy, overlay, view_top: scroll, view_bottom: scroll + (view.3 - view.1) };
    page.frag(&layout.root, canvas, 255, &mut later, true);
    // Positioned boxes on top, in order.
    let mut k = 0;
    while k < later.len() {
        let f = later[k];
        let mut more = Vec::new();
        page.frag(f, canvas, 255, &mut more, true);
        later.extend(more);
        k += 1;
    }
    canvas.restore_clip(old);
}

struct Painter<'a> {
    dx: i32,
    dy: i32,
    overlay: &'a Overlay,
    view_top: i32,
    view_bottom: i32,
}

fn faded(c: Color, opacity: u32) -> Color {
    if opacity >= 255 { c } else { c.with_alpha(c.alpha() * opacity / 255) }
}

impl Painter<'_> {
    fn rect(&self, f: &Frag) -> Rect {
        (f.x + self.dx, f.y + self.dy, f.x + f.w + self.dx, f.y + f.h + self.dy)
    }

    fn frag<'f>(&self, f: &'f Frag, canvas: &mut Canvas, opacity: u32, later: &mut Vec<&'f Frag>, top: bool) {
        if !top && f.positioned {
            later.push(f);
            return;
        }
        let s = &f.style;
        let opacity = opacity * s.opacity / 255;
        if opacity == 0 {
            return;
        }
        // Text far outside the view is skipped.
        if let FragKind::Text { .. } = f.kind
            && (f.y + f.h < self.view_top || f.y > self.view_bottom)
        {
            return;
        }
        let r = self.rect(f);
        if s.visible {
            match &f.kind {
                FragKind::Box | FragKind::Replaced(_) => self.boxed(f, s, r, canvas, opacity),
                _ => {}
            }
            match &f.kind {
                FragKind::Text { text, baseline, decoration } => {
                    self.text(f, s, text, *baseline, *decoration, canvas, opacity)
                }
                FragKind::Marker { text, baseline } => self.marker(f, s, text, *baseline, canvas, opacity),
                FragKind::Replaced(rep) => self.replaced(f, s, rep, r, canvas, opacity),
                FragKind::Box => {}
            }
        }
        if f.children.is_empty() {
            return;
        }
        let old = if s.overflow_hidden {
            let b = |i: usize| s.border(i);
            let inner = (r.0 + b(3), r.1 + b(0), r.2 - b(1), r.3 - b(2));
            let c = canvas.clip();
            Some(canvas.set_clip((inner.0.max(c.0), inner.1.max(c.1), inner.2.min(c.2), inner.3.min(c.3))))
        } else {
            None
        };
        for c in &f.children {
            self.frag(c, canvas, opacity, later, false);
        }
        if let Some(old) = old {
            canvas.restore_clip(old);
        }
        if self.overlay.ring.is_some() && f.node == self.overlay.ring && matches!(f.kind, FragKind::Box) {
            canvas.ring((r.0 - 2, r.1 - 1, r.2 + 2, r.3 + 1), 3, 2, Color(0xcc4d94ff));
        }
    }

    fn boxed(&self, f: &Frag, s: &Style, r: Rect, canvas: &mut Canvas, opacity: u32) {
        let radius = s.radius.min(f.w / 2).min(f.h / 2);
        let bg = faded(s.background, opacity);
        if bg.alpha() > 0 {
            canvas.fill_rounded(r, radius, bg);
        }
        let widths = [s.border(0), s.border(1), s.border(2), s.border(3)];
        if widths.iter().all(|&w| w == 0) {
            return;
        }
        let colors = [0, 1, 2, 3].map(|i| faded(s.border_color(i), opacity));
        let uniform = widths.iter().all(|&w| w == widths[0]) && colors.iter().all(|&c| c == colors[0]);
        if radius > 0 && uniform {
            canvas.ring(r, radius, widths[0], colors[0]);
            return;
        }
        let (x0, y0, x1, y1) = r;
        canvas.fill((x0, y0, x1, y0 + widths[0]), colors[0]);
        canvas.fill((x0, y1 - widths[2], x1, y1), colors[2]);
        canvas.fill((x0, y0 + widths[0], x0 + widths[3], y1 - widths[2]), colors[3]);
        canvas.fill((x1 - widths[1], y0 + widths[0], x1, y1 - widths[2]), colors[1]);
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        f: &Frag,
        s: &Style,
        text: &str,
        baseline: i32,
        deco: (u8, Color),
        canvas: &mut Canvas,
        opacity: u32,
    ) {
        let face = face_of(s);
        let color = faded(s.color, opacity);
        let (x, y) = (f.x + self.dx, f.y + self.dy);
        let end = canvas.text(x, y, text, &face, color, s.italic);
        if let Some(found) = &self.overlay.found
            && !found.is_empty()
        {
            self.highlight(text, found, x, y, &face, canvas);
        }
        let deco_bits = deco.0 | s.decoration;
        if deco_bits == 0 {
            return;
        }
        let deco_color = faded(if s.decoration != 0 { s.decoration_color.unwrap_or(s.color) } else { deco.1 }, opacity);
        let thick = (face.size / 14).max(1);
        let base = y + baseline;
        if deco_bits & UNDERLINE != 0 {
            canvas.fill((x, base + 2, end, base + 2 + thick), deco_color);
        }
        if deco_bits & LINE_THROUGH != 0 {
            let mid = base - face.size * 3 / 10;
            canvas.fill((x, mid, end, mid + thick), deco_color);
        }
        if deco_bits & OVERLINE != 0 {
            canvas.fill((x, y, end, y + thick), deco_color);
        }
    }

    /// Marks each place `found` appears in `text` (ignoring case).
    fn highlight(&self, text: &str, found: &str, x: i32, y: i32, face: &Face, canvas: &mut Canvas) {
        let lower = text.to_lowercase();
        let needle = found.to_lowercase();
        let mut from = 0;
        while let Some(i) = lower[from..].find(&needle) {
            let at = from + i;
            if !text.is_char_boundary(at) || !text.is_char_boundary((at + needle.len()).min(text.len())) {
                break;
            }
            let x0 = x + face.width(&text[..at]);
            let x1 = x + face.width(&text[..(at + needle.len()).min(text.len())]);
            canvas.fill((x0, y, x1, y + face.height), Color(0x66ffcc00));
            from = at + needle.len().max(1);
        }
    }

    fn marker(&self, f: &Frag, s: &Style, text: &str, baseline: i32, canvas: &mut Canvas, opacity: u32) {
        let color = faded(s.color, opacity);
        let face = face_of(s);
        let x = f.x + self.dx;
        let base = f.y + self.dy + baseline;
        let size = (face.size * 16 / 3).max(5 * 16);
        let cy = base * 16 - face.size * 16 * 3 / 10;
        let cx = (x + f.w) * 16 - size / 2;
        match text {
            "•" => canvas.disc(cx, cy, size / 2, color),
            "◦" => {
                canvas.disc(cx, cy, size / 2, color);
                canvas.disc(cx, cy, size / 2 - 20, faded(Color::WHITE, opacity));
            }
            "▪" => {
                let h = size / 32;
                canvas.fill((cx / 16 - h, cy / 16 - h, cx / 16 + h + 1, cy / 16 + h + 1), color);
            }
            _ => {
                canvas.text(x, base - face.ascent, text, &face, color, false);
            }
        }
    }

    fn replaced(&self, f: &Frag, s: &Style, rep: &Replaced, r: Rect, canvas: &mut Canvas, opacity: u32) {
        let face = face_of(s);
        let color = faded(s.color, opacity);
        let b = |i: usize| s.border(i);
        let pad = |i: usize| s.padding[i].or_zero(0);
        let inner = (r.0 + b(3) + pad(3), r.1 + b(0) + pad(0), r.2 - b(1) - pad(1), r.3 - b(2) - pad(2));
        let middle_top = |canvas: &Canvas| {
            let _ = canvas;
            inner.1 + (inner.3 - inner.1 - face.height) / 2
        };
        let old = canvas.set_clip((
            inner.0.max(canvas.clip().0),
            r.1.max(canvas.clip().1),
            inner.2.min(canvas.clip().2),
            r.3.min(canvas.clip().3),
        ));
        match rep {
            Replaced::Image { alt, .. } => {
                if r.2 - r.0 >= 2 && r.3 - r.1 >= 2 {
                    canvas.restore_clip(old);
                    let old2 = canvas.set_clip((
                        r.0.max(canvas.clip().0),
                        r.1.max(canvas.clip().1),
                        r.2.min(canvas.clip().2),
                        r.3.min(canvas.clip().3),
                    ));
                    canvas.fill(r, faded(Color(0xffeef1f5), opacity));
                    canvas.ring(r, 0, 1, faded(Color(0xffc9ced6), opacity));
                    let grey = faded(Color(0xff8a93a3), opacity);
                    if r.2 - r.0 >= 24 && r.3 - r.1 >= 20 {
                        // A small picture: a sun and a mountain.
                        let (ix, iy) = (r.0 + 5, r.1 + 4);
                        canvas.disc((ix + 4) * 16, (iy + 4) * 16, 2 * 16 + 8, grey);
                        canvas.line((ix, iy + 13), (ix + 6, iy + 7), 2, grey);
                        canvas.line((ix + 6, iy + 7), (ix + 13, iy + 13), 2, grey);
                    }
                    if !alt.is_empty() {
                        let alt_face = hln_type::face(false, false, 13);
                        let tx = if r.2 - r.0 >= 24 && r.3 - r.1 >= 20 { r.0 + 22 } else { r.0 + 4 };
                        let ty = r.1 + ((r.3 - r.1 - alt_face.height) / 2).max(0);
                        canvas.text(tx, ty, alt, &alt_face, faded(Color(0xff5c6370), opacity), false);
                    }
                    canvas.restore_clip(old2);
                    return;
                }
            }
            Replaced::Field { text, placeholder, password } => {
                let shown: alloc::string::String =
                    if *password { text.chars().map(|_| '•').collect() } else { text.clone() };
                let ink = if *placeholder { faded(Color(0xff8a8f98), opacity) } else { color };
                let ty = middle_top(canvas);
                // Long text shows its end while typing.
                let width = face.width(&shown);
                let room = inner.2 - inner.0;
                let focused = self.overlay.focus.is_some_and(|(n, _)| Some(n) == f.node);
                let scroll = if focused && width > room { width - room + 2 } else { 0 };
                canvas.text(inner.0 - scroll, ty, &shown, &face, ink, false);
                if let Some((n, caret)) = self.overlay.focus
                    && Some(n) == f.node
                {
                    let before: alloc::string::String =
                        if *placeholder { alloc::string::String::new() } else { shown.chars().take(caret).collect() };
                    let cx = inner.0 - scroll + face.width(&before);
                    canvas.fill((cx, ty + 2, cx + 1, ty + face.height - 2), color);
                }
            }
            Replaced::Button(text) => {
                let ty = middle_top(canvas);
                let tw = face.width(text);
                canvas.text(inner.0 + (inner.2 - inner.0 - tw) / 2, ty, text, &face, color, false);
            }
            Replaced::Select(text) => {
                let ty = middle_top(canvas);
                canvas.text(inner.0, ty, text, &face, color, false);
                canvas.restore_clip(old);
                let ax = r.2 - b(1) - 12;
                let ay = r.1 + (r.3 - r.1) / 2 - 1;
                canvas.line((ax - 3, ay - 2), (ax, ay + 1), 1, color);
                canvas.line((ax, ay + 1), (ax + 3, ay - 2), 1, color);
                return;
            }
            Replaced::Check { checked, radio } => {
                if *checked {
                    let (x0, y0, x1, y1) = r;
                    let accent = faded(Color(0xff1460f0), opacity);
                    if *radio {
                        canvas.disc((x0 + x1) * 8, (y0 + y1) * 8, (x1 - x0) * 16 / 4, accent);
                    } else {
                        canvas.restore_clip(old);
                        canvas.fill_rounded(r, s.radius, accent);
                        let w = faded(Color::WHITE, opacity);
                        let (w3, h2) = ((x1 - x0) / 4, (y1 - y0) / 2);
                        canvas.line((x0 + w3 - 1, y0 + h2), (x0 + w3 + 1, y1 - 4), 2, w);
                        canvas.line((x0 + w3 + 1, y1 - 4), (x1 - 3, y0 + 3), 2, w);
                        return;
                    }
                }
            }
            Replaced::Area(text) => {
                let mut y = inner.1;
                for line in text.lines() {
                    canvas.text(inner.0, y, line, &face, color, false);
                    y += face.height;
                    if y > inner.3 {
                        break;
                    }
                }
                if let Some((n, _)) = self.overlay.focus
                    && Some(n) == f.node
                {
                    let last = text.lines().last().unwrap_or("");
                    let lines = text.lines().count().max(1) as i32 - i32::from(text.ends_with('\n') || text.is_empty());
                    let cy = inner.1 + lines.max(0) * face.height;
                    let cx = inner.0 + if text.ends_with('\n') { 0 } else { face.width(last) };
                    canvas.fill((cx, cy + 2, cx + 1, cy + face.height - 2), color);
                }
            }
            Replaced::Embedded(what) => {
                if what != "svg" && r.2 - r.0 > 40 && r.3 - r.1 > 20 {
                    canvas.fill(r, faded(Color(0xfff1f3f6), opacity));
                    let label = match what.as_str() {
                        "video" => "Vidéo non prise en charge",
                        "audio" => "Son non pris en charge",
                        "iframe" => "Cadre intégré non pris en charge",
                        _ => "Contenu non pris en charge",
                    };
                    let small = hln_type::face(false, false, 13);
                    let tw = small.width(label);
                    canvas.text(
                        r.0 + (r.2 - r.0 - tw) / 2,
                        r.1 + (r.3 - r.1 - small.height) / 2,
                        label,
                        &small,
                        faded(Color(0xff6b7280), opacity),
                        false,
                    );
                }
            }
        }
        canvas.restore_clip(old);
    }
}

/// The fragments under the point (x, y) of the page, innermost last.
pub fn hit(layout: &Layout, x: i32, y: i32) -> Vec<&Frag> {
    fn walk<'a>(f: &'a Frag, x: i32, y: i32, out: &mut Vec<&'a Frag>, positioned: &mut Vec<&'a Frag>) {
        let inside = x >= f.x && x < f.x + f.w && y >= f.y && y < f.y + f.h;
        if inside {
            out.push(f);
        } else if f.style.overflow_hidden && !matches!(f.kind, FragKind::Text { .. }) {
            return;
        }
        for c in &f.children {
            if c.positioned {
                positioned.push(c);
            } else {
                walk(c, x, y, out, positioned);
            }
        }
    }
    let mut out = Vec::new();
    let mut positioned = Vec::new();
    walk(&layout.root, x, y, &mut out, &mut positioned);
    // Positioned boxes are on top: what they hold comes last.
    let mut k = 0;
    while k < positioned.len() {
        let p = positioned[k];
        let mut more = Vec::new();
        let mut found = Vec::new();
        walk(p, x, y, &mut found, &mut more);
        if p.style.position != Position::Static || !found.is_empty() {
            out.extend(found);
        }
        positioned.extend(more);
        k += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_blend_and_clip() {
        let mut c = Canvas::new(10, 10);
        c.fill((0, 0, 10, 10), Color::BLACK);
        c.fill((2, 2, 4, 4), Color(0x80ffffff));
        assert_eq!(c.pixels[2 * 10 + 2], 0x808080);
        let old = c.set_clip((5, 5, 10, 10));
        c.fill((0, 0, 10, 10), Color::rgb(255, 0, 0));
        c.restore_clip(old);
        assert_eq!(c.pixels[0], 0);
        assert_eq!(c.pixels[99], 0xff0000);
    }

    #[test]
    fn rounded_corners_and_text() {
        let mut c = Canvas::new(40, 40);
        c.fill_rounded((0, 0, 40, 40), 10, Color::BLACK);
        assert_eq!(c.pixels[0], 0xffffff, "the corner stays");
        assert_eq!(c.pixels[20 * 40 + 20], 0);
        let mut t = Canvas::new(100, 30);
        let face = hln_type::face(false, true, 16);
        let end = t.text(2, 2, "Âne", &face, Color::BLACK, false);
        assert!(end > 20);
        assert!(t.pixels.contains(&0));
        assert_eq!(isqrt(1_000_000), 1000);
    }
}
