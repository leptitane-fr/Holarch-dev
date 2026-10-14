//! Layout: the document and its styles become boxes ([`build`]), the boxes
//! get their places ([`layout`]): blocks and their margins, lines of text
//! that wrap around floats, lists and their markers, tables, flexible
//! boxes, positioned boxes. The result is a tree of [`Frag`]s in page
//! coordinates (pixels from the page's top left corner), which [`paint`]
//! draws and the browser hit-tests.
//!
//! [`paint`]: crate::paint

use crate::dom::{Document, NodeData, NodeId};
use crate::style::{
    Align, Clear, Color, Display, Float, Length, ListStyle, Position, Style, Styles, TextAlign, Transform, VAlign,
    WhiteSpace,
};
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;
use hln_type::Face;

// ---------------------------------------------------------------- boxes

/// What a replaced element shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Replaced {
    /// An image: its address and its text alternative.
    Image {
        src: String,
        alt: String,
    },
    /// A text field: its text, or its placeholder (shown grey) when empty.
    Field {
        text: String,
        placeholder: bool,
        password: bool,
    },
    Check {
        checked: bool,
        radio: bool,
    },
    /// A button drawn by the browser (`<input type=submit>`).
    Button(String),
    Select(String),
    /// Text over several lines (`<textarea>`).
    Area(String),
    /// What this browser does not show (`<iframe>`, `<video>`...): its name.
    Embedded(String),
}

#[derive(Clone, Debug)]
pub enum Kind {
    /// A block container: blocks, or lines of inline content.
    Block,
    /// An inline element (`<a>`, `<span>`).
    Inline,
    /// Text, as written in the document (white space not yet collapsed).
    Text(String),
    /// An inline-level block (`inline-block`, `<button>`).
    InlineBlock,
    Replaced(Replaced),
    Break,
    Table,
    RowGroup,
    Row,
    Cell,
    Caption,
    Flex,
}

pub struct LayoutBox {
    /// The node it comes from (none for anonymous boxes).
    pub node: Option<NodeId>,
    pub style: Rc<Style>,
    pub kind: Kind,
    pub children: Vec<LayoutBox>,
    /// The list marker's text, for a list item.
    pub marker: Option<String>,
    /// Decorations from ancestors (underline...) and their color.
    pub decoration: (u8, Color),
    /// Its intrinsic widths, once measured: (min-content, max-content).
    intrinsic: Cell<Option<(i32, i32)>>,
    /// `colspan` and `rowspan` of a cell.
    pub span: (usize, usize),
}

impl LayoutBox {
    fn new(node: Option<NodeId>, style: Rc<Style>, kind: Kind, decoration: (u8, Color)) -> Self {
        LayoutBox {
            node,
            style,
            kind,
            children: Vec::new(),
            marker: None,
            decoration,
            intrinsic: Cell::new(None),
            span: (1, 1),
        }
    }

    fn is_inline_level(&self) -> bool {
        matches!(self.kind, Kind::Inline | Kind::Text(_) | Kind::InlineBlock | Kind::Replaced(_) | Kind::Break)
            && !self.out_of_flow()
    }

    fn out_of_flow(&self) -> bool {
        // Text shares its parent's style, floats and positions included.
        !matches!(self.kind, Kind::Text(_) | Kind::Break)
            && (self.style.float != Float::None || matches!(self.style.position, Position::Absolute | Position::Fixed))
    }

    fn is_whitespace(&self) -> bool {
        match &self.kind {
            Kind::Text(t) => !self.style.white_space.keeps_spaces() && t.chars().all(|c| c.is_ascii_whitespace()),
            _ => false,
        }
    }

    /// Whether its lines avoid floats from outside (a new block formatting
    /// context: `overflow: hidden`, tables, flex containers, `inline-block`).
    fn new_context(&self) -> bool {
        self.style.overflow_hidden
            || matches!(self.kind, Kind::Table | Kind::Flex | Kind::InlineBlock | Kind::Cell)
            || self.out_of_flow()
    }
}

/// The marker of the `n`-th item (1-based) of a list.
pub fn marker_text(kind: ListStyle, n: i32) -> Option<String> {
    fn alpha(mut n: i32, upper: bool) -> String {
        let mut out = Vec::new();
        while n > 0 {
            n -= 1;
            out.push(((if upper { b'A' } else { b'a' }) + (n % 26) as u8) as char);
            n /= 26;
        }
        out.iter().rev().collect()
    }
    fn roman(mut n: i32, upper: bool) -> String {
        const R: [(i32, &str); 13] = [
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ];
        let mut out = String::new();
        for (v, s) in R {
            while n >= v {
                out.push_str(s);
                n -= v;
            }
        }
        if upper { out.to_ascii_uppercase() } else { out }
    }
    Some(match kind {
        ListStyle::None => return None,
        ListStyle::Disc => String::from("•"),
        ListStyle::Circle => String::from("◦"),
        ListStyle::Square => String::from("▪"),
        ListStyle::Decimal => alloc::format!("{n}."),
        ListStyle::DecimalZero => alloc::format!("{n:02}."),
        ListStyle::LowerAlpha if n > 0 => alloc::format!("{}.", alpha(n, false)),
        ListStyle::UpperAlpha if n > 0 => alloc::format!("{}.", alpha(n, true)),
        ListStyle::LowerRoman if (1..4000).contains(&n) => alloc::format!("{}.", roman(n, false)),
        ListStyle::UpperRoman if (1..4000).contains(&n) => alloc::format!("{}.", roman(n, true)),
        _ => alloc::format!("{n}."),
    })
}

/// What a form control or other replaced element shows; `None` for an
/// ordinary element.
fn replaced(doc: &Document, node: NodeId) -> Option<Replaced> {
    let e = doc.element(node)?;
    Some(match e.name.as_str() {
        "img" => Replaced::Image {
            src: String::from(e.attr("src").unwrap_or("")),
            alt: String::from(e.attr("alt").unwrap_or("")),
        },
        "input" => {
            let kind = e.attr("type").unwrap_or("text").to_ascii_lowercase();
            match kind.as_str() {
                "checkbox" => Replaced::Check { checked: e.checked, radio: false },
                "radio" => Replaced::Check { checked: e.checked, radio: true },
                "submit" | "button" | "reset" | "image" => {
                    Replaced::Button(String::from(e.attr("value").unwrap_or(match kind.as_str() {
                        "reset" => "Réinitialiser",
                        "submit" | "image" => "Envoyer",
                        _ => "",
                    })))
                }
                "range" | "color" | "file" => Replaced::Embedded(kind),
                _ => {
                    let value = e.current_value();
                    if value.is_empty() {
                        Replaced::Field {
                            text: String::from(e.attr("placeholder").unwrap_or("")),
                            placeholder: true,
                            password: false,
                        }
                    } else {
                        Replaced::Field { text: String::from(value), placeholder: false, password: kind == "password" }
                    }
                }
            }
        }
        "textarea" => Replaced::Area(match &e.value {
            Some(v) => v.clone(),
            None => doc.text_content(node),
        }),
        "select" => {
            let options: Vec<NodeId> = doc.descendants(node).into_iter().filter(|&n| doc.is(n, "option")).collect();
            let chosen = options
                .iter()
                .copied()
                .find(|&o| doc.element(o).is_some_and(|e| e.attr("selected").is_some()))
                .or(options.first().copied());
            Replaced::Select(chosen.map(|o| crate::dom::collapse(&doc.text_content(o))).unwrap_or_default())
        }
        "iframe" | "video" | "audio" | "canvas" | "embed" | "object" => Replaced::Embedded(e.name.clone()),
        "svg" => Replaced::Embedded(String::from("svg")),
        _ => return None,
    })
}

struct Builder<'a> {
    doc: &'a Document,
    styles: &'a Styles,
}

impl Builder<'_> {
    /// The boxes for `node` and its subtree, added to `out`.
    fn node(&self, node: NodeId, out: &mut Vec<LayoutBox>, deco: (u8, Color)) {
        let Some(style) = self.styles.get(node) else { return };
        match &self.doc.nodes[node].data {
            NodeData::Text(t) => {
                out.push(LayoutBox::new(Some(node), style.clone(), Kind::Text(t.clone()), deco));
            }
            NodeData::Element(e) => {
                if style.display == Display::None {
                    return;
                }
                let deco = if style.decoration != 0 {
                    (deco.0 | style.decoration, style.decoration_color.unwrap_or(style.color))
                } else {
                    deco
                };
                if style.display == Display::Contents {
                    self.children(node, out, deco);
                    return;
                }
                if e.name == "br" {
                    out.push(LayoutBox::new(Some(node), style.clone(), Kind::Break, deco));
                    return;
                }
                if let Some(r) = replaced(self.doc, node) {
                    out.push(LayoutBox::new(Some(node), style.clone(), Kind::Replaced(r), deco));
                    return;
                }
                let kind = match style.display {
                    Display::Inline if e.name == "button" => Kind::InlineBlock,
                    Display::Inline => Kind::Inline,
                    Display::InlineBlock => Kind::InlineBlock,
                    Display::Table => Kind::Table,
                    Display::TableRowGroup => Kind::RowGroup,
                    Display::TableRow => Kind::Row,
                    Display::TableCell => Kind::Cell,
                    Display::TableCaption => Kind::Caption,
                    Display::TableColumn => return,
                    Display::Flex => Kind::Flex,
                    Display::InlineFlex | Display::InlineTable => {
                        // An inline-level box holding the flex box or table.
                        let mut outer = LayoutBox::new(Some(node), style.clone(), Kind::InlineBlock, deco);
                        let inner_style = Rc::new(Style {
                            display: if style.display == Display::InlineFlex { Display::Flex } else { Display::Table },
                            margin: [Length::ZERO; 4],
                            padding: [Length::ZERO; 4],
                            border_style_set: [false; 4],
                            background: Color::TRANSPARENT,
                            width: Length::AUTO,
                            height: Length::AUTO,
                            ..(**style).clone()
                        });
                        let kind = if style.display == Display::InlineFlex { Kind::Flex } else { Kind::Table };
                        let mut inner = LayoutBox::new(None, inner_style, kind, deco);
                        self.children(node, &mut inner.children, deco);
                        outer.children.push(inner);
                        let mut outer_vec = alloc::vec![outer];
                        fix(&mut outer_vec[0]);
                        out.append(&mut outer_vec);
                        return;
                    }
                    _ => Kind::Block,
                };
                let mut b = LayoutBox::new(Some(node), style.clone(), kind, deco);
                if matches!(b.kind, Kind::Cell) {
                    let num = |a: &str| e.attr(a).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1);
                    b.span = (num("colspan").clamp(1, 1000), num("rowspan").clamp(1, 65534));
                }
                if style.display == Display::ListItem {
                    b.marker = self.marker(node, style);
                }
                if let Some(before) = self.styles.before.get(&node) {
                    b.children.push(self.pseudo(node, before, deco));
                }
                self.children(node, &mut b.children, deco);
                if let Some(after) = self.styles.after.get(&node) {
                    b.children.push(self.pseudo(node, after, deco));
                }
                fix(&mut b);
                out.push(b);
            }
            _ => {}
        }
    }

    fn pseudo(&self, node: NodeId, style: &Rc<Style>, deco: (u8, Color)) -> LayoutBox {
        let text = style.content.clone().unwrap_or_default();
        let kind = match style.display {
            Display::Block | Display::ListItem => Kind::Block,
            Display::InlineBlock => Kind::InlineBlock,
            _ => Kind::Inline,
        };
        let mut b = LayoutBox::new(Some(node), style.clone(), kind, deco);
        let text_style = Rc::new(style.inherit());
        b.children.push(LayoutBox::new(Some(node), text_style, Kind::Text(text), deco));
        b
    }

    fn children(&self, node: NodeId, out: &mut Vec<LayoutBox>, deco: (u8, Color)) {
        for &child in &self.doc.nodes[node].children {
            self.node(child, out, deco);
        }
    }

    /// The marker of a list item: its number among its list's items.
    fn marker(&self, node: NodeId, style: &Style) -> Option<String> {
        if style.list_style == ListStyle::None {
            return None;
        }
        let doc = self.doc;
        let parent = doc.parent(node)?;
        let list = doc.element(parent);
        let reversed = list.is_some_and(|l| l.attr("reversed").is_some());
        let items: Vec<NodeId> = doc
            .element_children(parent)
            .filter(|&c| self.styles.get(c).is_some_and(|s| s.display == Display::ListItem))
            .collect();
        let start = list.and_then(|l| l.attr("start")).and_then(|s| s.trim().parse::<i32>().ok());
        let mut n = if reversed { start.unwrap_or(items.len() as i32) } else { start.unwrap_or(1) };
        for &item in &items {
            if let Some(v) = doc.element(item).and_then(|e| e.attr("value")).and_then(|v| v.trim().parse().ok()) {
                n = v;
            }
            if item == node {
                break;
            }
            n += if reversed { -1 } else { 1 };
        }
        marker_text(style.list_style, n)
    }
}

/// An anonymous block holding `children`, styled after `parent`.
fn anonymous(parent: &LayoutBox, kind: Kind, children: Vec<LayoutBox>) -> LayoutBox {
    let mut style = parent.style.inherit();
    style.display = Display::Block;
    let mut b = LayoutBox::new(None, Rc::new(style), kind, parent.decoration);
    b.children = children;
    b
}

/// Puts a box's children in the shapes layout expects: inline runs among
/// blocks wrapped in anonymous blocks, inline boxes holding blocks made
/// blocks, table parts in their rows and groups, flex items blockified.
fn fix(b: &mut LayoutBox) {
    match b.kind {
        Kind::Inline => {
            // An inline element holding a block becomes a block.
            if b.children.iter().any(|c| !c.is_inline_level() && !c.out_of_flow()) {
                b.kind = Kind::Block;
                fix(b);
            }
        }
        Kind::Block | Kind::InlineBlock | Kind::Cell | Kind::Caption => {
            let mixed = b.children.iter().any(|c| !c.is_inline_level())
                && b.children.iter().any(|c| c.is_inline_level() && !c.is_whitespace());
            if mixed {
                let children = core::mem::take(&mut b.children);
                let mut run: Vec<LayoutBox> = Vec::new();
                for c in children {
                    if c.is_inline_level() {
                        run.push(c);
                    } else {
                        flush(b, &mut run);
                        b.children.push(c);
                    }
                }
                flush(b, &mut run);
            } else if b.children.iter().all(|c| !c.is_inline_level() || c.is_whitespace())
                && b.children.iter().any(|c| !c.is_inline_level())
            {
                b.children.retain(|c| !c.is_whitespace());
            }
        }
        Kind::Table => {
            let children = core::mem::take(&mut b.children);
            let mut loose: Vec<LayoutBox> = Vec::new();
            for c in children {
                match c.kind {
                    Kind::RowGroup | Kind::Caption => {
                        flush_rows(b, &mut loose);
                        b.children.push(c);
                    }
                    _ if c.is_whitespace() => {}
                    _ => loose.push(c),
                }
            }
            flush_rows(b, &mut loose);
        }
        Kind::RowGroup => {
            let children = core::mem::take(&mut b.children);
            let mut loose: Vec<LayoutBox> = Vec::new();
            for c in children {
                match c.kind {
                    Kind::Row => {
                        flush_cells_as_row(b, &mut loose);
                        b.children.push(c);
                    }
                    _ if c.is_whitespace() => {}
                    _ => loose.push(c),
                }
            }
            flush_cells_as_row(b, &mut loose);
        }
        Kind::Row => {
            let children = core::mem::take(&mut b.children);
            for c in children {
                match c.kind {
                    Kind::Cell => b.children.push(c),
                    _ if c.is_whitespace() => {}
                    _ => {
                        let cell = anonymous(b, Kind::Cell, alloc::vec![c]);
                        let mut cell = cell;
                        fix(&mut cell);
                        b.children.push(cell);
                    }
                }
            }
        }
        Kind::Flex => {
            let children = core::mem::take(&mut b.children);
            let mut run: Vec<LayoutBox> = Vec::new();
            for mut c in children {
                // Text runs become anonymous items; each element is one.
                if matches!(c.kind, Kind::Text(_) | Kind::Break) {
                    if !c.is_whitespace() {
                        run.push(c);
                    }
                    continue;
                }
                flush(b, &mut run);
                if matches!(c.kind, Kind::InlineBlock | Kind::Inline) {
                    c.kind = Kind::Block;
                    fix(&mut c);
                }
                b.children.push(c);
            }
            flush(b, &mut run);
        }
        _ => {}
    }
}

fn flush(parent: &mut LayoutBox, run: &mut Vec<LayoutBox>) {
    if run.is_empty() {
        return;
    }
    if run.iter().all(|c| c.is_whitespace()) {
        run.clear();
        return;
    }
    let children = core::mem::take(run);
    let b = anonymous(parent, Kind::Block, children);
    parent.children.push(b);
}

/// Loose content in a table: rows into a group, cells into a row.
fn flush_rows(table: &mut LayoutBox, loose: &mut Vec<LayoutBox>) {
    if loose.is_empty() {
        return;
    }
    let mut group = anonymous(table, Kind::RowGroup, core::mem::take(loose));
    fix(&mut group);
    table.children.push(group);
}

fn flush_cells_as_row(group: &mut LayoutBox, loose: &mut Vec<LayoutBox>) {
    if loose.is_empty() {
        return;
    }
    let mut row = anonymous(group, Kind::Row, core::mem::take(loose));
    fix(&mut row);
    group.children.push(row);
}

/// The boxes of a document: the root box (for `<html>`).
pub fn build(doc: &Document, styles: &Styles) -> Option<LayoutBox> {
    let html = doc.find("html")?;
    let mut out = Vec::new();
    Builder { doc, styles }.node(html, &mut out, (0, Color::BLACK));
    out.pop()
}

// ---------------------------------------------------------------- fragments

#[derive(Clone, Debug)]
pub enum FragKind {
    /// A box: its background and borders.
    Box,
    /// A run of text on one line; `baseline` from the fragment's top.
    Text {
        text: String,
        baseline: i32,
        decoration: (u8, Color),
    },
    Replaced(Replaced),
    /// A list marker drawn as a shape (disc, circle, square) or text.
    Marker {
        text: String,
        baseline: i32,
    },
}

#[derive(Clone, Debug)]
pub struct Frag {
    /// The border box, in page pixels.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub style: Rc<Style>,
    pub node: Option<NodeId>,
    pub kind: FragKind,
    pub children: Vec<Frag>,
    /// Drawn after the boxes in normal flow (positioned).
    pub positioned: bool,
    /// The first line's baseline, from the fragment's top, if it has
    /// lines (for inline-blocks, list markers, cells, flex items).
    pub baseline: Option<i32>,
}

impl Frag {
    fn boxed(x: i32, y: i32, w: i32, h: i32, b: &LayoutBox) -> Frag {
        Frag {
            x,
            y,
            w,
            h,
            style: b.style.clone(),
            node: b.node,
            kind: FragKind::Box,
            children: Vec::new(),
            positioned: false,
            baseline: None,
        }
    }

    /// Moves the fragment and everything in it.
    pub fn translate(&mut self, dx: i32, dy: i32) {
        if dx == 0 && dy == 0 {
            return;
        }
        self.x += dx;
        self.y += dy;
        for c in &mut self.children {
            c.translate(dx, dy);
        }
    }

    /// The lowest point of the fragment and its descendants.
    pub fn bottom(&self) -> i32 {
        let own = self.y + self.h;
        if self.style.overflow_hidden {
            return own;
        }
        self.children.iter().map(Frag::bottom).fold(own, i32::max)
    }
}

// ---------------------------------------------------------------- floats

/// The floats of a block formatting context: their margin boxes.
#[derive(Default)]
struct Floats {
    left: Vec<(i32, i32, i32, i32)>,
    right: Vec<(i32, i32, i32, i32)>,
}

impl Floats {
    /// The room between floats from `x0` to `x1`, over `y` to `y + h`.
    fn band(&self, y: i32, h: i32, x0: i32, x1: i32) -> (i32, i32) {
        let h = h.max(1);
        let overlaps = |r: &&(i32, i32, i32, i32)| r.1 < y + h && r.1 + r.3 > y;
        let left = self.left.iter().filter(overlaps).map(|r| r.0 + r.2).fold(x0, i32::max);
        let right = self.right.iter().filter(overlaps).map(|r| r.0).fold(x1, i32::min);
        (left, right)
    }

    /// The lowest bottom of the floats on `side` (`Clear::Both`: either).
    fn clear(&self, clear: Clear) -> i32 {
        let bottom = |v: &Vec<(i32, i32, i32, i32)>| v.iter().map(|r| r.1 + r.3).max().unwrap_or(i32::MIN);
        match clear {
            Clear::None => i32::MIN,
            Clear::Left => bottom(&self.left),
            Clear::Right => bottom(&self.right),
            Clear::Both => bottom(&self.left).max(bottom(&self.right)),
        }
    }

    fn bottom(&self) -> i32 {
        self.clear(Clear::Both)
    }

    /// The first `y` from `from` where a band at least `w` wide, `h` high,
    /// is free between `x0` and `x1`.
    fn fit(&self, from: i32, w: i32, h: i32, x0: i32, x1: i32) -> (i32, i32, i32) {
        let mut y = from;
        for _ in 0..200 {
            let (l, r) = self.band(y, h, x0, x1);
            if r - l >= w || (l == x0 && r == x1) {
                return (y, l, r);
            }
            // Below the next float bottom that ends after `y`.
            let next = self.left.iter().chain(&self.right).map(|f| f.1 + f.3).filter(|&b| b > y).min();
            match next {
                Some(b) => y = b,
                None => return (y, l, r),
            }
        }
        let (l, r) = self.band(y, h, x0, x1);
        (y, l, r)
    }
}

// ---------------------------------------------------------------- context

struct Ctx {
    viewport: (i32, i32),
    /// Absolutely positioned boxes waiting for their containing block:
    /// the box, and where it would have been (its static position).
    pending: Vec<(*const LayoutBox, i32, i32)>,
    /// How deep the layout is: very deep pages are cut.
    depth: usize,
}

/// The resolved box model of a box against a containing width.
#[derive(Clone, Copy, Default)]
struct Edges {
    margin: [i32; 4],
    border: [i32; 4],
    padding: [i32; 4],
}

impl Edges {
    fn of(style: &Style, cb: i32) -> Edges {
        let r = |l: Length| l.or_zero(cb);
        Edges {
            margin: style.margin.map(r),
            border: [0, 1, 2, 3].map(|i| style.border(i)),
            padding: style.padding.map(r),
        }
    }

    /// Left, right: border and padding.
    fn h_inner(&self) -> i32 {
        self.border[1] + self.border[3] + self.padding[1] + self.padding[3]
    }

    fn v_inner(&self) -> i32 {
        self.border[0] + self.border[2] + self.padding[0] + self.padding[2]
    }

    fn h_outer(&self) -> i32 {
        self.h_inner() + self.margin[1] + self.margin[3]
    }
}

// ---------------------------------------------------------------- text

/// The face a style draws its text with.
pub fn face_of(style: &Style) -> Face {
    hln_type::face(style.mono, style.bold, style.font_size)
}

/// Above and below the baseline, a style's line takes (half-leading).
fn line_metrics(style: &Style) -> (i32, i32) {
    let face = face_of(style);
    let lh = style.line_height_px();
    let ascent = face.ascent;
    let content = face.height;
    let above = ascent + (lh - content) / 2;
    (above, lh - above)
}

fn transform(text: &str, t: Transform, at_word_start: &mut bool) -> String {
    match t {
        Transform::None => String::from(text),
        Transform::Upper => text.to_uppercase(),
        Transform::Lower => text.to_lowercase(),
        Transform::Capitalize => {
            let mut out = String::with_capacity(text.len());
            for c in text.chars() {
                if *at_word_start && c.is_alphanumeric() {
                    out.extend(c.to_uppercase());
                    *at_word_start = false;
                } else {
                    out.push(c);
                    if c.is_whitespace() {
                        *at_word_start = true;
                    }
                }
            }
            out
        }
    }
}

/// One unit of inline content.
enum Piece<'a> {
    /// A word (or a run of kept spaces); `space`: a collapsible space
    /// follows it, where the line may break.
    Word {
        text: String,
        width: i32,
        space: i32,
        style: &'a Rc<Style>,
        node: Option<NodeId>,
        deco: (u8, Color),
    },
    /// An inline box opens: its left edge's width (margin, border, padding).
    Open {
        b: &'a LayoutBox,
        edge: i32,
    },
    Close {
        b: &'a LayoutBox,
        edge: i32,
    },
    /// An inline-level block or replaced element, laid out already.
    /// `space`: a collapsible space follows it.
    Atomic {
        frag: Box<Frag>,
        baseline: i32,
        margin: (i32, i32, i32, i32),
        space: i32,
    },
    /// A forced line break.
    Break,
    /// An out-of-flow box met in the text: placed when its line is.
    Float(&'a LayoutBox),
    Absolute(&'a LayoutBox),
}

impl Piece<'_> {
    /// The space after it, where a line may break.
    fn space(&self) -> i32 {
        match self {
            Piece::Word { space, .. } | Piece::Atomic { space, .. } => *space,
            _ => 0,
        }
    }

    /// Its width in sixteenths of a pixel, without the trailing space.
    fn width(&self) -> i32 {
        match self {
            Piece::Word { width, .. } => *width,
            Piece::Open { edge, .. } | Piece::Close { edge, .. } => edge * 16,
            Piece::Atomic { frag, margin, .. } => (frag.w + margin.1 + margin.3) * 16,
            _ => 0,
        }
    }
}

struct Collector<'a> {
    pieces: Vec<Piece<'a>>,
    /// Whether the last thing collected ends with a collapsible space (to
    /// collapse spaces across elements).
    space_pending: bool,
    word_start: bool,
}

impl<'a> Collector<'a> {
    fn text(&mut self, text: &str, b: &'a LayoutBox) {
        let style = &b.style;
        let ws = style.white_space;
        let face = face_of(style);
        let text = transform(text, style.transform, &mut self.word_start);
        let space_width = face.advance(' ');
        if ws.keeps_spaces() || ws.keeps_newlines() {
            // Lines are kept: each line's words, then a break.
            let mut first = true;
            for line in text.split('\n') {
                if !first {
                    self.pieces.push(Piece::Break);
                    self.space_pending = false;
                }
                first = false;
                if ws.keeps_spaces() {
                    let line = expand_tabs(line);
                    // Break opportunities after spaces (pre-wrap), none (pre).
                    let mut start = 0;
                    let chars: Vec<(usize, char)> = line.char_indices().collect();
                    let mut i = 0;
                    while i < chars.len() {
                        let (at, c) = chars[i];
                        i += 1;
                        if c == ' ' && ws.wraps() {
                            let end = at + 1;
                            let word = &line[start..end];
                            self.word(word, face.width16(word), 0, b);
                            start = end;
                        }
                    }
                    if start < line.len() {
                        let word = &line[start..];
                        self.word(word, face.width16(word), 0, b);
                    }
                } else {
                    self.collapsed(line, b, &face, space_width);
                }
            }
        } else {
            self.collapsed(&text, b, &face, space_width);
        }
    }

    fn collapsed(&mut self, text: &str, b: &'a LayoutBox, face: &Face, space_width: i32) {
        let starts_space = text.starts_with(|c: char| c.is_ascii_whitespace());
        if starts_space {
            self.mark_space();
        }
        let words: Vec<&str> = text.split_ascii_whitespace().collect();
        let ends_space = text.ends_with(|c: char| c.is_ascii_whitespace());
        for (i, w) in words.iter().enumerate() {
            let last = i + 1 == words.len();
            let space = if !last || ends_space { space_width } else { 0 };
            if b.style.white_space == WhiteSpace::NoWrap {
                // No break: one word with its spaces.
                self.word(w, face.width16(w), 0, b);
                if space > 0 {
                    self.word(" ", space, 0, b);
                }
                continue;
            }
            self.word(w, face.width16(w), space, b);
        }
        if words.is_empty() && !text.is_empty() {
            self.mark_space();
        } else {
            self.space_pending = ends_space;
        }
    }

    /// A collapsible space at this point: added to the last word or
    /// atomic box, if no space ends it yet.
    fn mark_space(&mut self) {
        if self.space_pending {
            return;
        }
        let last = self.pieces.iter_mut().rev().find(|p| !matches!(p, Piece::Open { .. } | Piece::Close { .. }));
        match last {
            Some(Piece::Word { space, style, .. }) if *space == 0 && style.white_space.wraps() => {
                *space = face_of(style).advance(' ');
            }
            Some(Piece::Atomic { space, frag, .. }) if *space == 0 => {
                *space = face_of(&frag.style).advance(' ');
            }
            _ => {}
        }
        self.space_pending = true;
    }

    fn word(&mut self, text: &str, width: i32, space: i32, b: &'a LayoutBox) {
        self.space_pending = false;
        self.word_start = space > 0;
        self.pieces.push(Piece::Word {
            text: String::from(text),
            width,
            space,
            style: &b.style,
            node: b.node,
            deco: b.decoration,
        });
    }
}

fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return String::from(line);
    }
    let mut out = String::new();
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let n = 8 - col % 8;
            for _ in 0..n {
                out.push(' ');
            }
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

// ---------------------------------------------------------------- intrinsic

/// A box's min-content and max-content widths, margins included.
fn intrinsic(b: &LayoutBox, ctx: &mut Ctx) -> (i32, i32) {
    if let Some(v) = b.intrinsic.get() {
        return v;
    }
    ctx.depth += 1;
    let v = if ctx.depth > 120 { (0, 0) } else { measure(b, ctx) };
    ctx.depth -= 1;
    b.intrinsic.set(Some(v));
    v
}

fn measure(b: &LayoutBox, ctx: &mut Ctx) -> (i32, i32) {
    let s = &b.style;
    let edges = Edges::of(s, 0);
    let outer = edges.h_outer();
    // A fixed width decides.
    if let Some(w) = s.width.resolve(0).filter(|_| !s.width.has_pct()) {
        let w = if s.border_box { w } else { w + edges.h_inner() };
        let w = w + edges.margin[1] + edges.margin[3];
        if !matches!(b.kind, Kind::Inline | Kind::Text(_)) {
            return (w, w);
        }
    }
    let (min, max) = match &b.kind {
        Kind::Text(_) | Kind::Inline | Kind::Break => inline_widths(core::slice::from_ref(b), ctx),
        Kind::Replaced(_) => {
            let f = replaced_frag(b, 0, 0, ctx);
            (f.w, f.w)
        }
        Kind::Table => table_widths(b, ctx),
        Kind::Flex => {
            let mut min = 0;
            let mut max = 0;
            let gap = s.column_gap.or_zero(0);
            for (i, c) in b.children.iter().filter(|c| !c.out_of_flow()).enumerate() {
                let (a, z) = intrinsic(c, ctx);
                if s.flex_row {
                    min = if s.flex_wrap { min.max(a) } else { min + a };
                    max += z + if i > 0 { gap } else { 0 };
                } else {
                    min = min.max(a);
                    max = max.max(z);
                }
            }
            (min, max)
        }
        _ => {
            if b.children.iter().any(LayoutBox::is_inline_level) {
                inline_widths(&b.children, ctx)
            } else {
                let mut min = 0;
                let mut max = 0;
                let mut floats = 0;
                for c in &b.children {
                    if matches!(c.style.position, Position::Absolute | Position::Fixed) {
                        continue;
                    }
                    let (a, z) = intrinsic(c, ctx);
                    min = min.max(a);
                    if c.style.float != Float::None {
                        floats += z;
                    } else {
                        max = max.max(z);
                    }
                }
                (min, max.max(floats))
            }
        }
    };
    let (min, max) = if matches!(b.kind, Kind::Inline | Kind::Text(_)) {
        (min, max)
    } else {
        let lo = s.min_width.resolve(0).filter(|_| !s.min_width.has_pct()).unwrap_or(0);
        let hi = s.max_width.resolve(0).filter(|_| !s.max_width.has_pct()).unwrap_or(i32::MAX / 4);
        let clamp = |v: i32| {
            let inner = if s.border_box { v + edges.h_inner() } else { v };
            let c = inner.min(hi).max(lo);
            if s.border_box { c - edges.h_inner() } else { c }
        };
        (clamp(min) + outer, clamp(max) + outer)
    };
    (min.max(0), max.max(min).max(0))
}

/// Min-content and max-content widths of inline content.
fn inline_widths(boxes: &[LayoutBox], ctx: &mut Ctx) -> (i32, i32) {
    let mut c = Collector { pieces: Vec::new(), space_pending: true, word_start: true };
    for b in boxes {
        collect(b, &mut c, ctx);
    }
    let (mut min, mut max, mut line) = (0i32, 0i32, 0i32);
    let mut word_run = 0;
    for p in &c.pieces {
        match p {
            Piece::Break => {
                max = max.max(line);
                line = 0;
                min = min.max(word_run);
                word_run = 0;
            }
            Piece::Float(f) => {
                let (a, z) = intrinsic(f, ctx);
                min = min.max(a * 16);
                line += z * 16;
            }
            Piece::Absolute(_) => {}
            _ => {
                let w = p.width();
                word_run += w;
                line += w;
                if p.space() > 0 {
                    line += p.space();
                    min = min.max(word_run);
                    word_run = 0;
                }
            }
        }
    }
    min = min.max(word_run);
    max = max.max(line);
    ((min + 15) / 16, (max + 15) / 16)
}

// ---------------------------------------------------------------- blocks

/// Lays out a block-level box whose margin box starts at (`x`, `y`) in a
/// containing block `cb_w` wide (and `cb_h` high, if known); `width`: a
/// width imposed by the parent (flex items, cells), border box.
#[allow(clippy::too_many_arguments)]
fn block(
    b: &LayoutBox,
    x: i32,
    y: i32,
    cb_w: i32,
    cb_h: Option<i32>,
    width: Option<i32>,
    floats: &mut Floats,
    ctx: &mut Ctx,
) -> Frag {
    let s = &b.style;
    let mut e = Edges::of(s, cb_w);
    ctx.depth += 1;
    if ctx.depth > 150 {
        ctx.depth -= 1;
        return Frag::boxed(x, y, 0, 0, b);
    }
    // The border box's width.
    let shrink = b.out_of_flow() || matches!(b.kind, Kind::InlineBlock | Kind::Table | Kind::Replaced(_));
    let specified = s.width.resolve(cb_w).map(|w| if s.border_box { w } else { w + e.h_inner() });
    let mut w = match (width, specified) {
        (Some(w), _) => w,
        (None, Some(w)) => w,
        (None, None) if shrink => {
            let (min, max) = intrinsic(b, ctx);
            let avail = cb_w - e.margin[1] - e.margin[3];
            (max - e.margin[1] - e.margin[3]).min(avail).max(min - e.margin[1] - e.margin[3])
        }
        (None, None) => cb_w - e.margin[1] - e.margin[3],
    };
    if width.is_none() {
        let inner = |v: i32| if s.border_box { v } else { v + e.h_inner() };
        if let Some(max) = s.max_width.resolve(cb_w) {
            w = w.min(inner(max));
        }
        if let Some(min) = s.min_width.resolve(cb_w) {
            w = w.max(inner(min));
        }
    }
    w = w.max(e.h_inner());
    // `margin: auto` centers a block of a known width.
    if width.is_none() && !shrink {
        let free = cb_w - w - e.margin[1] - e.margin[3];
        match (s.margin[3].auto, s.margin[1].auto) {
            (true, true) => {
                e.margin[3] = (cb_w - w).max(0) / 2;
                e.margin[1] = (cb_w - w - e.margin[3]).max(0);
            }
            (true, false) => e.margin[3] += free.max(0),
            (false, true) => e.margin[1] += free.max(0),
            _ => {}
        }
    } else if s.margin[3].auto && s.margin[1].auto && matches!(b.kind, Kind::Table) {
        e.margin[3] = (cb_w - w).max(0) / 2;
    }
    let bx = x + e.margin[3];
    let by = y + e.margin[0];
    let cx = bx + e.border[3] + e.padding[3];
    let cy = by + e.border[0] + e.padding[0];
    let cw = (w - e.h_inner()).max(0);
    // A definite height, for percentages inside.
    let specified_h = s.height.resolve(cb_h.unwrap_or(0)).filter(|_| !s.height.has_pct() || cb_h.is_some());
    let content_h = specified_h.map(|h| if s.border_box { (h - e.v_inner()).max(0) } else { h });
    let positioned = s.position != Position::Static;
    let pending_mark = ctx.pending.len();

    let mut frag = Frag::boxed(bx, by, w, 0, b);
    let mut own = Floats::default();
    let floats: &mut Floats = if b.new_context() { &mut own } else { floats };
    let (inner_h, baseline) = match &b.kind {
        Kind::Table => table(b, cx, cy, cw, &mut frag, ctx),
        Kind::Flex => flex(b, cx, cy, cw, content_h, &mut frag, ctx),
        Kind::Replaced(_) => {
            let f = replaced_frag(b, bx, by, ctx);
            let h = f.h - e.v_inner();
            frag.kind = f.kind;
            frag.w = f.w;
            (h.max(0), None)
        }
        _ => {
            if b.children.iter().any(LayoutBox::is_inline_level) {
                lines(b, &b.children, cx, cy, cw, floats, &mut frag, ctx)
            } else {
                blocks(b, cx, cy, cw, content_h, floats, &mut frag, ctx)
            }
        }
    };
    let mut h = content_h.unwrap_or(inner_h);
    if let Some(min) = s.min_height.resolve(cb_h.unwrap_or(0)).filter(|_| !s.min_height.has_pct() || cb_h.is_some()) {
        h = h.max(if s.border_box { min - e.v_inner() } else { min });
    }
    if let Some(max) = s.max_height.resolve(cb_h.unwrap_or(0)).filter(|_| !s.max_height.has_pct() || cb_h.is_some()) {
        h = h.min(if s.border_box { max - e.v_inner() } else { max });
    }
    // A new formatting context grows to hold its floats.
    if b.new_context() && content_h.is_none() {
        let fb = floats.bottom();
        if fb > cy + h {
            h = fb - cy;
        }
    }
    frag.h = h.max(0) + e.v_inner();
    if let Some(marker) = &b.marker {
        let base = baseline.unwrap_or(line_metrics(s).0);
        let face = face_of(s);
        let mw = face.width(marker);
        let (mx, text) =
            if s.list_inside { (cx, marker.clone()) } else { (cx - mw - face.size / 2 - 2, marker.clone()) };
        frag.children.insert(
            0,
            Frag {
                x: mx,
                y: cy,
                w: mw,
                h: line_metrics(s).0 + line_metrics(s).1,
                style: s.clone(),
                node: b.node,
                kind: FragKind::Marker { text, baseline: base },
                children: Vec::new(),
                positioned: false,
                baseline: None,
            },
        );
    }
    if positioned || b.node.is_none() && ctx.depth == 1 {
        absolutes(&mut frag, pending_mark, ctx);
    }
    if s.position == Position::Relative {
        let dx = s.inset[3].resolve(cb_w).or_else(|| s.inset[1].resolve(cb_w).map(|v| -v)).unwrap_or(0);
        let dy = s.inset[0]
            .resolve(cb_h.unwrap_or(0))
            .or_else(|| s.inset[2].resolve(cb_h.unwrap_or(0)).map(|v| -v))
            .unwrap_or(0);
        frag.translate(dx, dy);
        frag.positioned = true;
    }
    ctx.depth -= 1;
    frag.baseline = baseline.map(|b| b + cy - frag.y);
    frag
}

/// Lays out the absolutely positioned boxes found since `mark`, against
/// `cb`'s padding box.
fn absolutes(cb: &mut Frag, mark: usize, ctx: &mut Ctx) {
    let pending: Vec<_> = ctx.pending.drain(mark..).collect();
    let border = |i: usize| cb.style.border(i);
    let (px, py) = (cb.x + border(3), cb.y + border(0));
    let pw = cb.w - border(1) - border(3);
    let ph = cb.h - border(0) - border(2);
    for (bp, sx, sy) in pending {
        // SAFETY: the pointer is to a box of the tree being laid out, which
        // outlives this layout and is not changed during it.
        let b = unsafe { &*bp };
        let s = &b.style;
        let fixed = s.position == Position::Fixed;
        let (ox, oy, ow, oh) = if fixed { (0, 0, ctx.viewport.0, ctx.viewport.1) } else { (px, py, pw, ph) };
        let left = s.inset[3].resolve(ow);
        let right = s.inset[1].resolve(ow);
        let top = s.inset[0].resolve(oh);
        let bottom = s.inset[2].resolve(oh);
        // Width: given, else between left and right, else shrunk to fit.
        let e = Edges::of(s, ow);
        let width = match (s.width.resolve(ow), left, right) {
            (Some(_), _, _) => None,
            (None, Some(l), Some(r)) => Some((ow - l - r - e.margin[1] - e.margin[3]).max(0)),
            _ => None,
        };
        let mut floats = Floats::default();
        let mut f = block(b, 0, 0, ow, Some(oh), width, &mut floats, ctx);
        let fw = f.w + e.margin[1] + e.margin[3];
        let fh = f.h + e.margin[0] + e.margin[2];
        let x = match (left, right) {
            (Some(l), _) => ox + l,
            (None, Some(r)) => ox + ow - r - fw,
            (None, None) => sx,
        };
        let y = match (top, bottom) {
            (Some(t), _) => oy + t,
            (None, Some(bm)) => oy + oh - bm - fh,
            (None, None) => sy,
        };
        f.translate(x, y);
        f.positioned = true;
        cb.children.push(f);
    }
}

/// Block-level children, one under the other, margins collapsing between
/// siblings. Returns the content height and the first baseline.
#[allow(clippy::too_many_arguments)]
fn blocks(
    b: &LayoutBox,
    cx: i32,
    cy: i32,
    cw: i32,
    ch: Option<i32>,
    floats: &mut Floats,
    frag: &mut Frag,
    ctx: &mut Ctx,
) -> (i32, Option<i32>) {
    let mut y = cy;
    let mut prev_margin = 0;
    let mut baseline = None;
    let mut first = true;
    for c in &b.children {
        let s = &c.style;
        if matches!(s.position, Position::Absolute | Position::Fixed) {
            ctx.pending.push((c as *const LayoutBox, cx, y + prev_margin));
            continue;
        }
        if s.float != Float::None {
            let f = place_float(c, cx, y + prev_margin, cw, floats, ctx);
            frag.children.push(f);
            continue;
        }
        let e = Edges::of(s, cw);
        // Collapse with the previous sibling's bottom margin.
        let top = e.margin[0];
        let collapsed = if first {
            top
        } else if top >= 0 && prev_margin >= 0 {
            top.max(prev_margin)
        } else if top < 0 && prev_margin < 0 {
            top.min(prev_margin)
        } else {
            top + prev_margin
        };
        let mut at = y + collapsed - top;
        if s.clear != Clear::None {
            let below = floats.clear(s.clear);
            if below > at + top {
                at = below - top;
            }
        }
        let mut f = if c.new_context() && (!floats.left.is_empty() || !floats.right.is_empty()) {
            // Beside the floats, in the room they leave.
            let est = intrinsic(c, ctx).0;
            let (fy, l, r) = floats.fit(at + top, est, 1, cx, cx + cw);
            let room = r - l;
            let mut f = block(c, l, fy - top, room, ch, None, floats, ctx);
            if f.w > room && room < cw {
                // Too narrow: below them.
                let below = floats.bottom();
                f = block(c, cx, below - top, cw, ch, None, floats, ctx);
            }
            f
        } else {
            block(c, cx, at, cw, ch, None, floats, ctx)
        };
        if baseline.is_none()
            && let Some(bl) = f.baseline
        {
            baseline = Some(f.y - cy + bl);
        }
        y = f.y + f.h;
        prev_margin = e.margin[2];
        first = false;
        let _ = &mut f;
        frag.children.push(f);
    }
    // The last margin stays inside the parent (no collapsing through).
    (y + prev_margin.max(0) - cy, baseline)
}

/// Places a float at or below `y`, against the left or right edge of the
/// room the other floats leave.
fn place_float(c: &LayoutBox, cx: i32, y: i32, cw: i32, floats: &mut Floats, ctx: &mut Ctx) -> Frag {
    let s = &c.style;
    let e = Edges::of(s, cw);
    let mut f = block(c, 0, 0, cw, None, None, &mut Floats::default(), ctx);
    let ow = f.w + e.margin[1] + e.margin[3];
    let oh = f.h + e.margin[0] + e.margin[2];
    let mut at = y;
    if s.clear != Clear::None {
        at = at.max(floats.clear(s.clear));
    }
    let (fy, l, r) = floats.fit(at, ow.min(cw), oh, cx, cx + cw);
    let fx = if s.float == Float::Left { l } else { r - ow };
    f.translate(fx, fy);
    let rect = (fx, fy, ow, oh);
    if s.float == Float::Left {
        floats.left.push(rect);
    } else {
        floats.right.push(rect);
    }
    f
}

// ---------------------------------------------------------------- inline

/// Turns inline boxes into pieces.
fn collect<'a>(b: &'a LayoutBox, c: &mut Collector<'a>, ctx: &mut Ctx) {
    if b.out_of_flow() {
        if b.style.float != Float::None {
            c.pieces.push(Piece::Float(b));
        } else {
            c.pieces.push(Piece::Absolute(b));
        }
        return;
    }
    match &b.kind {
        Kind::Text(t) => c.text(t, b),
        Kind::Break => {
            c.pieces.push(Piece::Break);
            c.space_pending = true;
        }
        Kind::Inline => {
            let e = Edges::of(&b.style, 0);
            c.pieces.push(Piece::Open { b, edge: e.margin[3] + e.border[3] + e.padding[3] });
            for child in &b.children {
                collect(child, c, ctx);
            }
            c.pieces.push(Piece::Close { b, edge: e.margin[1] + e.border[1] + e.padding[1] });
        }
        _ => {
            // Atomic: laid out at (0, 0), moved into its line later.
            let e = Edges::of(&b.style, 0);
            let mut floats = Floats::default();
            let f = if let Kind::Replaced(_) = b.kind {
                replaced_frag(b, 0, 0, ctx)
            } else {
                block(b, 0, 0, ctx.line_width_hint(), None, None, &mut floats, ctx)
            };
            let fb = f.baseline;
            let (x0, y0) = (f.x, f.y);
            let mut f = f;
            f.translate(-x0, -y0);
            let baseline = match (&b.kind, fb) {
                (Kind::Replaced(Replaced::Field { .. } | Replaced::Button(_) | Replaced::Select(_)), _) => {
                    let face = face_of(&b.style);
                    (f.h - face.height) / 2 + face.ascent
                }
                (Kind::InlineBlock, Some(bl)) if !b.style.overflow_hidden => bl,
                _ => f.h + e.margin[2],
            };
            c.pieces.push(Piece::Atomic {
                frag: Box::new(f),
                baseline: baseline + e.margin[0],
                margin: (e.margin[0], e.margin[1], e.margin[2], e.margin[3]),
                space: 0,
            });
            c.space_pending = false;
            c.word_start = false;
        }
    }
}

impl Ctx {
    /// The width an inline-block may take: the viewport's (its own width
    /// is shrunk to fit its content anyway).
    fn line_width_hint(&self) -> i32 {
        self.viewport.0
    }
}

/// One placed piece of a line: where it starts (sixteenths of a pixel
/// from the line's left), and the piece's index.
struct Placed {
    x: i32,
    index: usize,
}

/// Lays out inline content in lines, from (`cx`, `cy`), `cw` wide,
/// avoiding `floats`. Returns the height used and the first baseline.
#[allow(clippy::too_many_arguments)]
fn lines(
    owner: &LayoutBox,
    boxes: &[LayoutBox],
    cx: i32,
    cy: i32,
    cw: i32,
    floats: &mut Floats,
    frag: &mut Frag,
    ctx: &mut Ctx,
) -> (i32, Option<i32>) {
    let mut c = Collector { pieces: Vec::new(), space_pending: true, word_start: true };
    for b in boxes {
        collect(b, &mut c, ctx);
    }
    let pieces = c.pieces;
    let s = &owner.style;
    let (strut_above, strut_below) = line_metrics(s);
    let strut_h = strut_above + strut_below;
    let mut y = cy;
    let mut first_baseline = None;
    let mut i = 0;
    let mut first_line = true;
    // Inline boxes open at the start of each line: carried over.
    let mut open: Vec<usize> = Vec::new();
    let wraps = s.white_space.wraps();
    while i < pieces.len() {
        let indent = if first_line { s.text_indent.or_zero(cw) } else { 0 };
        let (l, r) = floats.band(y, strut_h, cx, cx + cw);
        let avail = ((r - l - indent).max(0)) * 16;
        // Fill the line.
        let mut placed: Vec<Placed> = Vec::new();
        let mut pen = 0;
        let mut last_break: Option<(usize, usize)> = None;
        let start = i;
        let mut forced = false;
        while i < pieces.len() {
            let p = &pieces[i];
            match p {
                Piece::Break => {
                    i += 1;
                    forced = true;
                    break;
                }
                Piece::Float(fb) => {
                    // Placed now, at this line's top if it fits.
                    let f = place_float(fb, cx, y, cw, floats, ctx);
                    frag.children.push(f);
                    i += 1;
                    continue;
                }
                Piece::Absolute(ab) => {
                    ctx.pending.push((*ab as *const LayoutBox, l + pen / 16, y));
                    i += 1;
                    continue;
                }
                _ => {}
            }
            let w = p.width();
            let can_wrap = match p {
                Piece::Word { style, .. } => style.white_space.wraps(),
                Piece::Atomic { .. } => wraps,
                _ => true,
            };
            if pen + w > avail && can_wrap && !placed.is_empty() {
                // Break at the last opportunity, if any.
                if let Some((bi, bp)) = last_break {
                    placed.truncate(bp);
                    i = bi;
                }
                break;
            }
            placed.push(Placed { x: pen, index: i });
            pen += w;
            if p.space() > 0 {
                pen += p.space();
                last_break = Some((i + 1, placed.len()));
            }
            i += 1;
        }
        if placed.is_empty() && i == start && !forced {
            // Nothing fit, nothing to place: avoid looping.
            i += 1;
            continue;
        }
        // The trailing space does not count.
        let line_w = match placed.last() {
            Some(last) => pen - pieces[last.index].space(),
            None => 0,
        };
        // Heights: the strut, then each piece's.
        let mut above = strut_above;
        let mut below = strut_below;
        let mut stack_style: Vec<&Rc<Style>> = open
            .iter()
            .map(|&o| match &pieces[o] {
                Piece::Open { b, .. } => &b.style,
                _ => &owner.style,
            })
            .collect();
        for p in &placed {
            match &pieces[p.index] {
                Piece::Word { style, .. } => {
                    let (a, d) = line_metrics(style);
                    let shift = valign_shift(style);
                    above = above.max(a - shift);
                    below = below.max(d + shift);
                }
                Piece::Open { b, .. } => {
                    stack_style.push(&b.style);
                    let (a, d) = line_metrics(&b.style);
                    above = above.max(a);
                    below = below.max(d);
                }
                Piece::Close { .. } => {
                    stack_style.pop();
                }
                Piece::Atomic { frag: f, baseline, margin, .. } => {
                    let total = f.h + margin.0 + margin.2;
                    match f.style.vertical_align {
                        VAlign::Middle => {
                            let face = face_of(s);
                            let mid = face.ascent * 2 / 5;
                            above = above.max(total / 2 + mid);
                            below = below.max(total - total / 2 - mid);
                        }
                        VAlign::Top | VAlign::Bottom => {
                            above = above.max(total - strut_below);
                        }
                        _ => {
                            above = above.max(*baseline);
                            below = below.max(total - baseline);
                        }
                    }
                }
                _ => {}
            }
        }
        let line_h = above + below;
        // The band for this line's real height.
        let (l, r) = floats.band(y, line_h, cx, cx + cw);
        let room = (r - l - indent) * 16;
        let extra = (room - line_w).max(0);
        let last_line = i >= pieces.len() || forced;
        let (offset, per_space) = match s.text_align {
            TextAlign::Left => (0, 0),
            TextAlign::Right => (extra, 0),
            TextAlign::Center => (extra / 2, 0),
            TextAlign::Justify if !last_line => {
                let spaces = placed
                    .iter()
                    .filter(|p| matches!(&pieces[p.index], Piece::Word { space, .. } if *space > 0))
                    .count()
                    .saturating_sub(1);
                if spaces > 0 { (0, extra / spaces as i32) } else { (0, 0) }
            }
            TextAlign::Justify => (0, 0),
        };
        let baseline = y + above;
        if first_baseline.is_none() {
            first_baseline = Some(above);
        }
        let x0 = (l + indent) * 16 + offset;
        // Emit fragments: inline box backgrounds first, then text.
        let mut boxes_open: Vec<(usize, i32)> = open.iter().map(|&o| (o, x0)).collect();
        let mut texts: Vec<Frag> = Vec::new();
        let mut spaces_seen = 0;
        // A run of text being gathered: where, what, its style, node,
        // decoration and width.
        type Run<'s> = Option<(i32, String, &'s Rc<Style>, Option<NodeId>, (u8, Color), i32)>;
        let mut run: Run = None;
        let flush_run = |run: &mut Run, texts: &mut Vec<Frag>| {
            if let Some((rx, text, style, node, deco, rw)) = run.take() {
                let text = text.trim_end_matches(' ');
                let text = String::from(text);
                if text.is_empty() {
                    return;
                }
                let face = face_of(style);
                let shift = valign_shift(style);
                let top = baseline + shift - face.ascent;
                texts.push(Frag {
                    x: (rx + 8) / 16,
                    y: top,
                    w: (rw + 15) / 16,
                    h: face.height,
                    style: style.clone(),
                    node,
                    kind: FragKind::Text { text, baseline: face.ascent, decoration: deco },
                    children: Vec::new(),
                    positioned: false,
                    baseline: None,
                });
            }
        };
        let mut boxes_done: Vec<Frag> = Vec::new();
        let mut atomics: Vec<Frag> = Vec::new();
        for (k, p) in placed.iter().enumerate() {
            let px = x0 + p.x + per_space * spaces_seen;
            match &pieces[p.index] {
                Piece::Word { text, width, space, style, node, deco } => {
                    let same = run.as_ref().is_some_and(|r| Rc::ptr_eq(r.2, style) && r.3 == *node);
                    if !same {
                        flush_run(&mut run, &mut texts);
                        run = Some((px, String::new(), style, *node, *deco, 0));
                    }
                    if let Some(r) = run.as_mut()
                        && per_space > 0
                        && !r.1.is_empty()
                        && r.1.ends_with(' ')
                    {
                        // Justified: each word on its own.
                        flush_run(&mut run, &mut texts);
                        run = Some((px, String::new(), style, *node, *deco, 0));
                    }
                    if let Some(r) = run.as_mut() {
                        r.1.push_str(text);
                        r.5 = px - r.0 + width;
                        let last_on_line = k + 1 == placed.len();
                        if *space > 0 && !last_on_line {
                            r.1.push(' ');
                        }
                    }
                    if *space > 0 {
                        spaces_seen += 1;
                    }
                }
                Piece::Open { .. } => {
                    flush_run(&mut run, &mut texts);
                    boxes_open.push((p.index, px));
                }
                Piece::Close { b, edge } => {
                    flush_run(&mut run, &mut texts);
                    if let Some(at) = boxes_open
                        .iter()
                        .rposition(|(o, _)| matches!(&pieces[*o], Piece::Open { b: ob, .. } if core::ptr::eq(*ob, *b)))
                    {
                        let (o, start_x) = boxes_open.remove(at);
                        let first_part = placed.iter().any(|q| q.index == o);
                        boxes_done.push(inline_box(b, start_x, px + edge * 16, baseline, first_part, true));
                    }
                }
                Piece::Atomic { frag: f, baseline: fb, margin, .. } => {
                    flush_run(&mut run, &mut texts);
                    let mut f = (**f).clone();
                    let ax = (px + 8) / 16 + margin.3;
                    let ay = match f.style.vertical_align {
                        VAlign::Middle => {
                            baseline - face_of(s).ascent * 2 / 5 - (f.h + margin.0 + margin.2) / 2 + margin.0
                        }
                        VAlign::Top => y + margin.0,
                        VAlign::Bottom => y + line_h - f.h - margin.2,
                        _ => baseline - fb + margin.0,
                    };
                    f.translate(ax, ay);
                    atomics.push(f);
                }
                _ => {}
            }
        }
        flush_run(&mut run, &mut texts);
        // Boxes still open continue on the next line.
        let line_end = x0 + line_w;
        for (o, start_x) in &boxes_open {
            if let Piece::Open { b, .. } = &pieces[*o] {
                let first_part = placed.iter().any(|q| q.index == *o);
                boxes_done.push(inline_box(b, *start_x, line_end, baseline, first_part, false));
            }
        }
        open = boxes_open.iter().map(|(o, _)| *o).collect();
        frag.children.extend(boxes_done);
        frag.children.extend(texts);
        frag.children.extend(atomics);
        y += line_h;
        first_line = false;
    }
    let _ = strut_h;
    (y - cy, first_baseline)
}

/// How far a piece's baseline moves down (sub) or up (super).
fn valign_shift(style: &Style) -> i32 {
    match style.vertical_align {
        VAlign::Sub => style.font_size / 4,
        VAlign::Super => -style.font_size / 2,
        _ => 0,
    }
}

/// An inline box's part on one line: its background and borders around the
/// text, from `x0` to `x1` (sixteenths of a pixel).
fn inline_box(b: &LayoutBox, x0: i32, x1: i32, baseline: i32, first: bool, last: bool) -> Frag {
    let s = &b.style;
    let e = Edges::of(s, 0);
    let face = face_of(s);
    let left_margin = if first { e.margin[3] } else { 0 };
    let right_margin = if last { e.margin[1] } else { 0 };
    let x = (x0 + 8) / 16 + left_margin;
    let w = ((x1 + 8) / 16 - right_margin - x).max(0);
    let top = baseline - face.ascent - e.padding[0] - e.border[0];
    let h = face.height + e.v_inner();
    let mut style = (**s).clone();
    if !first {
        style.border_style_set[3] = false;
    }
    if !last {
        style.border_style_set[1] = false;
    }
    Frag {
        x,
        y: top,
        w,
        h,
        style: Rc::new(style),
        node: b.node,
        kind: FragKind::Box,
        children: Vec::new(),
        positioned: false,
        baseline: None,
    }
}

// ---------------------------------------------------------------- replaced

/// The fragment of a replaced element at (`x`, `y`), border box.
fn replaced_frag(b: &LayoutBox, x: i32, y: i32, _ctx: &mut Ctx) -> Frag {
    let s = &b.style;
    let e = Edges::of(s, 0);
    let Kind::Replaced(r) = &b.kind else { return Frag::boxed(x, y, 0, 0, b) };
    let face = face_of(s);
    let given_w = s.width.resolve(0).filter(|_| !s.width.has_pct());
    let given_h = s.height.resolve(0).filter(|_| !s.height.has_pct());
    let inner = |w: Option<i32>, h: Option<i32>, dw: i32, dh: i32| {
        let w = w.map(|w| if s.border_box { w - e.h_inner() } else { w }).unwrap_or(dw);
        let h = h.map(|h| if s.border_box { h - e.v_inner() } else { h }).unwrap_or(dh);
        (w.max(0), h.max(0))
    };
    let (w, h) = match r {
        Replaced::Image { alt, .. } => match (given_w, given_h) {
            (Some(w), Some(h)) => inner(Some(w), Some(h), 0, 0),
            (Some(w), None) => inner(Some(w), None, 0, w.min(64)),
            (None, Some(h)) => inner(None, Some(h), h.min(64), 0),
            (None, None) if alt.is_empty() => (0, 0),
            (None, None) => (face.width(alt) + 8, face.height + 4),
        },
        Replaced::Field { text, .. } => inner(given_w, given_h, face.width(text).max(150), face.height),
        Replaced::Button(text) => inner(given_w, given_h, face.width(text), face.height),
        Replaced::Select(text) => inner(given_w, given_h, face.width(text) + 4, face.height),
        Replaced::Check { .. } => inner(given_w, given_h, 13, 13),
        Replaced::Area(_) => inner(given_w, given_h, 300, face.height * 2),
        Replaced::Embedded(what) => {
            let (dw, dh) = if what == "svg" { (0, 0) } else { (300, 150) };
            inner(given_w, given_h, dw, dh)
        }
    };
    let mut w = w + e.h_inner();
    let mut h = h + e.v_inner();
    if let Some(max) = s.max_width.resolve(0).filter(|_| !s.max_width.has_pct()) {
        w = w.min(max + if s.border_box { 0 } else { e.h_inner() });
    }
    if let Some(max) = s.max_height.resolve(0).filter(|_| !s.max_height.has_pct()) {
        h = h.min(max + if s.border_box { 0 } else { e.v_inner() });
    }
    let mut f = Frag::boxed(x + e.margin[3], y + e.margin[0], w, h, b);
    f.kind = FragKind::Replaced(r.clone());
    f
}

// ---------------------------------------------------------------- tables

struct Grid<'a> {
    /// Rows in order (head, bodies, foot), each its row box and its cells
    /// with their first column.
    rows: Vec<(&'a LayoutBox, Vec<(usize, &'a LayoutBox)>)>,
    columns: usize,
}

fn grid(table: &LayoutBox) -> (Grid<'_>, Vec<&LayoutBox>) {
    let mut groups: Vec<&LayoutBox> = Vec::new();
    let mut captions = Vec::new();
    for c in &table.children {
        match c.kind {
            Kind::Caption => captions.push(c),
            _ => groups.push(c),
        }
    }
    // thead first, tfoot last.
    let tag = |g: &LayoutBox| g.node.map(|_| 0).unwrap_or(0);
    let _ = tag;
    let mut rows_in_order: Vec<&LayoutBox> = Vec::new();
    for g in &groups {
        for r in &g.children {
            rows_in_order.push(r);
        }
    }
    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut rows = Vec::new();
    let mut columns = 0;
    for (ri, r) in rows_in_order.iter().enumerate() {
        if occupied.len() <= ri {
            occupied.resize(ri + 1, Vec::new());
        }
        let mut col = 0;
        let mut cells = Vec::new();
        for c in &r.children {
            while occupied[ri].get(col).copied().unwrap_or(false) {
                col += 1;
            }
            let (cs, rs) = c.span;
            let cs = cs.min(200);
            let rs = rs.min(rows_in_order.len() - ri).max(1);
            for dr in 0..rs {
                if occupied.len() <= ri + dr {
                    occupied.resize(ri + dr + 1, Vec::new());
                }
                let row = &mut occupied[ri + dr];
                if row.len() < col + cs {
                    row.resize(col + cs, false);
                }
                for k in 0..cs {
                    row[col + k] = true;
                }
            }
            cells.push((col, c));
            col += cs;
            columns = columns.max(col);
        }
        rows.push((*r, cells));
    }
    (Grid { rows, columns }, captions)
}

fn table_widths(b: &LayoutBox, ctx: &mut Ctx) -> (i32, i32) {
    let (g, captions) = grid(b);
    let (mins, maxs) = column_widths(&g, ctx);
    let spacing = if b.style.border_collapse { 0 } else { b.style.border_spacing };
    let extra = spacing * (g.columns as i32 + 1);
    let mut min: i32 = mins.iter().sum::<i32>() + extra;
    let mut max: i32 = maxs.iter().sum::<i32>() + extra;
    for c in captions {
        let (a, z) = intrinsic(c, ctx);
        min = min.max(a);
        max = max.max(z);
    }
    (min, max)
}

fn column_widths(g: &Grid, ctx: &mut Ctx) -> (Vec<i32>, Vec<i32>) {
    let n = g.columns;
    let mut mins = alloc::vec![0; n];
    let mut maxs = alloc::vec![0; n];
    let mut spans = Vec::new();
    for (_, cells) in &g.rows {
        for &(col, c) in cells {
            let (a, z) = intrinsic(c, ctx);
            if c.span.0 == 1 {
                mins[col] = mins[col].max(a);
                maxs[col] = maxs[col].max(z);
            } else {
                spans.push((col, c.span.0.min(n - col), a, z));
            }
        }
    }
    for (col, span, a, z) in spans {
        let have_min: i32 = mins[col..col + span].iter().sum();
        let have_max: i32 = maxs[col..col + span].iter().sum();
        if a > have_min {
            let add = (a - have_min) / span as i32;
            for m in &mut mins[col..col + span] {
                *m += add;
            }
        }
        if z > have_max {
            let add = (z - have_max) / span as i32;
            for m in &mut maxs[col..col + span] {
                *m += add;
            }
        }
    }
    for i in 0..n {
        maxs[i] = maxs[i].max(mins[i]);
    }
    (mins, maxs)
}

/// A table's content in its box: columns sized, rows laid out. Returns
/// the height and the first row's baseline.
fn table(b: &LayoutBox, cx: i32, cy: i32, cw: i32, frag: &mut Frag, ctx: &mut Ctx) -> (i32, Option<i32>) {
    let (g, captions) = grid(b);
    let s = &b.style;
    let spacing = if s.border_collapse { 0 } else { s.border_spacing };
    let n = g.columns;
    let mut y = cy;
    for cap in &captions {
        let f = block(cap, cx, y, cw, None, None, &mut Floats::default(), ctx);
        y = f.y + f.h;
        frag.children.push(f);
    }
    if n == 0 {
        return (y - cy, None);
    }
    let (mins, maxs) = column_widths(&g, ctx);
    let avail = cw - spacing * (n as i32 + 1);
    let sum_min: i32 = mins.iter().sum();
    let sum_max: i32 = maxs.iter().sum();
    let fixed_width = s.width.resolve(cw).is_some();
    let widths: Vec<i32> = if sum_max <= avail {
        if fixed_width && sum_max > 0 {
            // Spread what is left in proportion to the widest contents.
            let extra = avail - sum_max;
            let mut w: Vec<i32> =
                maxs.iter().map(|&m| m + (i64::from(extra) * i64::from(m) / i64::from(sum_max)) as i32).collect();
            let left = avail - w.iter().sum::<i32>();
            if let Some(last) = w.last_mut() {
                *last += left;
            }
            w
        } else if fixed_width {
            alloc::vec![avail / n as i32; n]
        } else {
            maxs.clone()
        }
    } else if sum_min >= avail || sum_max == sum_min {
        mins.clone()
    } else {
        let room = i64::from(avail - sum_min);
        let range = i64::from(sum_max - sum_min);
        mins.iter().zip(&maxs).map(|(&a, &z)| a + (i64::from(z - a) * room / range) as i32).collect()
    };
    let col_x: Vec<i32> = {
        let mut xs = Vec::with_capacity(n);
        let mut x = cx + spacing;
        for w in &widths {
            xs.push(x);
            x += w + spacing;
        }
        xs
    };
    // Lay out every cell at its column, row by row.
    let mut baseline = None;
    let row_count = g.rows.len();
    let mut row_top = alloc::vec![0; row_count + 1];
    let mut cell_frags: Vec<(usize, usize, Frag, Option<i32>)> = Vec::new();
    y += spacing;
    for (ri, (row, cells)) in g.rows.iter().enumerate() {
        row_top[ri] = y;
        let mut row_h = row.style.height.resolve(0).filter(|_| !row.style.height.has_pct()).unwrap_or(0);
        for &(col, c) in cells {
            let span = c.span.0.min(n - col);
            let w = widths[col..col + span].iter().sum::<i32>() + spacing * (span as i32 - 1);
            let f = block(c, col_x[col], y, w, None, Some(w), &mut Floats::default(), ctx);
            let bl = f.baseline;
            if c.span.1 == 1 {
                row_h = row_h.max(f.h);
            }
            cell_frags.push((ri, c.span.1, f, bl));
        }
        y += row_h + spacing;
        row_top[ri + 1] = y;
    }
    // Cells spanning rows stretch the last row they span.
    for (ri, rs, f, _) in &cell_frags {
        if *rs > 1 {
            let last = (ri + rs).min(row_count);
            let have = row_top[last] - spacing - row_top[*ri];
            if f.h > have {
                let add = f.h - have;
                for t in &mut row_top[last..] {
                    *t += add;
                }
            }
        }
    }
    // Row backgrounds, then cells stretched to their rows and aligned.
    for (ri, (row, _)) in g.rows.iter().enumerate() {
        if row.style.background.alpha() > 0 {
            let h = row_top[ri + 1] - spacing - row_top[ri];
            let rf = Frag::boxed(cx, row_top[ri], cw, h, row);
            frag.children.push(rf);
        }
    }
    for (ri, rs, mut f, bl) in cell_frags {
        let last = (ri + rs).min(row_count);
        let top = row_top[ri];
        let h = row_top[last] - spacing - top;
        // Rows that moved down (row spans above) move their cells.
        f.translate(0, top - f.y);
        let content_h = f.bottom_of_content();
        let free = h - f.h;
        let dy = match f.style.vertical_align {
            VAlign::Middle => free / 2,
            VAlign::Bottom => free,
            _ => 0,
        };
        if dy > 0 {
            for c in &mut f.children {
                c.translate(0, dy);
            }
        }
        let _ = content_h;
        f.h = f.h.max(h);
        if baseline.is_none() && ri == 0 {
            baseline = bl.map(|b| b + top - cy + dy);
        }
        frag.children.push(f);
    }
    (row_top[row_count] - cy, baseline)
}

impl Frag {
    fn bottom_of_content(&self) -> i32 {
        self.children.iter().map(Frag::bottom).max().unwrap_or(self.y)
    }
}

// ---------------------------------------------------------------- flex

/// Where the `k`-th of `n` items moves along the main axis when `free`
/// pixels are left over.
fn distribute(justify: Align, free: i32, n: i32, k: i32) -> i32 {
    match justify {
        Align::End => free,
        Align::Center => free / 2,
        Align::SpaceBetween if n > 1 => free * k / (n - 1),
        Align::SpaceAround if n > 0 => free * (2 * k + 1) / (2 * n),
        Align::SpaceEvenly if n > 0 => free * (k + 1) / (n + 1),
        _ => 0,
    }
}

/// A flex container's items. Returns the content height and baseline.
fn flex(
    b: &LayoutBox,
    cx: i32,
    cy: i32,
    cw: i32,
    ch: Option<i32>,
    frag: &mut Frag,
    ctx: &mut Ctx,
) -> (i32, Option<i32>) {
    let s = &b.style;
    let mut items: Vec<&LayoutBox> = Vec::new();
    for c in &b.children {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            ctx.pending.push((c as *const LayoutBox, cx, cy));
        } else {
            items.push(c);
        }
    }
    items.sort_by_key(|c| c.style.order);
    if s.flex_reverse {
        items.reverse();
    }
    if !s.flex_row {
        return flex_column(b, &items, cx, cy, cw, ch, frag, ctx);
    }
    let gap = s.column_gap.or_zero(cw);
    let row_gap = s.row_gap.or_zero(ch.unwrap_or(0));
    // Each item's hypothetical width (border box) and its margins.
    struct Item<'a> {
        b: &'a LayoutBox,
        base: i32,
        min: i32,
        max: i32,
        margins: i32,
        w: i32,
    }
    let mut list: Vec<Item> = items
        .iter()
        .map(|c| {
            let cs = &c.style;
            let e = Edges::of(cs, cw);
            let (imin, imax) = intrinsic(c, ctx);
            let margins = e.margin[1] + e.margin[3];
            let to_border = |v: i32| if cs.border_box { v } else { v + e.h_inner() };
            let base = match cs.basis.resolve(cw) {
                Some(v) if !cs.basis.auto => to_border(v),
                _ => match cs.width.resolve(cw) {
                    Some(v) => to_border(v),
                    None => (imax - margins).min(cw),
                },
            };
            let min = match cs.min_width.resolve(cw) {
                Some(v) if v > 0 => to_border(v),
                _ if cs.overflow_hidden => e.h_inner(),
                _ => (imin - margins).min(cw).max(e.h_inner()),
            };
            let max = cs.max_width.resolve(cw).map_or(i32::MAX / 4, to_border);
            let base = base.max(0);
            Item { b: c, base, min: min.min(max), max, margins, w: base.clamp(min.min(max), max) }
        })
        .collect();
    // Lines.
    let mut lines_of: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (i, it) in list.iter().enumerate() {
        let need = it.w + it.margins + if i > start { gap } else { 0 };
        if s.flex_wrap && i > start && used + need > cw {
            lines_of.push((start, i));
            start = i;
            used = it.w + it.margins;
        } else {
            used += need;
        }
    }
    lines_of.push((start, list.len()));
    let mut y = cy;
    let mut baseline = None;
    for (li, &(a, z)) in lines_of.iter().enumerate() {
        if li > 0 {
            y += row_gap;
        }
        // Grow or shrink the line's items.
        for _round in 0..4 {
            let total: i32 =
                list[a..z].iter().map(|it| it.w + it.margins).sum::<i32>() + gap * (z - a).saturating_sub(1) as i32;
            let free = cw - total;
            if free > 0 {
                let grow: i64 = list[a..z].iter().filter(|it| it.w < it.max).map(|it| i64::from(it.b.style.grow)).sum();
                if grow == 0 {
                    break;
                }
                let mut changed = false;
                for it in &mut list[a..z] {
                    if it.w < it.max && it.b.style.grow > 0 {
                        let add = (i64::from(free) * i64::from(it.b.style.grow) / grow) as i32;
                        let new = (it.w + add).min(it.max);
                        changed |= new != it.w;
                        it.w = new;
                    }
                }
                if !changed {
                    break;
                }
            } else if free < 0 {
                let weight: i64 = list[a..z]
                    .iter()
                    .filter(|it| it.w > it.min)
                    .map(|it| i64::from(it.b.style.shrink) * i64::from(it.base.max(1)))
                    .sum();
                if weight == 0 {
                    break;
                }
                let mut changed = false;
                for it in &mut list[a..z] {
                    if it.w > it.min {
                        let w = i64::from(it.b.style.shrink) * i64::from(it.base.max(1));
                        let cut = (i64::from(-free) * w / weight) as i32;
                        let new = (it.w - cut.max(1)).max(it.min);
                        changed |= new != it.w;
                        it.w = new;
                    }
                }
                if !changed {
                    break;
                }
            } else {
                break;
            }
        }
        // Lay out each item at its width.
        let mut frags: Vec<(Frag, Option<i32>, i32)> = Vec::new();
        let mut x = 0;
        for (k, it) in list[a..z].iter().enumerate() {
            if k > 0 {
                x += gap;
            }
            let item_cb_h = ch.filter(|_| lines_of.len() == 1);
            let f = block(it.b, cx + x, y, it.w + it.margins, item_cb_h, Some(it.w), &mut Floats::default(), ctx);
            let bl = f.baseline;
            let e = Edges::of(&it.b.style, cw);
            x += it.w + it.margins;
            frags.push((f, bl, e.margin[0] + e.margin[2]));
        }
        let line_w = x;
        let cross = frags.iter().map(|(f, _, m)| f.h + m).max().unwrap_or(0);
        let cross = match ch {
            Some(h) if lines_of.len() == 1 => cross.max(h),
            _ => cross,
        };
        // Main axis distribution.
        let free = (cw - line_w).max(0);
        let n = frags.len() as i32;

        let line_baseline = frags.iter().filter_map(|(f, bl, _)| bl.map(|b| b + f.y - y)).max();
        for (k, (mut f, bl, m)) in frags.into_iter().enumerate() {
            let dx = distribute(s.justify, free, n, k as i32);
            let align = f.style.align_self.unwrap_or(s.align_items);
            let auto_h = f.style.height.auto;
            let dy = match align {
                Align::Center => (cross - f.h - m) / 2,
                Align::End => cross - f.h - m,
                Align::Baseline => match (line_baseline, bl) {
                    (Some(lb), Some(b)) => lb - (b + f.y - y),
                    _ => 0,
                },
                Align::Stretch if auto_h => {
                    f.h = cross - m;
                    0
                }
                _ => 0,
            };
            f.translate(dx, dy);
            if baseline.is_none()
                && let Some(b) = bl
            {
                baseline = Some(b + f.y - cy);
            }
            frag.children.push(f);
        }
        y += cross;
    }
    (y - cy, baseline)
}

#[allow(clippy::too_many_arguments)]
fn flex_column(
    b: &LayoutBox,
    items: &[&LayoutBox],
    cx: i32,
    cy: i32,
    cw: i32,
    ch: Option<i32>,
    frag: &mut Frag,
    ctx: &mut Ctx,
) -> (i32, Option<i32>) {
    let s = &b.style;
    let gap = s.row_gap.or_zero(ch.unwrap_or(0));
    let mut y = cy;
    let mut baseline = None;
    let mut frags = Vec::new();
    for (k, c) in items.iter().enumerate() {
        if k > 0 {
            y += gap;
        }
        let align = c.style.align_self.unwrap_or(s.align_items);
        let e = Edges::of(&c.style, cw);
        let width = if c.style.width.auto && align != Align::Stretch {
            let (min, max) = intrinsic(c, ctx);
            Some((max.min(cw).max(min) - e.margin[1] - e.margin[3]).max(0))
        } else {
            None
        };
        let mut f = block(c, cx, y, cw, None, width, &mut Floats::default(), ctx);
        let bl = f.baseline;
        if baseline.is_none() {
            baseline = bl.map(|b| b + f.y - cy);
        }
        let free = cw - f.w - e.margin[1] - e.margin[3];
        let dx = match align {
            Align::Center => free / 2,
            Align::End => free,
            _ => 0,
        };
        f.translate(dx, 0);
        y = f.y + f.h + e.margin[2];
        frags.push(f);
    }
    // Growing items fill a definite height.
    let used = y - cy;
    if let Some(h) = ch
        && h > used
    {
        let free = h - used;
        let grow: i64 = items.iter().map(|c| i64::from(c.style.grow)).sum();
        let n = frags.len() as i32;
        if grow > 0 {
            let mut shift = 0;
            for (f, c) in frags.iter_mut().zip(items) {
                f.translate(0, shift);
                let add = (i64::from(free) * i64::from(c.style.grow) / grow) as i32;
                f.h += add;
                shift += add;
            }
        } else {
            for (k, f) in frags.iter_mut().enumerate() {
                f.translate(0, distribute(s.justify, free, n, k as i32));
            }
        }
    }
    frag.children.extend(frags);
    (used, baseline)
}

// ---------------------------------------------------------------- page

/// A laid out page.
pub struct Layout {
    pub root: Frag,
    /// The page's full height, in pixels.
    pub height: i32,
    /// The canvas color: the root's or the body's background.
    pub canvas: Color,
}

/// Lays out the boxes of a page in a viewport `width` × `height` pixels.
pub fn layout(root: &LayoutBox, width: i32, height: i32) -> Layout {
    let mut ctx = Ctx { viewport: (width, height), pending: Vec::new(), depth: 0 };
    let mut floats = Floats::default();
    let mut frag = block(root, 0, 0, width, Some(height), None, &mut floats, &mut ctx);
    // Positioned boxes left over: against the page.
    absolutes(&mut frag, 0, &mut ctx);
    let height = frag.bottom().max(floats.bottom()).max(height);
    // The canvas takes the root's background, else the body's.
    let mut canvas = root.style.background;
    if canvas.alpha() == 0
        && let Some(body) = root.children.iter().find(|c| c.node.is_some())
    {
        canvas = body.style.background;
    }
    Layout { root: frag, height, canvas: if canvas.alpha() == 0 { Color::WHITE } else { canvas } }
}

/// Text of a fragment tree, for tests: each text run on its line.
pub fn text_of(frag: &Frag) -> String {
    let mut runs: Vec<(i32, i32, String)> = Vec::new();
    fn walk(f: &Frag, out: &mut Vec<(i32, i32, String)>) {
        if let FragKind::Text { text, .. } = &f.kind {
            out.push((f.y, f.x, text.clone()));
        }
        for c in &f.children {
            walk(c, out);
        }
    }
    walk(frag, &mut runs);
    runs.sort();
    let mut out = String::new();
    let mut last_y: Option<i32> = None;
    for (y, _, t) in runs {
        if let Some(ly) = last_y {
            out.push(if (y - ly).abs() > 3 { '\n' } else { ' ' });
        }
        out.push_str(&t);
        last_y = Some(y);
    }
    out.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{self, Media};
    use crate::{html, style};

    fn lay(page: &str, width: i32) -> (Layout, Document) {
        let doc = html::parse(page);
        let media = Media { width, height: 600, dark: false };
        let sheets: Vec<_> = doc
            .descendants(crate::dom::DOCUMENT)
            .into_iter()
            .filter(|&n| doc.is(n, "style"))
            .map(|n| css::parse_sheet(&doc.text_content(n), &media))
            .collect();
        let styles = style::compute(&doc, &sheets, &media);
        let root = build(&doc, &styles).unwrap();
        (layout(&root, width, 600), doc)
    }

    fn find<'a>(f: &'a Frag, doc: &Document, tag: &str) -> Option<&'a Frag> {
        if f.node.is_some_and(|n| doc.is(n, tag)) && matches!(f.kind, FragKind::Box | FragKind::Replaced(_)) {
            return Some(f);
        }
        f.children.iter().find_map(|c| find(c, doc, tag))
    }

    #[test]
    fn text_wraps_in_lines() {
        let (l, _) = lay("<p>Le petit âne gris marche sur le chemin de pierre, sous le soleil.</p>", 200);
        let text = text_of(&l.root);
        assert!(text.lines().count() >= 2, "{text}");
        assert!(text.starts_with("Le petit"));
        assert_eq!(text.split_whitespace().count(), 13);
    }

    #[test]
    fn blocks_stack_and_margins_collapse() {
        let (l, doc) =
            lay("<style>body{margin:0} div{height:50px;margin:20px 0}</style><div id=a></div><div id=b></div>", 400);
        let divs: Vec<&Frag> = {
            fn all<'a>(f: &'a Frag, doc: &Document, out: &mut Vec<&'a Frag>) {
                if f.node.is_some_and(|n| doc.is(n, "div")) {
                    out.push(f);
                }
                for c in &f.children {
                    all(c, doc, out);
                }
            }
            let mut v = Vec::new();
            all(&l.root, &doc, &mut v);
            v
        };
        assert_eq!(divs.len(), 2);
        assert_eq!(divs[0].y, 20);
        assert_eq!(divs[1].y, 90);
        assert_eq!(divs[0].w, 400);
    }

    #[test]
    fn centered_and_sized() {
        let (l, doc) = lay(
            "<style>body{margin:0} main{width:200px;margin:0 auto;padding:10px;border:5px solid}</style><main>x</main>",
            600,
        );
        let m = find(&l.root, &doc, "main").unwrap();
        assert_eq!((m.x, m.w), (185, 230));
    }

    #[test]
    fn floats_push_lines_aside() {
        let (l, doc) = lay(
            "<style>body{margin:0} .f{float:right;width:100px;height:100px}</style><div class=f></div><p>Du texte à côté du flottant.</p>",
            300,
        );
        let f = find(&l.root, &doc, "div").unwrap();
        assert_eq!((f.x, f.y), (200, 0));
        fn max_right(f: &Frag) -> i32 {
            let own = if matches!(f.kind, FragKind::Text { .. }) { f.x + f.w } else { 0 };
            f.children.iter().map(max_right).fold(own, i32::max)
        }
        assert!(max_right(&l.root) <= 200);
    }

    #[test]
    fn tables_share_columns() {
        let (l, doc) = lay(
            "<style>body{margin:0}</style><table><tr><td>a</td><td>une cellule bien plus longue</td></tr><tr><td colspan=2>deux colonnes</td></tr></table>",
            600,
        );
        let t = find(&l.root, &doc, "table").unwrap();
        assert!(t.w > 150 && t.w < 400, "{}", t.w);
        let text = text_of(&l.root);
        assert_eq!(text, "a une cellule bien plus longue\ndeux colonnes");
    }

    #[test]
    fn flex_rows_grow_and_center() {
        let (l, doc) = lay(
            "<style>body{margin:0} nav{display:flex;justify-content:space-between;align-items:center;height:40px} nav a{padding:0 10px}</style>\
             <nav><a href=#>Un</a><a href=#>Deux</a><a href=#>Trois</a></nav>",
            500,
        );
        let nav = find(&l.root, &doc, "nav").unwrap();
        assert_eq!(nav.h, 40);
        let xs: Vec<i32> = nav.children.iter().map(|c| c.x).collect();
        assert_eq!(xs.len(), 3);
        assert_eq!(xs[0], 0);
        let last = nav.children.last().unwrap();
        assert_eq!(last.x + last.w, 500);
        assert!(nav.children[0].y > 0);
    }

    #[test]
    fn lists_have_markers() {
        let (l, _) = lay("<ol start=3><li>a<li>b</ol><ul><li>c</ul>", 300);
        let mut markers = Vec::new();
        fn walk(f: &Frag, out: &mut Vec<String>) {
            if let FragKind::Marker { text, .. } = &f.kind {
                out.push(text.clone());
            }
            for c in &f.children {
                walk(c, out);
            }
        }
        walk(&l.root, &mut markers);
        assert_eq!(markers, ["3.", "4.", "•"]);
        assert_eq!(marker_text(ListStyle::UpperRoman, 1994).unwrap(), "MCMXCIV.");
        assert_eq!(marker_text(ListStyle::LowerAlpha, 28).unwrap(), "ab.");
    }

    #[test]
    fn absolute_boxes_sit_in_their_container() {
        let (l, doc) = lay(
            "<style>body{margin:0} section{position:relative;height:100px;margin-top:50px} b{position:absolute;right:10px;bottom:0;width:20px;height:20px}</style><section><b></b></section>",
            300,
        );
        let b = find(&l.root, &doc, "b").unwrap();
        assert_eq!((b.x, b.y), (270, 130));
    }

    #[test]
    fn inline_blocks_and_forms_sit_on_lines() {
        let (l, doc) = lay("<form><input name=q value='âne'> <button>Chercher</button></form>", 500);
        let input = find(&l.root, &doc, "input").unwrap();
        let button = find(&l.root, &doc, "button").unwrap();
        assert!(button.x > input.x + input.w);
        assert!((button.y - input.y).abs() < 6);
    }
}
