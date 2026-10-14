//! The browser around the engine: tabs, each with its own history, the
//! address bar, bookmarks, the browser's own pages, finding text in a
//! page, and how keys and the pointer drive all of it.
//!
//! It draws itself on a [`Canvas`] ([`Browser::draw`]) and reaches the
//! network only through [`Net`], one request at a time
//! ([`Browser::step`]), so the holon and the bench share every line.
//! A page is loaded in stages: its document, then its style sheets and
//! what they `@import`; the page shown stays until the new one is whole.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod chrome;
pub mod edit;
pub mod net;
pub mod pages;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use edit::Line;
use hln_web::dom::NodeId;
use hln_web::page::Page;
use hln_web::paint::{Canvas, Overlay};
use hln_web::url::{self, Url};
use net::{Net, NetError};
use pages::Saved;

pub use chrome::{BAR, FIND, TABS};

/// Where words typed in the address bar are searched.
pub const SEARCH: &str = "https://html.duckduckgo.com/html/?q=";
/// The page a new tab opens on.
pub const HOME: &str = "about:accueil";
/// How many pages the history of the session keeps.
const HISTORY: usize = 200;
/// How many times a page may send to another one at once (`meta refresh`).
const REDIRECTS: u8 = 5;
/// How far an arrow key or a notch of the wheel moves the page.
const STEP: i32 = 48;

/// A key, as the browser understands it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
}

/// The keys held with another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { ctrl: false, shift: false, alt: false };
    pub const CTRL: Mods = Mods { ctrl: true, shift: false, alt: false };
    pub const ALT: Mods = Mods { ctrl: false, shift: false, alt: true };
}

/// What reaches the browser from the person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Key(Key, Mods),
    /// The pointer, in the browser's pixels, and whether the left button
    /// is held.
    Pointer {
        x: i32,
        y: i32,
        left: bool,
    },
    /// The wheel, in notches: positive goes down the page.
    Wheel(i32),
    /// The window's new size.
    Resize(i32, i32),
}

/// Where the keys go.
#[derive(Clone, PartialEq, Eq)]
enum Focus {
    Page,
    Address,
    /// A text field of the page, and what is being typed in it.
    Field(NodeId),
    Find,
}

/// A page in a tab's history.
struct Entry {
    url: Url,
    post: Option<String>,
    title: String,
    /// Where the page was scrolled when left.
    scroll: i32,
}

/// Why a page is being loaded: how the tab's history changes once it is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A new page: it ends the history after the current one.
    New,
    /// The current page again, or another in its place (a redirection).
    Replace,
    /// The page at this place in the history.
    Back(usize),
}

enum Stage {
    Document,
    /// The page has come; its style sheets are loading.
    Sheets {
        page: alloc::boxed::Box<Page>,
        queue: Vec<(NodeId, Url)>,
        seen: Vec<Url>,
    },
}

struct Load {
    url: Url,
    post: Option<String>,
    kind: Kind,
    stage: Stage,
    redirects: u8,
    /// Requests made and known, for the progress bar.
    done: usize,
    total: usize,
}

pub struct Tab {
    pub page: Page,
    entries: Vec<Entry>,
    at: usize,
    scroll: i32,
    load: Option<Load>,
    /// The link or control the keyboard is on.
    ring: Option<NodeId>,
    /// The page came over `https` (not one of the browser's own pages).
    pub secure: bool,
}

impl Tab {
    /// What the tab's label says.
    pub fn title(&self) -> String {
        let t = self.page.title();
        let t = t.trim();
        if !t.is_empty() {
            String::from(t)
        } else if self.page.url.scheme == "about" {
            String::from("Nouvel onglet")
        } else {
            self.page.url.pretty()
        }
    }

    pub fn loading(&self) -> bool {
        self.load.is_some()
    }

    /// Where the tab is in its history, or will be once the page it is
    /// going back or forward to has come.
    fn position(&self) -> usize {
        match &self.load {
            Some(Load { kind: Kind::Back(i), .. }) => *i,
            _ => self.at,
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.position() > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.position() + 1 < self.entries.len()
    }
}

pub struct Browser {
    pub tabs: Vec<Tab>,
    pub current: usize,
    pub width: i32,
    pub height: i32,
    focus: Focus,
    /// The address bar, while it is being typed in.
    address: Line,
    /// The suggestion picked under the address bar, if any.
    picked: Option<usize>,
    /// The field being typed in, if the focus is on one.
    field: Line,
    /// The find bar: open, its text, and which place found is shown.
    finding: bool,
    find: Line,
    found: usize,
    menu: bool,
    pub bookmarks: Vec<Saved>,
    pub history: Vec<Saved>,
    pointer: (i32, i32),
    left: bool,
    /// The person drags the scroll bar's thumb, grabbed this far below its
    /// top.
    dragging: Option<i32>,
    /// The page as last painted, to draw again without painting it when
    /// only the browser around it changed (a letter in the address bar,
    /// the pointer over a button).
    painted: Option<(Painted, Vec<u32>)>,
}

/// What a painted page depends on.
#[derive(PartialEq, Eq)]
struct Painted {
    generation: u32,
    view: (i32, i32, i32, i32),
    scroll: i32,
    overlay: Overlay,
}

impl Browser {
    /// A browser of `width` × `height` pixels with one tab, on its home page.
    pub fn new(width: i32, height: i32) -> Browser {
        let mut b = Browser {
            tabs: Vec::new(),
            current: 0,
            width,
            height,
            focus: Focus::Page,
            address: Line::default(),
            picked: None,
            field: Line::default(),
            finding: false,
            find: Line::default(),
            found: 0,
            menu: false,
            bookmarks: Vec::new(),
            history: Vec::new(),
            pointer: (-1, -1),
            left: false,
            dragging: None,
            painted: None,
        };
        b.new_tab(None);
        b
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.current]
    }

    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.current]
    }

    /// The window's title: the page's.
    pub fn title(&self) -> String {
        self.tab().title()
    }

    /// Whether a tab is still loading (call [`step`](Self::step)).
    pub fn busy(&self) -> bool {
        self.tabs.iter().any(|t| t.load.is_some())
    }

    /// The rectangle of the window the page is shown in.
    pub fn view(&self) -> (i32, i32, i32, i32) {
        let bottom = if self.finding { self.height - FIND } else { self.height };
        (0, TABS + BAR, self.width, bottom.max(TABS + BAR + 1))
    }

    fn view_height(&self) -> i32 {
        let v = self.view();
        v.3 - v.1
    }

    // ------------------------------------------------------------ tabs

    /// Opens a tab after the current one, on `url` (else the home page),
    /// and shows it.
    pub fn new_tab(&mut self, url: Option<Url>) {
        let home = Url::parse(HOME).unwrap_or_else(|| unreachable!());
        let blank = Page::from_html(home.clone(), "");
        let tab = Tab { page: blank, entries: Vec::new(), at: 0, scroll: 0, load: None, ring: None, secure: false };
        let at = if self.tabs.is_empty() { 0 } else { self.current + 1 };
        self.tabs.insert(at, tab);
        self.current = at;
        self.focus = Focus::Page;
        self.menu = false;
        let blank = url.is_none();
        self.go(url.unwrap_or(home), None, Kind::New);
        if blank {
            self.focus_address();
        }
    }

    /// Closes tab `i`; the last one closed leaves a new one.
    pub fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.current = 0;
            self.new_tab(None);
            return;
        }
        if self.current > i || self.current >= self.tabs.len() {
            self.current = self.current.saturating_sub(1);
        }
        self.leave_focus();
    }

    pub fn select_tab(&mut self, i: usize) {
        if i < self.tabs.len() && i != self.current {
            self.current = i;
            self.leave_focus();
        }
    }

    fn leave_focus(&mut self) {
        self.focus = Focus::Page;
        self.menu = false;
        self.picked = None;
        self.finding = false;
    }

    // ------------------------------------------------------------ going places

    /// Opens what was typed in the address bar: an address, or words to
    /// search.
    pub fn open(&mut self, typed: &str) {
        if let Some(url) = url::from_typed(typed, SEARCH) {
            self.focus = Focus::Page;
            self.go(url, None, Kind::New);
        }
    }

    /// Starts loading `url` in the current tab. The browser's own pages,
    /// a jump within the page and what cannot be fetched are shown at once.
    fn go(&mut self, url: Url, post: Option<String>, kind: Kind) {
        self.go_in(self.current, url, post, kind, 0);
    }

    fn go_in(&mut self, t: usize, url: Url, post: Option<String>, kind: Kind, redirects: u8) {
        self.menu = false;
        let tab = &mut self.tabs[t];
        // A jump to an anchor of the page shown.
        if kind == Kind::New
            && post.is_none()
            && url.fragment.is_some()
            && !tab.entries.is_empty()
            && url.same_document(&tab.page.url)
            && tab.load.is_none()
        {
            let y = tab.page.anchor_y(url.fragment.as_deref().unwrap_or(""));
            let title = tab.page.title();
            Self::push_entry(tab, url.clone(), None, title);
            tab.page.url = url;
            if let Some(y) = y {
                tab.scroll = y;
            }
            self.clamp_scroll(t);
            return;
        }
        let name = url.path.clone();
        match url.scheme.as_str() {
            "about" => {
                let html = pages::about(&name, SEARCH, &self.bookmarks, &self.history)
                    .unwrap_or_else(|| pages::unknown(&name));
                let page = Page::from_html(url.clone(), &html);
                self.tabs[t].load = None;
                self.install(t, page, url, None, kind, redirects);
            }
            "http" | "https" | "file" | "view-source" => {
                self.tabs[t].load =
                    Some(Load { url, post, kind, stage: Stage::Document, redirects, done: 0, total: 1 });
            }
            _ => {
                let page = Page::from_html(url.clone(), &pages::error(&url, &NetError::Unsupported));
                self.tabs[t].load = None;
                self.install(t, page, url, None, kind, redirects);
            }
        }
    }

    fn push_entry(tab: &mut Tab, url: Url, post: Option<String>, title: String) {
        if let Some(e) = tab.entries.get_mut(tab.at) {
            e.scroll = tab.scroll;
        }
        if !tab.entries.is_empty() {
            tab.entries.truncate(tab.at + 1);
        }
        tab.entries.push(Entry { url, post, title, scroll: 0 });
        tab.at = tab.entries.len() - 1;
    }

    /// Shows a page that has come, in tab `t`.
    fn install(&mut self, t: usize, mut page: Page, asked: Url, post: Option<String>, kind: Kind, redirects: u8) {
        let (w, h) = (self.width, self.view_height());
        page.lay_out(w, h);
        let title = page.title();
        let tab = &mut self.tabs[t];
        // The address shown is where the answer came from, with the anchor asked.
        if page.url.fragment.is_none() && asked.fragment.is_some() && page.url.same_document(&asked) {
            page.url.fragment = asked.fragment.clone();
        }
        let url = page.url.clone();
        let mut scroll = 0;
        match kind {
            Kind::New => Self::push_entry(tab, url.clone(), post, title.clone()),
            Kind::Replace => {
                if tab.entries.is_empty() {
                    Self::push_entry(tab, url.clone(), post, title.clone());
                } else {
                    // Reloading keeps the place in the page.
                    if tab.page.url.same_document(&url) {
                        scroll = tab.scroll;
                    }
                    let e = &mut tab.entries[tab.at];
                    (e.url, e.post, e.title) = (url.clone(), post, title.clone());
                }
            }
            Kind::Back(i) => {
                if let Some(e) = tab.entries.get_mut(tab.at) {
                    e.scroll = tab.scroll;
                }
                tab.at = i.min(tab.entries.len().saturating_sub(1));
                if let Some(e) = tab.entries.get_mut(tab.at) {
                    scroll = e.scroll;
                    e.title = title.clone();
                }
            }
        }
        if !matches!(kind, Kind::Back(_))
            && let Some(f) = &url.fragment
            && let Some(y) = page.anchor_y(f)
        {
            scroll = y;
        }
        let refresh = page.refresh();
        let autofocus = page.autofocus();
        tab.secure = url.scheme == "https";
        tab.page = page;
        tab.scroll = scroll;
        tab.ring = None;
        self.clamp_scroll(t);
        if matches!(url.scheme.as_str(), "http" | "https" | "file") {
            self.history.retain(|s| s.url != url);
            self.history.insert(0, Saved { title, url: url.clone() });
            self.history.truncate(HISTORY);
        }
        if t == self.current {
            if self.focus != Focus::Address {
                self.focus = Focus::Page;
            }
            if let Some(n) = autofocus
                && self.focus == Focus::Page
            {
                self.focus_field(n);
            }
            if self.finding {
                self.found = 0;
            }
        }
        if let Some(target) = refresh
            && redirects < REDIRECTS
            && target != url
        {
            self.go_in(t, target, None, Kind::Replace, redirects + 1);
        }
    }

    /// Makes one request for the tab loading (the current one first).
    /// Returns whether anything changed on screen.
    pub fn step(&mut self, net: &mut dyn Net) -> bool {
        let order = core::iter::once(self.current).chain(0..self.tabs.len());
        let Some(t) = order.into_iter().find(|&t| t < self.tabs.len() && self.tabs[t].load.is_some()) else {
            return false;
        };
        let Some(load) = self.tabs[t].load.take() else { return false };
        let Load { url, post, kind, stage, redirects, done, total } = load;
        match stage {
            Stage::Document => {
                let (fetched, source) = match url.scheme.as_str() {
                    "view-source" => match Url::parse(&url.path) {
                        Some(inner) => (net.fetch(&inner, None), true),
                        None => (Err(NetError::Unsupported), true),
                    },
                    _ => (net.fetch(&url, post.as_deref()), false),
                };
                match fetched {
                    Ok(r) => {
                        let page = if source {
                            let mut p = Page::new(url.clone(), &r.body, Some("text/plain"));
                            p.url = url.clone();
                            p
                        } else {
                            Page::new(r.url, &r.body, r.content_type.as_deref())
                        };
                        let queue = page.stylesheet_links();
                        let total = done + 1 + queue.len();
                        let stage = Stage::Sheets { page: alloc::boxed::Box::new(page), queue, seen: Vec::new() };
                        self.tabs[t].load = Some(Load { url, post, kind, stage, redirects, done: done + 1, total });
                    }
                    Err(e) => {
                        let page = Page::from_html(url.clone(), &pages::error(&url, &e));
                        self.install(t, page, url, post, kind, redirects);
                        self.tabs[t].secure = false;
                    }
                }
            }
            Stage::Sheets { mut page, mut queue, mut seen } => {
                if queue.is_empty() {
                    // Every sheet came: what they import, once each.
                    queue = page.imports().into_iter().filter(|(_, u)| !seen.contains(u)).collect();
                    if queue.is_empty() {
                        self.install(t, *page, url, post, kind, redirects);
                        return true;
                    }
                }
                let (node, sheet) = queue.remove(0);
                let total = total.max(done + 1 + queue.len());
                if !seen.contains(&sheet) {
                    if let Ok(r) = net.fetch(&sheet, None) {
                        page.add_sheet(node, String::from_utf8_lossy(&r.body).into_owned(), r.url);
                    }
                    seen.push(sheet);
                }
                let stage = Stage::Sheets { page, queue, seen };
                self.tabs[t].load = Some(Load { url, post, kind, stage, redirects, done: done + 1, total });
            }
        }
        true
    }

    /// The share of the current tab's load done, in thousandths.
    pub fn progress(&self) -> Option<i32> {
        let l = self.tab().load.as_ref()?;
        Some((l.done as i32 * 900 / l.total.max(1) as i32 + 100).min(1000))
    }

    pub fn back(&mut self) {
        let at = self.tab().position();
        if at > 0 {
            self.history_go(at - 1);
        }
    }

    pub fn forward(&mut self) {
        let at = self.tab().position();
        if at + 1 < self.tab().entries.len() {
            self.history_go(at + 1);
        }
    }

    fn history_go(&mut self, i: usize) {
        let tab = self.tab_mut();
        let (url, post) = (tab.entries[i].url.clone(), tab.entries[i].post.clone());
        // Another place in the same page: no need to load it again.
        if tab.page.url.same_document(&url) && post.is_none() && tab.load.is_none() {
            tab.entries[tab.at].scroll = tab.scroll;
            tab.at = i;
            tab.scroll = tab.entries[i].scroll;
            tab.page.url = url;
            return;
        }
        self.focus = Focus::Page;
        self.go(url, post, Kind::Back(i));
    }

    /// Loads the page again, or stops loading it.
    pub fn reload(&mut self) {
        if self.tab().load.is_some() {
            self.tab_mut().load = None;
            return;
        }
        let tab = self.tab();
        let url = tab.page.url.clone();
        let post = tab.entries.get(tab.at).and_then(|e| e.post.clone());
        self.focus = Focus::Page;
        self.go(url, post, Kind::Replace);
    }

    pub fn home(&mut self) {
        self.focus = Focus::Page;
        if let Some(home) = Url::parse(HOME) {
            self.go(home, None, Kind::New);
        }
    }

    /// Whether the page shown is in the bookmarks.
    pub fn bookmarked(&self) -> bool {
        let url = &self.tab().page.url;
        self.bookmarks.iter().any(|s| &s.url == url)
    }

    /// Adds the page shown to the bookmarks, or takes it out.
    pub fn toggle_bookmark(&mut self) {
        let url = self.tab().page.url.clone();
        if self.bookmarks.iter().any(|s| s.url == url) {
            self.bookmarks.retain(|s| s.url != url);
        } else if url.scheme != "about" {
            let title = self.tab().title();
            self.bookmarks.push(Saved { title, url });
        }
    }

    // ------------------------------------------------------------ scrolling

    fn max_scroll(&self, t: usize) -> i32 {
        let h = self.tabs[t].page.layout.as_ref().map_or(0, |l| l.height);
        (h - self.view_height()).max(0)
    }

    fn clamp_scroll(&mut self, t: usize) {
        let max = self.max_scroll(t);
        let tab = &mut self.tabs[t];
        tab.scroll = tab.scroll.clamp(0, max);
    }

    pub fn scroll_by(&mut self, dy: i32) {
        self.tab_mut().scroll += dy;
        self.clamp_scroll(self.current);
    }

    pub fn scroll_to(&mut self, y: i32) {
        self.tab_mut().scroll = y;
        self.clamp_scroll(self.current);
    }

    pub fn scroll(&self) -> i32 {
        self.tab().scroll
    }

    /// Scrolls so that the page's band from `top` to `bottom` is seen.
    fn reveal(&mut self, top: i32, bottom: i32) {
        let h = self.view_height();
        let s = self.tab().scroll;
        if top < s + 8 {
            self.scroll_to(top - 24);
        } else if bottom > s + h - 8 {
            self.scroll_to(bottom - h + 24);
        }
    }

    /// The page's size changed: lays it out again.
    pub fn resize(&mut self, width: i32, height: i32) {
        self.width = width.max(200);
        self.height = height.max(TABS + BAR + 40);
        self.relayout();
    }

    fn relayout(&mut self) {
        let (w, h) = (self.width, self.view_height());
        for t in 0..self.tabs.len() {
            self.tabs[t].page.lay_out(w, h);
            self.clamp_scroll(t);
        }
    }

    // ------------------------------------------------------------ focus

    /// Puts the keys in the address bar, its text all selected.
    pub fn focus_address(&mut self) {
        let text = self.address_text();
        self.address.set(&text);
        self.focus = Focus::Address;
        self.picked = None;
        self.menu = false;
    }

    /// What the address bar shows when it is not typed in.
    pub fn address_text(&self) -> String {
        let tab = self.tab();
        if let Some(l) = &tab.load {
            return l.url.to_string();
        }
        let url = &tab.page.url;
        if url.scheme == "about" && url.path == "accueil" { String::new() } else { url.to_string() }
    }

    fn focus_field(&mut self, node: NodeId) {
        let value = self.tab().page.field_value(node);
        self.field = Line::default();
        self.field.text = value;
        self.field.end();
        self.focus = Focus::Field(node);
        self.tab_mut().ring = Some(node);
        if let Some((_, y, _, h)) = self.tab().page.rect_of(node) {
            self.reveal(y, y + h);
        }
    }

    pub fn open_find(&mut self) {
        let was = self.finding;
        self.finding = true;
        self.focus = Focus::Find;
        self.find.all = true;
        self.find.caret = self.find.len();
        self.menu = false;
        if !was {
            self.relayout();
        }
    }

    fn close_find(&mut self) {
        self.finding = false;
        self.focus = Focus::Page;
        self.relayout();
    }

    /// Shows the next place (or the previous one) the find bar's text is.
    fn find_next(&mut self, back: bool) {
        let places = self.tab().page.found_at(&self.find.text);
        if places.is_empty() {
            return;
        }
        let n = places.len();
        self.found = match (self.found < n, back) {
            (false, false) => 0,
            (false, true) => n - 1,
            (true, false) => (self.found + 1) % n,
            (true, true) => (self.found + n - 1) % n,
        };
        let y = places[self.found];
        self.reveal(y, y + 24);
    }

    /// The suggestions under the address bar: bookmarks first, then
    /// history, whose title or address holds what is typed.
    pub fn suggestions(&self) -> Vec<&Saved> {
        if self.focus != Focus::Address || self.address.all || self.address.text.trim().is_empty() {
            return Vec::new();
        }
        let typed = self.address.text.trim().to_lowercase();
        let mut out: Vec<&Saved> = Vec::new();
        for s in self.bookmarks.iter().chain(self.history.iter()) {
            let hit = s.url.to_string().to_lowercase().contains(&typed) || s.title.to_lowercase().contains(&typed);
            if hit && !out.iter().any(|o| o.url == s.url) {
                out.push(s);
            }
            if out.len() == 6 {
                break;
            }
        }
        out
    }

    // ------------------------------------------------------------ events

    /// Handles what the person did. Returns whether the window must be
    /// drawn again.
    pub fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key, mods) => {
                self.key(key, mods);
                true
            }
            Event::Pointer { x, y, left } => {
                let moved = (x, y) != self.pointer;
                let pressed = left && !self.left;
                self.pointer = (x, y);
                self.left = left;
                if !left {
                    self.dragging = None;
                }
                if let Some(grab) = self.dragging {
                    self.drag_thumb(y - grab);
                    return true;
                }
                if pressed {
                    self.click(x, y);
                    return true;
                }
                moved
            }
            Event::Wheel(notches) => {
                if self.menu {
                    return false;
                }
                self.scroll_by(notches * STEP * 2);
                true
            }
            Event::Resize(w, h) => {
                self.resize(w, h);
                true
            }
        }
    }

    fn key(&mut self, key: Key, mods: Mods) {
        // What works wherever the keys are.
        if mods.ctrl {
            match key {
                Key::Char('l' | 'L') => return self.focus_address(),
                Key::Char('t' | 'T') => return self.new_tab(None),
                Key::Char('w' | 'W') => return self.close_tab(self.current),
                Key::Char('f' | 'F') => return self.open_find(),
                Key::Char('d' | 'D') => return self.toggle_bookmark(),
                Key::Char('r' | 'R') => return self.reload(),
                Key::Char('h' | 'H') => {
                    return self.go(Url::parse("about:historique").unwrap_or_else(|| unreachable!()), None, Kind::New);
                }
                Key::Tab => {
                    let n = self.tabs.len();
                    let next = if mods.shift { (self.current + n - 1) % n } else { (self.current + 1) % n };
                    return self.select_tab(next);
                }
                _ => {}
            }
        }
        if mods.alt {
            match key {
                Key::Left => return self.back(),
                Key::Right => return self.forward(),
                Key::Home => return self.home(),
                _ => {}
            }
        }
        if self.menu {
            if key == Key::Escape {
                self.menu = false;
            }
            return;
        }
        match self.focus.clone() {
            Focus::Address => self.key_address(key),
            Focus::Find => self.key_find(key, mods),
            Focus::Field(node) => self.key_field(node, key, mods),
            Focus::Page => self.key_page(key, mods),
        }
    }

    fn edit(line: &mut Line, key: Key) -> bool {
        match key {
            Key::Char(c) if c >= ' ' => line.insert(c),
            Key::Backspace => line.backspace(),
            Key::Delete => line.delete(),
            Key::Left => line.go(-1),
            Key::Right => line.go(1),
            Key::Home => line.home(),
            Key::End => line.end(),
            _ => return false,
        }
        true
    }

    fn key_address(&mut self, key: Key) {
        let n = self.suggestions().len();
        match key {
            Key::Enter => {
                let chosen = self.picked.and_then(|i| self.suggestions().get(i).map(|s| s.url.clone()));
                self.picked = None;
                match chosen {
                    Some(url) => {
                        self.focus = Focus::Page;
                        self.go(url, None, Kind::New);
                    }
                    None => {
                        let typed = self.address.text.clone();
                        self.open(&typed);
                    }
                }
            }
            Key::Escape => {
                self.focus = Focus::Page;
                self.picked = None;
            }
            Key::Down if n > 0 => self.picked = Some(self.picked.map_or(0, |i| (i + 1) % n)),
            Key::Up if n > 0 => self.picked = Some(self.picked.map_or(n - 1, |i| (i + n - 1) % n)),
            Key::Tab => self.focus = Focus::Page,
            k => {
                if Self::edit(&mut self.address, k) {
                    self.picked = None;
                }
            }
        }
    }

    fn key_find(&mut self, key: Key, mods: Mods) {
        match key {
            Key::Enter => self.find_next(mods.shift),
            Key::Escape => self.close_find(),
            Key::Tab => self.focus = Focus::Page,
            k => {
                if Self::edit(&mut self.find, k) {
                    self.found = usize::MAX;
                    self.find_next(false);
                }
            }
        }
    }

    fn key_field(&mut self, node: NodeId, key: Key, mods: Mods) {
        let textarea = self.tab().page.doc.is(node, "textarea");
        match key {
            Key::Enter if textarea => {
                self.field.insert('\n');
                self.store_field(node);
            }
            Key::Enter => self.submit(node),
            Key::Escape => self.focus = Focus::Page,
            Key::Tab => self.next_focusable(mods.shift),
            Key::Up | Key::Down => {}
            k => {
                if Self::edit(&mut self.field, k) {
                    self.store_field(node);
                }
            }
        }
    }

    fn store_field(&mut self, node: NodeId) {
        let text = self.field.text.clone();
        let (w, h) = (self.width, self.view_height());
        let page = &mut self.tab_mut().page;
        page.set_field_value(node, text);
        page.lay_out(w, h);
    }

    fn key_page(&mut self, key: Key, mods: Mods) {
        let h = self.view_height();
        match key {
            Key::Up => self.scroll_by(-STEP),
            Key::Down => self.scroll_by(STEP),
            Key::PageUp => self.scroll_by(-(h - 48)),
            Key::PageDown => self.scroll_by(h - 48),
            Key::Char(' ') => self.scroll_by(if mods.shift { -(h - 48) } else { h - 48 }),
            Key::Home => self.scroll_to(0),
            Key::End => self.scroll_to(i32::MAX / 2),
            Key::Backspace => self.back(),
            Key::Tab => self.next_focusable(mods.shift),
            Key::Enter => {
                if let Some(n) = self.tab().ring {
                    self.activate(n);
                }
            }
            Key::Escape => {
                if self.tab().load.is_some() {
                    self.tab_mut().load = None;
                } else {
                    self.tab_mut().ring = None;
                }
            }
            _ => {}
        }
    }

    /// Moves the keyboard to the next link or control (the previous one
    /// with `back`).
    fn next_focusable(&mut self, back: bool) {
        let list = self.tab().page.focusable();
        if list.is_empty() {
            return;
        }
        let at = self.tab().ring.and_then(|r| list.iter().position(|&n| n == r));
        let n = list.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        let node = list[next];
        if self.tab().page.is_text_field(node) {
            self.focus_field(node);
        } else {
            self.focus = Focus::Page;
            self.tab_mut().ring = Some(node);
            if let Some((_, y, _, h)) = self.tab().page.rect_of(node) {
                self.reveal(y, y + h);
            }
        }
    }

    /// What a click on (or Entrée on) `node` of the page does.
    fn activate(&mut self, node: NodeId) {
        let page = &self.tab().page;
        if page.is_text_field(node) {
            return self.focus_field(node);
        }
        if page.is_submit(node) {
            return self.submit(node);
        }
        if let Some((_, url)) = page.link_of(node) {
            self.focus = Focus::Page;
            return self.go(url, None, Kind::New);
        }
        let Some(e) = page.doc.element(node) else { return };
        let kind = e.attr("type").unwrap_or("").to_ascii_lowercase();
        match e.name.as_str() {
            "input" if kind == "checkbox" || kind == "radio" => {
                self.tab_mut().page.toggle(node);
                self.tab_mut().ring = Some(node);
                self.relayout();
            }
            "select" => {
                self.next_option(node);
                self.tab_mut().ring = Some(node);
            }
            "label" => {
                // A label gives its click to its field.
                let target = e
                    .attr("for")
                    .and_then(|id| page.doc.by_id(id))
                    .or_else(|| page.doc.descendants(node).into_iter().find(|&n| page.focusable().contains(&n)));
                if let Some(t) = target
                    && t != node
                {
                    self.activate(t);
                }
            }
            _ => {
                if self.focus != Focus::Page {
                    self.focus = Focus::Page;
                }
                self.tab_mut().ring = None;
            }
        }
    }

    /// A list (`<select>`) shows its next choice.
    fn next_option(&mut self, select: NodeId) {
        let page = &mut self.tab_mut().page;
        let options: Vec<NodeId> =
            page.doc.descendants(select).into_iter().filter(|&o| page.doc.is(o, "option")).collect();
        if options.is_empty() {
            return;
        }
        let at = options
            .iter()
            .position(|&o| page.doc.element(o).is_some_and(|e| e.attr("selected").is_some()))
            .unwrap_or(0);
        let next = options[(at + 1) % options.len()];
        for &o in &options {
            if let Some(e) = page.doc.element_mut(o) {
                e.attrs.retain(|(k, _)| k != "selected");
                if o == next {
                    e.attrs.push((String::from("selected"), String::new()));
                }
            }
        }
        page.invalidate();
        self.relayout();
    }

    fn submit(&mut self, node: NodeId) {
        if let Some(s) = self.tab().page.submit(node) {
            self.focus = Focus::Page;
            self.go(s.url, s.post, Kind::New);
        }
    }

    fn click(&mut self, x: i32, y: i32) {
        if let Some(action) = chrome::hit(self, x, y) {
            if !matches!(action, chrome::Action::Menu(_) | chrome::Action::MenuButton) {
                self.menu = false;
            }
            return self.act(action);
        }
        self.menu = false;
        let (vx, vy, vx1, vy1) = self.view();
        if x < vx || x >= vx1 || y < vy || y >= vy1 {
            return;
        }
        let (px, py) = (x - vx, y - vy + self.tab().scroll);
        match self.tab().page.node_at(px, py) {
            Some(node) => {
                // The innermost element that does something.
                let page = &self.tab().page;
                let target = page.ancestor(node, &["a", "area", "input", "button", "select", "textarea", "label"]);
                match target {
                    Some(t) => self.activate(t),
                    None => {
                        self.focus = Focus::Page;
                        self.tab_mut().ring = None;
                    }
                }
            }
            None => self.focus = Focus::Page,
        }
    }

    fn act(&mut self, action: chrome::Action) {
        use chrome::Action;
        match action {
            Action::Tab(i) => self.select_tab(i),
            Action::CloseTab(i) => self.close_tab(i),
            Action::NewTab => self.new_tab(None),
            Action::Back => self.back(),
            Action::Forward => self.forward(),
            Action::Reload => self.reload(),
            Action::Home => self.home(),
            Action::Address => {
                if self.focus != Focus::Address {
                    self.focus_address();
                } else {
                    self.address.all = false;
                }
            }
            Action::Suggestion(i) => {
                if let Some(url) = self.suggestions().get(i).map(|s| s.url.clone()) {
                    self.focus = Focus::Page;
                    self.go(url, None, Kind::New);
                }
            }
            Action::Star => self.toggle_bookmark(),
            Action::MenuButton => self.menu = !self.menu,
            Action::Menu(item) => {
                self.menu = false;
                let about = |name: &str| Url::parse(&alloc::format!("about:{name}"));
                match item {
                    chrome::MenuItem::NewTab => self.new_tab(None),
                    chrome::MenuItem::Find => self.open_find(),
                    chrome::MenuItem::Page(name) => {
                        if let Some(u) = about(name) {
                            self.focus = Focus::Page;
                            self.go(u, None, Kind::New);
                        }
                    }
                }
            }
            Action::FindField => self.focus = Focus::Find,
            Action::FindNext(back) => self.find_next(back),
            Action::FindClose => self.close_find(),
            Action::Thumb(grab) => self.dragging = Some(grab),
            Action::Track(y) => {
                let h = self.view_height();
                let s = self.tab().scroll;
                self.scroll_to(if y < chrome::thumb(self).map_or(0, |t| t.0) { s - (h - 48) } else { s + (h - 48) });
            }
        }
    }

    /// The thumb of the scroll bar is dragged to `top` (window pixels).
    fn drag_thumb(&mut self, top: i32) {
        let (_, vy, _, vy1) = self.view();
        let Some((_, len)) = chrome::thumb(self) else { return };
        let room = (vy1 - vy - len).max(1);
        let max = self.max_scroll(self.current);
        self.scroll_to(((top - vy).clamp(0, room) as i64 * max as i64 / room as i64) as i32);
    }

    // ------------------------------------------------------------ drawing

    /// Draws the whole browser on `canvas` (as large as the browser).
    pub fn draw(&mut self, canvas: &mut Canvas) {
        let (w, h) = (self.width, self.view_height());
        let view = self.view();
        let scroll = self.tab().scroll;
        let overlay = Overlay {
            focus: match &self.focus {
                Focus::Field(n) => Some((*n, self.field.caret)),
                _ => None,
            },
            // Drawn by the browser itself, around links too (see `chrome`).
            ring: None,
            found: (self.finding && !self.find.text.is_empty()).then(|| self.find.text.clone()),
        };
        let page = &mut self.tabs[self.current].page;
        page.lay_out(w, h);
        let page = &self.tabs[self.current].page;
        let key = Painted { generation: page.generation(), view, scroll, overlay };
        let (x0, y0, x1, y1) = (view.0.max(0), view.1.max(0), view.2.min(canvas.width), view.3.min(canvas.height));
        let (row, rows) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
        let stride = canvas.width as usize;
        let at = |y: usize| (y0 as usize + y) * stride + x0 as usize;
        // What changed since the page was last painted: nothing, a few
        // fields (typed in, or the caret moved), or anything.
        let part = match &self.painted {
            Some((k, pixels)) if k.view == key.view && k.scroll == key.scroll && pixels.len() == row * rows => {
                let fields = if k.overlay.found == key.overlay.found && k.overlay.ring == key.overlay.ring {
                    page.patched_since(k.generation)
                } else {
                    None
                };
                fields.and_then(|mut nodes| {
                    nodes.extend(k.overlay.focus.iter().chain(key.overlay.focus.iter()).map(|f| f.0));
                    if k.overlay.focus != key.overlay.focus && nodes.is_empty() {
                        return None;
                    }
                    let mut part = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                    for n in nodes {
                        let (x, y, w, h) = page.rect_of(n)?;
                        let (x, y) = (view.0 + x, view.1 + y - scroll);
                        // The caret and the field's ring reach a little out.
                        part = (part.0.min(x - 4), part.1.min(y - 4), part.2.max(x + w + 4), part.3.max(y + h + 4));
                    }
                    Some(part)
                })
            }
            _ => None,
        };
        match (part, &mut self.painted, &page.layout) {
            (Some(part), Some((k, pixels)), Some(layout)) => {
                for y in 0..rows {
                    canvas.pixels[at(y)..at(y) + row].copy_from_slice(&pixels[y * row..(y + 1) * row]);
                }
                if part.0 < part.2 {
                    hln_web::paint::paint_part(layout, canvas, view, scroll, &key.overlay, part);
                    let (top, bottom) = ((part.1 - y0).max(0) as usize, ((part.3 - y0).max(0) as usize).min(rows));
                    for y in top..bottom {
                        pixels[y * row..(y + 1) * row].copy_from_slice(&canvas.pixels[at(y)..at(y) + row]);
                    }
                }
                *k = key;
            }
            _ => {
                if let Some(layout) = &page.layout {
                    hln_web::paint::paint(layout, canvas, view, scroll, &key.overlay);
                }
                let mut pixels = Vec::with_capacity(row * rows);
                for y in 0..rows {
                    pixels.extend_from_slice(&canvas.pixels[at(y)..at(y) + row]);
                }
                self.painted = Some((key, pixels));
            }
        }
        chrome::draw(self, canvas);
    }

    /// The link under the pointer, for the bubble at the bottom.
    fn hovered_link(&self) -> Option<Url> {
        let (x, y) = self.pointer;
        let (vx, vy, vx1, vy1) = self.view();
        if x < vx || x >= vx1 || y < vy || y >= vy1 || self.menu {
            return None;
        }
        let page = &self.tab().page;
        let node = page.node_at(x - vx, y - vy + self.tab().scroll)?;
        page.link_of(node).map(|(_, u)| u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use net::Response;

    /// A network of a few pages, counting the requests.
    struct Pages(Vec<(&'static str, &'static str)>, usize);

    impl Net for Pages {
        fn fetch(&mut self, url: &Url, post: Option<&str>) -> Result<Response, NetError> {
            self.1 += 1;
            let key = url.without_fragment().to_string();
            let body = self.0.iter().find(|(u, _)| *u == key).map(|(_, b)| *b);
            match body {
                Some(b) => Ok(Response {
                    url: url.clone(),
                    status: 200,
                    content_type: Some(String::from(if key.ends_with(".css") { "text/css" } else { "text/html" })),
                    body: alloc::format!("{b}{}", post.unwrap_or("")).into_bytes(),
                }),
                None => Err(NetError::Failed(String::from("Aucune réponse."))),
            }
        }
    }

    fn net() -> Pages {
        Pages(
            alloc::vec![
                (
                    "https://a.fr/",
                    "<title>A</title><link rel=stylesheet href=s.css><p><a href=/b>vers B</a></p><div style='height:3000px'></div><p id=bas>bas</p>"
                ),
                ("https://a.fr/s.css", "@import 'i.css'; p { margin: 0 }"),
                ("https://a.fr/i.css", "body { margin: 0 }"),
                (
                    "https://a.fr/b",
                    "<title>B</title><form action=/c method=post><input name=q autofocus><button>OK</button></form>"
                ),
                ("https://a.fr/c", "<title>C</title>reçu : "),
            ],
            0,
        )
    }

    fn settle(b: &mut Browser, n: &mut dyn Net) {
        for _ in 0..50 {
            if !b.step(n) {
                return;
            }
        }
        panic!("the load never ends");
    }

    fn typed(b: &mut Browser, text: &str) {
        for c in text.chars() {
            b.handle(Event::Key(Key::Char(c), Mods::NONE));
        }
    }

    #[test]
    fn starts_home_with_the_address_bar_ready() {
        let mut b = Browser::new(800, 600);
        assert_eq!(b.tab().page.url.to_string(), "about:accueil");
        assert!(b.focus == Focus::Address);
        assert_eq!(b.title(), "Nouvel onglet");
        let mut canvas = Canvas::new(800, 600);
        b.draw(&mut canvas);
    }

    #[test]
    fn loads_in_stages_and_goes_back() {
        let mut b = Browser::new(800, 600);
        let mut n = net();
        typed(&mut b, "a.fr");
        b.handle(Event::Key(Key::Enter, Mods::NONE));
        assert!(b.busy());
        settle(&mut b, &mut n);
        // The document, its sheet, then what the sheet imports.
        assert_eq!(n.1, 3);
        assert_eq!(b.title(), "A");
        assert_eq!(b.history[0].url.to_string(), "https://a.fr/");
        // A jump to an anchor loads nothing.
        b.go(Url::parse("https://a.fr/#bas").unwrap(), None, Kind::New);
        assert!(!b.busy() && b.scroll() > 2000);
        b.back();
        assert_eq!(b.scroll(), 0);
        // Following a link, then a form posted from a field focused by itself.
        let link = b.tab().page.doc.find("a").unwrap();
        b.activate(link);
        settle(&mut b, &mut n);
        assert_eq!(b.title(), "B");
        assert!(matches!(b.focus, Focus::Field(_)));
        typed(&mut b, "âne");
        b.handle(Event::Key(Key::Enter, Mods::NONE));
        settle(&mut b, &mut n);
        assert_eq!(b.title(), "C");
        assert!(b.tab().page.doc.text_content(0).contains("q=%C3%A2ne"));
        b.back();
        b.back();
        settle(&mut b, &mut n);
        assert_eq!(b.title(), "A");
        assert!(b.tab().can_go_forward());
    }

    #[test]
    fn errors_are_pages() {
        let mut b = Browser::new(800, 600);
        b.open("https://ailleurs.fr/");
        settle(&mut b, &mut net::Offline);
        assert_eq!(b.title(), "Pas de réseau");
        assert_eq!(b.tab().page.url.to_string(), "https://ailleurs.fr/");
        b.go(Url::parse("mailto:ane@exemple.fr").unwrap(), None, Kind::New);
        assert_eq!(b.title(), "Adresse non prise en charge");
        b.open("about:rien");
        assert_eq!(b.title(), "Page inconnue");
    }

    #[test]
    fn tabs_bookmarks_and_suggestions() {
        let mut b = Browser::new(800, 600);
        let mut n = net();
        b.open("https://a.fr/");
        settle(&mut b, &mut n);
        b.toggle_bookmark();
        assert!(b.bookmarked());
        b.handle(Event::Key(Key::Char('t'), Mods::CTRL));
        assert_eq!((b.tabs.len(), b.current), (2, 1));
        typed(&mut b, "a.");
        assert_eq!(b.suggestions().len(), 1);
        b.handle(Event::Key(Key::Down, Mods::NONE));
        b.handle(Event::Key(Key::Enter, Mods::NONE));
        settle(&mut b, &mut n);
        assert_eq!(b.title(), "A");
        b.handle(Event::Key(Key::Char('w'), Mods::CTRL));
        b.handle(Event::Key(Key::Char('w'), Mods::CTRL));
        assert_eq!(b.tabs.len(), 1);
        assert_eq!(b.tab().page.url.to_string(), "about:accueil");
        b.open("about:favoris");
        assert!(b.tab().page.doc.text_content(0).contains("A"));
    }

    #[test]
    fn find_scrolls_to_the_text() {
        let mut b = Browser::new(800, 600);
        let mut n = net();
        b.open("https://a.fr/");
        settle(&mut b, &mut n);
        b.handle(Event::Key(Key::Char('f'), Mods::CTRL));
        typed(&mut b, "bas");
        assert!(b.scroll() > 2000);
        b.handle(Event::Key(Key::Escape, Mods::NONE));
        assert!(!b.finding);
    }

    #[test]
    fn clicks_on_the_chrome_and_the_page() {
        let mut b = Browser::new(800, 600);
        let mut n = net();
        b.open("https://a.fr/");
        settle(&mut b, &mut n);
        let mut canvas = Canvas::new(800, 600);
        b.draw(&mut canvas);
        // The link, at the page's top left.
        b.handle(Event::Pointer { x: 10, y: TABS + BAR + 8, left: true });
        b.handle(Event::Pointer { x: 10, y: TABS + BAR + 8, left: false });
        settle(&mut b, &mut n);
        assert_eq!(b.title(), "B");
        let (x, y) = chrome::center(&b, chrome::Action::Back).unwrap();
        b.handle(Event::Pointer { x, y, left: true });
        b.handle(Event::Pointer { x, y, left: false });
        settle(&mut b, &mut n);
        assert_eq!(b.title(), "A");
        let (x, y) = chrome::center(&b, chrome::Action::NewTab).unwrap();
        b.handle(Event::Pointer { x, y, left: true });
        assert_eq!(b.tabs.len(), 2);
        b.draw(&mut canvas);
        assert_eq!(b.tabs[0].title().to_string(), "A");
    }

    #[test]
    fn typing_draws_like_a_full_paint() {
        let mut b = Browser::new(800, 600);
        let mut canvas = Canvas::new(800, 600);
        b.draw(&mut canvas);
        let input = b.tab().page.doc.find("input").unwrap();
        b.focus_field(input);
        b.draw(&mut canvas);
        for c in "Le P'tit Âne".chars() {
            b.handle(Event::Key(Key::Char(c), Mods::NONE));
            b.draw(&mut canvas);
        }
        b.handle(Event::Key(Key::Left, Mods::NONE));
        b.draw(&mut canvas);
        b.painted = None;
        let mut full = Canvas::new(800, 600);
        b.draw(&mut full);
        assert!(canvas.pixels == full.pixels);
    }
}
