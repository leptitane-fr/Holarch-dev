//! Reading CSS: tokens (CSS Syntax Level 3, simplified), style sheets with
//! their rules and `@media`, and selectors, matched against the document.
//!
//! Numbers are kept in thousandths ([`Num`]): no floating point.

use crate::dom::{Document, NodeData, NodeId};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A number, in thousandths (1.5 is 1500).
pub type Num = i64;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Function(String),
    AtKeyword(String),
    /// `#name`; whether it is a valid identifier (an id selector).
    Hash(String, bool),
    Str(String),
    Url(String),
    Number(Num),
    Percentage(Num),
    Dimension(Num, String),
    Ws,
    Colon,
    Semicolon,
    Comma,
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Delim(char),
}

// ---------------------------------------------------------------- tokens

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_name(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

struct Lexer<'a> {
    chars: core::iter::Peekable<core::str::Chars<'a>>,
    ahead: Vec<char>,
}

impl Lexer<'_> {
    fn peek_n(&mut self, n: usize) -> Option<char> {
        while self.ahead.len() <= n {
            let c = self.chars.next()?;
            self.ahead.push(c);
        }
        self.ahead.get(n).copied()
    }

    fn peek(&mut self) -> Option<char> {
        self.peek_n(0)
    }

    fn bump(&mut self) -> Option<char> {
        if self.ahead.is_empty() { self.chars.next() } else { Some(self.ahead.remove(0)) }
    }

    fn escape(&mut self) -> char {
        // after '\'
        let mut hex = String::new();
        while hex.len() < 6 && self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            hex.push(self.bump().unwrap_or('0'));
        }
        if hex.is_empty() {
            return self.bump().unwrap_or('\u{fffd}');
        }
        if self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.bump();
        }
        u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).filter(|&c| c != '\0').unwrap_or('\u{fffd}')
    }

    fn valid_escape(&mut self, at: usize) -> bool {
        self.peek_n(at) == Some('\\') && self.peek_n(at + 1).is_some_and(|c| c != '\n')
    }

    fn starts_ident(&mut self, at: usize) -> bool {
        match self.peek_n(at) {
            Some('-') => {
                self.peek_n(at + 1).is_some_and(|c| is_name_start(c) || c == '-') || self.valid_escape(at + 1)
            }
            Some(c) if is_name_start(c) => true,
            Some('\\') => self.valid_escape(at),
            _ => false,
        }
    }

    fn name(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.peek() {
                Some(c) if is_name(c) => {
                    out.push(c);
                    self.bump();
                }
                Some('\\') if self.valid_escape(0) => {
                    self.bump();
                    out.push(self.escape());
                }
                _ => return out,
            }
        }
    }

    fn starts_number(&mut self) -> bool {
        match self.peek() {
            Some('+' | '-') => {
                self.peek_n(1).is_some_and(|c| c.is_ascii_digit())
                    || self.peek_n(1) == Some('.') && self.peek_n(2).is_some_and(|c| c.is_ascii_digit())
            }
            Some('.') => self.peek_n(1).is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    fn number(&mut self) -> Num {
        let negative = match self.peek() {
            Some('-') => {
                self.bump();
                true
            }
            Some('+') => {
                self.bump();
                false
            }
            _ => false,
        };
        let mut whole: i64 = 0;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
            whole = (whole * 10 + i64::from(d)).min(1 << 40);
            self.bump();
        }
        let mut frac: i64 = 0;
        let mut scale = 100;
        if self.peek() == Some('.') && self.peek_n(1).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
                frac += i64::from(d) * scale;
                scale /= 10;
                self.bump();
            }
        }
        let mut value = whole * 1000 + frac;
        if matches!(self.peek(), Some('e' | 'E'))
            && (self.peek_n(1).is_some_and(|c| c.is_ascii_digit())
                || matches!(self.peek_n(1), Some('+' | '-')) && self.peek_n(2).is_some_and(|c| c.is_ascii_digit()))
        {
            self.bump();
            let neg = match self.peek() {
                Some('-') => {
                    self.bump();
                    true
                }
                Some('+') => {
                    self.bump();
                    false
                }
                _ => false,
            };
            let mut e = 0;
            while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
                e = (e * 10 + d).min(30);
                self.bump();
            }
            for _ in 0..e {
                value = if neg { value / 10 } else { (value * 10).min(1 << 50) };
            }
        }
        if negative { -value } else { value }
    }

    fn string(&mut self, quote: char) -> String {
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                c if c == quote => break,
                '\n' => break,
                '\\' => match self.peek() {
                    Some('\n') => {
                        self.bump();
                    }
                    Some(_) => out.push(self.escape()),
                    None => {}
                },
                c => out.push(c),
            }
        }
        out
    }

    fn url(&mut self) -> Tok {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.bump();
        }
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                ')' => break,
                '\\' => out.push(self.escape()),
                c if c.is_ascii_whitespace() => {
                    while self.peek().is_some_and(|c| c != ')') {
                        self.bump();
                    }
                }
                c => out.push(c),
            }
        }
        Tok::Url(out)
    }

    fn next(&mut self) -> Option<Tok> {
        loop {
            // Comments are dropped.
            if self.peek() == Some('/') && self.peek_n(1) == Some('*') {
                self.bump();
                self.bump();
                loop {
                    match self.bump() {
                        Some('*') if self.peek() == Some('/') => {
                            self.bump();
                            break;
                        }
                        Some(_) => {}
                        None => return None,
                    }
                }
                continue;
            }
            break;
        }
        let c = self.peek()?;
        if c.is_ascii_whitespace() {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.bump();
            }
            return Some(Tok::Ws);
        }
        if self.starts_number() {
            let n = self.number();
            if self.starts_ident(0) {
                let unit = self.name().to_ascii_lowercase();
                return Some(Tok::Dimension(n, unit));
            }
            if self.peek() == Some('%') {
                self.bump();
                return Some(Tok::Percentage(n));
            }
            return Some(Tok::Number(n));
        }
        if self.starts_ident(0) {
            let name = self.name();
            if self.peek() == Some('(') {
                self.bump();
                if name.eq_ignore_ascii_case("url") {
                    let mut i = 0;
                    while self.peek_n(i).is_some_and(|c| c.is_ascii_whitespace()) {
                        i += 1;
                    }
                    if !matches!(self.peek_n(i), Some('"' | '\'')) {
                        return Some(self.url());
                    }
                }
                return Some(Tok::Function(name.to_ascii_lowercase()));
            }
            return Some(Tok::Ident(name));
        }
        self.bump();
        Some(match c {
            '"' | '\'' => Tok::Str(self.string(c)),
            '#' => {
                if self.peek().is_some_and(is_name) || self.valid_escape(0) {
                    let id = self.starts_ident(0);
                    Tok::Hash(self.name(), id)
                } else {
                    Tok::Delim('#')
                }
            }
            '@' if self.starts_ident(0) => Tok::AtKeyword(self.name().to_ascii_lowercase()),
            ':' => Tok::Colon,
            ';' => Tok::Semicolon,
            ',' => Tok::Comma,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            '<' if self.peek() == Some('!') && self.peek_n(1) == Some('-') && self.peek_n(2) == Some('-') => {
                self.bump();
                self.bump();
                self.bump();
                Tok::Ws
            }
            '-' if self.peek() == Some('-') && self.peek_n(1) == Some('>') => {
                self.bump();
                self.bump();
                Tok::Ws
            }
            '\\' => Tok::Delim(self.escape()),
            c => Tok::Delim(c),
        })
    }
}

/// Splits CSS text into tokens.
pub fn tokenize(text: &str) -> Vec<Tok> {
    let mut lexer = Lexer { chars: text.chars().peekable(), ahead: Vec::new() };
    let mut out = Vec::new();
    while let Some(t) = lexer.next() {
        out.push(t);
    }
    out
}

// ---------------------------------------------------------------- sheets

/// One declaration: `name: value [!important]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    /// In lower case, except custom properties (`--name`), kept as written.
    pub name: String,
    pub value: Vec<Tok>,
    pub important: bool,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub decls: Vec<Decl>,
}

/// What `@media` queries are asked about.
#[derive(Clone, Copy, Debug)]
pub struct Media {
    /// The viewport, in CSS pixels.
    pub width: i32,
    pub height: i32,
    pub dark: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
    /// The addresses of `@import` rules, in order.
    pub imports: Vec<String>,
}

/// A run of tokens up to a matching close, with nesting.
fn block_end(toks: &[Tok], open: usize) -> usize {
    let mut depth = 0;
    for (i, t) in toks.iter().enumerate().skip(open) {
        match t {
            Tok::LBrace | Tok::LParen | Tok::LBracket | Tok::Function(_) => depth += 1,
            Tok::RBrace | Tok::RParen | Tok::RBracket => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    toks.len()
}

/// Reads declarations (`a: b; c: d !important`).
pub fn parse_declarations(toks: &[Tok]) -> Vec<Decl> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        // One declaration up to a ';' outside any block.
        let mut end = i;
        while end < toks.len() && toks[end] != Tok::Semicolon {
            if matches!(toks[end], Tok::LBrace | Tok::LParen | Tok::LBracket | Tok::Function(_)) {
                end = block_end(toks, end);
            }
            end += 1;
        }
        let part = &toks[i..end.min(toks.len())];
        i = end + 1;
        let mut p = part.iter().skip_while(|t| **t == Tok::Ws);
        let Some(Tok::Ident(name)) = p.next() else { continue };
        let rest: Vec<Tok> = p.cloned().collect();
        let mut rest = rest.as_slice();
        while rest.first() == Some(&Tok::Ws) {
            rest = &rest[1..];
        }
        let Some(Tok::Colon) = rest.first() else { continue };
        let mut value: Vec<Tok> = rest[1..].to_vec();
        while value.last() == Some(&Tok::Ws) {
            value.pop();
        }
        let mut important = false;
        let n = value.len();
        if n >= 2
            && let Tok::Ident(word) = &value[n - 1]
            && word.eq_ignore_ascii_case("important")
        {
            let mut k = n - 2;
            while k > 0 && value[k] == Tok::Ws {
                k -= 1;
            }
            if value[k] == Tok::Delim('!') {
                important = true;
                value.truncate(k);
                while value.last() == Some(&Tok::Ws) {
                    value.pop();
                }
            }
        }
        while value.first() == Some(&Tok::Ws) {
            value.remove(0);
        }
        let name = if name.starts_with("--") { name.clone() } else { name.to_ascii_lowercase() };
        out.push(Decl { name, value, important });
    }
    out
}

/// Whether a media query list holds (`screen and (max-width: 600px), print`).
pub fn media_matches(toks: &[Tok], media: &Media) -> bool {
    if toks.iter().all(|t| *t == Tok::Ws) {
        return true;
    }
    toks.split(|t| *t == Tok::Comma).any(|query| one_query(query, media))
}

fn one_query(query: &[Tok], media: &Media) -> bool {
    let words: Vec<&Tok> = query.iter().filter(|t| **t != Tok::Ws).collect();
    let mut negate = false;
    let mut ok = true;
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            Tok::Ident(w) => match w.to_ascii_lowercase().as_str() {
                "not" => negate = true,
                "only" | "and" => {}
                "screen" | "all" => {}
                _ => ok = false,
            },
            Tok::LParen => {
                let end = words[i..].iter().position(|t| **t == Tok::RParen).map_or(words.len(), |p| i + p);
                ok &= feature(&words[i + 1..end], media);
                i = end;
            }
            _ => ok = false,
        }
        i += 1;
    }
    ok != negate
}

fn px_of(t: &Tok) -> Option<i64> {
    match t {
        Tok::Dimension(n, u) => Some(match u.as_str() {
            "em" | "rem" => n * 16 / 1000,
            _ => n / 1000,
        }),
        Tok::Number(n) => Some(n / 1000),
        _ => None,
    }
}

fn feature(words: &[&Tok], media: &Media) -> bool {
    let name = match words.first() {
        Some(Tok::Ident(n)) => n.to_ascii_lowercase(),
        _ => return false,
    };
    let value = words.get(2).copied();
    let (w, h) = (i64::from(media.width), i64::from(media.height));
    match (name.as_str(), value) {
        ("min-width", Some(v)) => px_of(v).is_some_and(|p| w >= p),
        ("max-width", Some(v)) => px_of(v).is_some_and(|p| w <= p),
        ("min-height", Some(v)) => px_of(v).is_some_and(|p| h >= p),
        ("max-height", Some(v)) => px_of(v).is_some_and(|p| h <= p),
        ("width", Some(_)) | ("height", Some(_)) => false,
        ("prefers-color-scheme", Some(Tok::Ident(v))) => (v == "dark") == media.dark,
        ("prefers-reduced-motion", Some(Tok::Ident(v))) => v == "reduce",
        ("orientation", Some(Tok::Ident(v))) => (v == "landscape") == (w >= h),
        ("hover", Some(Tok::Ident(v))) => v == "hover",
        ("pointer", Some(Tok::Ident(v))) => v == "fine",
        ("any-hover", Some(Tok::Ident(v))) => v == "hover",
        ("color", _) => true,
        ("min-resolution" | "-webkit-min-device-pixel-ratio", _) => false,
        _ => false,
    }
}

/// Whether `@supports` holds: it does unless it asks for what this
/// engine does not do.
fn supports(toks: &[Tok]) -> bool {
    let mut text = String::new();
    for t in toks {
        match t {
            Tok::Ident(s) | Tok::Function(s) => text.push_str(s),
            Tok::Delim(c) => text.push(*c),
            _ => text.push(' '),
        }
    }
    let text = text.to_ascii_lowercase();
    let unsupported = ["grid", "selector", "subgrid", "container", "backdrop", "anchor", "@"];
    let mentions = unsupported.iter().any(|u| text.contains(u));
    if text.trim_start().starts_with("not") { mentions } else { !mentions }
}

/// Reads a style sheet, keeping the rules whose `@media` holds.
pub fn parse_sheet(text: &str, media: &Media) -> Sheet {
    let toks = tokenize(text);
    let mut sheet = Sheet::default();
    rules(&toks, media, &mut sheet, true);
    sheet
}

fn rules(toks: &[Tok], media: &Media, sheet: &mut Sheet, top: bool) {
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Ws | Tok::Semicolon => i += 1,
            Tok::AtKeyword(name) => {
                // The prelude, up to `;` or a block.
                let mut j = i + 1;
                while j < toks.len() && !matches!(toks[j], Tok::Semicolon | Tok::LBrace) {
                    if matches!(toks[j], Tok::LParen | Tok::Function(_) | Tok::LBracket) {
                        j = block_end(toks, j);
                    }
                    j += 1;
                }
                let prelude = &toks[i + 1..j.min(toks.len())];
                if j >= toks.len() || toks[j] == Tok::Semicolon {
                    if name == "import" && top {
                        let url = prelude.iter().find_map(|t| match t {
                            Tok::Url(u) | Tok::Str(u) => Some(u.clone()),
                            _ => None,
                        });
                        let rest: Vec<Tok> = prelude
                            .iter()
                            .filter(|t| !matches!(t, Tok::Url(_) | Tok::Str(_)))
                            .cloned()
                            .collect();
                        if let Some(url) = url
                            && media_matches(&rest, media)
                        {
                            sheet.imports.push(url);
                        }
                    }
                    i = j + 1;
                    continue;
                }
                let end = block_end(toks, j);
                let body = &toks[j + 1..end.min(toks.len())];
                match name.as_str() {
                    "media" if media_matches(prelude, media) => rules(body, media, sheet, false),
                    "supports" if supports(prelude) => rules(body, media, sheet, false),
                    "layer" | "document" | "-moz-document" | "scope" | "starting-style" => {
                        if name == "layer" {
                            rules(body, media, sheet, false)
                        }
                    }
                    _ => {}
                }
                i = end + 1;
            }
            _ => {
                // A style rule: selectors, then a block.
                let mut j = i;
                while j < toks.len() && toks[j] != Tok::LBrace {
                    j += 1;
                }
                if j >= toks.len() {
                    break;
                }
                let end = block_end(toks, j);
                let prelude = &toks[i..j];
                let body = &toks[j + 1..end.min(toks.len())];
                let selectors = parse_selectors(prelude);
                if !selectors.is_empty() {
                    // Nested rules (CSS nesting) are not read; plain
                    // declarations are.
                    let decls = parse_declarations(body);
                    if !decls.is_empty() {
                        sheet.rules.push(Rule { selectors, decls });
                    }
                }
                i = end + 1;
            }
        }
    }
}

// ---------------------------------------------------------------- selectors

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    Next,
    Later,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrOp {
    Exists,
    Equals,
    Word,
    Dash,
    Prefix,
    Suffix,
    Contains,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrSel {
    pub name: String,
    pub op: AttrOp,
    pub value: String,
    pub case_insensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    /// `an+b`, counted from the start or the end, among all elements or
    /// those of the same type.
    Nth { a: i32, b: i32, from_end: bool, of_type: bool },
    Root,
    Empty,
    Link,
    Checked,
    Disabled,
    Enabled,
    Not(Vec<Selector>),
    Is(Vec<Selector>),
    /// `:where()`: like `:is()` without specificity.
    Where(Vec<Selector>),
    Has,
    /// A state this engine never is in (`:hover`, `:focus`...), or one it
    /// does not know: never matches.
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PseudoElement {
    Before,
    After,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<AttrSel>,
    pub pseudos: Vec<Pseudo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    /// From the rightmost compound leftwards: each with the combinator
    /// that joins it to the next one on its left.
    pub parts: Vec<(Compound, Combinator)>,
    /// (ids, classes, types) packed: ids × 2^20 + classes × 2^10 + types.
    pub specificity: u32,
    pub pseudo_element: Option<PseudoElement>,
}

/// Reads a selector list; any selector that cannot be read makes the
/// whole list invalid (as browsers do), except inside `:is()` and
/// `:where()`, which keep the others.
pub fn parse_selectors(toks: &[Tok]) -> Vec<Selector> {
    let mut out = Vec::new();
    for part in split_commas(toks) {
        match parse_selector(part) {
            Some(s) => out.push(s),
            None => return Vec::new(),
        }
    }
    out
}

fn forgiving(toks: &[Tok]) -> Vec<Selector> {
    split_commas(toks).into_iter().filter_map(parse_selector).collect()
}

fn split_commas(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::LParen | Tok::Function(_) | Tok::LBracket => depth += 1,
            Tok::RParen | Tok::RBracket => depth -= 1,
            Tok::Comma if depth == 0 => {
                out.push(&toks[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&toks[start..]);
    out
}

fn specificity(sel: &[(Compound, Combinator)]) -> u32 {
    let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
    for (comp, _) in sel {
        a += u32::from(comp.id.is_some());
        b += (comp.classes.len() + comp.attrs.len()) as u32;
        c += u32::from(comp.tag.is_some());
        for p in &comp.pseudos {
            match p {
                Pseudo::Not(list) | Pseudo::Is(list) => {
                    let best = list.iter().map(|s| s.specificity).max().unwrap_or(0);
                    a += best >> 20;
                    b += (best >> 10) & 1023;
                    c += best & 1023;
                }
                Pseudo::Where(_) => {}
                _ => b += 1,
            }
        }
    }
    a.min(1023) << 20 | b.min(1023) << 10 | c.min(1023)
}

fn parse_nth(toks: &[Tok]) -> Option<(i32, i32)> {
    let mut text = String::new();
    for t in toks {
        match t {
            Tok::Ws => {}
            Tok::Ident(s) => text.push_str(s),
            Tok::Number(n) => {
                if *n >= 0 && !text.is_empty() && !text.ends_with(['+', '-']) {
                    text.push('+');
                }
                push_int(&mut text, n / 1000);
            }
            Tok::Dimension(n, u) => {
                push_int(&mut text, n / 1000);
                text.push_str(u);
            }
            Tok::Delim(c) => text.push(*c),
            _ => return None,
        }
    }
    let text = text.to_ascii_lowercase();
    match text.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    match text.find('n') {
        None => Some((0, text.parse().ok()?)),
        Some(i) => {
            let a = match &text[..i] {
                "" | "+" => 1,
                "-" => -1,
                s => s.parse().ok()?,
            };
            let rest = &text[i + 1..];
            let b = if rest.is_empty() { 0 } else { rest.trim_start_matches('+').parse().ok()? };
            Some((a, b))
        }
    }
}

fn push_int(text: &mut String, n: i64) {
    use core::fmt::Write;
    let _ = write!(text, "{n}");
}

fn parse_selector(toks: &[Tok]) -> Option<Selector> {
    let mut i = 0;
    let n = toks.len();
    let mut compounds: Vec<(Compound, Combinator)> = Vec::new();
    let mut current = Compound::default();
    let mut has = false;
    let mut pending = Combinator::Descendant;
    let mut pseudo_element = None;
    while i < n && toks[i] == Tok::Ws {
        i += 1;
    }
    while i < n {
        match &toks[i] {
            Tok::Ws | Tok::Delim('>' | '+' | '~') => {
                let mut comb = Combinator::Descendant;
                while i < n && matches!(toks[i], Tok::Ws | Tok::Delim('>' | '+' | '~')) {
                    match toks[i] {
                        Tok::Delim('>') => comb = Combinator::Child,
                        Tok::Delim('+') => comb = Combinator::Next,
                        Tok::Delim('~') => comb = Combinator::Later,
                        _ => {}
                    }
                    i += 1;
                }
                if i >= n {
                    break;
                }
                if !has {
                    return None;
                }
                compounds.push((core::mem::take(&mut current), pending));
                pending = comb;
                has = false;
                continue;
            }
            Tok::Ident(name) => {
                current.tag = Some(name.to_ascii_lowercase());
            }
            Tok::Delim('*') => {}
            Tok::Hash(id, true) => current.id = Some(id.clone()),
            Tok::Delim('.') => match toks.get(i + 1) {
                Some(Tok::Ident(c)) => {
                    current.classes.push(c.clone());
                    i += 1;
                }
                _ => return None,
            },
            Tok::LBracket => {
                let end = block_end(toks, i);
                let inner: Vec<&Tok> = toks[i + 1..end.min(n)].iter().filter(|t| **t != Tok::Ws).collect();
                let Some(Tok::Ident(name)) = inner.first() else { return None };
                let mut sel = AttrSel {
                    name: name.to_ascii_lowercase(),
                    op: AttrOp::Exists,
                    value: String::new(),
                    case_insensitive: false,
                };
                let mut k = 1;
                if let Some(Tok::Delim(c)) = inner.get(k) {
                    sel.op = match c {
                        '=' => AttrOp::Equals,
                        '~' => AttrOp::Word,
                        '|' => AttrOp::Dash,
                        '^' => AttrOp::Prefix,
                        '$' => AttrOp::Suffix,
                        '*' => AttrOp::Contains,
                        _ => return None,
                    };
                    k += 1;
                    if sel.op != AttrOp::Equals {
                        if inner.get(k) != Some(&&Tok::Delim('=')) {
                            return None;
                        }
                        k += 1;
                    }
                    sel.value = match inner.get(k) {
                        Some(Tok::Ident(v) | Tok::Str(v)) => v.clone(),
                        Some(Tok::Number(v)) => {
                            let mut s = String::new();
                            push_int(&mut s, v / 1000);
                            s
                        }
                        _ => return None,
                    };
                    k += 1;
                    if let Some(Tok::Ident(flag)) = inner.get(k) {
                        sel.case_insensitive = flag.eq_ignore_ascii_case("i");
                    }
                }
                current.attrs.push(sel);
                i = end;
            }
            Tok::Colon => {
                let element = toks.get(i + 1) == Some(&Tok::Colon);
                if element {
                    i += 1;
                }
                match toks.get(i + 1) {
                    Some(Tok::Ident(name)) => {
                        let name = name.to_ascii_lowercase();
                        i += 1;
                        match name.as_str() {
                            "before" => pseudo_element = Some(PseudoElement::Before),
                            "after" => pseudo_element = Some(PseudoElement::After),
                            _ if element => current.pseudos.push(Pseudo::Never),
                            "first-child" => current.pseudos.push(Pseudo::FirstChild),
                            "last-child" => current.pseudos.push(Pseudo::LastChild),
                            "only-child" => current.pseudos.push(Pseudo::OnlyChild),
                            "first-of-type" => {
                                current.pseudos.push(Pseudo::Nth { a: 0, b: 1, from_end: false, of_type: true })
                            }
                            "last-of-type" => {
                                current.pseudos.push(Pseudo::Nth { a: 0, b: 1, from_end: true, of_type: true })
                            }
                            "root" => current.pseudos.push(Pseudo::Root),
                            "empty" => current.pseudos.push(Pseudo::Empty),
                            "link" | "any-link" => current.pseudos.push(Pseudo::Link),
                            "checked" => current.pseudos.push(Pseudo::Checked),
                            "disabled" => current.pseudos.push(Pseudo::Disabled),
                            "enabled" => current.pseudos.push(Pseudo::Enabled),
                            "first-line" | "first-letter" => current.pseudos.push(Pseudo::Never),
                            _ => current.pseudos.push(Pseudo::Never),
                        }
                    }
                    Some(Tok::Function(name)) => {
                        let start = i + 1;
                        let end = block_end(toks, start);
                        let inner = &toks[start + 1..end.min(n)];
                        let p = match name.as_str() {
                            "not" => Pseudo::Not(forgiving(inner)),
                            "is" | "matches" | "-webkit-any" | "-moz-any" => Pseudo::Is(forgiving(inner)),
                            "where" => Pseudo::Where(forgiving(inner)),
                            "has" => Pseudo::Has,
                            "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                                // `of S` is not read.
                                let (a, b) = parse_nth(inner).unwrap_or((0, -1));
                                Pseudo::Nth {
                                    a,
                                    b,
                                    from_end: name.contains("last"),
                                    of_type: name.ends_with("of-type"),
                                }
                            }
                            _ => Pseudo::Never,
                        };
                        if !element {
                            current.pseudos.push(p);
                        } else {
                            current.pseudos.push(Pseudo::Never);
                        }
                        i = end;
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
        has = true;
        i += 1;
    }
    if !has {
        return None;
    }
    compounds.push((current, pending));
    // Each compound was pushed with the combinator to its left neighbour:
    // reversed, they read from the right.
    compounds.reverse();
    let specificity = specificity(&compounds) + u32::from(pseudo_element.is_some());
    Some(Selector { parts: compounds, specificity, pseudo_element })
}

// ---------------------------------------------------------------- matching

fn attr_matches(doc: &Document, node: NodeId, sel: &AttrSel) -> bool {
    let Some(e) = doc.element(node) else { return false };
    let Some(value) = e.attr(&sel.name) else { return false };
    let (v, want) = if sel.case_insensitive {
        (value.to_ascii_lowercase(), sel.value.to_ascii_lowercase())
    } else {
        (String::from(value), sel.value.clone())
    };
    match sel.op {
        AttrOp::Exists => true,
        AttrOp::Equals => v == want,
        AttrOp::Word => v.split_ascii_whitespace().any(|w| w == want),
        AttrOp::Dash => v == want || v.starts_with(&alloc::format!("{want}-")),
        AttrOp::Prefix => !want.is_empty() && v.starts_with(&want),
        AttrOp::Suffix => !want.is_empty() && v.ends_with(&want),
        AttrOp::Contains => !want.is_empty() && v.contains(&want),
    }
}

/// The position of `node` among its parent's element children (1-based),
/// counted from the end with `from_end`, among those of its type only with
/// `of_type`.
fn position(doc: &Document, node: NodeId, from_end: bool, of_type: bool) -> i32 {
    let Some(parent) = doc.parent(node) else { return 1 };
    let name = doc.element(node).map(|e| e.name.as_str()).unwrap_or("");
    let mut siblings: Vec<NodeId> = doc
        .element_children(parent)
        .filter(|&c| !of_type || doc.element(c).is_some_and(|e| e.name == name))
        .collect();
    if from_end {
        siblings.reverse();
    }
    siblings.iter().position(|&c| c == node).map_or(1, |p| p as i32 + 1)
}

fn nth(a: i32, b: i32, pos: i32) -> bool {
    if a == 0 {
        return pos == b;
    }
    let diff = pos - b;
    diff % a == 0 && diff / a >= 0
}

fn compound_matches(doc: &Document, node: NodeId, c: &Compound) -> bool {
    let Some(e) = doc.element(node) else { return false };
    if let Some(tag) = &c.tag
        && *tag != e.name
    {
        return false;
    }
    if let Some(id) = &c.id
        && e.id() != Some(id.as_str())
    {
        return false;
    }
    if !c.classes.iter().all(|cl| e.has_class(cl)) {
        return false;
    }
    if !c.attrs.iter().all(|a| attr_matches(doc, node, a)) {
        return false;
    }
    c.pseudos.iter().all(|p| match p {
        Pseudo::FirstChild => doc.previous_element(node).is_none(),
        Pseudo::LastChild => doc.next_element(node).is_none(),
        Pseudo::OnlyChild => doc.previous_element(node).is_none() && doc.next_element(node).is_none(),
        Pseudo::Nth { a, b, from_end, of_type } => nth(*a, *b, position(doc, node, *from_end, *of_type)),
        Pseudo::Root => e.name == "html",
        Pseudo::Empty => doc.nodes[node].children.iter().all(|&ch| match &doc.nodes[ch].data {
            NodeData::Text(t) => t.is_empty(),
            NodeData::Comment => true,
            _ => false,
        }),
        Pseudo::Link => matches!(e.name.as_str(), "a" | "area") && e.attr("href").is_some(),
        Pseudo::Checked => e.checked || e.name == "option" && e.attr("selected").is_some(),
        Pseudo::Disabled => e.attr("disabled").is_some(),
        Pseudo::Enabled => {
            matches!(e.name.as_str(), "input" | "button" | "select" | "textarea") && e.attr("disabled").is_none()
        }
        Pseudo::Not(list) => !list.iter().any(|s| matches(doc, node, s)),
        Pseudo::Is(list) | Pseudo::Where(list) => list.iter().any(|s| matches(doc, node, s)),
        Pseudo::Has | Pseudo::Never => false,
    })
}

fn matches_from(doc: &Document, node: NodeId, parts: &[(Compound, Combinator)]) -> bool {
    let Some(((first, comb), rest)) = parts.split_first() else { return true };
    if !compound_matches(doc, node, first) {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    match comb {
        Combinator::Child => doc.parent(node).is_some_and(|p| matches_from(doc, p, rest)),
        Combinator::Descendant => {
            let mut at = doc.parent(node);
            while let Some(p) = at {
                if matches_from(doc, p, rest) {
                    return true;
                }
                at = doc.parent(p);
            }
            false
        }
        Combinator::Next => doc.previous_element(node).is_some_and(|p| matches_from(doc, p, rest)),
        Combinator::Later => {
            let mut at = doc.previous_element(node);
            while let Some(p) = at {
                if matches_from(doc, p, rest) {
                    return true;
                }
                at = doc.previous_element(p);
            }
            false
        }
    }
}

/// Whether `node` matches `sel` (its pseudo-element aside).
pub fn matches(doc: &Document, node: NodeId, sel: &Selector) -> bool {
    matches_from(doc, node, &sel.parts)
}

// ---------------------------------------------------------------- index

/// Where a rule's selector comes from, for the cascade's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Agent,
    Author,
}

/// One selector of one rule, ready to be matched.
#[derive(Clone, Debug)]
pub struct Indexed {
    pub origin: Origin,
    /// The order of the rule among all sheets.
    pub order: u32,
    pub selector: Selector,
    pub rule: Box<[Decl]>,
}

/// Every selector of a page's sheets, found by the rightmost compound's
/// id, class or tag, so that each element is matched against few.
#[derive(Default)]
pub struct Index {
    pub entries: Vec<Indexed>,
    by_id: BTreeMap<String, Vec<usize>>,
    by_class: BTreeMap<String, Vec<usize>>,
    by_tag: BTreeMap<String, Vec<usize>>,
    any: Vec<usize>,
}

impl Index {
    pub fn add(&mut self, sheet: &Sheet, origin: Origin) {
        for rule in &sheet.rules {
            let order = self.entries.len() as u32;
            let decls: Box<[Decl]> = rule.decls.clone().into_boxed_slice();
            for selector in &rule.selectors {
                let i = self.entries.len();
                let key = &selector.parts[0].0;
                if let Some(id) = &key.id {
                    self.by_id.entry(id.clone()).or_default().push(i);
                } else if let Some(class) = key.classes.first() {
                    self.by_class.entry(class.clone()).or_default().push(i);
                } else if let Some(tag) = &key.tag {
                    self.by_tag.entry(tag.clone()).or_default().push(i);
                } else {
                    self.any.push(i);
                }
                self.entries.push(Indexed { origin, order, selector: selector.clone(), rule: decls.clone() });
            }
        }
    }

    /// The entries that match `node`, in cascade order (origin,
    /// specificity, then order).
    pub fn matching(&self, doc: &Document, node: NodeId) -> Vec<&Indexed> {
        let Some(e) = doc.element(node) else { return Vec::new() };
        let mut found: Vec<usize> = Vec::new();
        if let Some(id) = e.id()
            && let Some(list) = self.by_id.get(id)
        {
            found.extend(list);
        }
        for class in e.classes() {
            if let Some(list) = self.by_class.get(class) {
                found.extend(list);
            }
        }
        if let Some(list) = self.by_tag.get(&e.name) {
            found.extend(list);
        }
        found.extend(&self.any);
        found.sort_unstable();
        found.dedup();
        let mut out: Vec<&Indexed> =
            found.into_iter().map(|i| &self.entries[i]).filter(|x| matches(doc, node, &x.selector)).collect();
        out.sort_by_key(|x| (x.origin, x.selector.specificity, x.order));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html;

    const MEDIA: Media = Media { width: 1000, height: 700, dark: false };

    #[test]
    fn tokens() {
        let t = tokenize("a{color:#fff;width:calc(100% - 2.5em)} /* x */ @media");
        assert!(t.contains(&Tok::Hash(String::from("fff"), true)));
        assert!(t.contains(&Tok::Percentage(100_000)));
        assert!(t.contains(&Tok::Dimension(2500, String::from("em"))));
        assert!(t.contains(&Tok::Function(String::from("calc"))));
        assert!(t.contains(&Tok::AtKeyword(String::from("media"))));
        assert_eq!(tokenize("url( x.png )"), [Tok::Url(String::from("x.png"))]);
        assert_eq!(tokenize("-.5e1"), [Tok::Number(-5000)]);
        assert_eq!(tokenize("\"a\\\"b\""), [Tok::Str(String::from("a\"b"))]);
    }

    #[test]
    fn declarations_and_importance() {
        let d = parse_declarations(&tokenize("color: red !important; ; margin:0 auto;--Main-Color: blue; bad"));
        assert_eq!(d.len(), 3);
        assert!(d[0].important && d[0].value == [Tok::Ident(String::from("red"))]);
        assert_eq!(d[2].name, "--Main-Color");
    }

    #[test]
    fn media_and_sheets() {
        let sheet = parse_sheet(
            "@import url(a.css); @import 'p.css' print; @charset 'x'; \
             p{a:b} @media (max-width: 600px){p{c:d}} @media screen and (min-width:900px){h1{e:f}} \
             @font-face{font-family:x} @supports (display:grid){q{x:y}} @supports not (display:grid){r{x:y}}",
            &MEDIA,
        );
        assert_eq!(sheet.imports, [String::from("a.css")]);
        let names: Vec<_> = sheet.rules.iter().map(|r| r.selectors[0].parts[0].0.tag.clone().unwrap()).collect();
        assert_eq!(names, ["p", "h1", "r"]);
    }

    #[test]
    fn selectors_match() {
        let doc = html::parse(
            "<div id=main class='a b'><p class=x>1</p><p lang=fr-CA>2</p><span>3</span><p>4</p></div><ul><li>a<li>b<li>c</ul>",
        );
        let all = doc.descendants(crate::dom::DOCUMENT);
        let count = |sel: &str| {
            let s = parse_selectors(&tokenize(sel));
            assert!(!s.is_empty(), "{sel}");
            all.iter().filter(|&&n| s.iter().any(|s| matches(&doc, n, s))).count()
        };
        assert_eq!(count("p"), 3);
        assert_eq!(count("#main > p"), 3);
        assert_eq!(count("div.a.b p.x"), 1);
        assert_eq!(count("[lang|=fr]"), 1);
        assert_eq!(count("p + span"), 1);
        assert_eq!(count("p ~ p"), 2);
        assert_eq!(count("li:nth-child(2n+1)"), 2);
        assert_eq!(count("li:last-child, li:first-child"), 2);
        assert_eq!(count("p:not(.x)"), 2);
        assert_eq!(count("p:is(.x, [lang])"), 2);
        assert_eq!(count("div p:first-of-type"), 1);
        assert_eq!(count("a:hover"), 0);
        assert_eq!(count("body *"), 9);
        assert!(parse_selectors(&tokenize("p, ..bad")).is_empty());
        let s = &parse_selectors(&tokenize("#a .b p::before"))[0];
        assert_eq!(s.pseudo_element, Some(PseudoElement::Before));
        assert_eq!(s.specificity, 1 << 20 | 1 << 10 | 2);
    }

    #[test]
    fn index_orders_the_cascade() {
        let doc = html::parse("<p id=i class=c>x</p>");
        let p = doc.find("p").unwrap();
        let mut index = Index::default();
        index.add(&parse_sheet("#i{a:1} .c{a:2} p{a:3} *{a:4} div{a:5}", &MEDIA), Origin::Author);
        index.add(&parse_sheet("p{a:0}", &MEDIA), Origin::Agent);
        let got: Vec<_> = index.matching(&doc, p).iter().map(|x| x.rule[0].value.clone()).collect();
        let want: Vec<Vec<Tok>> = [0, 4, 3, 2, 1].iter().map(|&n| alloc::vec![Tok::Number(n * 1000)]).collect();
        assert_eq!(got, want);
    }
}
