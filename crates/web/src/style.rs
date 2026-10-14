//! Computed styles: the cascade (the browser's own sheet, attributes such
//! as `bgcolor`, the page's sheets and `style` attributes, `!important`),
//! custom properties and `var()`, inheritance, and the values of the
//! properties this engine lays out and draws.
//!
//! Lengths are kept in thousandths of a pixel ([`Length`]); a percentage
//! stays one until layout knows what it is a percentage of.

use crate::css::{self, Decl, Index, Media, Num, Origin, PseudoElement, Sheet, Tok};
use crate::dom::{Document, NodeData, NodeId};
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

/// A color, `0xAARRGGBB` (255 is opaque).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const BLACK: Color = Color(0xff00_0000);
    pub const WHITE: Color = Color(0xffff_ffff);
    pub const TRANSPARENT: Color = Color(0);

    pub const fn rgb(r: u32, g: u32, b: u32) -> Color {
        Color(0xff00_0000 | r << 16 | g << 8 | b)
    }

    pub fn alpha(self) -> u32 {
        self.0 >> 24
    }

    pub fn with_alpha(self, a: u32) -> Color {
        Color((self.0 & 0x00ff_ffff) | a.min(255) << 24)
    }
}

/// A length: thousandths of a pixel plus thousandths of a percent of
/// what it is measured against (`calc(100% - 2em)` is both), or `auto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Length {
    pub milli: i32,
    pub pct: i32,
    pub auto: bool,
}

impl Length {
    pub const AUTO: Length = Length { milli: 0, pct: 0, auto: true };
    pub const ZERO: Length = Length { milli: 0, pct: 0, auto: false };

    pub const fn px(px: i32) -> Length {
        Length { milli: px * 1000, pct: 0, auto: false }
    }

    /// In pixels, against `basis` pixels; `auto` gives `None`.
    pub fn resolve(self, basis: i32) -> Option<i32> {
        if self.auto {
            return None;
        }
        let v = i64::from(self.milli) + i64::from(basis) * i64::from(self.pct) / 100;
        Some(((v + if v >= 0 { 500 } else { -500 }) / 1000) as i32)
    }

    /// In pixels, `auto` and percentages of an unknown size as 0.
    pub fn or_zero(self, basis: i32) -> i32 {
        self.resolve(basis).unwrap_or(0)
    }

    pub fn has_pct(self) -> bool {
        self.pct != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Table,
    InlineTable,
    TableRowGroup,
    TableRow,
    TableCell,
    TableCaption,
    TableColumn,
    Flex,
    InlineFlex,
    Contents,
}

impl Display {
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::Inline | Display::InlineBlock | Display::InlineTable | Display::InlineFlex)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Float {
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clear {
    None,
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Pre,
    NoWrap,
    PreWrap,
    PreLine,
}

impl WhiteSpace {
    /// Whether runs of spaces are kept.
    pub fn keeps_spaces(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::PreWrap)
    }

    /// Whether line breaks in the text are kept.
    pub fn keeps_newlines(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine)
    }

    /// Whether lines wrap at the box's edge.
    pub fn wraps(self) -> bool {
        !matches!(self, WhiteSpace::Pre | WhiteSpace::NoWrap)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    None,
    Upper,
    Lower,
    Capitalize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyle {
    None,
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Baseline,
    Sub,
    Super,
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineHeight {
    Normal,
    /// A multiple of the font's size, in thousandths.
    Factor(i32),
    Px(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
    Center,
    Stretch,
    Baseline,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

pub const UNDERLINE: u8 = 1;
pub const LINE_THROUGH: u8 = 2;
pub const OVERLINE: u8 = 4;

/// The custom properties (`--name`) in force, shared with descendants
/// until one changes them.
pub type Custom = Rc<BTreeMap<String, Vec<Tok>>>;

#[derive(Clone, Debug)]
pub struct Style {
    pub display: Display,
    pub position: Position,
    pub float: Float,
    pub clear: Clear,
    pub visible: bool,
    pub color: Color,
    pub background: Color,
    /// The first `url()` of `background-image` (drawn when images are).
    pub background_image: Option<String>,
    /// In pixels.
    pub font_size: i32,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub decoration: u8,
    pub decoration_color: Option<Color>,
    pub transform: Transform,
    pub white_space: WhiteSpace,
    pub text_indent: Length,
    pub vertical_align: VAlign,
    /// Top, right, bottom, left.
    pub margin: [Length; 4],
    pub padding: [Length; 4],
    /// In pixels, as declared: drawn only on sides whose style is set
    /// (see [`Style::border`]).
    pub border_width: [i32; 4],
    pub border_color: [Option<Color>; 4],
    pub border_style_set: [bool; 4],
    pub radius: i32,
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub min_height: Length,
    /// `auto` for `none`.
    pub max_width: Length,
    pub max_height: Length,
    pub border_box: bool,
    pub inset: [Length; 4],
    pub overflow_hidden: bool,
    /// 0 to 255.
    pub opacity: u32,
    pub list_style: ListStyle,
    pub list_inside: bool,
    pub flex_row: bool,
    pub flex_reverse: bool,
    pub flex_wrap: bool,
    pub justify: Align,
    pub align_items: Align,
    pub align_self: Option<Align>,
    pub grow: i32,
    pub shrink: i32,
    pub basis: Length,
    pub row_gap: Length,
    pub column_gap: Length,
    pub order: i32,
    pub border_collapse: bool,
    pub border_spacing: i32,
    /// For `::before` and `::after`: the text it adds.
    pub content: Option<String>,
    /// A grid shown as blocks (grid layout is not done yet).
    pub grid: bool,
    pub custom: Custom,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            visible: true,
            color: Color::BLACK,
            background: Color::TRANSPARENT,
            background_image: None,
            font_size: 16,
            bold: false,
            italic: false,
            mono: false,
            line_height: LineHeight::Normal,
            text_align: TextAlign::Left,
            decoration: 0,
            decoration_color: None,
            transform: Transform::None,
            white_space: WhiteSpace::Normal,
            text_indent: Length::ZERO,
            vertical_align: VAlign::Baseline,
            margin: [Length::ZERO; 4],
            padding: [Length::ZERO; 4],
            border_width: [3; 4],
            border_color: [None; 4],
            border_style_set: [false; 4],
            radius: 0,
            width: Length::AUTO,
            height: Length::AUTO,
            min_width: Length::ZERO,
            min_height: Length::ZERO,
            max_width: Length::AUTO,
            max_height: Length::AUTO,
            border_box: false,
            inset: [Length::AUTO; 4],
            overflow_hidden: false,
            opacity: 255,
            list_style: ListStyle::Disc,
            list_inside: false,
            flex_row: true,
            flex_reverse: false,
            flex_wrap: false,
            justify: Align::Start,
            align_items: Align::Stretch,
            align_self: None,
            grow: 0,
            shrink: 1000,
            basis: Length::AUTO,
            row_gap: Length::ZERO,
            column_gap: Length::ZERO,
            order: 0,
            border_collapse: false,
            border_spacing: 0,
            content: None,
            grid: false,
            custom: Rc::new(BTreeMap::new()),
        }
    }
}

impl Style {
    /// What a child inherits: the inherited properties, the others back to
    /// their initial values.
    pub fn inherit(&self) -> Style {
        Style {
            color: self.color,
            font_size: self.font_size,
            bold: self.bold,
            italic: self.italic,
            mono: self.mono,
            line_height: self.line_height,
            text_align: self.text_align,
            transform: self.transform,
            white_space: self.white_space,
            text_indent: self.text_indent,
            visible: self.visible,
            list_style: self.list_style,
            list_inside: self.list_inside,
            border_collapse: self.border_collapse,
            border_spacing: self.border_spacing,
            custom: self.custom.clone(),
            ..Style::default()
        }
    }

    /// The height of a line, in pixels.
    pub fn line_height_px(&self) -> i32 {
        match self.line_height {
            LineHeight::Normal => {
                let face = hln_type::face(self.mono, self.bold, self.font_size);
                face.height * self.font_size / face.size.max(1)
            }
            LineHeight::Factor(f) => (self.font_size as i64 * i64::from(f) / 1000) as i32,
            LineHeight::Px(p) => p,
        }
        .max(1)
    }

    pub fn border_color(&self, side: usize) -> Color {
        self.border_color[side].unwrap_or(self.color)
    }
}

// ---------------------------------------------------------------- values

/// What lengths are measured against: the element's font, the root's,
/// the viewport.
#[derive(Clone, Copy)]
pub struct Units {
    pub font: i32,
    pub root_font: i32,
    pub vw: i32,
    pub vh: i32,
}

fn trim(toks: &[Tok]) -> &[Tok] {
    let mut t = toks;
    while t.first() == Some(&Tok::Ws) {
        t = &t[1..];
    }
    while t.last() == Some(&Tok::Ws) {
        t = &t[..t.len() - 1];
    }
    t
}

/// A value's parts: each token or function call (with its arguments),
/// split at white space; commas are parts of their own.
pub fn parts(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Ws => i += 1,
            Tok::Function(_) | Tok::LParen | Tok::LBracket => {
                let mut depth = 0;
                let start = i;
                while i < toks.len() {
                    match toks[i] {
                        Tok::Function(_) | Tok::LParen | Tok::LBracket => depth += 1,
                        Tok::RParen | Tok::RBracket => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                out.push(&toks[start..(i + 1).min(toks.len())]);
                i += 1;
            }
            _ => {
                out.push(&toks[i..i + 1]);
                i += 1;
            }
        }
    }
    out
}

/// The arguments of a function call part, split at top-level commas.
fn args(part: &[Tok]) -> Vec<&[Tok]> {
    let inner = if part.len() >= 2 { &part[1..part.len() - 1] } else { &[][..] };
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, t) in inner.iter().enumerate() {
        match t {
            Tok::Function(_) | Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Comma if depth == 0 => {
                out.push(trim(&inner[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(trim(&inner[start..]));
    out
}

fn ident(part: &[Tok]) -> Option<String> {
    match part {
        [Tok::Ident(s)] => Some(s.to_ascii_lowercase()),
        _ => None,
    }
}

fn mul(n: Num, k: i64) -> i32 {
    (n * k / 1000).clamp(-(1 << 30), 1 << 30) as i32
}

/// A length part: `12px`, `1.5em`, `50%`, `0`, `calc(...)`, `auto`.
pub fn length(part: &[Tok], u: &Units) -> Option<Length> {
    match part {
        [Tok::Ident(s)] if s.eq_ignore_ascii_case("auto") => Some(Length::AUTO),
        [Tok::Number(0)] => Some(Length::ZERO),
        [Tok::Number(n)] => Some(Length { milli: (*n).clamp(-(1 << 30), 1 << 30) as i32, pct: 0, auto: false }),
        [Tok::Percentage(n)] => Some(Length { milli: 0, pct: (*n).clamp(-(1 << 30), 1 << 30) as i32, auto: false }),
        [Tok::Dimension(n, unit)] => {
            let milli = match unit.as_str() {
                "px" => mul(*n, 1000),
                "em" => mul(*n, i64::from(u.font) * 1000),
                "rem" => mul(*n, i64::from(u.root_font) * 1000),
                "ex" => mul(*n, i64::from(u.font) * 500),
                "ch" => mul(*n, i64::from(u.font) * 550),
                "pt" => mul(*n, 4000) / 3,
                "pc" => mul(*n, 16000),
                "in" => mul(*n, 96000),
                "cm" => mul(*n, 37795),
                "mm" => mul(*n, 3780),
                "q" => mul(*n, 945),
                "vw" | "svw" | "lvw" | "dvw" => mul(*n, i64::from(u.vw) * 10),
                "vh" | "svh" | "lvh" | "dvh" => mul(*n, i64::from(u.vh) * 10),
                "vmin" => mul(*n, i64::from(u.vw.min(u.vh)) * 10),
                "vmax" => mul(*n, i64::from(u.vw.max(u.vh)) * 10),
                _ => return None,
            };
            Some(Length { milli, pct: 0, auto: false })
        }
        [Tok::Function(f), ..] => {
            let a = args(part);
            match f.as_str() {
                "calc" | "-webkit-calc" => calc(a.first()?, u),
                "min" | "max" => {
                    let values: Vec<Length> = a.iter().filter_map(|x| calc(x, u)).collect();
                    if values.is_empty() {
                        return None;
                    }
                    // Without a basis, percentages cannot be compared: the
                    // first is kept when any has one.
                    if values.iter().any(|v| v.has_pct()) {
                        return values.first().copied();
                    }
                    let pick = if f == "min" { values.iter().min_by_key(|v| v.milli) } else { values.iter().max_by_key(|v| v.milli) };
                    pick.copied()
                }
                "clamp" => {
                    let [lo, mid, hi] = a.as_slice() else { return None };
                    let (lo, mid, hi) = (calc(lo, u)?, calc(mid, u)?, calc(hi, u)?);
                    if lo.has_pct() || mid.has_pct() || hi.has_pct() {
                        return Some(mid);
                    }
                    Some(Length { milli: mid.milli.min(hi.milli).max(lo.milli), pct: 0, auto: false })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `calc()`'s expression: sums and differences of lengths, products and
/// quotients by numbers.
fn calc(toks: &[Tok], u: &Units) -> Option<Length> {
    let ps = parts(toks);
    let mut i = 0;
    let (v, _) = calc_sum(&ps, &mut i, u)?;
    Some(v)
}

/// A value of calc: a length, or a bare number (`is_number`).
fn calc_sum(ps: &[&[Tok]], i: &mut usize, u: &Units) -> Option<(Length, bool)> {
    let (mut acc, mut num) = calc_product(ps, i, u)?;
    while *i < ps.len() {
        let sign = match ps[*i] {
            [Tok::Delim('+')] => 1,
            [Tok::Delim('-')] => -1,
            _ => break,
        };
        *i += 1;
        let (v, n) = calc_product(ps, i, u)?;
        acc.milli += sign * v.milli;
        acc.pct += sign * v.pct;
        num &= n;
    }
    Some((acc, num))
}

fn calc_product(ps: &[&[Tok]], i: &mut usize, u: &Units) -> Option<(Length, bool)> {
    let (mut acc, mut num) = calc_value(ps, i, u)?;
    while *i < ps.len() {
        let op = match ps[*i] {
            [Tok::Delim('*')] => '*',
            [Tok::Delim('/')] => '/',
            _ => break,
        };
        *i += 1;
        let (v, n) = calc_value(ps, i, u)?;
        if op == '*' {
            if n {
                acc.milli = (i64::from(acc.milli) * i64::from(v.milli) / 1000) as i32;
                acc.pct = (i64::from(acc.pct) * i64::from(v.milli) / 1000) as i32;
            } else if num {
                let k = acc.milli;
                acc = Length {
                    milli: (i64::from(v.milli) * i64::from(k) / 1000) as i32,
                    pct: (i64::from(v.pct) * i64::from(k) / 1000) as i32,
                    auto: false,
                };
                num = false;
            } else {
                return None;
            }
        } else {
            if !n || v.milli == 0 {
                return None;
            }
            acc.milli = (i64::from(acc.milli) * 1000 / i64::from(v.milli)) as i32;
            acc.pct = (i64::from(acc.pct) * 1000 / i64::from(v.milli)) as i32;
        }
    }
    Some((acc, num))
}

fn calc_value(ps: &[&[Tok]], i: &mut usize, u: &Units) -> Option<(Length, bool)> {
    let part = *ps.get(*i)?;
    *i += 1;
    match part {
        [Tok::Number(n)] => Some((Length { milli: *n as i32, pct: 0, auto: false }, true)),
        [Tok::LParen, .., Tok::RParen] => calc(&part[1..part.len() - 1], u).map(|l| (l, false)),
        _ => length(part, u).map(|l| (l, false)),
    }
}

/// The named colors of CSS.
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
    // System colors, as browsers draw them on a light page.
    ("canvas", 0xffffff),
    ("canvastext", 0x000000),
    ("buttonface", 0xefefef),
    ("buttontext", 0x000000),
    ("linktext", 0x0b57d0),
    ("graytext", 0x6d6d6d),
    ("field", 0xffffff),
    ("fieldtext", 0x000000),
    ("highlight", 0x3390ff),
    ("highlighttext", 0xffffff),
    ("window", 0xffffff),
    ("windowtext", 0x000000),
];

fn hex_digit(c: u8) -> Option<u32> {
    (c as char).to_digit(16)
}

/// A color component: a number 0–255 or a percentage, in 0–255.
fn channel(t: &Tok) -> Option<u32> {
    match t {
        Tok::Number(n) => Some((n / 1000).clamp(0, 255) as u32),
        Tok::Percentage(p) => Some((p * 255 / 100_000).clamp(0, 255) as u32),
        Tok::Ident(s) if s == "none" => Some(0),
        _ => None,
    }
}

/// An alpha component: a number 0–1 or a percentage, in 0–255.
fn alpha(t: &Tok) -> Option<u32> {
    match t {
        Tok::Number(n) => Some((n * 255 / 1000).clamp(0, 255) as u32),
        Tok::Percentage(p) => Some((p * 255 / 100_000).clamp(0, 255) as u32),
        _ => None,
    }
}

fn hue_to(m1: i64, m2: i64, h: i64) -> i64 {
    // h in thousandths of a degree.
    const TURN: i64 = 360_000;
    let h = h.rem_euclid(TURN);
    if h * 6 < TURN {
        m1 + (m2 - m1) * h * 6 / TURN
    } else if h * 2 < TURN {
        m2
    } else if h * 3 < 2 * TURN {
        m1 + (m2 - m1) * (2 * TURN / 3 - h) * 6 / TURN
    } else {
        m1
    }
}

/// A color part; `None` for what is not one (`currentcolor` included).
pub fn color(part: &[Tok]) -> Option<Color> {
    match part {
        [Tok::Ident(name)] => {
            let name = name.to_ascii_lowercase();
            if name == "transparent" {
                return Some(Color::TRANSPARENT);
            }
            NAMED.iter().find(|(n, _)| *n == name).map(|(_, v)| Color(0xff00_0000 | v))
        }
        [Tok::Hash(h, _)] => {
            let b = h.as_bytes();
            let d: Option<Vec<u32>> = b.iter().map(|&c| hex_digit(c)).collect();
            let d = d?;
            let (r, g, bl, a) = match d.len() {
                3 => (d[0] * 17, d[1] * 17, d[2] * 17, 255),
                4 => (d[0] * 17, d[1] * 17, d[2] * 17, d[3] * 17),
                6 => (d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5], 255),
                8 => (d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5], d[6] * 16 + d[7]),
                _ => return None,
            };
            Some(Color(a << 24 | r << 16 | g << 8 | bl))
        }
        [Tok::Function(f), ..] => {
            let inner: Vec<&Tok> =
                part[1..part.len() - 1].iter().filter(|t| !matches!(t, Tok::Ws | Tok::Comma | Tok::Delim('/'))).collect();
            match f.as_str() {
                "rgb" | "rgba" => {
                    let (r, g, b) = (channel(inner.first()?)?, channel(inner.get(1)?)?, channel(inner.get(2)?)?);
                    let a = inner.get(3).map_or(Some(255), |t| alpha(t))?;
                    Some(Color(a << 24 | r << 16 | g << 8 | b))
                }
                "hsl" | "hsla" => {
                    let h = match inner.first()? {
                        Tok::Number(n) => *n,
                        Tok::Dimension(n, u) if u == "deg" => *n,
                        Tok::Dimension(n, u) if u == "turn" => n * 360,
                        Tok::Dimension(n, u) if u == "rad" => n * 57_296 / 1000,
                        _ => return None,
                    };
                    let pct = |t: &Tok| match t {
                        Tok::Percentage(p) => Some(p / 100),
                        Tok::Number(n) => Some(n / 100),
                        _ => None,
                    };
                    let (s, l) = (pct(inner.get(1)?)?.clamp(0, 1000), pct(inner.get(2)?)?.clamp(0, 1000));
                    let a = inner.get(3).map_or(Some(255), |t| alpha(t))?;
                    let m2 = if l <= 500 { l * (1000 + s) / 1000 } else { l + s - l * s / 1000 };
                    let m1 = 2 * l - m2;
                    let to = |v: i64| (v * 255 / 1000).clamp(0, 255) as u32;
                    let (r, g, b) =
                        (to(hue_to(m1, m2, h + 120_000)), to(hue_to(m1, m2, h)), to(hue_to(m1, m2, h - 120_000)));
                    Some(Color(a << 24 | r << 16 | g << 8 | b))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- agent

/// The browser's own style sheet.
pub const AGENT: &str = r#"
html, address, blockquote, body, dd, div, dl, dt, fieldset, form, frame, frameset, h1, h2, h3, h4, h5, h6,
noframes, ol, p, ul, center, dir, hr, menu, pre, header, footer, nav, article, section, aside, main, figure,
figcaption, hgroup, details, summary, legend, search, listing, xmp, plaintext, optgroup { display: block }
head, script, style, title, meta, link, base, template, datalist, area, param, source, track, noembed,
noframes, [hidden], input[type=hidden], svg *, rp, object param, select option { display: none }
li { display: list-item }
table { display: table; border-spacing: 2px; border-collapse: separate; box-sizing: border-box }
caption { display: table-caption; text-align: center }
thead, tbody, tfoot { display: table-row-group; vertical-align: middle }
tr { display: table-row }
td, th { display: table-cell; padding: 1px; vertical-align: middle }
th { font-weight: bold; text-align: center }
col { display: table-column }
colgroup { display: table-column }
body { margin: 8px }
h1 { font-size: 2em; margin: .67em 0; font-weight: bold }
h2 { font-size: 1.5em; margin: .83em 0; font-weight: bold }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }
h4 { margin: 1.33em 0; font-weight: bold }
h5 { font-size: .83em; margin: 1.67em 0; font-weight: bold }
h6 { font-size: .67em; margin: 2.33em 0; font-weight: bold }
p, dl, listing, xmp, plaintext { margin: 1em 0 }
blockquote, figure { margin: 1em 40px }
ul, ol, menu, dir { margin: 1em 0; padding-left: 40px }
ul, menu, dir { list-style-type: disc }
ol { list-style-type: decimal }
ul ul, ol ul, menu ul { list-style-type: circle }
ul ul ul, ol ul ul, ol ol ul, ul ol ul { list-style-type: square }
ul ul, ul ol, ol ul, ol ol { margin-top: 0; margin-bottom: 0 }
dd { margin-left: 40px }
b, strong { font-weight: bold }
i, em, cite, var, dfn, address { font-style: italic }
code, kbd, samp, pre, tt, listing, xmp, plaintext { font-family: monospace }
pre, listing, xmp, plaintext { white-space: pre; margin: 1em 0 }
a:link { color: #0b57d0; text-decoration: underline }
u, ins { text-decoration: underline }
s, strike, del { text-decoration: line-through }
abbr[title], acronym[title] { text-decoration: underline }
small { font-size: smaller }
big { font-size: larger }
sub { vertical-align: sub; font-size: smaller }
sup { vertical-align: super; font-size: smaller }
hr { border: 0; border-top: 1px solid #c4c8cf; margin: .5em 0; color: gray }
center { text-align: center }
mark { background-color: #ffe066; color: black }
nobr { white-space: nowrap }
img, video, canvas, iframe, embed, object, svg, input, button, select, textarea, meter, progress { display: inline-block }
iframe { border: 2px inset #ccc; width: 300px; height: 150px }
video, canvas { width: 300px; height: 150px }
input, select, textarea, button { font-size: 13px; color: black; font-weight: normal; font-style: normal; line-height: normal; text-align: left; text-indent: 0; text-transform: none; white-space: nowrap }
input, textarea, select { border: 1px solid #8f8f9d; background-color: white; padding: 2px 4px; border-radius: 3px }
input { width: 150px }
input[size] { width: auto }
button, input[type=submit], input[type=button], input[type=reset], input[type=image] { border: 1px solid #8f8f9d; background-color: #e9e9ed; padding: 2px 8px; border-radius: 4px; width: auto; text-align: center }
input[type=checkbox], input[type=radio] { width: 13px; height: 13px; padding: 0; margin: 3px 4px; border-radius: 2px }
input[type=radio] { border-radius: 7px }
textarea { white-space: pre-wrap; width: 300px; height: 40px; font-family: monospace; vertical-align: text-bottom }
select { padding-right: 20px }
fieldset { margin: 0 2px; padding: .35em .75em .625em; border: 2px groove #c0c0c0 }
legend { padding: 0 2px }
summary { display: list-item; list-style-type: disclosure-closed }
details[open] > summary { list-style-type: disclosure-open }
details:not([open]) > :not(summary) { display: none }
dialog:not([open]) { display: none }
q::before { content: "“" }
q::after { content: "”" }
ruby { display: inline }
rt { font-size: 50% }
"#;

// ---------------------------------------------------------------- cascade

/// A computed style for each node (text nodes get their parent's), and the
/// styles of `::before` and `::after` where there are some.
pub struct Styles {
    pub nodes: Vec<Option<Rc<Style>>>,
    pub before: BTreeMap<NodeId, Rc<Style>>,
    pub after: BTreeMap<NodeId, Rc<Style>>,
}

impl Styles {
    pub fn get(&self, node: NodeId) -> Option<&Rc<Style>> {
        self.nodes.get(node).and_then(Option::as_ref)
    }
}

/// The page's sheets: `<style>` and loaded `<link>` texts, in document
/// order, read against `media`.
pub fn compute(doc: &Document, author: &[Sheet], media: &Media) -> Styles {
    let mut index = Index::default();
    index.add(&css::parse_sheet(AGENT, media), Origin::Agent);
    for sheet in author {
        index.add(sheet, Origin::Author);
    }
    let mut styles =
        Styles { nodes: alloc::vec![None; doc.nodes.len()], before: BTreeMap::new(), after: BTreeMap::new() };
    let root = Rc::new(Style { display: Display::Block, ..Style::default() });
    let mut root_font = 16;
    // Parents come before their children in the array, except nodes the
    // tree builder moved: walk the tree.
    let mut stack: Vec<(NodeId, Rc<Style>)> = alloc::vec![(crate::dom::DOCUMENT, root)];
    while let Some((node, parent)) = stack.pop() {
        let style = match &doc.nodes[node].data {
            NodeData::Element(_) => {
                let units = Units { font: parent.font_size, root_font, vw: media.width, vh: media.height };
                let s = Rc::new(element_style(doc, node, &parent, &index, units, None));
                if doc.is(node, "html") {
                    root_font = s.font_size;
                }
                for which in [PseudoElement::Before, PseudoElement::After] {
                    let units = Units { font: s.font_size, ..units };
                    let p = element_style(doc, node, &s, &index, units, Some(which));
                    if p.content.is_some() && p.display != Display::None {
                        let map = if which == PseudoElement::Before { &mut styles.before } else { &mut styles.after };
                        map.insert(node, Rc::new(p));
                    }
                }
                s
            }
            _ => parent.clone(),
        };
        for &child in doc.nodes[node].children.iter().rev() {
            stack.push((child, style.clone()));
        }
        styles.nodes[node] = Some(style);
    }
    styles
}

/// Declarations from attributes (`bgcolor`, `width`, `align`...), between
/// the browser's sheet and the page's.
fn hints(doc: &Document, node: NodeId) -> Vec<Decl> {
    let Some(e) = doc.element(node) else { return Vec::new() };
    let mut text = String::new();
    let mut push = |name: &str, value: &str| {
        text.push_str(name);
        text.push(':');
        text.push_str(value);
        text.push(';');
    };
    let dim = |v: &str| -> Option<String> {
        let v = v.trim();
        let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        Some(if v[digits.len()..].starts_with('%') { alloc::format!("{digits}%") } else { alloc::format!("{digits}px") })
    };
    if let Some(c) = e.attr("bgcolor") {
        push("background-color", &legacy_color(c));
    }
    if let Some(c) = e.attr("color").filter(|_| e.name == "font") {
        push("color", &legacy_color(c));
    }
    if let Some(c) = e.attr("text").filter(|_| e.name == "body") {
        push("color", &legacy_color(c));
    }
    if matches!(e.name.as_str(), "img" | "table" | "td" | "th" | "col" | "iframe" | "video" | "canvas" | "svg" | "hr" | "object" | "embed" | "input")
    {
        if let Some(w) = e.attr("width").and_then(dim) {
            push("width", &w);
        }
        if let Some(h) = e.attr("height").and_then(dim) {
            push("height", &h);
        }
    }
    if let Some(a) = e.attr("align") {
        let a = a.to_ascii_lowercase();
        match (e.name.as_str(), a.as_str()) {
            ("img" | "table" | "iframe", "left") => push("float", "left"),
            ("img" | "table" | "iframe", "right") => push("float", "right"),
            ("table", "center") => push("margin", "0 auto"),
            ("img", _) => {}
            (_, "left" | "right" | "center" | "justify") => push("text-align", &a),
            _ => {}
        }
    }
    if let Some(v) = e.attr("valign") {
        push("vertical-align", v);
    }
    if e.name == "table" {
        if let Some(b) = e.attr("border") {
            let w = b.trim().parse::<i32>().unwrap_or(1);
            if w > 0 {
                push("border", &alloc::format!("{w}px outset gray"));
            }
        }
        if let Some(s) = e.attr("cellspacing").and_then(dim) {
            push("border-spacing", &s);
        }
    }
    if matches!(e.name.as_str(), "td" | "th") {
        if let Some(table) = doc.closest(node, "table").and_then(|t| doc.element(t)) {
            if let Some(p) = table.attr("cellpadding").and_then(dim) {
                push("padding", &p);
            }
            if table.attr("border").is_some_and(|b| b.trim().parse::<i32>().unwrap_or(1) > 0) {
                push("border", "1px inset gray");
            }
        }
        if e.attr("nowrap").is_some() {
            push("white-space", "nowrap");
        }
    }
    if e.name == "font"
        && let Some(s) = e.attr("size")
    {
        let n: i32 = s.trim().trim_start_matches('+').parse().unwrap_or(3);
        let n = if s.trim().starts_with(['+', '-']) { 3 + if s.contains('-') { -n.abs() } else { n } } else { n };
        let size = ["10px", "13px", "16px", "18px", "24px", "32px", "48px"][(n.clamp(1, 7) - 1) as usize];
        push("font-size", size);
    }
    if e.name == "font"
        && let Some(f) = e.attr("face")
    {
        push("font-family", f);
    }
    if e.name == "input"
        && let Some(s) = e.attr("size").and_then(|s| s.trim().parse::<i32>().ok())
    {
        push("width", &alloc::format!("{}px", s.clamp(1, 200) * 8 + 8));
    }
    if e.name == "textarea" {
        if let Some(c) = e.attr("cols").and_then(|s| s.trim().parse::<i32>().ok()) {
            push("width", &alloc::format!("{}px", c.clamp(1, 300) * 8 + 8));
        }
        if let Some(r) = e.attr("rows").and_then(|s| s.trim().parse::<i32>().ok()) {
            push("height", &alloc::format!("{}px", r.clamp(1, 200) * 16 + 6));
        }
    }
    if e.name == "ol"
        && let Some(t) = e.attr("type")
    {
        let kind = match t {
            "a" => "lower-alpha",
            "A" => "upper-alpha",
            "i" => "lower-roman",
            "I" => "upper-roman",
            _ => "decimal",
        };
        push("list-style-type", kind);
    }
    if e.name == "body" {
        for (attr, prop) in [("leftmargin", "margin-left"), ("topmargin", "margin-top"), ("marginwidth", "margin-left")] {
            if let Some(v) = e.attr(attr).and_then(dim) {
                push(prop, &v);
            }
        }
    }
    css::parse_declarations(&css::tokenize(&text))
}

/// A color as old attributes write it (`#fff`, `fff`, `red`).
fn legacy_color(c: &str) -> String {
    let c = c.trim();
    if c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()) { alloc::format!("#{c}") } else { String::from(c) }
}

fn element_style(
    doc: &Document,
    node: NodeId,
    parent: &Style,
    index: &Index,
    units: Units,
    pseudo: Option<PseudoElement>,
) -> Style {
    let mut s = parent.inherit();
    if pseudo.is_some() {
        s.display = Display::Inline;
    }
    let matched: Vec<&css::Indexed> =
        index.matching(doc, node).into_iter().filter(|m| m.selector.pseudo_element == pseudo).collect();
    if pseudo.is_some() && matched.is_empty() {
        return s;
    }
    let hints = if pseudo.is_none() { hints(doc, node) } else { Vec::new() };
    let inline: Vec<Decl> = match (pseudo, doc.element(node).and_then(|e| e.attr("style"))) {
        (None, Some(text)) => css::parse_declarations(&css::tokenize(text)),
        _ => Vec::new(),
    };
    // Cascade order, normal then important.
    let mut decls: Vec<&Decl> = Vec::new();
    let agent = matched.iter().filter(|m| m.origin == Origin::Agent);
    let author = matched.iter().filter(|m| m.origin == Origin::Author);
    for m in agent.clone() {
        decls.extend(m.rule.iter().filter(|d| !d.important));
    }
    decls.extend(hints.iter());
    for m in author.clone() {
        decls.extend(m.rule.iter().filter(|d| !d.important));
    }
    decls.extend(inline.iter().filter(|d| !d.important));
    for m in author {
        decls.extend(m.rule.iter().filter(|d| d.important));
    }
    decls.extend(inline.iter().filter(|d| d.important));
    for m in agent {
        decls.extend(m.rule.iter().filter(|d| d.important));
    }
    // Custom properties first: other values may use them.
    if decls.iter().any(|d| d.name.starts_with("--")) {
        let mut custom = (*s.custom).clone();
        for d in decls.iter().filter(|d| d.name.starts_with("--")) {
            custom.insert(d.name.clone(), d.value.clone());
        }
        s.custom = Rc::new(custom);
    }
    // The font's size next: `em` lengths depend on it.
    let mut units = units;
    for d in decls.iter().filter(|d| matches!(d.name.as_str(), "font-size" | "font")) {
        let value = substitute(&d.value, &s.custom);
        apply(&mut s, parent, &d.name, &value, &units);
    }
    units.font = s.font_size;
    for d in decls.iter().filter(|d| !d.name.starts_with("--") && d.name != "font-size") {
        let value = substitute(&d.value, &s.custom);
        apply(&mut s, parent, &d.name, &value, &units);
    }
    fixup(&mut s, parent, doc, node, pseudo.is_some());
    s
}

/// Corrections every browser makes once the values are known.
fn fixup(s: &mut Style, parent: &Style, doc: &Document, node: NodeId, pseudo: bool) {
    // Floated and absolutely positioned boxes are blocks.
    if s.float != Float::None || matches!(s.position, Position::Absolute | Position::Fixed) {
        s.display = match s.display {
            Display::Inline | Display::InlineBlock | Display::TableCell | Display::TableRow | Display::TableRowGroup => {
                Display::Block
            }
            Display::InlineTable => Display::Table,
            Display::InlineFlex => Display::Flex,
            d => d,
        };
        if matches!(s.position, Position::Absolute | Position::Fixed) {
            s.float = Float::None;
        }
    }
    if doc.is(node, "html") && !pseudo {
        s.display = if s.display == Display::None { Display::None } else { Display::Block };
    }
    let _ = parent;
}

/// `value` with each `var(--name, fallback)` replaced.
fn substitute(value: &[Tok], custom: &Custom) -> Vec<Tok> {
    if !value.iter().any(|t| matches!(t, Tok::Function(f) if f == "var")) {
        return value.to_vec();
    }
    let mut out = Vec::with_capacity(value.len());
    let mut i = 0;
    let mut depth = 0;
    while i < value.len() {
        if let Tok::Function(f) = &value[i]
            && f == "var"
        {
            // Its arguments, up to the matching ')'.
            let mut j = i + 1;
            let mut d = 1;
            while j < value.len() {
                match value[j] {
                    Tok::Function(_) | Tok::LParen => d += 1,
                    Tok::RParen => {
                        d -= 1;
                        if d == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let inner = trim(&value[i + 1..j.min(value.len())]);
            let comma = inner.iter().position(|t| *t == Tok::Comma);
            let name = match inner.first() {
                Some(Tok::Ident(n)) => n.as_str(),
                _ => "",
            };
            let replacement = match custom.get(name) {
                Some(v) if depth < 8 => substitute(v, custom),
                _ => comma.map(|c| substitute(trim(&inner[c + 1..]), custom)).unwrap_or_default(),
            };
            out.extend(replacement);
            i = j + 1;
            depth += 1;
            continue;
        }
        out.push(value[i].clone());
        i += 1;
    }
    out
}

/// The four values of a box shorthand (`margin: 1px 2px`).
fn four<T: Copy>(values: &[T]) -> Option<[T; 4]> {
    Some(match values {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return None,
    })
}

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

fn border_width(part: &[Tok], u: &Units) -> Option<i32> {
    match ident(part).as_deref() {
        Some("thin") => Some(1),
        Some("medium") => Some(3),
        Some("thick") => Some(5),
        _ => {
            let l = length(part, u)?;
            if l.auto || l.has_pct() {
                return None;
            }
            l.resolve(0).map(|w| if w == 0 && l.milli > 0 { 1 } else { w })
        }
    }
}

fn is_border_style(word: &str) -> Option<bool> {
    match word {
        "none" | "hidden" => Some(false),
        "solid" | "dotted" | "dashed" | "double" | "groove" | "ridge" | "inset" | "outset" => Some(true),
        _ => None,
    }
}

/// `border: 1px solid red` and its sides' shorthands.
fn border_shorthand(s: &mut Style, sides: &[usize], ps: &[&[Tok]], u: &Units) {
    let (mut width, mut style, mut col) = (None, None, None);
    for p in ps {
        if let Some(w) = ident(p).as_deref().and_then(is_border_style) {
            style = Some(w);
        } else if let Some(c) = color(p) {
            col = Some(c);
        } else if let Some(w) = border_width(p, u) {
            width = Some(w);
        } else if ident(p).as_deref() == Some("currentcolor") {
            col = None;
        }
    }
    for &side in sides {
        s.border_style_set[side] = style.unwrap_or(false);
        s.border_width[side] = width.unwrap_or(3);
        s.border_color[side] = col;
    }
}

fn font_family(s: &mut Style, value: &[Tok]) {
    let text: String = value
        .iter()
        .map(|t| match t {
            Tok::Ident(w) | Tok::Str(w) => w.to_ascii_lowercase(),
            Tok::Comma => String::from(","),
            _ => String::from(" "),
        })
        .collect();
    let first = text.split(',').map(str::trim).find(|f| !f.is_empty()).unwrap_or("");
    s.mono = ["mono", "courier", "consol", "menlo", "code", "fixed", "terminal"].iter().any(|m| first.contains(m));
}

fn font_size(part: &[Tok], parent: &Style, u: &Units) -> Option<i32> {
    let base = parent.font_size;
    if let Some(word) = ident(part) {
        return Some(match word.as_str() {
            "xx-small" => 9,
            "x-small" => 10,
            "small" => 13,
            "medium" => 16,
            "large" => 18,
            "x-large" => 24,
            "xx-large" => 32,
            "xxx-large" => 48,
            "smaller" => base * 5 / 6,
            "larger" => base * 6 / 5,
            _ => return None,
        });
    }
    let u = Units { font: base, ..*u };
    let l = length(part, &u)?;
    if l.auto {
        return None;
    }
    l.resolve(base).map(|v| v.clamp(1, 400))
}

fn weight(part: &[Tok], parent: &Style) -> Option<bool> {
    match part {
        [Tok::Number(n)] => Some(*n >= 600_000),
        _ => match ident(part)?.as_str() {
            "bold" | "bolder" => Some(true),
            "normal" | "lighter" => Some(false),
            "inherit" => Some(parent.bold),
            _ => None,
        },
    }
}

fn align(word: &str) -> Option<Align> {
    Some(match word {
        "flex-start" | "start" | "left" | "self-start" | "normal" => Align::Start,
        "flex-end" | "end" | "right" | "self-end" => Align::End,
        "center" => Align::Center,
        "stretch" => Align::Stretch,
        "baseline" | "first" | "last" => Align::Baseline,
        "space-between" => Align::SpaceBetween,
        "space-around" => Align::SpaceAround,
        "space-evenly" => Align::SpaceEvenly,
        _ => return None,
    })
}

fn content(value: &[Tok]) -> Option<String> {
    let mut out = String::new();
    for t in value {
        match t {
            Tok::Str(s) => out.push_str(s),
            Tok::Ident(w) => match w.as_str() {
                "none" | "normal" => return None,
                "open-quote" => out.push('“'),
                "close-quote" => out.push('”'),
                _ => {}
            },
            _ => {}
        }
    }
    Some(out)
}

/// Applies one declaration.
fn apply(s: &mut Style, parent: &Style, name: &str, value: &[Tok], u: &Units) {
    let ps = parts(value);
    if ps.is_empty() {
        return;
    }
    let word = ident(ps[0]);
    let single = ps.len() == 1;
    // `inherit`, `initial`, `unset`, `revert` for any property.
    if single && let Some(w) = word.as_deref() {
        let initial = Style::default();
        let from = match w {
            "inherit" => Some(parent),
            "initial" => Some(&initial),
            "unset" | "revert" | "revert-layer" => {
                if matches!(
                    name,
                    "color" | "font-size" | "font-weight" | "font-style" | "font-family" | "line-height" | "text-align"
                        | "text-transform" | "white-space" | "visibility" | "list-style-type"
                ) {
                    Some(parent)
                } else {
                    Some(&initial)
                }
            }
            _ => None,
        };
        if let Some(from) = from {
            copy_property(s, from, name);
            return;
        }
    }
    let len = |i: usize| ps.get(i).and_then(|p| length(p, u));
    match name {
        "display" => {
            let words: Vec<String> = ps.iter().filter_map(|p| ident(p)).collect();
            let has = |w: &str| words.iter().any(|x| x == w);
            s.grid = has("grid") || has("inline-grid");
            s.display = if has("none") {
                Display::None
            } else if has("contents") {
                Display::Contents
            } else if has("list-item") {
                Display::ListItem
            } else if has("inline-block") || has("inline-grid") || has("inline") && (has("flow-root") || has("grid")) {
                Display::InlineBlock
            } else if has("inline-flex") || has("inline") && has("flex") {
                Display::InlineFlex
            } else if has("inline-table") {
                Display::InlineTable
            } else if has("inline") || has("ruby") {
                Display::Inline
            } else if has("flex") {
                Display::Flex
            } else if has("table") {
                Display::Table
            } else if has("table-row") {
                Display::TableRow
            } else if has("table-cell") {
                Display::TableCell
            } else if has("table-row-group") || has("table-header-group") || has("table-footer-group") {
                Display::TableRowGroup
            } else if has("table-caption") {
                Display::TableCaption
            } else if has("table-column") || has("table-column-group") {
                Display::TableColumn
            } else if has("block") || has("flow-root") || has("grid") || has("flow") || has("run-in") {
                Display::Block
            } else {
                return;
            };
        }
        "position" => {
            s.position = match word.as_deref() {
                Some("relative") | Some("sticky") | Some("-webkit-sticky") => Position::Relative,
                Some("absolute") => Position::Absolute,
                Some("fixed") => Position::Fixed,
                Some("static") => Position::Static,
                _ => return,
            }
        }
        "float" => {
            s.float = match word.as_deref() {
                Some("left") | Some("inline-start") => Float::Left,
                Some("right") | Some("inline-end") => Float::Right,
                Some("none") => Float::None,
                _ => return,
            }
        }
        "clear" => {
            s.clear = match word.as_deref() {
                Some("left") => Clear::Left,
                Some("right") => Clear::Right,
                Some("both") => Clear::Both,
                Some("none") => Clear::None,
                _ => return,
            }
        }
        "visibility" => s.visible = !matches!(word.as_deref(), Some("hidden") | Some("collapse")),
        "color" => {
            if word.as_deref() == Some("currentcolor") {
                s.color = parent.color;
            } else if let Some(c) = color(ps[0]) {
                s.color = c;
            }
        }
        "background-color" => {
            if word.as_deref() == Some("currentcolor") {
                s.background = s.color;
            } else if let Some(c) = color(ps[0]) {
                s.background = c;
            }
        }
        "background" => {
            // The last layer's color, and the first image.
            let mut col = Color::TRANSPARENT;
            for p in &ps {
                if let Some(c) = color(p) {
                    col = c;
                } else if ident(p).as_deref() == Some("currentcolor") {
                    col = s.color;
                }
            }
            s.background = col;
            s.background_image = ps.iter().find_map(|p| match p {
                [Tok::Url(url)] => Some(url.clone()),
                _ => None,
            });
        }
        "background-image" => {
            s.background_image = ps.iter().find_map(|p| match p {
                [Tok::Url(url)] => Some(url.clone()),
                _ => None,
            })
        }
        "font-size" => {
            if let Some(px) = font_size(ps[0], parent, u) {
                s.font_size = px;
            }
        }
        "font-weight" => {
            if let Some(b) = weight(ps[0], parent) {
                s.bold = b;
            }
        }
        "font-style" => s.italic = matches!(word.as_deref(), Some("italic") | Some("oblique")),
        "font-family" => font_family(s, value),
        "font" => {
            // [style] [variant] [weight] size[/line-height] family
            if single && word.is_some() {
                // System fonts (`caption`, `menu`...): the defaults.
                return;
            }
            let mut i = 0;
            s.italic = false;
            s.bold = false;
            s.line_height = LineHeight::Normal;
            while i < ps.len() {
                match ident(ps[i]).as_deref() {
                    Some("italic") | Some("oblique") => s.italic = true,
                    Some("bold") | Some("bolder") => s.bold = true,
                    Some("normal") | Some("small-caps") | Some("lighter") => {}
                    _ => {
                        if let [Tok::Number(n)] = ps[i] {
                            s.bold = *n >= 600_000;
                        } else if let Some(px) = font_size(ps[i], parent, u) {
                            s.font_size = px;
                            if ps.get(i + 1) == Some(&&[Tok::Delim('/')][..])
                                && let Some(lh) = ps.get(i + 2)
                            {
                                line_height(s, lh, u);
                                i += 2;
                            }
                            font_family(s, &value[value.len().min(1)..]);
                            let rest: Vec<Tok> =
                                ps[i + 1..].iter().flat_map(|p| p.iter().cloned()).collect();
                            font_family(s, &rest);
                            return;
                        }
                    }
                }
                i += 1;
            }
        }
        "line-height" => line_height(s, ps[0], u),
        "text-align" => {
            s.text_align = match word.as_deref() {
                Some("left") | Some("start") | Some("-webkit-left") => TextAlign::Left,
                Some("right") | Some("end") | Some("-webkit-right") => TextAlign::Right,
                Some("center") | Some("-webkit-center") | Some("-moz-center") => TextAlign::Center,
                Some("justify") => TextAlign::Justify,
                _ => return,
            }
        }
        "text-decoration" | "text-decoration-line" => {
            let mut d = 0;
            for p in &ps {
                match ident(p).as_deref() {
                    Some("underline") => d |= UNDERLINE,
                    Some("line-through") => d |= LINE_THROUGH,
                    Some("overline") => d |= OVERLINE,
                    _ => {
                        if let Some(c) = color(p) {
                            s.decoration_color = Some(c);
                        }
                    }
                }
            }
            s.decoration = d;
        }
        "text-decoration-color" => s.decoration_color = color(ps[0]),
        "text-transform" => {
            s.transform = match word.as_deref() {
                Some("uppercase") => Transform::Upper,
                Some("lowercase") => Transform::Lower,
                Some("capitalize") => Transform::Capitalize,
                _ => Transform::None,
            }
        }
        "white-space" | "white-space-collapse" | "text-wrap" | "text-wrap-mode" => {
            s.white_space = match word.as_deref() {
                Some("normal") | Some("wrap") | Some("collapse") => {
                    if name == "white-space-collapse" || name == "white-space" {
                        WhiteSpace::Normal
                    } else if s.white_space == WhiteSpace::NoWrap {
                        WhiteSpace::Normal
                    } else {
                        s.white_space
                    }
                }
                Some("pre") | Some("preserve") => WhiteSpace::Pre,
                Some("nowrap") => {
                    if s.white_space == WhiteSpace::PreWrap || s.white_space == WhiteSpace::Pre {
                        WhiteSpace::Pre
                    } else {
                        WhiteSpace::NoWrap
                    }
                }
                Some("pre-wrap") | Some("break-spaces") => WhiteSpace::PreWrap,
                Some("pre-line") | Some("preserve-breaks") => WhiteSpace::PreLine,
                Some("balance") | Some("pretty") | Some("stable") => s.white_space,
                _ => return,
            }
        }
        "text-indent" => {
            if let Some(l) = len(0) {
                s.text_indent = l;
            }
        }
        "vertical-align" => {
            s.vertical_align = match word.as_deref() {
                Some("sub") => VAlign::Sub,
                Some("super") => VAlign::Super,
                Some("top") | Some("text-top") => VAlign::Top,
                Some("middle") => VAlign::Middle,
                Some("bottom") | Some("text-bottom") => VAlign::Bottom,
                _ => VAlign::Baseline,
            }
        }
        "margin" | "padding" => {
            let values: Option<Vec<Length>> = ps.iter().map(|p| length(p, u)).collect();
            if let Some(v) = values.as_deref().and_then(four) {
                if name == "margin" {
                    s.margin = v;
                } else {
                    s.padding = v.map(|l| if l.auto { Length::ZERO } else { l });
                }
            }
        }
        "margin-block" | "padding-block" | "margin-inline" | "padding-inline" => {
            let values: Option<Vec<Length>> = ps.iter().map(|p| length(p, u)).collect();
            let Some(v) = values else { return };
            let (a, b) = match v.as_slice() {
                [a] => (*a, *a),
                [a, b] => (*a, *b),
                _ => return,
            };
            let (i, j) = if name.ends_with("block") { (0, 2) } else { (3, 1) };
            let target = if name.starts_with("margin") { &mut s.margin } else { &mut s.padding };
            target[i] = a;
            target[j] = b;
        }
        "border" => border_shorthand(s, &[0, 1, 2, 3], &ps, u),
        "border-width" => {
            let values: Option<Vec<i32>> = ps.iter().map(|p| border_width(p, u)).collect();
            if let Some(v) = values.as_deref().and_then(four) {
                s.border_width = v;
            }
        }
        "border-style" => {
            let values: Option<Vec<bool>> = ps.iter().map(|p| ident(p).as_deref().and_then(is_border_style)).collect();
            if let Some(v) = values.as_deref().and_then(four) {
                s.border_style_set = v;
            }
        }
        "border-color" => {
            let values: Vec<Option<Color>> = ps.iter().map(|p| color(p)).collect();
            if let Some(v) = four(&values) {
                s.border_color = v;
            }
        }
        "border-radius" => {
            if let Some(l) = len(0) {
                s.radius = l.resolve(0).unwrap_or(0).clamp(0, 10_000);
                if l.has_pct() {
                    s.radius = 9999;
                }
            }
        }
        "box-sizing" => s.border_box = word.as_deref() == Some("border-box"),
        "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height" | "inline-size"
        | "block-size" | "min-inline-size" | "max-inline-size" | "min-block-size" | "max-block-size" => {
            let l = match word.as_deref() {
                Some("none") => Length::AUTO,
                Some("fit-content") | Some("max-content") | Some("min-content") | Some("-webkit-fill-available")
                | Some("stretch") => Length::AUTO,
                _ => match len(0) {
                    Some(l) => l,
                    None => return,
                },
            };
            match name {
                "width" | "inline-size" => s.width = l,
                "height" | "block-size" => s.height = l,
                "min-width" | "min-inline-size" => s.min_width = if l.auto { Length::ZERO } else { l },
                "min-height" | "min-block-size" => s.min_height = if l.auto { Length::ZERO } else { l },
                "max-width" | "max-inline-size" => s.max_width = l,
                _ => s.max_height = l,
            }
        }
        "top" | "right" | "bottom" | "left" => {
            if let Some(l) = len(0) {
                s.inset[SIDES.iter().position(|x| *x == name).unwrap_or(0)] = l;
            }
        }
        "inset" => {
            let values: Option<Vec<Length>> = ps.iter().map(|p| length(p, u)).collect();
            if let Some(v) = values.as_deref().and_then(four) {
                s.inset = v;
            }
        }
        "overflow" | "overflow-x" | "overflow-y" => {
            s.overflow_hidden = ps.iter().any(|p| {
                matches!(ident(p).as_deref(), Some("hidden") | Some("clip") | Some("auto") | Some("scroll"))
            })
        }
        "opacity" => {
            s.opacity = match ps[0] {
                [Tok::Number(n)] => (n * 255 / 1000).clamp(0, 255) as u32,
                [Tok::Percentage(p)] => (p * 255 / 100_000).clamp(0, 255) as u32,
                _ => return,
            }
        }
        "list-style-type" | "list-style" => {
            for p in &ps {
                match ident(p).as_deref() {
                    Some("inside") => s.list_inside = true,
                    Some("outside") => s.list_inside = false,
                    Some(w) => {
                        if let Some(l) = list_style(w) {
                            s.list_style = l;
                        }
                    }
                    None => {
                        if let [Tok::Str(_)] = p {
                            s.list_style = ListStyle::Disc;
                        }
                    }
                }
            }
        }
        "list-style-position" => s.list_inside = word.as_deref() == Some("inside"),
        "flex-direction" => {
            let w = word.unwrap_or_default();
            s.flex_row = w.starts_with("row");
            s.flex_reverse = w.ends_with("reverse");
        }
        "flex-wrap" => s.flex_wrap = matches!(word.as_deref(), Some("wrap") | Some("wrap-reverse")),
        "flex-flow" => {
            for p in &ps {
                match ident(p).as_deref() {
                    Some("row") | Some("row-reverse") | Some("column") | Some("column-reverse") => {
                        let w = ident(p).unwrap_or_default();
                        s.flex_row = w.starts_with("row");
                        s.flex_reverse = w.ends_with("reverse");
                    }
                    Some("wrap") | Some("wrap-reverse") => s.flex_wrap = true,
                    Some("nowrap") => s.flex_wrap = false,
                    _ => {}
                }
            }
        }
        "justify-content" => {
            if let Some(a) = ps.iter().filter_map(|p| ident(p)).find_map(|w| align(&w)) {
                s.justify = a;
            }
        }
        "align-items" => {
            if let Some(a) = ps.iter().filter_map(|p| ident(p)).find_map(|w| align(&w)) {
                s.align_items = a;
            }
        }
        "align-self" => s.align_self = word.as_deref().and_then(align),
        "flex-grow" => {
            if let [Tok::Number(n)] = ps[0] {
                s.grow = (*n).clamp(0, 1 << 30) as i32;
            }
        }
        "flex-shrink" => {
            if let [Tok::Number(n)] = ps[0] {
                s.shrink = (*n).clamp(0, 1 << 30) as i32;
            }
        }
        "flex-basis" => {
            if let Some(l) = len(0) {
                s.basis = l;
            } else if word.is_some() {
                s.basis = Length::AUTO;
            }
        }
        "flex" => match word.as_deref() {
            Some("none") => {
                s.grow = 0;
                s.shrink = 0;
                s.basis = Length::AUTO;
            }
            Some("auto") => {
                s.grow = 1000;
                s.shrink = 1000;
                s.basis = Length::AUTO;
            }
            _ => {
                let mut numbers = Vec::new();
                let mut basis = None;
                for p in &ps {
                    match p {
                        [Tok::Number(n)] => numbers.push((*n).clamp(0, 1 << 30) as i32),
                        _ => basis = length(p, u),
                    }
                }
                s.grow = numbers.first().copied().unwrap_or(1000);
                s.shrink = numbers.get(1).copied().unwrap_or(1000);
                s.basis = basis.unwrap_or(Length::ZERO);
            }
        },
        "gap" | "grid-gap" => {
            if let Some(r) = len(0) {
                s.row_gap = r;
                s.column_gap = len(1).unwrap_or(r);
            }
        }
        "row-gap" | "grid-row-gap" => {
            if let Some(r) = len(0) {
                s.row_gap = r;
            }
        }
        "column-gap" | "grid-column-gap" => {
            if let Some(c) = len(0) {
                s.column_gap = c;
            }
        }
        "order" => {
            if let [Tok::Number(n)] = ps[0] {
                s.order = (n / 1000) as i32;
            }
        }
        "border-collapse" => s.border_collapse = word.as_deref() == Some("collapse"),
        "border-spacing" => {
            if let Some(l) = len(0) {
                s.border_spacing = l.or_zero(0);
            }
        }
        "content" => s.content = content(value),
        _ => {
            // border-top, border-left-width, border-bottom-color...
            if let Some(rest) = name.strip_prefix("border-") {
                let side = |w: &str| SIDES.iter().position(|x| *x == w).or(match w {
                    "block-start" => Some(0),
                    "inline-end" => Some(1),
                    "block-end" => Some(2),
                    "inline-start" => Some(3),
                    _ => None,
                });
                if let Some(i) = side(rest) {
                    border_shorthand(s, &[i], &ps, u);
                } else if let Some((which, what)) = rest.rsplit_once('-')
                    && let Some(i) = side(which)
                {
                    match what {
                        "width" => {
                            if let Some(w) = border_width(ps[0], u) {
                                s.border_width[i] = w;
                            }
                        }
                        "style" => {
                            if let Some(v) = word.as_deref().and_then(is_border_style) {
                                s.border_style_set[i] = v;
                            }
                        }
                        "color" => s.border_color[i] = color(ps[0]),
                        _ => {}
                    }
                } else if rest.ends_with("radius") {
                    if s.radius == 0
                        && let Some(l) = len(0)
                    {
                        s.radius = l.resolve(0).unwrap_or(0).clamp(0, 10_000) / 2;
                    }
                }
                return;
            }
            if let Some(rest) = name.strip_prefix("margin-").or_else(|| name.strip_prefix("padding-")) {
                let i = match rest {
                    "top" | "block-start" => 0,
                    "right" | "inline-end" => 1,
                    "bottom" | "block-end" => 2,
                    "left" | "inline-start" => 3,
                    _ => return,
                };
                if let Some(l) = len(0) {
                    if name.starts_with("margin") {
                        s.margin[i] = l;
                    } else {
                        s.padding[i] = if l.auto { Length::ZERO } else { l };
                    }
                }
            }
        }
    }
}

fn list_style(word: &str) -> Option<ListStyle> {
    Some(match word {
        "none" => ListStyle::None,
        "disc" | "disclosure-closed" | "disclosure-open" => ListStyle::Disc,
        "circle" => ListStyle::Circle,
        "square" => ListStyle::Square,
        "decimal" => ListStyle::Decimal,
        "decimal-leading-zero" => ListStyle::DecimalZero,
        "lower-alpha" | "lower-latin" => ListStyle::LowerAlpha,
        "upper-alpha" | "upper-latin" => ListStyle::UpperAlpha,
        "lower-roman" => ListStyle::LowerRoman,
        "upper-roman" => ListStyle::UpperRoman,
        _ => return None,
    })
}

fn line_height(s: &mut Style, part: &[Tok], u: &Units) {
    s.line_height = match part {
        [Tok::Number(n)] => LineHeight::Factor((*n).clamp(0, 100_000) as i32),
        [Tok::Ident(w)] if w.eq_ignore_ascii_case("normal") => LineHeight::Normal,
        [Tok::Percentage(p)] => LineHeight::Px((i64::from(s.font_size) * p / 100_000) as i32),
        _ => match length(part, &Units { font: s.font_size, ..*u }) {
            Some(l) if !l.auto => LineHeight::Px(l.or_zero(0)),
            _ => return,
        },
    }
}

/// `inherit` and friends: copies `name` from `from`.
fn copy_property(s: &mut Style, from: &Style, name: &str) {
    match name {
        "color" => s.color = from.color,
        "background-color" | "background" => s.background = from.background,
        "font-size" => s.font_size = from.font_size,
        "font-weight" => s.bold = from.bold,
        "font-style" => s.italic = from.italic,
        "font-family" => s.mono = from.mono,
        "font" => {
            s.font_size = from.font_size;
            s.bold = from.bold;
            s.italic = from.italic;
            s.mono = from.mono;
        }
        "line-height" => s.line_height = from.line_height,
        "text-align" => s.text_align = from.text_align,
        "text-decoration" | "text-decoration-line" => s.decoration = from.decoration,
        "text-transform" => s.transform = from.transform,
        "white-space" => s.white_space = from.white_space,
        "display" => s.display = from.display,
        "visibility" => s.visible = from.visible,
        "width" => s.width = from.width,
        "height" => s.height = from.height,
        "max-width" => s.max_width = from.max_width,
        "min-width" => s.min_width = from.min_width,
        "margin" => s.margin = from.margin,
        "padding" => s.padding = from.padding,
        "border" => {
            s.border_width = from.border_width;
            s.border_color = from.border_color;
            s.border_style_set = from.border_style_set;
        }
        "position" => s.position = from.position,
        "float" => s.float = from.float,
        "list-style-type" | "list-style" => s.list_style = from.list_style,
        "vertical-align" => s.vertical_align = from.vertical_align,
        _ => {}
    }
}

impl Style {
    /// The width of a side's border, in pixels: none without a style.
    pub fn border(&self, side: usize) -> i32 {
        if self.border_style_set[side] { self.border_width[side].max(0) } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html;

    const MEDIA: Media = Media { width: 1000, height: 700, dark: false };

    fn style_of(page: &str, tag: &str) -> Rc<Style> {
        let doc = html::parse(page);
        let sheets: Vec<Sheet> = doc
            .descendants(crate::dom::DOCUMENT)
            .into_iter()
            .filter(|&n| doc.is(n, "style"))
            .map(|n| css::parse_sheet(&doc.text_content(n), &MEDIA))
            .collect();
        let styles = compute(&doc, &sheets, &MEDIA);
        styles.get(doc.find(tag).unwrap()).unwrap().clone()
    }

    #[test]
    fn colors() {
        let c = |s: &str| color(&css::tokenize(s));
        assert_eq!(c("red"), Some(Color(0xffff0000)));
        assert_eq!(c("#0f08"), Some(Color(0x8800ff00)));
        assert_eq!(c("rgb(10, 20, 30)"), Some(Color::rgb(10, 20, 30)));
        assert_eq!(c("rgba(10 20 30 / 50%)"), Some(Color(0x7f0a141e)));
        assert_eq!(c("hsl(120, 100%, 50%)"), Some(Color::rgb(0, 255, 0)));
        assert_eq!(c("hsl(0deg 100% 50%)"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(c("nope"), None);
    }

    #[test]
    fn lengths_and_calc() {
        let u = Units { font: 20, root_font: 16, vw: 1000, vh: 800 };
        let l = |s: &str| length(&css::tokenize(s), &u);
        assert_eq!(l("1.5em").unwrap().resolve(0), Some(30));
        assert_eq!(l("2rem").unwrap().resolve(0), Some(32));
        assert_eq!(l("12pt").unwrap().resolve(0), Some(16));
        assert_eq!(l("50%").unwrap().resolve(300), Some(150));
        assert_eq!(l("10vw").unwrap().resolve(0), Some(100));
        assert_eq!(l("calc(100% - 2em)").unwrap().resolve(500), Some(460));
        assert_eq!(l("calc((100% - 10px) / 2)").unwrap().resolve(510), Some(250));
        assert_eq!(l("calc(2 * 8px + 1px)").unwrap().resolve(0), Some(17));
        assert_eq!(l("min(10px, 2em)").unwrap().resolve(0), Some(10));
        assert_eq!(l("clamp(10px, 5vw, 40px)").unwrap().resolve(0), Some(40));
        assert!(l("auto").unwrap().auto);
    }

    #[test]
    fn cascade_inheritance_and_vars() {
        let s = style_of(
            "<style>:root{--accent: #f00; --pad: 3px} p{color:blue} .x{color:green !important} #y{color:black}\
             div{font-size:20px} span{padding:var(--pad) var(--nope, 7px); background: var(--accent)}</style>\
             <div><p class=x id=y style='color:orange'>a<span>b</span></p></div>",
            "span",
        );
        assert_eq!(s.color, Color::rgb(0, 128, 0));
        assert_eq!(s.font_size, 20);
        assert_eq!(s.padding[0].resolve(0), Some(3));
        assert_eq!(s.padding[1].resolve(0), Some(7));
        assert_eq!(s.background, Color::rgb(255, 0, 0));
        assert_eq!(s.display, Display::Inline);
        let h1 = style_of("<h1>t</h1>", "h1");
        assert!(h1.bold && h1.font_size == 32 && h1.display == Display::Block);
        assert_eq!(h1.margin[0].resolve(0), Some(21));
    }

    #[test]
    fn shorthands() {
        let s = style_of(
            "<style>div{margin:1px 2px 3px; border:2px solid #00f; border-left:none; padding:0 1em;\
             font: italic bold 12px/1.5 Menlo, monospace; display:flex; flex: 1 0 50%; list-style: square inside}</style><div>x</div>",
            "div",
        );
        assert_eq!(s.margin.map(|m| m.resolve(0).unwrap()), [1, 2, 3, 2]);
        assert_eq!([s.border(0), s.border(1), s.border(2), s.border(3)], [2, 2, 2, 0]);
        assert_eq!(s.border_color(0), Color::rgb(0, 0, 255));
        assert!(s.italic && s.bold && s.mono && s.font_size == 12);
        assert_eq!(s.line_height, LineHeight::Factor(1500));
        assert_eq!(s.padding[1].resolve(0), Some(12));
        assert_eq!((s.display, s.grow, s.shrink), (Display::Flex, 1000, 0));
        assert_eq!(s.basis.pct, 50_000);
        assert_eq!((s.list_style, s.list_inside), (ListStyle::Square, true));
        let t = style_of("<style>p{border-width:4px; border-style:dashed}</style><p>x</p>", "p");
        assert_eq!(t.border(0), 4);
    }

    #[test]
    fn hints_and_media() {
        let s = style_of("<table bgcolor=ff0000 width=50% border=1><tr><td>x</td></tr></table>", "table");
        assert_eq!(s.background, Color::rgb(255, 0, 0));
        assert_eq!(s.width.pct, 50_000);
        assert_eq!(s.border(0), 1);
        let p = style_of("<style>@media (max-width:600px){p{color:red}} @media (min-width:600px){p{color:lime}}</style><p>x", "p");
        assert_eq!(p.color, Color::rgb(0, 255, 0));
        let hidden = style_of("<p hidden>x</p>", "p");
        assert_eq!(hidden.display, Display::None);
    }
}
