//! What the browser draws around the page, dark: the tabs, the bar with
//! its buttons and the address, the progress bar, the suggestions and the
//! menu, the find bar, the scroll bar and the bubble that shows where a
//! link goes. [`regions`] says where each thing that can be clicked is,
//! for drawing and for the pointer alike.

use crate::{Browser, Focus};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hln_type::{Face, face};
use hln_web::paint::{Canvas, Rect};
use hln_web::style::Color;

/// The height of the tabs' strip.
pub const TABS: i32 = 34;
/// The height of the bar under it.
pub const BAR: i32 = 40;
/// The height of the find bar, at the bottom, when open.
pub const FIND: i32 = 40;
const SCROLLBAR: i32 = 10;
const ROW: i32 = 32;
const MENU_WIDTH: i32 = 260;

const STRIP: Color = Color::rgb(0x16, 0x18, 0x1d);
const SURFACE: Color = Color::rgb(0x2a, 0x2d, 0x35);
const FIELD: Color = Color::rgb(0x16, 0x18, 0x1d);
const HOVER: Color = Color::rgb(0x3a, 0x3e, 0x49);
const LINE: Color = Color::rgb(0x3a, 0x3e, 0x49);
const TEXT: Color = Color::rgb(0xe6, 0xe8, 0xee);
const MUTED: Color = Color::rgb(0x9a, 0xa1, 0xb0);
const FAINT: Color = Color::rgb(0x5c, 0x62, 0x70);
const ACCENT: Color = Color::rgb(0x6f, 0x9b, 0xff);
const SELECTION: Color = Color::rgb(0x2f, 0x4f, 0x8f);
const SECURE: Color = Color::rgb(0x7f, 0xd1, 0x9b);
const STAR: Color = Color::rgb(0xff, 0xc8, 0x57);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    NewTab,
    Find,
    /// One of the browser's own pages (`about:<name>`).
    Page(&'static str),
}

const MENU: [(&str, &str, MenuItem); 7] = [
    ("Nouvel onglet", "Ctrl T", MenuItem::NewTab),
    ("Rechercher dans la page", "Ctrl F", MenuItem::Find),
    ("Favoris", "", MenuItem::Page("favoris")),
    ("Historique", "Ctrl H", MenuItem::Page("historique")),
    ("Page d'essai", "", MenuItem::Page("essai")),
    ("Aide", "", MenuItem::Page("aide")),
    ("À propos", "", MenuItem::Page("a-propos")),
];

/// What a click does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Tab(usize),
    CloseTab(usize),
    NewTab,
    Back,
    Forward,
    Reload,
    Home,
    Address,
    Suggestion(usize),
    Star,
    MenuButton,
    Menu(MenuItem),
    FindField,
    /// The previous place found (`true`) or the next one.
    FindNext(bool),
    FindClose,
    /// The scroll bar's thumb, grabbed this far below its top.
    Thumb(i32),
    /// The scroll bar's track, at this height.
    Track(i32),
}

fn ui(size: i32) -> Face {
    face(false, false, size)
}

fn inside((x0, y0, x1, y1): Rect, x: i32, y: i32) -> bool {
    x >= x0 && x < x1 && y >= y0 && y < y1
}

/// How wide each tab is.
fn tab_width(b: &Browser) -> i32 {
    ((b.width - 48) / b.tabs.len().max(1) as i32).clamp(56, 220)
}

fn address_field(b: &Browser) -> Rect {
    (152, TABS + 5, (b.width - 52).max(260), TABS + BAR - 5)
}

/// The scroll bar's thumb: its top and its length, in window pixels, if
/// the page is longer than the view.
pub fn thumb(b: &Browser) -> Option<(i32, i32)> {
    let (_, vy, _, vy1) = b.view();
    let view = vy1 - vy;
    let height = b.tab().page.layout.as_ref()?.height;
    if height <= view {
        return None;
    }
    let len = (view as i64 * view as i64 / height as i64).max(24) as i32;
    let max = (height - view).max(1);
    let top = vy + ((view - len) as i64 * b.tab().scroll as i64 / max as i64) as i32;
    Some((top, len))
}

/// Everything that can be clicked, from bottom to top.
fn regions(b: &Browser) -> Vec<(Rect, Action)> {
    let mut out = Vec::new();
    let tw = tab_width(b);
    for i in 0..b.tabs.len() {
        let x = i as i32 * tw;
        out.push(((x, 4, x + tw - 2, TABS), Action::Tab(i)));
        if tw >= 90 || i == b.current {
            out.push(((x + tw - 28, 9, x + tw - 6, TABS - 3), Action::CloseTab(i)));
        }
    }
    let plus = b.tabs.len() as i32 * tw + 4;
    out.push(((plus, 6, plus + 28, TABS - 2), Action::NewTab));
    let y = TABS + 4;
    for (i, a) in [Action::Back, Action::Forward, Action::Reload, Action::Home].into_iter().enumerate() {
        let x = 8 + i as i32 * 35;
        out.push(((x, y, x + 32, y + 32), a));
    }
    let field = address_field(b);
    out.push((field, Action::Address));
    out.push(((field.2 - 34, field.1, field.2, field.3), Action::Star));
    out.push(((b.width - 44, y, b.width - 8, y + 32), Action::MenuButton));
    if let Some((top, len)) = thumb(b) {
        let (_, vy, vx1, vy1) = b.view();
        out.push(((vx1 - SCROLLBAR - 2, vy, vx1, vy1), Action::Track(0)));
        out.push(((vx1 - SCROLLBAR - 2, top, vx1, top + len), Action::Thumb(0)));
    }
    if b.finding {
        let y = b.height - FIND;
        out.push(((12, y + 6, 332, y + FIND - 6), Action::FindField));
        out.push(((420, y + 4, 452, y + FIND - 4), Action::FindNext(true)));
        out.push(((454, y + 4, 486, y + FIND - 4), Action::FindNext(false)));
        out.push(((b.width - 40, y + 4, b.width - 8, y + FIND - 4), Action::FindClose));
    }
    for i in 0..b.suggestions().len() {
        let top = TABS + BAR + i as i32 * ROW;
        out.push(((field.0, top, field.2, top + ROW), Action::Suggestion(i)));
    }
    if b.menu {
        let x = b.width - MENU_WIDTH - 8;
        for (i, (_, _, item)) in MENU.iter().enumerate() {
            let top = TABS + BAR + 4 + i as i32 * ROW;
            out.push(((x, top, x + MENU_WIDTH, top + ROW), Action::Menu(*item)));
        }
    }
    out
}

/// What is under (x, y) of the window, if it is the browser's.
pub fn hit(b: &Browser, x: i32, y: i32) -> Option<Action> {
    let (rect, action) = regions(b).into_iter().rev().find(|(r, _)| inside(*r, x, y))?;
    Some(match action {
        Action::Thumb(_) => Action::Thumb(y - rect.1),
        Action::Track(_) => Action::Track(y),
        a => a,
    })
}

/// The middle of what does `action`, if it is shown.
#[cfg(test)]
pub fn center(b: &Browser, action: Action) -> Option<(i32, i32)> {
    regions(b).into_iter().find(|(_, a)| *a == action).map(|(r, _)| ((r.0 + r.2) / 2, (r.1 + r.3) / 2))
}

fn hovered(b: &Browser, rect: Rect) -> bool {
    inside(rect, b.pointer.0, b.pointer.1)
}

// ---------------------------------------------------------------- icons

/// Cosines and sines of the sixteen directions, in thousandths, from east
/// turning clockwise (the window's y goes down).
const ROSE: [(i32, i32); 16] = [
    (1000, 0),
    (924, 383),
    (707, 707),
    (383, 924),
    (0, 1000),
    (-383, 924),
    (-707, 707),
    (-924, 383),
    (-1000, 0),
    (-924, -383),
    (-707, -707),
    (-383, -924),
    (0, -1000),
    (383, -924),
    (707, -707),
    (924, -383),
];

fn at((cx, cy): (i32, i32), (c, s): (i32, i32), r: i32) -> (i32, i32) {
    (cx + c * r / 1000, cy + s * r / 1000)
}

fn arrow(c: &mut Canvas, (cx, cy): (i32, i32), left: bool, color: Color) {
    let d = if left { -1 } else { 1 };
    c.line((cx - 7 * d, cy), (cx + 7 * d, cy), 2, color);
    c.line((cx + 7 * d, cy), (cx + 2 * d, cy - 5), 2, color);
    c.line((cx + 7 * d, cy), (cx + 2 * d, cy + 5), 2, color);
}

fn cross(c: &mut Canvas, (cx, cy): (i32, i32), r: i32, w: i32, color: Color) {
    c.line((cx - r, cy - r), (cx + r, cy + r), w, color);
    c.line((cx - r, cy + r), (cx + r, cy - r), w, color);
}

fn reload(c: &mut Canvas, center: (i32, i32), color: Color) {
    // Three quarters of a circle and its arrow, at the top right.
    for i in 1..14 {
        c.line(at(center, ROSE[i], 7), at(center, ROSE[i + 1], 7), 2, color);
    }
    let end = at(center, ROSE[14], 7);
    c.line(end, (end.0 + 5, end.1), 2, color);
    c.line(end, (end.0, end.1 - 5), 2, color);
}

fn house(c: &mut Canvas, (cx, cy): (i32, i32), color: Color) {
    c.line((cx - 8, cy - 1), (cx, cy - 8), 2, color);
    c.line((cx, cy - 8), (cx + 8, cy - 1), 2, color);
    c.line((cx - 6, cy - 2), (cx - 6, cy + 7), 2, color);
    c.line((cx + 6, cy - 2), (cx + 6, cy + 7), 2, color);
    c.line((cx - 6, cy + 7), (cx + 6, cy + 7), 2, color);
}

fn star(c: &mut Canvas, center: (i32, i32), filled: bool, color: Color) {
    // Ten points from the top, outside and inside in turn.
    const POINTS: [(i32, i32); 10] = [
        (0, -1000),
        (588, -809),
        (951, -309),
        (951, 309),
        (588, 809),
        (0, 1000),
        (-588, 809),
        (-951, 309),
        (-951, -309),
        (-588, -809),
    ];
    let p = |i: usize| at(center, POINTS[i % 10], if i.is_multiple_of(2) { 8 } else { 4 });
    if filled {
        for r in 0..8 {
            for i in 0..10 {
                let scale = |q: (i32, i32)| (center.0 + (q.0 - center.0) * r / 8, center.1 + (q.1 - center.1) * r / 8);
                c.line(scale(p(i)), scale(p(i + 1)), 2, color);
            }
        }
    }
    for i in 0..10 {
        c.line(p(i), p(i + 1), if filled { 2 } else { 1 }, color);
    }
}

fn lock(c: &mut Canvas, (x, y): (i32, i32), color: Color) {
    c.fill_rounded((x, y + 5, x + 10, y + 13), 2, color);
    c.ring((x + 2, y, x + 8, y + 10), 3, 2, color);
}

fn burger(c: &mut Canvas, (cx, cy): (i32, i32), color: Color) {
    for dy in [-5, 0, 5] {
        c.fill((cx - 7, cy + dy - 1, cx + 7, cy + dy + 1), color);
    }
}

fn button(c: &mut Canvas, b: &Browser, rect: Rect, enabled: bool) -> (Color, (i32, i32)) {
    if enabled && hovered(b, rect) {
        c.fill_rounded(rect, 8, HOVER);
    }
    (if enabled { TEXT } else { FAINT }, ((rect.0 + rect.2) / 2, (rect.1 + rect.3) / 2))
}

// ---------------------------------------------------------------- drawing

/// `text` cut to `width` with « … », from (x, y) the line's top.
fn label(c: &mut Canvas, x: i32, y: i32, width: i32, text: &str, face: &Face, color: Color) -> i32 {
    let (kept, cut) = face.fit(text, width);
    let mut end = c.text(x, y, &text[..kept], face, color, false);
    if cut {
        end = c.text(end, y, "…", face, color, false);
    }
    end
}

/// A line being typed, in `rect`: its text scrolled to keep the caret
/// seen, the caret, and the selection.
fn typed(c: &mut Canvas, rect: Rect, line: &crate::edit::Line, face: &Face, placeholder: &str) {
    let ty = face.top_for((rect.1 + rect.3) / 2);
    let room = rect.2 - rect.0;
    let before = &line.text[..crate::edit::byte_at(&line.text, line.caret)];
    let caret = face.width(before);
    let shift = (caret - room + 4).max(0);
    let old = c.set_clip(rect);
    if line.text.is_empty() {
        c.text(rect.0, ty, placeholder, face, FAINT, false);
    } else {
        if line.all {
            let w = face.width(&line.text).min(room);
            c.fill_rounded((rect.0 - 2, ty, rect.0 + w + 2, ty + face.height), 3, SELECTION);
        }
        c.text(rect.0 - shift, ty, &line.text, face, TEXT, false);
    }
    if !line.all {
        let x = rect.0 + caret - shift;
        c.fill((x, ty + 1, x + 1, ty + face.height - 1), ACCENT);
    }
    c.restore_clip(old);
}

pub fn draw(b: &Browser, c: &mut Canvas) {
    let small = ui(13);
    ring(b, c);
    tabs(b, c, &small);
    bar(b, c, &small);
    scrollbar(b, c);
    bubble(b, c);
    if b.finding {
        find_bar(b, c, &small);
    }
    suggestions(b, c, &small);
    if b.menu {
        menu(b, c, &small);
    }
}

/// A ring around the link or control the keyboard is on.
fn ring(b: &Browser, c: &mut Canvas) {
    // A field typed in shows its caret instead.
    if b.focus != Focus::Page {
        return;
    }
    let Some(node) = b.tab().ring else { return };
    let Some((x, y, w, h)) = b.tab().page.rect_of(node) else { return };
    let view = b.view();
    let (x, y) = (view.0 + x, view.1 + y - b.tab().scroll);
    let old = c.set_clip(view);
    c.ring((x - 3, y - 2, x + w + 3, y + h + 2), 5, 2, ACCENT);
    c.restore_clip(old);
}

fn tabs(b: &Browser, c: &mut Canvas, f: &Face) {
    c.fill((0, 0, b.width, TABS), STRIP);
    let tw = tab_width(b);
    for (i, tab) in b.tabs.iter().enumerate() {
        let x = i as i32 * tw;
        let rect = (x, 4, x + tw - 2, TABS + 6);
        let current = i == b.current;
        if current {
            c.fill_rounded(rect, 8, SURFACE);
        } else if hovered(b, (x, 4, x + tw - 2, TABS)) {
            c.fill_rounded(rect, 8, Color::rgb(0x22, 0x25, 0x2c));
        } else if i + 1 != b.current && i + 1 < b.tabs.len() {
            c.fill((x + tw - 2, 12, x + tw - 1, TABS - 8), LINE);
        }
        let closes = tw >= 90 || current;
        let mut tx = x + 12;
        if tab.loading() {
            // A small ring while loading.
            c.ring((tx, 13, tx + 12, 25), 6, 2, ACCENT);
            tx += 18;
        }
        let room = x + tw - tx - if closes { 32 } else { 8 };
        let color = if current { TEXT } else { MUTED };
        label(c, tx, f.top_for(TABS / 2 + 2), room, &tab.title(), f, color);
        if closes {
            let r = (x + tw - 28, 9, x + tw - 6, TABS - 3);
            if hovered(b, r) {
                c.fill_rounded(r, 6, HOVER);
            }
            cross(c, ((r.0 + r.2) / 2, (r.1 + r.3) / 2), 4, 2, color);
        }
    }
    let plus = b.tabs.len() as i32 * tw + 4;
    let r = (plus, 6, plus + 28, TABS - 2);
    let (color, (cx, cy)) = button(c, b, r, true);
    c.fill((cx - 6, cy - 1, cx + 6, cy + 1), color);
    c.fill((cx - 1, cy - 6, cx + 1, cy + 6), color);
}

fn bar(b: &Browser, c: &mut Canvas, f: &Face) {
    let top = TABS;
    c.fill((0, top, b.width, top + BAR), SURFACE);
    c.fill((0, top + BAR - 1, b.width, top + BAR), STRIP);
    let tab = b.tab();
    let y = top + 4;
    let r = |i: i32| (8 + i * 35, y, 40 + i * 35, y + 32);
    let (color, center) = button(c, b, r(0), tab.can_go_back());
    arrow(c, center, true, color);
    let (color, center) = button(c, b, r(1), tab.can_go_forward());
    arrow(c, center, false, color);
    let (color, center) = button(c, b, r(2), true);
    if tab.loading() {
        cross(c, center, 6, 2, color);
    } else {
        reload(c, center, color);
    }
    let (color, center) = button(c, b, r(3), true);
    house(c, center, color);

    // The address.
    let field = address_field(b);
    let focused = b.focus == Focus::Address;
    c.fill_rounded(field, 15, FIELD);
    if focused {
        c.ring(field, 15, 2, ACCENT);
    }
    let mut tx = field.0 + 14;
    if !focused && tab.secure && tab.load.is_none() {
        lock(c, (tx, f.top_for((field.1 + field.3) / 2) + 1), SECURE);
        tx += 18;
    }
    let text_rect = (tx, field.1, field.2 - 38, field.3);
    if focused {
        typed(c, text_rect, &b.address, f, "Rechercher ou saisir une adresse");
    } else {
        let text = b.address_text();
        let ty = f.top_for((field.1 + field.3) / 2);
        if text.is_empty() {
            c.text(tx, ty, "Rechercher ou saisir une adresse", f, FAINT, false);
        } else {
            // The host bright, the rest dim.
            let shown = text.strip_prefix("https://").unwrap_or(&text);
            let shown = if shown.find('/') == Some(shown.len() - 1) { &shown[..shown.len() - 1] } else { shown };
            let host_end = shown.find('/').unwrap_or(shown.len());
            let old = c.set_clip(text_rect);
            let end = c.text(tx, ty, &shown[..host_end], f, TEXT, false);
            label(c, end, ty, text_rect.2 - end, &shown[host_end..], f, MUTED);
            c.restore_clip(old);
        }
    }
    let star_rect = (field.2 - 34, field.1 + 1, field.2 - 4, field.3 - 1);
    if hovered(b, star_rect) {
        c.fill_rounded(star_rect, 12, HOVER);
    }
    let marked = b.bookmarked();
    star(
        c,
        ((star_rect.0 + star_rect.2) / 2, (star_rect.1 + star_rect.3) / 2),
        marked,
        if marked { STAR } else { MUTED },
    );

    let (color, center) = button(c, b, (b.width - 44, y, b.width - 8, y + 32), true);
    burger(c, center, if b.menu { ACCENT } else { color });

    if let Some(p) = b.progress() {
        let w = b.width * p / 1000;
        c.fill((0, top + BAR - 2, w, top + BAR), ACCENT);
    }
}

fn scrollbar(b: &Browser, c: &mut Canvas) {
    let Some((top, len)) = thumb(b) else { return };
    let (_, _, vx1, _) = b.view();
    let x = vx1 - SCROLLBAR;
    let active = b.dragging.is_some() || hovered(b, (x - 2, top, vx1, top + len));
    let color = if active { Color(0xc0_6a_70_80) } else { Color(0x90_80_86_94) };
    c.fill_rounded((x + 2, top + 2, vx1 - 2, top + len - 2), 3, color);
}

fn bubble(b: &Browser, c: &mut Canvas) {
    let f = ui(12);
    let text = match &b.tab().load {
        Some(l) => match l.url.scheme.as_str() {
            "file" => String::from("Ouverture du fichier…"),
            _ => format!("Chargement de {}…", l.url.authority()),
        },
        None => match b.hovered_link() {
            Some(u) => u.to_string(),
            None => return,
        },
    };
    let (_, _, vx1, vy1) = b.view();
    let room = (vx1 / 2).max(200);
    let w = f.width(&text).min(room) + 16;
    let rect = (0, vy1 - f.height - 8, w, vy1);
    c.fill_rounded((rect.0 - 8, rect.1, rect.2, rect.3 + 8), 8, SURFACE);
    c.fill((rect.2 - 1, rect.1 + 8, rect.2, rect.3), LINE);
    label(c, 8, rect.1 + 4, room, &text, &f, MUTED);
}

fn find_bar(b: &Browser, c: &mut Canvas, f: &Face) {
    let y = b.height - FIND;
    c.fill((0, y, b.width, b.height), SURFACE);
    c.fill((0, y, b.width, y + 1), STRIP);
    let field = (12, y + 6, 332, y + FIND - 6);
    let focused = b.focus == Focus::Find;
    c.fill_rounded(field, 14, FIELD);
    if focused {
        c.ring(field, 14, 2, ACCENT);
    }
    let inner = (field.0 + 12, field.1, field.2 - 10, field.3);
    if focused {
        typed(c, inner, &b.find, f, "Rechercher dans la page");
    } else if b.find.text.is_empty() {
        c.text(inner.0, f.top_for((field.1 + field.3) / 2), "Rechercher dans la page", f, FAINT, false);
    } else {
        label(c, inner.0, f.top_for((field.1 + field.3) / 2), inner.2 - inner.0, &b.find.text, f, TEXT);
    }
    let count = b.tab().page.found_at(&b.find.text).len();
    let note = match (b.find.text.is_empty(), count) {
        (true, _) => String::new(),
        (false, 0) => String::from("Introuvable"),
        (false, n) => format!("{} sur {n}", (b.found % n) + 1),
    };
    c.text(342, f.top_for(y + FIND / 2), &note, f, MUTED, false);
    for (rect, up) in [((420, y + 4, 452, y + FIND - 4), true), ((454, y + 4, 486, y + FIND - 4), false)] {
        let (color, (cx, cy)) = button(c, b, rect, count > 0);
        let d = if up { -1 } else { 1 };
        c.line((cx - 6, cy - 3 * d), (cx, cy + 3 * d), 2, color);
        c.line((cx, cy + 3 * d), (cx + 6, cy - 3 * d), 2, color);
    }
    let (color, center) = button(c, b, (b.width - 40, y + 4, b.width - 8, y + FIND - 4), true);
    cross(c, center, 5, 2, color);
}

fn panel(c: &mut Canvas, rect: Rect) {
    // A shadow, then the panel.
    c.fill_rounded((rect.0 - 1, rect.1 + 1, rect.2 + 1, rect.3 + 4), 10, Color(0x60_00_00_00));
    c.fill_rounded(rect, 10, SURFACE);
    c.ring(rect, 10, 1, LINE);
}

fn suggestions(b: &Browser, c: &mut Canvas, f: &Face) {
    let list = b.suggestions();
    if list.is_empty() {
        return;
    }
    let field = address_field(b);
    let top = TABS + BAR;
    panel(c, (field.0, top, field.2, top + list.len() as i32 * ROW + 6));
    let small = ui(12);
    for (i, s) in list.iter().enumerate() {
        let y = top + 3 + i as i32 * ROW;
        let row = (field.0 + 4, y, field.2 - 4, y + ROW);
        if b.picked == Some(i) || hovered(b, row) {
            c.fill_rounded(row, 6, if b.picked == Some(i) { SELECTION } else { HOVER });
        }
        let ty = f.top_for(y + ROW / 2);
        let end = label(c, field.0 + 14, ty, (field.2 - field.0) / 2, &s.label(), f, TEXT);
        let rest = field.2 - 14 - end - 16;
        if rest > 40 {
            label(c, end + 16, small.top_for(y + ROW / 2), rest, &s.url.pretty(), &small, MUTED);
        }
    }
}

fn menu(b: &Browser, c: &mut Canvas, f: &Face) {
    let x = b.width - MENU_WIDTH - 8;
    let top = TABS + BAR;
    panel(c, (x, top, x + MENU_WIDTH, top + MENU.len() as i32 * ROW + 8));
    let small = ui(12);
    for (i, (name, keys, _)) in MENU.iter().enumerate() {
        let y = top + 4 + i as i32 * ROW;
        let row = (x + 4, y, x + MENU_WIDTH - 4, y + ROW);
        if hovered(b, row) {
            c.fill_rounded(row, 6, HOVER);
        }
        c.text(x + 16, f.top_for(y + ROW / 2), name, f, TEXT, false);
        if !keys.is_empty() {
            let w = small.width(keys);
            c.text(x + MENU_WIDTH - 16 - w, small.top_for(y + ROW / 2), keys, &small, FAINT, false);
        }
        // Le P'tit Âne draws the line here.
        if i == 3 {
            c.fill((x + 12, y + ROW + 1, x + MENU_WIDTH - 12, y + ROW + 2), LINE);
        }
    }
}
