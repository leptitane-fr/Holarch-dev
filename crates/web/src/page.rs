//! A page: its address, its document, its style sheets and its layout,
//! and what the browser asks of it (the link under the pointer, where an
//! anchor is, what a form sends).

use crate::css::{self, Media, Sheet};
use crate::dom::{DOCUMENT, Document, NodeData, NodeId};
use crate::html;
use crate::layout::{self, FragKind, Layout};
use crate::paint;
use crate::style;
use crate::url::{self, Url};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

/// What a form sends: the address to load (`GET`), or the address and the
/// body (`POST`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    pub url: Url,
    pub post: Option<String>,
}

pub struct Page {
    pub url: Url,
    pub doc: Document,
    /// The text of each style sheet, in document order (inline `<style>`
    /// and loaded `<link>`), kept to read again when the viewport changes.
    sheets: Vec<(usize, String, Url)>,
    pub layout: Option<Layout>,
    /// The viewport the layout is for.
    size: (i32, i32),
    pub dark: bool,
    /// Changes whenever what the page draws may have changed; never the
    /// same for two pages (see [`Page::generation`]).
    generation: u32,
    /// The fields whose text changed in the layout without making it
    /// again: (generation before, generation after, field), in order.
    patches: Vec<(u32, u32, NodeId)>,
}

/// The last generation given to a page.
static GENERATION: AtomicU32 = AtomicU32::new(0);

fn fresh() -> u32 {
    GENERATION.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
}

fn is_stylesheet_link(doc: &Document, n: NodeId) -> bool {
    doc.element(n).is_some_and(|e| {
        e.name == "link"
            && e.attr("rel").is_some_and(|r| r.split_ascii_whitespace().any(|w| w.eq_ignore_ascii_case("stylesheet")))
            && !e.attr("rel").is_some_and(|r| r.to_ascii_lowercase().contains("alternate"))
            && e.attr("media").is_none_or(|m| {
                let m = m.to_ascii_lowercase();
                m.contains("screen") || m.contains("all") || m.trim().is_empty()
            })
    })
}

impl Page {
    /// A page from its bytes; `content_type` from the answer.
    pub fn new(url: Url, bytes: &[u8], content_type: Option<&str>) -> Page {
        let ct = content_type.unwrap_or("").to_ascii_lowercase();
        let looks_html = || {
            let head: String = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
            head.contains("<html") || head.contains("<!doctype") || head.contains("<head") || head.contains("<body")
        };
        let text = html::decode(bytes, content_type);
        let doc = if ct.starts_with("text/plain")
            || ct.contains("json")
            || ct.contains("javascript")
            || ct.starts_with("text/css")
            || (ct.is_empty() || ct.starts_with("application/octet")) && !looks_html()
        {
            plain(&text)
        } else {
            html::parse(&text)
        };
        let mut page = Page {
            url,
            doc,
            sheets: Vec::new(),
            layout: None,
            size: (0, 0),
            dark: false,
            generation: fresh(),
            patches: Vec::new(),
        };
        page.read_inline_sheets();
        page
    }

    /// A page from HTML text (the browser's own pages).
    pub fn from_html(url: Url, text: &str) -> Page {
        let mut page = Page {
            url,
            doc: html::parse(text),
            sheets: Vec::new(),
            layout: None,
            size: (0, 0),
            dark: false,
            generation: fresh(),
            patches: Vec::new(),
        };
        page.read_inline_sheets();
        page
    }

    fn read_inline_sheets(&mut self) {
        for n in self.doc.descendants(DOCUMENT) {
            if self.doc.is(n, "style") {
                let text = self.doc.text_content(n);
                self.sheets.push((n, text, self.base()));
            }
        }
    }

    /// The address links are resolved against (`<base href>`, else the
    /// page's).
    pub fn base(&self) -> Url {
        self.doc
            .find("base")
            .and_then(|b| self.doc.element(b))
            .and_then(|e| e.attr("href"))
            .and_then(|h| self.url.join(h))
            .unwrap_or_else(|| self.url.clone())
    }

    pub fn title(&self) -> String {
        self.doc.title()
    }

    /// The style sheets to load: their addresses, each with the node it
    /// comes from.
    pub fn stylesheet_links(&self) -> Vec<(NodeId, Url)> {
        let base = self.base();
        self.doc
            .descendants(DOCUMENT)
            .into_iter()
            .filter(|&n| is_stylesheet_link(&self.doc, n))
            .filter_map(|n| {
                let href = self.doc.element(n)?.attr("href")?;
                Some((n, base.join(href)?))
            })
            .collect()
    }

    /// Adds a loaded style sheet, at the place of its node; `url`: where
    /// it came from (its own links resolve against it).
    pub fn add_sheet(&mut self, node: NodeId, text: String, url: Url) {
        let at = self.sheets.iter().position(|(n, _, _)| *n > node).unwrap_or(self.sheets.len());
        self.sheets.insert(at, (node, text, url));
        self.invalidate();
    }

    /// The `@import`s of the sheets read so far, resolved.
    pub fn imports(&self) -> Vec<(NodeId, Url)> {
        let media = Media { width: 1024, height: 768, dark: self.dark };
        let mut out = Vec::new();
        for (n, text, url) in &self.sheets {
            if !text.contains("@import") {
                continue;
            }
            for i in css::parse_sheet(text, &media).imports {
                if let Some(u) = url.join(&i) {
                    out.push((*n, u));
                }
            }
        }
        out
    }

    /// Lays the page out for a viewport `width` × `height` (again only if
    /// it changed).
    pub fn lay_out(&mut self, width: i32, height: i32) -> &Layout {
        if self.layout.is_none() || self.size != (width, height) {
            let media = Media { width, height, dark: self.dark };
            let sheets: Vec<Sheet> = self.sheets.iter().map(|(_, t, _)| css::parse_sheet(t, &media)).collect();
            let styles = style::compute(&self.doc, &sheets, &media);
            let layout = match layout::build(&self.doc, &styles) {
                Some(root) => layout::layout(&root, width, height),
                None => {
                    let root = layout::build(&html::parse(""), &styles);
                    match root {
                        Some(r) => layout::layout(&r, width, height),
                        None => unreachable_layout(width, height),
                    }
                }
            };
            self.layout = Some(layout);
            self.size = (width, height);
            self.generation = fresh();
            self.patches.clear();
        }
        self.layout.as_ref().unwrap_or_else(|| unreachable!())
    }

    /// Forgets the layout (a form field changed, a sheet came).
    pub fn invalidate(&mut self) {
        self.layout = None;
        self.generation = fresh();
        self.patches.clear();
    }

    /// The fields typed in since `generation`, if nothing else changed
    /// since (none if it is the current one); `None` if the page changed
    /// otherwise, or is another page.
    pub fn patched_since(&self, generation: u32) -> Option<Vec<NodeId>> {
        let mut at = generation;
        let mut nodes = Vec::new();
        let start = self.patches.iter().position(|p| p.0 == generation);
        if let Some(start) = start {
            for &(before, after, node) in &self.patches[start..] {
                if before != at {
                    return None;
                }
                at = after;
                nodes.push(node);
            }
        }
        (at == self.generation).then_some(nodes)
    }

    /// What tells two drawings of the page apart: it changes whenever the
    /// layout does, and differs from one page to another, so an image of
    /// the page made under the same generation (and scroll, view and
    /// overlay) is still right.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The nearest ancestor (or itself) of `node` that is an element named
    /// one of `names`.
    pub fn ancestor(&self, mut node: NodeId, names: &[&str]) -> Option<NodeId> {
        loop {
            if let Some(e) = self.doc.element(node)
                && names.contains(&e.name.as_str())
            {
                return Some(node);
            }
            node = self.doc.parent(node)?;
        }
    }

    /// The link (`<a href>`) holding `node`, and where it goes.
    pub fn link_of(&self, node: NodeId) -> Option<(NodeId, Url)> {
        let mut at = Some(node);
        while let Some(n) = at {
            if let Some(e) = self.doc.element(n)
                && matches!(e.name.as_str(), "a" | "area")
                && let Some(href) = e.attr("href")
            {
                return Some((n, self.base().join(href)?));
            }
            at = self.doc.parent(n);
        }
        None
    }

    /// The innermost node under page point (x, y).
    pub fn node_at(&self, x: i32, y: i32) -> Option<NodeId> {
        let layout = self.layout.as_ref()?;
        let hits = paint::hit(layout, x, y);
        // Text and replaced elements first, then the smallest box.
        hits.iter()
            .rev()
            .find(|f| matches!(f.kind, FragKind::Text { .. } | FragKind::Replaced(_)))
            .or_else(|| hits.iter().rev().find(|f| f.node.is_some()))
            .and_then(|f| f.node)
    }

    /// Where the element named by a fragment (`#haut`) starts, in page
    /// pixels.
    pub fn anchor_y(&self, fragment: &str) -> Option<i32> {
        let fragment = url::decode(fragment);
        if fragment.is_empty() || fragment.eq_ignore_ascii_case("top") {
            return Some(0);
        }
        let node = self.doc.by_id(&fragment)?;
        self.y_of(node)
    }

    /// The top of the first fragment of `node` or what it holds.
    pub fn y_of(&self, node: NodeId) -> Option<i32> {
        let layout = self.layout.as_ref()?;
        let nodes = self.doc.descendants(node);
        let mut best: Option<i32> = None;
        let mut stack = alloc::vec![&layout.root];
        while let Some(f) = stack.pop() {
            if let Some(n) = f.node
                && nodes.contains(&n)
            {
                best = Some(best.map_or(f.y, |b| b.min(f.y)));
            }
            stack.extend(f.children.iter());
        }
        best
    }

    /// The fragment box of `node` (its first), in page pixels.
    pub fn rect_of(&self, node: NodeId) -> Option<(i32, i32, i32, i32)> {
        let layout = self.layout.as_ref()?;
        let mut stack = alloc::vec![&layout.root];
        while let Some(f) = stack.pop() {
            if f.node == Some(node) && !matches!(f.kind, FragKind::Text { .. }) {
                return Some((f.x, f.y, f.w, f.h));
            }
            stack.extend(f.children.iter().rev());
        }
        None
    }

    /// What can take the keyboard, in document order: links and form
    /// controls.
    pub fn focusable(&self) -> Vec<NodeId> {
        self.doc
            .descendants(DOCUMENT)
            .into_iter()
            .filter(|&n| {
                self.doc.element(n).is_some_and(|e| match e.name.as_str() {
                    "a" => e.attr("href").is_some(),
                    "input" => e.attr("type").is_none_or(|t| !t.eq_ignore_ascii_case("hidden")),
                    "button" | "select" | "textarea" => true,
                    _ => false,
                })
            })
            .filter(|&n| self.rect_of(n).is_some())
            .collect()
    }

    /// Whether `node` is a field that takes text.
    pub fn is_text_field(&self, node: NodeId) -> bool {
        self.doc.element(node).is_some_and(|e| match e.name.as_str() {
            "textarea" => true,
            "input" => matches!(
                e.attr("type").unwrap_or("text").to_ascii_lowercase().as_str(),
                "text" | "search" | "email" | "url" | "password" | "tel" | "number" | ""
            ),
            _ => false,
        })
    }

    /// Whether `node` submits its form when clicked.
    pub fn is_submit(&self, node: NodeId) -> bool {
        self.doc.element(node).is_some_and(|e| match e.name.as_str() {
            "button" => e.attr("type").is_none_or(|t| t.eq_ignore_ascii_case("submit")),
            "input" => {
                e.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("submit") || t.eq_ignore_ascii_case("image"))
            }
            _ => false,
        })
    }

    /// Ticks a checkbox, or picks a radio button (unticking the others of
    /// its group).
    pub fn toggle(&mut self, node: NodeId) {
        let Some(e) = self.doc.element(node) else { return };
        let kind = e.attr("type").unwrap_or("").to_ascii_lowercase();
        if kind == "checkbox" {
            if let Some(e) = self.doc.element_mut(node) {
                e.checked = !e.checked;
            }
        } else if kind == "radio" {
            let name = String::from(e.attr("name").unwrap_or(""));
            let form = self.ancestor(node, &["form"]);
            for n in self.doc.descendants(DOCUMENT) {
                let same = self.doc.element(n).is_some_and(|o| {
                    o.name == "input"
                        && o.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("radio"))
                        && o.attr("name").unwrap_or("") == name
                }) && self.ancestor(n, &["form"]) == form;
                if same && let Some(o) = self.doc.element_mut(n) {
                    o.checked = n == node;
                }
            }
        }
        self.invalidate();
    }

    /// The value of a text field.
    pub fn field_value(&self, node: NodeId) -> String {
        match self.doc.element(node) {
            Some(e) if e.name == "textarea" => e.value.clone().unwrap_or_else(|| self.doc.text_content(node)),
            Some(e) => String::from(e.current_value()),
            None => String::new(),
        }
    }

    pub fn set_field_value(&mut self, node: NodeId, value: String) {
        if let Some(e) = self.doc.element_mut(node) {
            e.value = Some(value);
        }
        // A field keeps its size whatever it holds: its text changes in
        // the layout, which is not made again for each letter typed.
        let patched = match (layout::replaced(&self.doc, node), self.layout.as_mut()) {
            (Some(r), Some(l)) => patch(&mut l.root, node, &r),
            _ => false,
        };
        if patched {
            let before = self.generation;
            self.generation = fresh();
            if self.patches.len() >= 64 {
                self.patches.clear();
            }
            self.patches.push((before, self.generation, node));
        } else {
            self.invalidate();
        }
    }

    /// What the form holding `node` sends when `node` submits it (or Entrée
    /// is pressed in it).
    pub fn submit(&self, node: NodeId) -> Option<Submission> {
        let form = self.ancestor(node, &["form"]);
        let fe = form.and_then(|f| self.doc.element(f));
        let action = fe.and_then(|e| e.attr("action")).filter(|a| !a.trim().is_empty());
        let target = match action {
            Some(a) => self.base().join(a)?,
            None => self.url.clone(),
        };
        let post = fe.and_then(|e| e.attr("method")).is_some_and(|m| m.eq_ignore_ascii_case("post"));
        let scope = form.unwrap_or(DOCUMENT);
        let mut pairs: Vec<(String, String)> = Vec::new();
        for n in self.doc.descendants(scope) {
            let Some(e) = self.doc.element(n) else { continue };
            let Some(name) = e.attr("name").filter(|s| !s.is_empty()) else { continue };
            if e.attr("disabled").is_some() {
                continue;
            }
            match e.name.as_str() {
                "input" => {
                    let kind = e.attr("type").unwrap_or("text").to_ascii_lowercase();
                    match kind.as_str() {
                        "checkbox" | "radio" => {
                            if e.checked {
                                pairs.push((String::from(name), String::from(e.attr("value").unwrap_or("on"))));
                            }
                        }
                        "submit" | "image" | "button" => {
                            if n == node {
                                pairs.push((String::from(name), String::from(e.attr("value").unwrap_or(""))));
                            }
                        }
                        "reset" | "file" => {}
                        _ => pairs.push((String::from(name), String::from(e.current_value()))),
                    }
                }
                "button" => {
                    if n == node {
                        pairs.push((String::from(name), String::from(e.attr("value").unwrap_or(""))));
                    }
                }
                "textarea" => pairs.push((String::from(name), self.field_value(n))),
                "select" => {
                    let options: Vec<NodeId> =
                        self.doc.descendants(n).into_iter().filter(|&o| self.doc.is(o, "option")).collect();
                    let chosen = options
                        .iter()
                        .copied()
                        .find(|&o| self.doc.element(o).is_some_and(|e| e.attr("selected").is_some()))
                        .or(options.first().copied());
                    if let Some(o) = chosen {
                        let oe = self.doc.element(o);
                        let value = match oe.and_then(|e| e.attr("value")) {
                            Some(v) => String::from(v),
                            None => crate::dom::collapse(&self.doc.text_content(o)),
                        };
                        pairs.push((String::from(name), value));
                    }
                }
                _ => {}
            }
        }
        let mut query = String::new();
        for (k, v) in pairs {
            if !query.is_empty() {
                query.push('&');
            }
            query.push_str(&url::form_encode(&k));
            query.push('=');
            query.push_str(&url::form_encode(&v));
        }
        if post {
            Some(Submission { url: target, post: Some(query) })
        } else {
            let mut u = target;
            u.query = Some(query);
            u.fragment = None;
            Some(Submission { url: u, post: None })
        }
    }

    /// The first text field of the page marked `autofocus`.
    pub fn autofocus(&self) -> Option<NodeId> {
        self.doc
            .descendants(DOCUMENT)
            .into_iter()
            .find(|&n| self.doc.element(n).is_some_and(|e| e.attr("autofocus").is_some()) && self.is_text_field(n))
    }

    /// The page's text, for « Rechercher dans la page »: how many times
    /// `needle` appears (ignoring case).
    pub fn count(&self, needle: &str) -> usize {
        if needle.is_empty() {
            return 0;
        }
        let needle = needle.to_lowercase();
        let Some(layout) = &self.layout else { return 0 };
        let mut n = 0;
        let mut stack = alloc::vec![&layout.root];
        while let Some(f) = stack.pop() {
            if let FragKind::Text { text, .. } = &f.kind {
                n += text.to_lowercase().matches(&needle).count();
            }
            stack.extend(f.children.iter());
        }
        n
    }

    /// The tops of the lines where `needle` appears, in page order.
    pub fn found_at(&self, needle: &str) -> Vec<i32> {
        let needle = needle.to_lowercase();
        let Some(layout) = &self.layout else { return Vec::new() };
        let mut out = Vec::new();
        let mut stack = alloc::vec![&layout.root];
        while let Some(f) = stack.pop() {
            if let FragKind::Text { text, .. } = &f.kind
                && !needle.is_empty()
                && text.to_lowercase().contains(&needle)
            {
                out.push(f.y);
            }
            stack.extend(f.children.iter());
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The `<meta http-equiv=refresh>` target, if it goes at once.
    pub fn refresh(&self) -> Option<Url> {
        for n in self.doc.descendants(DOCUMENT) {
            let Some(e) = self.doc.element(n) else { continue };
            if e.name != "meta" || !e.attr("http-equiv").is_some_and(|h| h.eq_ignore_ascii_case("refresh")) {
                continue;
            }
            let content = e.attr("content")?;
            let (delay, rest) = content.split_once([';', ',']).unwrap_or((content, ""));
            if delay.trim().parse::<u32>().ok()? > 3 {
                return None;
            }
            let target = rest.trim().trim_start_matches(|c: char| c.is_ascii_alphabetic() && c != '=').trim();
            let target = target.strip_prefix(['u', 'U']).map_or(target, |t| t);
            let target = target.trim_start_matches(['r', 'R', 'l', 'L', '=', ' ']).trim_matches(['\'', '"']);
            return if target.is_empty() { None } else { self.base().join(target) };
        }
        None
    }

    /// Whether `node` lies in the page's document as a text node.
    pub fn is_text(&self, node: NodeId) -> bool {
        matches!(self.doc.nodes.get(node).map(|n| &n.data), Some(NodeData::Text(_)))
    }
}

/// A text document shown as it is.
fn plain(text: &str) -> Document {
    let mut doc = html::parse("<pre style='white-space:pre-wrap; margin:8px'></pre>");
    if let Some(pre) = doc.find("pre") {
        doc.append_text(pre, text);
    }
    doc
}

fn unreachable_layout(_w: i32, _h: i32) -> Layout {
    unreachable!("the browser's sheet always gives <html> a box")
}

/// Gives the fragments of `node` its new content; whether there was one.
fn patch(f: &mut layout::Frag, node: NodeId, to: &layout::Replaced) -> bool {
    let mut found = false;
    if f.node == Some(node)
        && let FragKind::Replaced(r) = &mut f.kind
    {
        *r = to.clone();
        found = true;
    }
    for c in &mut f.children {
        found |= patch(c, node, to);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn page(html: &str) -> Page {
        let mut p = Page::from_html(Url::parse("https://exemple.fr/dossier/page.html").unwrap(), html);
        p.lay_out(800, 600);
        p
    }

    #[test]
    fn links_and_anchors() {
        let p = page(
            "<p style='margin:0'><a href='autre.html#x'>lien</a></p><div style='height:900px'></div><h2 id=bas>Bas</h2>",
        );
        let node = p.node_at(10, 8).unwrap();
        let (_, url) = p.link_of(node).unwrap();
        assert_eq!(url.to_string(), "https://exemple.fr/dossier/autre.html#x");
        assert!(p.anchor_y("bas").unwrap() > 900);
        assert_eq!(p.title(), "");
    }

    #[test]
    fn forms_submit_their_fields() {
        let mut p = page(
            "<form action='/cherche'><input name=q value='âne gris'><input type=hidden name=l value=fr>\
             <input type=checkbox name=c checked><select name=s><option value=1>Un<option selected>Deux</select>\
             <button name=go value=1>OK</button></form>",
        );
        let button = p.doc.find("button").unwrap();
        let q = p.doc.find("input").unwrap();
        p.set_field_value(q, String::from("le p'tit âne"));
        let s = p.submit(button).unwrap();
        assert_eq!(s.url.to_string(), "https://exemple.fr/cherche?q=le+p%27tit+%C3%A2ne&l=fr&c=on&s=Deux&go=1");
        assert!(p.is_text_field(q) && p.is_submit(button));
    }

    #[test]
    fn stylesheets_and_text_pages() {
        let p = page(
            "<base href='https://cdn.exemple.fr/'><link rel=stylesheet href=a.css><link rel='alternate stylesheet' href=b.css>",
        );
        let links = p.stylesheet_links();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].1.to_string(), "https://cdn.exemple.fr/a.css");
        let t = Page::new(Url::parse("https://e.fr/x.txt").unwrap(), b"<b>pas du HTML</b>", Some("text/plain"));
        assert_eq!(t.doc.text_content(t.doc.find("pre").unwrap()), "<b>pas du HTML</b>");
        let r = page("<meta http-equiv=refresh content='0; url=/ailleurs'>");
        assert_eq!(r.refresh().unwrap().to_string(), "https://exemple.fr/ailleurs");
    }

    #[test]
    fn typing_keeps_the_layout() {
        let mut p = page("<form><input name=q placeholder='Rechercher'></form>");
        let input = p.doc.find("input").unwrap();
        let before = p.generation();
        p.set_field_value(input, String::from("âne"));
        assert!(p.layout.is_some() && p.generation() != before);
        fn text(f: &layout::Frag, node: NodeId) -> Option<layout::Replaced> {
            if f.node == Some(node)
                && let FragKind::Replaced(r) = &f.kind
            {
                return Some(r.clone());
            }
            f.children.iter().find_map(|c| text(c, node))
        }
        let shown = text(&p.layout.as_ref().unwrap().root, input);
        assert_eq!(
            shown,
            Some(layout::Replaced::Field { text: String::from("âne"), placeholder: false, password: false })
        );
        assert_eq!(p.patched_since(before), Some(alloc::vec![input]));
        assert_eq!(p.patched_since(p.generation()), Some(Vec::new()));
        let mid = p.generation();
        p.set_field_value(input, String::from("ânes"));
        assert_eq!(p.patched_since(before), Some(alloc::vec![input, input]));
        assert_eq!(p.patched_since(mid), Some(alloc::vec![input]));
        p.invalidate();
        assert_eq!(p.patched_since(mid), None);
        assert_ne!(page("").generation(), page("").generation());
    }
}
