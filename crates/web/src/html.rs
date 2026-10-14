//! Reading HTML: the bytes into text (its character set), the text into
//! tokens, the tokens into a document tree, with the corrections browsers
//! make to real pages (missing `<html>`, `<head>` and `<body>`, paragraphs
//! and list items left open, tables without their body or rows).

use crate::dom::{DOCUMENT, Document, Element, NodeData, NodeId};
use crate::entities;
use alloc::string::String;
use alloc::vec::Vec;

/// Elements that never have children.
pub const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
    "keygen",
];

/// Elements whose start closes an open `<p>`.
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "center",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "ul",
    "li",
    "dd",
    "dt",
    "listing",
    "plaintext",
    "xmp",
];

/// Elements that belong in `<head>` when they come before the body.
const HEAD: &[&str] = &["title", "meta", "link", "style", "script", "base", "noscript", "template"];

/// The edges of the "in scope" search.
const SCOPE: &[&str] = &["html", "table", "td", "th", "caption", "marquee", "object", "applet", "template"];

// ---------------------------------------------------------------- text

/// The page's bytes as text: a byte order mark, else the charset of the
/// `Content-Type` (`content_type`), else a `<meta charset>` near the top,
/// else UTF-8 when the bytes are valid UTF-8, else Windows-1252.
pub fn decode(bytes: &[u8], content_type: Option<&str>) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let from_header = content_type.and_then(charset_of);
    let sniffed = || {
        let head = &bytes[..bytes.len().min(2048)];
        let head = String::from_utf8_lossy(head).to_ascii_lowercase();
        let at = head.find("charset")?;
        let rest = head[at + 7..].trim_start_matches([' ', '=', '"', '\'']);
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
        Some(String::from(&rest[..end]))
    };
    let charset = from_header.or_else(sniffed).unwrap_or_default();
    let latin = matches!(
        charset.as_str(),
        "iso-8859-1" | "iso-8859-15" | "latin1" | "windows-1252" | "cp1252" | "us-ascii" | "ascii"
    );
    match core::str::from_utf8(bytes) {
        Ok(text) if !latin => String::from(text),
        _ if !latin && charset.starts_with("utf") => String::from_utf8_lossy(bytes).into_owned(),
        _ => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn charset_of(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    let at = lower.find("charset=")?;
    Some(String::from(lower[at + 8..].trim_matches(|c: char| c == '"' || c == '\'' || c == ' ' || c == ';')))
}

/// A Windows-1252 byte as a character.
pub fn windows_1252(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d,
        0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153,
        0x9d, 0x17e, 0x178,
    ];
    match b {
        0x80..=0x9f => char::from_u32(u32::from(HIGH[usize::from(b - 0x80)])).unwrap_or('?'),
        _ => b as char,
    }
}

// ---------------------------------------------------------------- tokens

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Doctype,
    Start { name: String, attrs: Vec<(String, String)>, self_closing: bool },
    End { name: String },
    Text(String),
    Comment,
}

/// Splits HTML text into tokens.
pub struct Tokenizer<'a> {
    text: &'a str,
    at: usize,
    /// After `<script>`, `<style>`... the element whose end tag ends the
    /// raw text, and whether character references count in it.
    raw: Option<(String, bool)>,
}

/// Decodes the character references in `text` (`&amp;`, `&#233;`...).
/// `attribute`: in a value, `&name` followed by `=` or a letter stays.
pub fn unescape(text: &str, attribute: bool) -> String {
    if !text.contains('&') {
        return String::from(text);
    }
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let Some(off) = text[i..].find('&') else {
            out.push_str(&text[i..]);
            break;
        };
        out.push_str(&text[i..i + off]);
        i += off;
        let rest = &text[i + 1..];
        if let Some(num) = rest.strip_prefix('#') {
            let (hex, digits) = match num.strip_prefix(['x', 'X']) {
                Some(h) => (true, h),
                None => (false, num),
            };
            let len = digits.bytes().take_while(|c| if hex { c.is_ascii_hexdigit() } else { c.is_ascii_digit() }).count();
            if len > 0 {
                let value = u32::from_str_radix(&digits[..len.min(8)], if hex { 16 } else { 10 }).unwrap_or(0xfffd);
                let c = match value {
                    0 => '\u{fffd}',
                    0x80..=0x9f => windows_1252(value as u8),
                    v => char::from_u32(v).unwrap_or('\u{fffd}'),
                };
                out.push(c);
                let consumed = 1 + usize::from(hex) + len;
                i += 1 + consumed;
                if text[i..].starts_with(';') {
                    i += 1;
                }
                continue;
            }
        } else {
            let len = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
            // The longest name that is an entity (`&notin;` vs `&not`).
            let mut found = None;
            for l in (2..=len.min(32)).rev() {
                if let Some(c) = entities::lookup(&rest[..l]) {
                    found = Some((c, l));
                    break;
                }
            }
            if let Some((c, l)) = found {
                let semi = rest[l..].starts_with(';');
                let next = rest[l..].bytes().next();
                if semi || !(attribute && next.is_some_and(|n| n == b'=' || n.is_ascii_alphanumeric())) {
                    out.push(c);
                    i += 1 + l + usize::from(semi);
                    continue;
                }
            }
        }
        out.push('&');
        i += 1;
    }
    out
}

impl<'a> Tokenizer<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, at: 0, raw: None }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    /// The text up to the end tag of a raw text element.
    fn raw_text(&mut self, name: &str, entities: bool) -> Token {
        let rest = self.rest();
        let lower = rest.to_ascii_lowercase();
        let mut from = 0;
        let end = loop {
            match lower[from..].find("</") {
                Some(i) => {
                    let after = &lower[from + i + 2..];
                    if after.starts_with(name)
                        && after[name.len()..].starts_with(|c: char| c == '>' || c == '/' || c.is_ascii_whitespace())
                    {
                        break from + i;
                    }
                    from += i + 2;
                }
                None => break rest.len(),
            }
        };
        self.at += end;
        let text = &rest[..end];
        Token::Text(if entities { unescape(text, false) } else { String::from(text) })
    }

    fn tag(&mut self) -> Option<Token> {
        let rest = self.rest();
        let b = rest.as_bytes();
        // rest starts with '<'
        if rest.starts_with("<!--") {
            let end = rest[4..].find("-->").map_or(rest.len(), |i| i + 7);
            self.at += end;
            return Some(Token::Comment);
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = rest.find('>').map_or(rest.len(), |i| i + 1);
            self.at += end;
            let doctype = rest.len() > 9 && rest[2..9].eq_ignore_ascii_case("doctype");
            return Some(if doctype { Token::Doctype } else { Token::Comment });
        }
        let end_tag = b.get(1) == Some(&b'/');
        let start = if end_tag { 2 } else { 1 };
        if !b.get(start).is_some_and(u8::is_ascii_alphabetic) {
            if end_tag && b.get(2) == Some(&b'>') {
                self.at += 3;
                return Some(Token::Comment);
            }
            return None;
        }
        let mut i = start;
        while i < b.len() && !(b[i].is_ascii_whitespace() || b[i] == b'/' || b[i] == b'>') {
            i += 1;
        }
        let name = rest[start..i].to_ascii_lowercase();
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut self_closing = false;
        loop {
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
                if b[i] == b'/' {
                    self_closing = b.get(i + 1) == Some(&b'>');
                }
                i += 1;
            }
            if i >= b.len() {
                break;
            }
            if b[i] == b'>' {
                i += 1;
                break;
            }
            let name_start = i;
            i += 1;
            while i < b.len() && !(b[i].is_ascii_whitespace() || b[i] == b'/' || b[i] == b'>' || b[i] == b'=') {
                i += 1;
            }
            let attr = rest[name_start..i].to_ascii_lowercase();
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let mut value = String::new();
            if b.get(i) == Some(&b'=') {
                i += 1;
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                match b.get(i) {
                    Some(&q @ (b'"' | b'\'')) => {
                        let end = rest[i + 1..].find(q as char).map_or(b.len(), |e| i + 1 + e);
                        value = unescape(&rest[i + 1..end], true);
                        i = (end + 1).min(b.len());
                    }
                    _ => {
                        let s = i;
                        while i < b.len() && !(b[i].is_ascii_whitespace() || b[i] == b'>') {
                            i += 1;
                        }
                        value = unescape(&rest[s..i], true);
                    }
                }
            }
            if !attrs.iter().any(|(n, _)| *n == attr) {
                attrs.push((attr, value));
            }
        }
        self.at += i;
        if end_tag {
            return Some(Token::End { name });
        }
        match name.as_str() {
            "script" | "style" | "xmp" | "iframe" | "noembed" | "noframes" => {
                self.raw = Some((name.clone(), false));
            }
            "textarea" | "title" => self.raw = Some((name.clone(), true)),
            "plaintext" => self.raw = Some((String::from("\u{0}"), false)),
            _ => {}
        }
        if self_closing && self.raw.is_some() && !matches!(name.as_str(), "textarea" | "title") {
            // <script/> in HTML is not closed; leave it raw as browsers do.
        }
        Some(Token::Start { name, attrs, self_closing })
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        if let Some((name, entities)) = self.raw.take() {
            let token = self.raw_text(&name, entities);
            if token != Token::Text(String::new()) {
                return Some(token);
            }
        }
        let rest = self.rest();
        if rest.is_empty() {
            return None;
        }
        if rest.starts_with('<')
            && let Some(token) = self.tag()
        {
            return Some(token);
        }
        // Text up to the next '<' that may start a tag.
        let b = rest.as_bytes();
        let mut end = 1;
        while end < b.len() {
            if b[end] == b'<'
                && b.get(end + 1).is_some_and(|&c| c.is_ascii_alphabetic() || c == b'/' || c == b'!' || c == b'?')
            {
                break;
            }
            end += 1;
        }
        let text = &rest[..end];
        self.at += end;
        Some(Token::Text(unescape(text, false)))
    }
}

// ---------------------------------------------------------------- tree

struct Builder {
    doc: Document,
    stack: Vec<NodeId>,
    html: NodeId,
    head: NodeId,
    body: Option<NodeId>,
    /// Inside `<svg>` or `<math>`: self-closing tags close.
    foreign: usize,
}

impl Builder {
    fn current(&self) -> NodeId {
        *self.stack.last().unwrap_or(&self.html)
    }

    fn name(&self, id: NodeId) -> &str {
        self.doc.element(id).map_or("", |e| e.name.as_str())
    }

    fn body(&mut self) -> NodeId {
        if let Some(b) = self.body {
            return b;
        }
        let b = self.doc.append(self.html, NodeData::Element(Element::new("body", Vec::new())));
        self.body = Some(b);
        // Anything opened in the head is closed.
        self.stack.truncate(1);
        self.stack.push(b);
        b
    }

    /// Where `name` is in the stack, if it is in scope (`extra`: more
    /// elements that end the search).
    fn in_scope(&self, name: &str, extra: &[&str]) -> Option<usize> {
        for (i, &n) in self.stack.iter().enumerate().rev() {
            let here = self.name(n);
            if here == name {
                return Some(i);
            }
            if SCOPE.contains(&here) || extra.contains(&here) {
                return None;
            }
        }
        None
    }

    fn close(&mut self, name: &str, extra: &[&str]) -> bool {
        match self.in_scope(name, extra) {
            Some(i) => {
                self.stack.truncate(i.max(1));
                true
            }
            None => false,
        }
    }

    fn insert(&mut self, name: &str, attrs: Vec<(String, String)>, push: bool) -> NodeId {
        let parent = self.current();
        let id = self.doc.append(parent, NodeData::Element(Element::new(name, attrs)));
        if push {
            self.stack.push(id);
        }
        id
    }

    fn start(&mut self, name: String, attrs: Vec<(String, String)>, self_closing: bool) {
        let name = if name == "image" { String::from("img") } else { name };
        let n = name.as_str();
        if self.foreign > 0 {
            let id = self.insert(n, attrs, !self_closing);
            let _ = id;
            if !self_closing && (n == "svg" || n == "math") {
                self.foreign += 1;
            }
            return;
        }
        match n {
            "html" => {
                self.merge(self.html, attrs);
                return;
            }
            "body" => {
                let had = self.body.is_some();
                let b = self.body();
                if had {
                    self.merge(b, attrs);
                } else if let Some(e) = self.doc.element_mut(b) {
                    e.attrs = attrs;
                }
                return;
            }
            "head" => return,
            _ => {}
        }
        if self.body.is_none() && HEAD.contains(&n) {
            let id = self.doc.append(self.head, NodeData::Element(Element::new(n, attrs)));
            if !VOID.contains(&n) {
                // Its text (title, style, script) comes next; then its end.
                self.stack.push(id);
            }
            return;
        }
        self.body();
        if CLOSES_P.contains(&n) {
            self.close("p", &["button"]);
        }
        match n {
            "li" => {
                self.close("li", &["ul", "ol", "menu", "dir"]);
            }
            "dd" | "dt" => {
                if !self.close("dd", &["dl"]) {
                    self.close("dt", &["dl"]);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if matches!(self.name(self.current()), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                    self.stack.pop();
                }
            }
            "option" => {
                if self.name(self.current()) == "option" {
                    self.stack.pop();
                }
            }
            "optgroup" => {
                while matches!(self.name(self.current()), "option" | "optgroup") {
                    self.stack.pop();
                }
            }
            "a" => {
                self.close("a", &[]);
            }
            "button" => {
                self.close("button", &[]);
            }
            "nobr" => {
                self.close("nobr", &[]);
            }
            "tr" => {
                self.close_cells();
                self.close("tr", &["tbody", "thead", "tfoot"]);
                if self.name(self.current()) == "table" {
                    self.insert("tbody", Vec::new(), true);
                }
            }
            "td" | "th" => {
                self.close_cells();
                match self.name(self.current()) {
                    "table" => {
                        self.insert("tbody", Vec::new(), true);
                        self.insert("tr", Vec::new(), true);
                    }
                    "tbody" | "thead" | "tfoot" => {
                        self.insert("tr", Vec::new(), true);
                    }
                    _ => {}
                }
            }
            "tbody" | "thead" | "tfoot" => {
                self.close_cells();
                self.close("tr", &["tbody", "thead", "tfoot"]);
                for g in ["tbody", "thead", "tfoot"] {
                    self.close(g, &[]);
                }
            }
            "caption" | "colgroup" => {
                self.close_cells();
            }
            "svg" | "math" => {
                if !self_closing {
                    self.foreign += 1;
                }
                self.insert(n, attrs, !self_closing);
                return;
            }
            _ => {}
        }
        let push = !VOID.contains(&n);
        self.insert(n, attrs, push);
    }

    /// Closes an open cell or row before a new row or cell, inside the
    /// nearest table only.
    fn close_cells(&mut self) {
        if let Some(i) = self.in_scope_table(&["td", "th"]) {
            self.stack.truncate(i);
        }
    }

    /// Where the innermost of `names` is, without crossing a table.
    fn in_scope_table(&self, names: &[&str]) -> Option<usize> {
        for (i, &n) in self.stack.iter().enumerate().rev() {
            let here = self.name(n);
            if names.contains(&here) {
                return Some(i);
            }
            if matches!(here, "table" | "html") {
                return None;
            }
        }
        None
    }

    fn merge(&mut self, id: NodeId, attrs: Vec<(String, String)>) {
        if let Some(e) = self.doc.element_mut(id) {
            for (n, v) in attrs {
                if !e.attrs.iter().any(|(m, _)| *m == n) {
                    e.attrs.push((n, v));
                }
            }
        }
    }

    fn end(&mut self, name: &str) {
        if self.foreign > 0 {
            if let Some(i) = self.stack.iter().rposition(|&n| self.name(n) == name) {
                self.stack.truncate(i.max(1));
                if name == "svg" || name == "math" {
                    self.foreign -= 1;
                }
            }
            return;
        }
        match name {
            "html" | "body" | "head" => {
                if name == "head" && self.body.is_none() {
                    self.stack.truncate(1);
                }
            }
            "p" => {
                if !self.close("p", &["button"]) {
                    self.body();
                    self.insert("p", Vec::new(), false);
                }
            }
            "br" => {
                self.body();
                self.insert("br", Vec::new(), false);
            }
            "li" => {
                self.close("li", &["ul", "ol"]);
            }
            "table" | "td" | "th" | "tr" | "caption" => {
                // These cross the table edges they are.
                if let Some(i) = self.stack.iter().rposition(|&n| self.name(n) == name) {
                    let blocked = self.stack[i + 1..].iter().any(|&n| self.name(n) == "table");
                    if !blocked {
                        self.stack.truncate(i.max(1));
                    }
                }
            }
            _ => {
                if HEAD.contains(&name)
                    && self.body.is_none()
                    && let Some(i) = self.stack.iter().rposition(|&n| self.name(n) == name)
                {
                    self.stack.truncate(i.max(1));
                    return;
                }
                self.close(name, &[]);
            }
        }
    }

    fn text(&mut self, text: &str) {
        let current = self.current();
        let in_head = self.body.is_none();
        if in_head {
            let raw = matches!(self.name(current), "title" | "style" | "script" | "noscript" | "template");
            if raw {
                self.doc.append_text(current, text);
                return;
            }
            if text.bytes().all(|b| b.is_ascii_whitespace()) {
                return;
            }
            self.body();
        }
        let current = self.current();
        self.doc.append_text(current, text);
    }
}

/// Reads HTML text into a document: always `<html>` with `<head>` and
/// `<body>`.
pub fn parse(text: &str) -> Document {
    let mut doc = Document::new();
    let html = doc.append(DOCUMENT, NodeData::Element(Element::new("html", Vec::new())));
    let head = doc.append(html, NodeData::Element(Element::new("head", Vec::new())));
    let mut b = Builder { doc, stack: alloc::vec![html], html, head, body: None, foreign: 0 };
    for token in Tokenizer::new(text) {
        match token {
            Token::Start { name, attrs, self_closing } => b.start(name, attrs, self_closing),
            Token::End { name } => b.end(&name),
            Token::Text(t) => b.text(&t),
            Token::Doctype | Token::Comment => {}
        }
    }
    b.body();
    b.doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// The tree as text: `name(children)`, text in quotes.
    fn show(doc: &Document, id: NodeId) -> String {
        let node = &doc.nodes[id];
        let kids: Vec<String> = node.children.iter().map(|&c| show(doc, c)).collect();
        match &node.data {
            NodeData::Element(e) if kids.is_empty() => e.name.clone(),
            NodeData::Element(e) => format!("{}({})", e.name, kids.join(" ")),
            NodeData::Text(t) => format!("{t:?}"),
            _ => kids.join(" "),
        }
    }

    fn tree(html: &str) -> String {
        let doc = parse(html);
        show(&doc, DOCUMENT)
    }

    #[test]
    fn adds_the_missing_structure() {
        assert_eq!(tree("Bonjour"), r#"html(head body("Bonjour"))"#);
        assert_eq!(
            tree("<!DOCTYPE html><title>T</title><p>a<p>b"),
            r#"html(head(title("T")) body(p("a") p("b")))"#
        );
    }

    #[test]
    fn closes_what_browsers_close() {
        assert_eq!(tree("<ul><li>a<li>b</ul>"), r#"html(head body(ul(li("a") li("b"))))"#);
        assert_eq!(tree("<p>a<div>b</div>"), r#"html(head body(p("a") div("b")))"#);
        assert_eq!(tree("<dl><dt>a<dd>b<dt>c</dl>"), r#"html(head body(dl(dt("a") dd("b") dt("c"))))"#);
        assert_eq!(
            tree("<table><td>a<td>b<tr><td>c</table>"),
            r#"html(head body(table(tbody(tr(td("a") td("b")) tr(td("c"))))))"#
        );
        assert_eq!(tree("<a href=x>1<a>2</a>"), r#"html(head body(a("1") a("2")))"#);
        assert_eq!(tree("a<br>b</br>c"), r#"html(head body("a" br "b" br "c"))"#);
        assert_eq!(tree("<b>x</i>y</b>"), r#"html(head body(b("xy")))"#);
    }

    #[test]
    fn raw_text_and_references() {
        let doc = parse("<script>if (a<b) x='</p>';</script><p title='&eacute;t&eacute; &amp; &copy=1'>&lt;&#233;&#x20AC;&nbsp;&notit;&hellip;</p>");
        let p = doc.find("p").unwrap();
        assert_eq!(doc.element(p).unwrap().attr("title"), Some("été & &copy=1"));
        assert_eq!(doc.text_content(p), "<é€\u{a0}¬it;…");
        let s = doc.find("script").unwrap();
        assert_eq!(doc.text_content(s), "if (a<b) x='</p>';");
        assert_eq!(unescape("&#150; &#0; &amp", false), "– \u{fffd} &");
    }

    #[test]
    fn attributes_in_every_form() {
        let doc = parse(r#"<input type=checkbox checked value="a b" data-x='y' CLASS=c1>"#);
        let e = doc.element(doc.find("input").unwrap()).unwrap();
        assert_eq!(e.attr("type"), Some("checkbox"));
        assert_eq!(e.attr("checked"), Some(""));
        assert_eq!(e.attr("value"), Some("a b"));
        assert_eq!(e.attr("class"), Some("c1"));
        assert!(e.checked);
    }

    #[test]
    fn foreign_content_closes_itself() {
        assert_eq!(
            tree("<svg><path d=x /><g><rect/></g></svg><p>a"),
            r#"html(head body(svg(path g(rect)) p("a")))"#
        );
    }

    #[test]
    fn charsets() {
        assert_eq!(decode(b"caf\xe9", None), "café");
        assert_eq!(decode("café".as_bytes(), None), "café");
        assert_eq!(decode(b"\x80", Some("text/html; charset=windows-1252")), "€");
        assert_eq!(decode(b"<meta charset=iso-8859-1>\xe9", None), "<meta charset=iso-8859-1>é");
    }
}
