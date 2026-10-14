//! The browser around the engine: tabs with their history, the address
//! bar and its suggestions, bookmarks, the find bar, the menu, and the
//! browser's own pages. It is driven by [`Event`]s, loads through a
//! [`Net`] one step at a time (so a frame can show the progress between
//! steps), and draws itself on a [`Canvas`]. The holon and the bench share
//! all of it.

#![no_std]

extern crate alloc;

mod chrome;
pub mod edit;
pub mod net;
pub mod pages;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use edit::{Line, byte_at};
use hln_web::dom::NodeId;
use hln_web::page::Page;
use hln_web::paint::{self, Canvas, Overlay, Rect};
use hln_web::url::{self, Url};
pub use net::{Net, NetError, Offline, Response};
use pages::{Book, Mark};

/// The tab strip's height, in pixels.
pub const TABS: i32 = 34;
/// The toolbar's height.
pub const BAR: i32 = 40;
/// Where the page starts.
pub const TOP: i32 = TABS + BAR;
pub const SCROLLBAR: i32 = 10;
/// The find bar's height, at the bottom when open.
pub const FIND: i32 = 36;
/// One step of the arrows and the wheel.
const LINE: i32 = 48;
pub const HOME: &str = "about:accueil";
/// Where searches typed in the address bar go.
pub const SEARCH: &str = "https://html.duckduckgo.com/html/?q=";
const MAX_VISITED: usize = 500;
const MAX_REFRESHES: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
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

/// The pointer's buttons.
pub const LEFT: u32 = 1;
pub const RIGHT: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Char(char),
    Key(Key),
    /// A key with Ctrl held: its letter in lower case, or `'\t'`.
    Ctrl(char),
    /// Where the pointer is, in the window, and which buttons are down.
    Pointer {
        x: i32,
        y: i32,
        buttons: u32,
    },
    /// Wheel notches, positive toward the page's bottom.
    Wheel(i32),
    Resize(i32, i32),
}

/// Where the keys go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Page,
    Address,
    /// A text field of the page.
    Field(NodeId),
    Find,
}

/// What the browser can be asked to do, from a key, a button or the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    NewTab,
    CloseTab,
    NextTab,
    Back,
    Forward,
    Reload,
    Home,
    Bookmark,
    Bookmarks,
    History,
    Find,
    Source,
    Help,
    About,
}

/// The menu (≡), in order.
pub const MENU: &[(&str, Command)] = &[
    ("Nouvel onglet", Command::NewTab),
    ("Fermer l'onglet", Command::CloseTab),
    ("Ajouter aux favoris", Command::Bookmark),
    ("Favoris", Command::Bookmarks),
    ("Historique", Command::History),
    ("Rechercher dans la page", Command::Find),
    ("Code source de la page", Command::Source),
    ("Recharger", Command::Reload),
    ("Aide", Command::Help),
    ("À propos", Command::About),
];

/// One page of a tab's history.
struct Entry {
    url: Url,
    post: Option<String>,
    scroll: i32,
}

/// What becomes of the history when a load ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum How {
    /// A new page after the current one.
    Push,
    /// In place of the current one (a refresh, a redirect by the page).
    Replace,
    /// The current entry again (back, forward, reload), at this scroll.
    Revisit(i32),
}

enum Stage {
    Document,
    /// The document came; its style sheets are coming.
    Sheets {
        page: Box<Page>,
        queue: Vec<(NodeId, Url)>,
        seen: Vec<Url>,
    },
}

struct Load {
    url: Url,
    post: Option<String>,
    how: How,
    stage: Stage,
    /// Steps done, and known to do.
    done: usize,
    total: usize,
}

pub(crate) struct Tab {
    page: Page,
    entries: Vec<Entry>,
    at: usize,
    scroll: i32,
    load: Option<Load>,
    /// The link or control the keyboard is on.
    ring: Option<NodeId>,
    /// The caret in the focused field, in characters.
    caret: usize,
    refreshes: u8,
}

impl Tab {
    fn blank() -> Tab {
        Tab {
            page: Page::from_html(Url::parse(HOME).unwrap_or_else(|| unreachable!()), ""),
            entries: Vec::new(),
            at: 0,
            scroll: 0,
            load: None,
            ring: None,
            caret: 0,
            refreshes: 0,
        }
    }

    /// The address the tab is at, or going to.
    pub(crate) fn url(&self) -> &Url {
        match &self.load {
            Some(l) => &l.url,
            None => &self.page.url,
        }
    }

    pub(crate) fn title(&self) -> String {
        if self.load.is_some() {
            return String::from("Chargement…");
        }
        let t = self.page.title();
        if !t.is_empty() {
            t
        } else if is_home(&self.page.url) {
            String::from("Nouvel onglet")
        } else {
            self.page.url.pretty()
        }
    }

    pub(crate) fn progress(&self) -> Option<(usize, usize)> {
        self.load.as_ref().map(|l| (l.done, l.total))
    }

    fn can_back(&self) -> bool {
        self.at > 0
    }

    fn can_forward(&self) -> bool {
        self.at + 1 < self.entries.len()
    }
}

fn is_home(url: &Url) -> bool {
    url.scheme == "about" && matches!(url.path.as_str(), "accueil" | "" | "blank" | "home" | "newtab")
}

/// What the pointer is over, when no button is down.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Hover {
    Nothing,
    Link(String),
    Chrome(chrome::Target),
}

pub struct Browser {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) current: usize,
    pub(crate) focus: Focus,
    pub(crate) address: Line,
    pub(crate) find: Line,
    pub(crate) find_open: bool,
    find_at: usize,
    pub(crate) menu: bool,
    pub(crate) bookmarks: Vec<Mark>,
    pub(crate) visited: Vec<Mark>,
    pub(crate) suggestions: Vec<Mark>,
    pub(crate) pick: Option<usize>,
    buttons: u32,
    pub(crate) hover: Hover,
    /// Where the scrollbar's thumb was taken, from its top.
    drag: Option<i32>,
    /// Where searches go.
    pub search: String,
    dirty: bool,
}

impl Browser {
    /// A browser `width` × `height` pixels with one tab on the home page.
    pub fn new(width: i32, height: i32) -> Browser {
        let mut b = Browser {
            width,
            height,
            tabs: Vec::new(),
            current: 0,
            focus: Focus::Page,
            address: Line::default(),
            find: Line::default(),
            find_open: false,
            find_at: 0,
            menu: false,
            bookmarks: Vec::new(),
            visited: Vec::new(),
            suggestions: Vec::new(),
            pick: None,
            buttons: 0,
            hover: Hover::Nothing,
            drag: None,
            search: String::from(SEARCH),
            dirty: true,
        };
        b.open_tab(Url::parse(HOME).unwrap_or_else(|| unreachable!()));
        b
    }

    /// Opens `typed` (an address, a host, or words to search) in the
    /// current tab.
    pub fn open(&mut self, typed: &str) {
        if let Some(url) = url::from_typed(typed, &self.search) {
            self.focus = Focus::Page;
            self.navigate(url, None, How::Push);
        }
    }

    pub fn bookmark(&mut self, title: &str, url: Url) {
        self.bookmarks.push(Mark { title: String::from(title), url });
    }

    /// Asks for everything to be drawn again (the window was covered).
    pub fn redraw(&mut self) {
        self.dirty = true;
    }

    /// Whether something changed since the last [`Browser::draw`].
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Whether a tab waits for the network: call [`Browser::load`].
    pub fn needs_load(&self) -> bool {
        self.tabs.iter().any(|t| t.load.is_some())
    }

    /// The window's title: the current page's.
    pub fn title(&self) -> String {
        self.tab().title()
    }

    pub fn url(&self) -> String {
        self.tab().url().to_string()
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub(crate) fn tab(&self) -> &Tab {
        &self.tabs[self.current]
    }

    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.current]
    }

    pub(crate) fn geo(&self) -> chrome::Geo {
        chrome::Geo { w: self.width, h: self.height, tabs: self.tabs.len(), find: self.find_open }
    }

    /// The page's viewport in the window.
    pub(crate) fn view(&self) -> Rect {
        self.geo().content()
    }

    fn view_size(&self) -> (i32, i32) {
        let (x0, y0, x1, y1) = self.view();
        ((x1 - x0).max(1), (y1 - y0).max(1))
    }

    /// Lays the current page out for the viewport; its height.
    fn lay_out(&mut self) -> i32 {
        let (w, h) = self.view_size();
        self.tab_mut().page.lay_out(w, h).height
    }

    pub(crate) fn max_scroll(&self) -> i32 {
        let h = self.tab().page.layout.as_ref().map_or(0, |l| l.height);
        (h - self.view_size().1).max(0)
    }

    fn scroll_to(&mut self, y: i32) {
        self.lay_out();
        let max = self.max_scroll();
        let tab = self.tab_mut();
        let y = y.clamp(0, max);
        if tab.scroll != y {
            tab.scroll = y;
            self.dirty = true;
        }
    }

    fn scroll_by(&mut self, dy: i32) {
        let y = self.tab().scroll + dy;
        self.scroll_to(y);
    }

    /// Scrolls so that page rows `top`..`bottom` are seen.
    fn reveal(&mut self, top: i32, bottom: i32) {
        let (_, h) = self.view_size();
        let s = self.tab().scroll;
        if top < s {
            self.scroll_to(top - 16);
        } else if bottom > s + h {
            self.scroll_to(bottom - h + 16);
        }
    }

    // ------------------------------------------------------------ navigation

    fn navigate(&mut self, url: Url, post: Option<String>, how: How) {
        let i = self.current;
        self.go(i, url, post, how);
    }

    fn go(&mut self, i: usize, url: Url, post: Option<String>, how: How) {
        if url.scheme == "javascript" {
            return;
        }
        self.menu = false;
        self.dirty = true;
        let tab = &mut self.tabs[i];
        // A jump within the page: no load.
        if how == How::Push
            && post.is_none()
            && url.fragment.is_some()
            && tab.load.is_none()
            && url.same_document(&tab.page.url)
            && !tab.entries.is_empty()
        {
            if let Some(e) = tab.entries.get_mut(tab.at) {
                e.scroll = tab.scroll;
            }
            tab.entries.truncate(tab.at + 1);
            tab.entries.push(Entry { url: url.clone(), post: None, scroll: 0 });
            tab.at += 1;
            let y = tab.page.anchor_y(url.fragment.as_deref().unwrap_or("")).unwrap_or(0);
            tab.page.url = url;
            if i == self.current {
                self.scroll_to(y);
                self.show_address();
            }
            return;
        }
        if let Some(e) = tab.entries.get_mut(tab.at) {
            e.scroll = tab.scroll;
        }
        if how != How::Replace {
            tab.refreshes = 0;
        }
        let local = url.scheme == "about";
        tab.load = Some(Load { url, post, how, stage: Stage::Document, done: 0, total: 1 });
        if i == self.current {
            if matches!(self.focus, Focus::Field(_)) {
                self.focus = Focus::Page;
            }
            if self.focus != Focus::Address {
                self.show_address();
            }
        }
        // The browser's own pages come at once.
        if local {
            self.step(i, &mut Offline);
        }
    }

    fn open_tab(&mut self, url: Url) {
        self.tabs.push(Tab::blank());
        self.current = self.tabs.len() - 1;
        let home = is_home(&url);
        self.navigate(url, None, How::Push);
        if home {
            self.focus = Focus::Address;
            self.address.clear();
        } else {
            self.focus = Focus::Page;
        }
        self.find_open = false;
    }

    fn close_tab(&mut self, i: usize) {
        if self.tabs.len() == 1 {
            self.tabs.clear();
            self.open_tab(Url::parse(HOME).unwrap_or_else(|| unreachable!()));
            return;
        }
        self.tabs.remove(i);
        if self.current > i || self.current == self.tabs.len() {
            self.current = self.current.saturating_sub(1);
        }
        self.switched();
    }

    fn switch(&mut self, i: usize) {
        if i < self.tabs.len() && i != self.current {
            self.current = i;
            self.switched();
        }
    }

    fn switched(&mut self) {
        self.focus = Focus::Page;
        self.find_open = false;
        self.menu = false;
        self.show_address();
        self.dirty = true;
    }

    fn history_step(&mut self, back: bool) {
        let tab = self.tab_mut();
        if back && !tab.can_back() || !back && !tab.can_forward() {
            return;
        }
        if let Some(e) = tab.entries.get_mut(tab.at) {
            e.scroll = tab.scroll;
        }
        tab.at = if back { tab.at - 1 } else { tab.at + 1 };
        let e = &tab.entries[tab.at];
        let (url, post, scroll) = (e.url.clone(), e.post.clone(), e.scroll);
        // Back within one document: only scroll.
        if post.is_none() && tab.load.is_none() && url.same_document(&tab.page.url) {
            tab.page.url = url;
            self.scroll_to(scroll);
            self.show_address();
            return;
        }
        self.navigate(url, post, How::Revisit(scroll));
    }

    fn reload(&mut self) {
        let tab = self.tab();
        let (url, post) = match tab.entries.get(tab.at) {
            Some(e) => (e.url.clone(), e.post.clone()),
            None => (tab.page.url.clone(), None),
        };
        let scroll = tab.scroll;
        self.navigate(url, post, How::Revisit(scroll));
    }

    /// Shows the current tab's address in the bar.
    fn show_address(&mut self) {
        let url = self.tab().url().clone();
        if is_home(&url) {
            self.address.clear();
        } else {
            self.address.set(&url.to_string());
        }
        self.suggestions.clear();
        self.pick = None;
    }

    pub fn run(&mut self, command: Command) {
        self.menu = false;
        self.dirty = true;
        let about = |name: &str| Url::parse(&format!("about:{name}")).unwrap_or_else(|| unreachable!());
        match command {
            Command::NewTab => self.open_tab(about("accueil")),
            Command::CloseTab => self.close_tab(self.current),
            Command::NextTab => self.switch((self.current + 1) % self.tabs.len()),
            Command::Back => self.history_step(true),
            Command::Forward => self.history_step(false),
            Command::Reload => self.reload(),
            Command::Home => self.navigate(about("accueil"), None, How::Push),
            Command::Bookmark => {
                let url = self.tab().page.url.clone();
                if let Some(at) = self.bookmarks.iter().position(|m| m.url == url) {
                    self.bookmarks.remove(at);
                } else if !url.scheme.eq("about") || !is_home(&url) {
                    let title = self.tab().page.title();
                    self.bookmarks.push(Mark { title, url });
                }
            }
            Command::Bookmarks => self.navigate(about("favoris"), None, How::Push),
            Command::History => self.navigate(about("historique"), None, How::Push),
            Command::Find => {
                self.find_open = true;
                self.focus = Focus::Find;
                self.find.all = true;
            }
            Command::Source => {
                let url = self.tab().page.url.without_fragment();
                if url.scheme != "view-source"
                    && let Some(u) = Url::parse(&format!("view-source:{url}"))
                {
                    self.open_tab(u);
                }
            }
            Command::Help => self.navigate(about("aide"), None, How::Push),
            Command::About => self.navigate(about("a-propos"), None, How::Push),
        }
    }

    pub(crate) fn is_bookmarked(&self) -> bool {
        let url = &self.tab().page.url;
        self.bookmarks.iter().any(|m| &m.url == url)
    }

    pub(crate) fn can(&self, command: Command) -> bool {
        match command {
            Command::Back => self.tab().can_back(),
            Command::Forward => self.tab().can_forward(),
            _ => true,
        }
    }

    // ------------------------------------------------------------ loading

    /// Takes one step of a load (a document or a style sheet), the current
    /// tab's first.
    pub fn load(&mut self, net: &mut dyn Net) {
        let Some(i) = (self.tabs[self.current].load.is_some())
            .then_some(self.current)
            .or_else(|| self.tabs.iter().position(|t| t.load.is_some()))
        else {
            return;
        };
        self.step(i, net);
    }

    /// Takes one step of tab `i`'s load.
    fn step(&mut self, i: usize, net: &mut dyn Net) {
        self.dirty = true;
        let Some(mut load) = self.tabs[i].load.take() else { return };
        match core::mem::replace(&mut load.stage, Stage::Document) {
            Stage::Document => {
                let url = load.url.clone();
                let book = Book { bookmarks: &self.bookmarks, visited: &self.visited, search: &self.search };
                let (page, record) = match url.scheme.as_str() {
                    "about" => match pages::about(&url.path, &book) {
                        Some(html) => (Page::from_html(url.clone(), &html), false),
                        None => (Page::from_html(url.clone(), &pages::error(&url, &NetError::Unsupported)), false),
                    },
                    "view-source" => {
                        let inner = match &url.query {
                            Some(q) => format!("{}?{q}", url.path),
                            None => url.path.clone(),
                        };
                        match Url::parse(&inner).ok_or(NetError::Unsupported).and_then(|u| net.fetch(&u, None)) {
                            Ok(r) => (Page::new(url.clone(), &r.body, Some("text/plain")), false),
                            Err(e) => (Page::from_html(url.clone(), &pages::error(&url, &e)), false),
                        }
                    }
                    "http" | "https" | "file" => match net.fetch(&url.without_fragment(), load.post.as_deref()) {
                        Ok(r) => {
                            let mut final_url = r.url;
                            final_url.fragment = url.fragment.clone();
                            (Page::new(final_url, &r.body, r.content_type.as_deref()), true)
                        }
                        Err(e) => (Page::from_html(url.clone(), &pages::error(&url, &e)), false),
                    },
                    _ => (Page::from_html(url.clone(), &pages::error(&url, &NetError::Unsupported)), false),
                };
                let queue = if record { page.stylesheet_links() } else { Vec::new() };
                load.done = 1;
                load.total = 1 + queue.len();
                if queue.is_empty() && page.imports().is_empty() {
                    self.finish(i, page, load, record);
                } else {
                    load.stage = Stage::Sheets { page: Box::new(page), queue, seen: Vec::new() };
                    self.tabs[i].load = Some(load);
                }
            }
            Stage::Sheets { mut page, mut queue, mut seen } => {
                if !queue.is_empty() {
                    let (node, sheet) = queue.remove(0);
                    if !seen.contains(&sheet) {
                        if let Ok(r) = net.fetch(&sheet, None) {
                            let text = String::from_utf8_lossy(&r.body).into_owned();
                            page.add_sheet(node, text, r.url);
                        }
                        seen.push(sheet);
                    }
                    load.done += 1;
                }
                for (node, u) in page.imports() {
                    if !seen.contains(&u) && !queue.iter().any(|(_, q)| *q == u) && seen.len() < 64 {
                        queue.push((node, u));
                        load.total += 1;
                    }
                }
                if queue.is_empty() {
                    self.finish(i, *page, load, true);
                } else {
                    load.stage = Stage::Sheets { page, queue, seen };
                    self.tabs[i].load = Some(load);
                }
            }
        }
    }

    fn finish(&mut self, i: usize, page: Page, load: Load, record: bool) {
        let (w, h) = self.view_size();
        let tab = &mut self.tabs[i];
        let entry = Entry { url: page.url.clone(), post: load.post, scroll: 0 };
        match load.how {
            How::Push => {
                if !tab.entries.is_empty() {
                    tab.entries.truncate(tab.at + 1);
                }
                tab.entries.push(entry);
                tab.at = tab.entries.len() - 1;
            }
            How::Replace | How::Revisit(_) if !tab.entries.is_empty() => {
                let at = tab.at.min(tab.entries.len() - 1);
                tab.entries[at] = entry;
            }
            _ => {
                tab.entries.push(entry);
                tab.at = 0;
            }
        }
        tab.page = page;
        tab.ring = None;
        let height = tab.page.lay_out(w, h).height;
        let wanted = match load.how {
            How::Revisit(s) => s,
            _ => tab.page.url.fragment.clone().and_then(|f| tab.page.anchor_y(&f)).unwrap_or(0),
        };
        tab.scroll = wanted.clamp(0, (height - h).max(0));
        let refresh = tab.page.refresh().filter(|_| tab.refreshes < MAX_REFRESHES);
        if record {
            self.visited.push(Mark { title: self.tabs[i].page.title(), url: self.tabs[i].page.url.clone() });
            if self.visited.len() > MAX_VISITED {
                self.visited.remove(0);
            }
        }
        if i == self.current {
            if self.focus == Focus::Page
                && let Some(n) = self.tabs[i].page.autofocus()
            {
                self.focus = Focus::Field(n);
                self.tabs[i].caret = self.tabs[i].page.field_value(n).chars().count();
                self.tabs[i].ring = Some(n);
            }
            if self.focus != Focus::Address {
                self.show_address();
            }
            if self.find_open {
                self.find_at = 0;
            }
        }
        if let Some(url) = refresh {
            self.tabs[i].refreshes += 1;
            self.go(i, url, None, How::Replace);
        }
    }

    // ------------------------------------------------------------ events

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Resize(w, h) => {
                if (w, h) != (self.width, self.height) {
                    self.width = w.max(160);
                    self.height = h.max(TOP + 40);
                    self.dirty = true;
                }
            }
            Event::Wheel(n) => {
                self.menu = false;
                self.scroll_by(n * LINE);
            }
            Event::Pointer { x, y, buttons } => self.pointer(x, y, buttons),
            Event::Ctrl(c) => self.ctrl(c),
            Event::Char(c) => self.char(c),
            Event::Key(k) => self.key(k),
        }
    }

    fn ctrl(&mut self, c: char) {
        let command = match c.to_ascii_lowercase() {
            't' => Command::NewTab,
            'w' => Command::CloseTab,
            '\t' => Command::NextTab,
            'r' => Command::Reload,
            'd' => Command::Bookmark,
            'f' => Command::Find,
            'h' => Command::History,
            'b' => Command::Bookmarks,
            'u' => Command::Source,
            'l' => {
                self.edit_address();
                return;
            }
            _ => return,
        };
        self.run(command);
    }

    fn edit_address(&mut self) {
        self.menu = false;
        self.focus = Focus::Address;
        self.show_address();
        self.dirty = true;
    }

    fn char(&mut self, c: char) {
        self.dirty = true;
        match self.focus {
            Focus::Address => {
                self.address.insert(c);
                self.suggest();
            }
            Focus::Find => {
                self.find.insert(c);
                self.find_at = 0;
                self.jump_to_found();
            }
            Focus::Field(n) => self.type_in(n, Some(c)),
            Focus::Page => match c {
                ' ' => {
                    let (_, h) = self.view_size();
                    self.scroll_by(h - LINE);
                }
                '/' => self.run(Command::Find),
                _ => {}
            },
        }
    }

    fn key(&mut self, key: Key) {
        self.dirty = true;
        if self.menu && key == Key::Escape {
            self.menu = false;
            return;
        }
        match self.focus {
            Focus::Address => self.address_key(key),
            Focus::Find => match key {
                Key::Enter | Key::Down => {
                    self.find_at += 1;
                    self.jump_to_found();
                }
                Key::Up => {
                    self.find_at = self.find_at.wrapping_sub(1);
                    self.jump_to_found();
                }
                Key::Backspace => {
                    self.find.backspace();
                    self.find_at = 0;
                    self.jump_to_found();
                }
                Key::Delete => self.find.delete(),
                Key::Left => self.find.go(-1),
                Key::Right => self.find.go(1),
                Key::Home => self.find.home(),
                Key::End => self.find.end(),
                Key::Escape | Key::Tab => {
                    self.find_open = false;
                    self.focus = Focus::Page;
                }
                Key::PageUp | Key::PageDown => {}
            },
            Focus::Field(n) => self.field_key(n, key),
            Focus::Page => self.page_key(key),
        }
    }

    fn address_key(&mut self, key: Key) {
        match key {
            Key::Enter => {
                let chosen = self.pick.and_then(|p| self.suggestions.get(p)).map(|m| m.url.clone());
                self.focus = Focus::Page;
                match chosen {
                    Some(url) => self.navigate(url, None, How::Push),
                    None => {
                        let typed = self.address.text.clone();
                        self.open(&typed);
                    }
                }
                self.suggestions.clear();
                self.pick = None;
            }
            Key::Escape | Key::Tab => {
                self.focus = Focus::Page;
                self.show_address();
            }
            Key::Backspace => {
                self.address.backspace();
                self.suggest();
            }
            Key::Delete => {
                self.address.delete();
                self.suggest();
            }
            Key::Left => self.address.go(-1),
            Key::Right => self.address.go(1),
            Key::Home => self.address.home(),
            Key::End => self.address.end(),
            Key::Down if !self.suggestions.is_empty() => {
                self.pick = Some(self.pick.map_or(0, |p| (p + 1) % self.suggestions.len()));
            }
            Key::Up if !self.suggestions.is_empty() => {
                self.pick = match self.pick {
                    None | Some(0) => None,
                    Some(p) => Some(p - 1),
                };
            }
            _ => {}
        }
    }

    /// What the address bar suggests for what is typed: bookmarks, then
    /// pages visited, the latest first.
    fn suggest(&mut self) {
        self.pick = None;
        self.suggestions.clear();
        let typed = self.address.text.trim().to_lowercase();
        if typed.is_empty() {
            return;
        }
        for m in self.bookmarks.iter().chain(self.visited.iter().rev()) {
            let hit = m.url.to_string().to_lowercase().contains(&typed) || m.title.to_lowercase().contains(&typed);
            if hit && !self.suggestions.iter().any(|s| s.url == m.url) {
                self.suggestions.push(m.clone());
                if self.suggestions.len() == 6 {
                    break;
                }
            }
        }
    }

    fn page_key(&mut self, key: Key) {
        let (_, h) = self.view_size();
        match key {
            Key::Up => self.scroll_by(-LINE),
            Key::Down => self.scroll_by(LINE),
            Key::PageUp => self.scroll_by(-(h - LINE)),
            Key::PageDown => self.scroll_by(h - LINE),
            Key::Home => self.scroll_to(0),
            Key::End => self.scroll_to(i32::MAX / 2),
            Key::Backspace => self.history_step(true),
            Key::Escape => self.edit_address(),
            Key::Tab => self.next_focusable(),
            Key::Enter => {
                if let Some(n) = self.tab().ring {
                    self.activate(n, false);
                }
            }
            Key::Left | Key::Right | Key::Delete => {}
        }
    }

    /// Moves the keyboard to the page's next link or control.
    fn next_focusable(&mut self) {
        self.lay_out();
        let tab = self.tab();
        let all = tab.page.focusable();
        if all.is_empty() {
            return;
        }
        let next = match tab.ring.and_then(|r| all.iter().position(|&n| n == r)) {
            Some(i) => all[(i + 1) % all.len()],
            None => {
                // The first one at or below the top of what is seen.
                let s = tab.scroll;
                all.iter().copied().find(|&n| tab.page.rect_of(n).is_some_and(|r| r.1 >= s)).unwrap_or(all[0])
            }
        };
        let text = self.tab().page.is_text_field(next);
        let tab = self.tab_mut();
        tab.ring = Some(next);
        if let Some((_, y, _, h)) = tab.page.rect_of(next) {
            self.reveal(y, y + h);
        }
        if text {
            self.focus_field(next);
        } else {
            self.focus = Focus::Page;
        }
    }

    fn focus_field(&mut self, n: NodeId) {
        let tab = self.tab_mut();
        tab.caret = tab.page.field_value(n).chars().count();
        tab.ring = Some(n);
        self.focus = Focus::Field(n);
        self.dirty = true;
    }

    /// Types `c` in the field `n` (`None`: nothing, to fix the caret).
    fn type_in(&mut self, n: NodeId, c: Option<char>) {
        let tab = self.tab_mut();
        let mut value = tab.page.field_value(n);
        let caret = tab.caret.min(value.chars().count());
        if let Some(c) = c {
            value.insert(byte_at(&value, caret), c);
            tab.caret = caret + 1;
            tab.page.set_field_value(n, value);
        }
    }

    fn field_key(&mut self, n: NodeId, key: Key) {
        let textarea = self.tab().page.doc.is(n, "textarea");
        let tab = self.tab_mut();
        let mut value = tab.page.field_value(n);
        let len = value.chars().count();
        let caret = tab.caret.min(len);
        match key {
            Key::Enter if textarea => {
                self.type_in(n, Some('\n'));
            }
            Key::Enter => {
                if let Some(s) = self.tab().page.submit(n) {
                    self.focus = Focus::Page;
                    self.navigate(s.url, s.post, How::Push);
                }
            }
            Key::Backspace if caret > 0 => {
                value.remove(byte_at(&value, caret - 1));
                tab.caret = caret - 1;
                tab.page.set_field_value(n, value);
            }
            Key::Delete if caret < len => {
                value.remove(byte_at(&value, caret));
                tab.page.set_field_value(n, value);
            }
            Key::Left => tab.caret = caret.saturating_sub(1),
            Key::Right => tab.caret = (caret + 1).min(len),
            Key::Home => tab.caret = 0,
            Key::End => tab.caret = len,
            Key::Escape => self.focus = Focus::Page,
            Key::Tab => self.next_focusable(),
            Key::Up | Key::Down | Key::PageUp | Key::PageDown => {
                self.focus = Focus::Page;
                self.page_key(key);
            }
            _ => {}
        }
    }

    fn jump_to_found(&mut self) {
        self.lay_out();
        let found = self.tab().page.found_at(&self.find.text);
        if found.is_empty() {
            return;
        }
        let k = self.find_at % found.len();
        self.find_at = k;
        let (_, h) = self.view_size();
        self.scroll_to(found[k] - h / 3);
    }

    /// The find bar's count: the match shown and how many there are.
    pub(crate) fn found(&self) -> (usize, usize) {
        let n = self.tab().page.count(&self.find.text);
        (if n == 0 { 0 } else { self.find_at % n + 1 }, n)
    }

    // ------------------------------------------------------------ pointer

    fn pointer(&mut self, x: i32, y: i32, buttons: u32) {
        let pressed = buttons & !self.buttons;
        self.buttons = buttons;
        if let Some(grab) = self.drag {
            if buttons & LEFT != 0 {
                self.drag_thumb(y - grab);
                return;
            }
            self.drag = None;
        }
        let target = self.geo().target(self, x, y);
        if pressed == 0 {
            let hover = match target {
                chrome::Target::Page => self.link_at(x, y).map_or(Hover::Nothing, Hover::Link),
                t => Hover::Chrome(t),
            };
            if hover != self.hover {
                self.hover = hover;
                self.dirty = true;
            }
            return;
        }
        self.dirty = true;
        let right = pressed & RIGHT != 0;
        if self.menu && !matches!(target, chrome::Target::MenuItem(_) | chrome::Target::Menu) {
            self.menu = false;
            return;
        }
        use chrome::Target as T;
        match target {
            T::Tab(i) => self.switch(i),
            T::CloseTab(i) => self.close_tab(i),
            T::NewTab => self.run(Command::NewTab),
            T::Back => self.run(Command::Back),
            T::Forward => self.run(Command::Forward),
            T::Reload => self.run(Command::Reload),
            T::Home => self.run(Command::Home),
            T::Star => self.run(Command::Bookmark),
            T::Menu => self.menu = !self.menu,
            T::MenuItem(i) => self.run(MENU[i].1),
            T::Address => {
                if self.focus != Focus::Address {
                    self.edit_address();
                } else {
                    self.address.end();
                }
            }
            T::Suggestion(i) => {
                if let Some(m) = self.suggestions.get(i) {
                    let url = m.url.clone();
                    self.focus = Focus::Page;
                    self.navigate(url, None, How::Push);
                }
            }
            T::FindPrev => {
                self.find_at = self.find_at.wrapping_sub(1);
                self.jump_to_found();
            }
            T::FindNext => {
                self.find_at += 1;
                self.jump_to_found();
            }
            T::FindClose => {
                self.find_open = false;
                self.focus = Focus::Page;
            }
            T::FindField => self.focus = Focus::Find,
            T::Scrollbar => self.press_scrollbar(y),
            T::Strip | T::Toolbar => {}
            T::Page => {
                if self.focus == Focus::Address {
                    self.focus = Focus::Page;
                    self.show_address();
                }
                let (px, py) = self.page_point(x, y);
                self.lay_out();
                match self.tab().page.node_at(px, py) {
                    Some(n) => self.activate(n, right),
                    None => {
                        self.focus = Focus::Page;
                        self.tab_mut().ring = None;
                    }
                }
            }
        }
    }

    /// The page point under window point (x, y).
    fn page_point(&self, x: i32, y: i32) -> (i32, i32) {
        let (x0, y0, _, _) = self.view();
        (x - x0, y - y0 + self.tab().scroll)
    }

    fn link_at(&mut self, x: i32, y: i32) -> Option<String> {
        if self.tab().load.is_some() && self.tab().page.layout.is_none() {
            return None;
        }
        let (px, py) = self.page_point(x, y);
        self.lay_out();
        let page = &self.tab().page;
        let node = page.node_at(px, py)?;
        page.link_of(node).map(|(_, u)| u.pretty())
    }

    /// What a click (or Entrée) on `node` does: follow a link, focus a
    /// field, submit a form, tick a box, pick the next option.
    fn activate(&mut self, node: NodeId, new_tab: bool) {
        let page = &self.tab().page;
        if let Some((_, url)) = page.link_of(node) {
            if new_tab {
                let keep = self.current;
                self.open_tab(url);
                // A link opened aside: the person stays where they were.
                self.current = keep;
                self.switched();
            } else {
                self.navigate(url, None, How::Push);
            }
            return;
        }
        let mut control = page.ancestor(node, &["input", "button", "select", "textarea", "label"]);
        if let Some(c) = control
            && page.doc.is(c, "label")
        {
            let e = page.doc.element(c);
            control = e.and_then(|e| e.attr("for")).and_then(|id| page.doc.by_id(id)).or_else(|| {
                page.doc.descendants(c).into_iter().find(|&n| page.doc.is(n, "input") || page.doc.is(n, "select"))
            });
        }
        let Some(c) = control else {
            self.focus = Focus::Page;
            self.tab_mut().ring = None;
            return;
        };
        self.tab_mut().ring = Some(c);
        let page = &self.tab().page;
        if page.is_text_field(c) {
            self.focus_field(c);
        } else if page.is_submit(c) {
            if let Some(s) = page.submit(c) {
                self.focus = Focus::Page;
                self.navigate(s.url, s.post, How::Push);
            }
        } else if page.doc.is(c, "select") {
            self.focus = Focus::Page;
            self.next_option(c);
        } else if page.doc.is(c, "input") {
            self.focus = Focus::Page;
            self.tab_mut().page.toggle(c);
        } else {
            self.focus = Focus::Page;
        }
    }

    /// Picks the option after the chosen one of `select`.
    fn next_option(&mut self, select: NodeId) {
        let page = &mut self.tab_mut().page;
        let options: Vec<NodeId> =
            page.doc.descendants(select).into_iter().filter(|&o| page.doc.is(o, "option")).collect();
        if options.is_empty() {
            return;
        }
        let chosen = options
            .iter()
            .position(|&o| page.doc.element(o).is_some_and(|e| e.attr("selected").is_some()))
            .unwrap_or(0);
        let next = options[(chosen + 1) % options.len()];
        for &o in &options {
            if let Some(e) = page.doc.element_mut(o) {
                e.attrs.retain(|(k, _)| k != "selected");
                if o == next {
                    e.attrs.push((String::from("selected"), String::new()));
                }
            }
        }
        page.invalidate();
    }

    pub(crate) fn thumb(&self) -> Option<(i32, i32)> {
        let (_, h) = self.view_size();
        let full = self.tab().page.layout.as_ref()?.height;
        if full <= h {
            return None;
        }
        let size = (h * h / full).max(24).min(h);
        let max = self.max_scroll().max(1);
        let top = self.tab().scroll * (h - size) / max;
        Some((top, size))
    }

    fn press_scrollbar(&mut self, y: i32) {
        let (_, y0, _, _) = self.view();
        let Some((top, size)) = self.thumb() else { return };
        let at = y - y0;
        if at >= top && at < top + size {
            self.drag = Some(at - top + y0);
        } else {
            // Page up or down toward the click.
            let (_, h) = self.view_size();
            self.scroll_by(if at < top { -(h - LINE) } else { h - LINE });
        }
    }

    fn drag_thumb(&mut self, thumb_top: i32) {
        let Some((_, size)) = self.thumb() else { return };
        let (_, h) = self.view_size();
        let room = (h - size).max(1);
        let y = thumb_top.clamp(0, room) * self.max_scroll() / room;
        self.scroll_to(y);
    }

    // ------------------------------------------------------------ drawing

    /// Draws the browser on `canvas` (its size: the browser's).
    pub fn draw(&mut self, canvas: &mut Canvas) {
        self.lay_out();
        let view = self.view();
        let tab = self.tab();
        let overlay = Overlay {
            focus: match self.focus {
                Focus::Field(n) => Some((n, tab.caret)),
                _ => None,
            },
            ring: tab.ring,
            found: (self.find_open && !self.find.is_empty()).then(|| self.find.text.clone()),
        };
        if let Some(layout) = &tab.page.layout {
            paint::paint(layout, canvas, view, tab.scroll, &overlay);
        }
        chrome::draw(self, canvas);
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests;
