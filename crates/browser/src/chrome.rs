//! The browser's frame, drawn around the page: the tab strip, the toolbar
//! (back, forward, reload, home, the address bar with its lock and star,
//! the menu), the loading bar, the scrollbar, the status bubble, the find
//! bar, the menu and the address bar's suggestions. Dark, whatever the
//! page. [`Geo`] places everything, for drawing and for the pointer alike.

use crate::{Browser, FIND, Focus, Hover, MENU, SCROLLBAR, TABS, TOP};
use alloc::format;
use alloc::string::String;
use hln_type::{Face, face};
use hln_web::paint::{Canvas, Rect};
use hln_web::style::Color;

const STRIP: Color = Color::rgb(0x16, 0x18, 0x1e);
const BAR: Color = Color::rgb(0x26, 0x2a, 0x34);
const FIELD: Color = Color::rgb(0x16, 0x18, 0x1e);
const HOVER: Color = Color::rgb(0x36, 0x3b, 0x49);
const POPUP: Color = Color::rgb(0x2c, 0x30, 0x3b);
const EDGE: Color = Color::rgb(0x3e, 0x44, 0x53);
const TEXT: Color = Color::rgb(0xe6, 0xe9, 0xef);
const DIM: Color = Color::rgb(0x8b, 0x93, 0xa5);
const FAINT: Color = Color::rgb(0x55, 0x5c, 0x6b);
const ACCENT: Color = Color::rgb(0x7a, 0xa2, 0xf7);
const GOLD: Color = Color::rgb(0xf5, 0xc4, 0x51);

/// What lies under a point of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Strip,
    Tab(usize),
    CloseTab(usize),
    NewTab,
    Toolbar,
    Back,
    Forward,
    Reload,
    Home,
    Address,
    Star,
    Menu,
    MenuItem(usize),
    Suggestion(usize),
    FindField,
    FindPrev,
    FindNext,
    FindClose,
    Scrollbar,
    Page,
}

fn inside((x0, y0, x1, y1): Rect, x: i32, y: i32) -> bool {
    x >= x0 && x < x1 && y >= y0 && y < y1
}

/// Where everything goes, for a window `w` × `h`.
pub struct Geo {
    pub w: i32,
    pub h: i32,
    pub tabs: usize,
    pub find: bool,
}

const BUTTON: i32 = 30;
const SUGGESTION: i32 = 36;

impl Geo {
    fn tab_width(&self) -> i32 {
        ((self.w - 8 - 40) / self.tabs.max(1) as i32).clamp(36, 220)
    }

    pub fn tab(&self, i: usize) -> Rect {
        let tw = self.tab_width();
        let x = 8 + i as i32 * tw;
        (x, 6, x + tw - 2, TABS)
    }

    pub fn close(&self, i: usize) -> Rect {
        let t = self.tab(i);
        (t.2 - 24, 10, t.2 - 4, 30)
    }

    pub fn new_tab(&self) -> Rect {
        let x = (8 + self.tabs as i32 * self.tab_width() + 4).min(self.w - 30);
        (x, 8, x + 26, 32)
    }

    /// The toolbar's buttons: back, forward, reload, home.
    pub fn button(&self, k: i32) -> Rect {
        let x = 6 + k * (BUTTON + 2);
        (x, TABS + 5, x + BUTTON, TABS + 5 + BUTTON)
    }

    pub fn menu_button(&self) -> Rect {
        (self.w - 6 - BUTTON, TABS + 5, self.w - 6, TABS + 5 + BUTTON)
    }

    pub fn address(&self) -> Rect {
        (self.button(4).0 + 4, TABS + 6, self.w - 12 - BUTTON, TOP - 6)
    }

    pub fn star(&self) -> Rect {
        let a = self.address();
        (a.2 - 30, a.1, a.2 - 2, a.3)
    }

    /// The height of a menu item: smaller in a short window, for all of
    /// them to show.
    fn item(&self) -> i32 {
        ((self.h - TOP - 8) / MENU.len() as i32).clamp(20, 30)
    }

    pub fn menu(&self) -> Rect {
        (self.w - 262, TOP - 2, self.w - 6, TOP + 4 + self.item() * MENU.len() as i32)
    }

    pub fn menu_item(&self, i: usize) -> Rect {
        let m = self.menu();
        let y = m.1 + 3 + i as i32 * self.item();
        (m.0 + 4, y, m.2 - 4, y + self.item())
    }

    pub fn suggestion(&self, i: usize) -> Rect {
        let a = self.address();
        let y = TOP + 2 + i as i32 * SUGGESTION;
        (a.0, y, a.2, y + SUGGESTION)
    }

    pub fn content(&self) -> Rect {
        (0, TOP, self.w - SCROLLBAR, self.bottom())
    }

    fn bottom(&self) -> i32 {
        if self.find { self.h - FIND } else { self.h }
    }

    pub fn scrollbar(&self) -> Rect {
        (self.w - SCROLLBAR, TOP, self.w, self.bottom())
    }

    pub fn find_field(&self) -> Rect {
        (104, self.h - FIND + 6, (self.w - 220).max(160), self.h - 6)
    }

    pub fn find_prev(&self) -> Rect {
        (self.w - 102, self.h - FIND + 4, self.w - 72, self.h - 4)
    }

    pub fn find_next(&self) -> Rect {
        (self.w - 70, self.h - FIND + 4, self.w - 40, self.h - 4)
    }

    pub fn find_close(&self) -> Rect {
        (self.w - 36, self.h - FIND + 4, self.w - 6, self.h - 4)
    }

    /// What is under (x, y), popups first.
    pub fn target(&self, b: &Browser, x: i32, y: i32) -> Target {
        if b.menu {
            for i in 0..MENU.len() {
                if inside(self.menu_item(i), x, y) {
                    return Target::MenuItem(i);
                }
            }
        }
        if b.focus == Focus::Address {
            for i in 0..b.suggestions.len() {
                if inside(self.suggestion(i), x, y) {
                    return Target::Suggestion(i);
                }
            }
        }
        if y < TABS {
            for i in 0..self.tabs {
                if inside(self.close(i), x, y) && self.tab_width() > 80 {
                    return Target::CloseTab(i);
                }
                if inside(self.tab(i), x, y) {
                    return Target::Tab(i);
                }
            }
            return if inside(self.new_tab(), x, y) { Target::NewTab } else { Target::Strip };
        }
        if y < TOP {
            let buttons = [Target::Back, Target::Forward, Target::Reload, Target::Home];
            for (k, t) in buttons.into_iter().enumerate() {
                if inside(self.button(k as i32), x, y) {
                    return t;
                }
            }
            return if inside(self.star(), x, y) {
                Target::Star
            } else if inside(self.address(), x, y) {
                Target::Address
            } else if inside(self.menu_button(), x, y) {
                Target::Menu
            } else {
                Target::Toolbar
            };
        }
        if self.find && y >= self.h - FIND {
            return if inside(self.find_prev(), x, y) {
                Target::FindPrev
            } else if inside(self.find_next(), x, y) {
                Target::FindNext
            } else if inside(self.find_close(), x, y) {
                Target::FindClose
            } else {
                Target::FindField
            };
        }
        if inside(self.scrollbar(), x, y) { Target::Scrollbar } else { Target::Page }
    }
}

fn center((x0, y0, x1, y1): Rect) -> (i32, i32) {
    ((x0 + x1) / 2, (y0 + y1) / 2)
}

/// Writes `text` cut to `width` (with « … »), its capitals centered on
/// `middle`; returns where it ends.
fn label(canvas: &mut Canvas, x: i32, middle: i32, text: &str, f: &Face, color: Color, width: i32) -> i32 {
    let (kept, cut) = f.fit(text, width);
    let top = f.top_for(middle);
    let end = canvas.text(x, top, &text[..kept], f, color, false);
    if cut { canvas.text(end, top, "…", f, color, false) } else { end }
}

fn hovered(b: &Browser, t: Target) -> bool {
    b.hover == Hover::Chrome(t)
}

fn button_back(canvas: &mut Canvas, b: &Browser, t: Target, r: Rect) {
    if hovered(b, t) {
        canvas.fill_rounded(r, 8, HOVER);
    }
}

fn arrow(canvas: &mut Canvas, (cx, cy): (i32, i32), dir: i32, color: Color) {
    canvas.line((cx - 7, cy), (cx + 7, cy), 2, color);
    let tip = cx + 7 * dir;
    canvas.line((tip, cy), (tip - 6 * dir, cy - 6), 2, color);
    canvas.line((tip, cy), (tip - 6 * dir, cy + 6), 2, color);
}

fn reload(canvas: &mut Canvas, (cx, cy): (i32, i32), color: Color, under: Color) {
    canvas.ring((cx - 7, cy - 7, cx + 8, cy + 8), 8, 2, color);
    canvas.fill((cx + 1, cy - 8, cx + 9, cy), under);
    canvas.line((cx + 1, cy - 6), (cx - 3, cy - 10), 2, color);
    canvas.line((cx + 1, cy - 6), (cx - 3, cy - 2), 2, color);
}

fn house(canvas: &mut Canvas, (cx, cy): (i32, i32), color: Color) {
    canvas.line((cx - 8, cy - 1), (cx, cy - 8), 2, color);
    canvas.line((cx, cy - 8), (cx + 8, cy - 1), 2, color);
    canvas.ring((cx - 5, cy - 2, cx + 6, cy + 8), 1, 2, color);
}

fn lock(canvas: &mut Canvas, x: i32, cy: i32, color: Color) {
    canvas.ring((x + 1, cy - 8, x + 9, cy + 1), 4, 2, color);
    canvas.fill_rounded((x, cy - 2, x + 10, cy + 6), 2, color);
}

/// The star's points, thousandths of its radius, from the top clockwise.
const STAR: [(i32, i32); 10] = [
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

/// Whether (x, y) lies in the polygon `points` (even-odd rule).
fn in_polygon(points: &[(i32, i32)], x: i32, y: i32) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let ((xi, yi), (xj, yj)) = (points[i], points[j]);
        if (yi > y) != (yj > y) && x < xi + (i64::from(xj - xi) * i64::from(y - yi) / i64::from(yj - yi)) as i32 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// A five-pointed star of radius `r` pixels, smoothed (16 samples a
/// pixel).
fn star(canvas: &mut Canvas, (cx, cy): (i32, i32), r: i32, color: Color) {
    let cy = cy + 1;
    let mut points = [(0, 0); 10];
    for (k, &(dx, dy)) in STAR.iter().enumerate() {
        let radius = if k % 2 == 0 { r * 16 } else { r * 16 * 39 / 100 };
        points[k] = (cx * 16 + dx * radius / 1000, cy * 16 + dy * radius / 1000);
    }
    for y in cy - r - 1..=cy + r {
        for x in cx - r - 1..=cx + r {
            let mut cover = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    cover += u32::from(in_polygon(&points, x * 16 + sx * 4 + 2, y * 16 + sy * 4 + 2));
                }
            }
            if cover > 0 {
                canvas.blend(x, y, color, cover * 255 / 16);
            }
        }
    }
}

fn menu_icon(canvas: &mut Canvas, (cx, cy): (i32, i32), color: Color) {
    for dy in [-5, 0, 5] {
        canvas.fill_rounded((cx - 7, cy + dy - 1, cx + 8, cy + dy + 1), 1, color);
    }
}

fn plus(canvas: &mut Canvas, (cx, cy): (i32, i32), color: Color) {
    canvas.fill((cx - 6, cy - 1, cx + 7, cy + 1), color);
    canvas.fill((cx - 1, cy - 6, cx + 1, cy + 7), color);
}

fn cross(canvas: &mut Canvas, (cx, cy): (i32, i32), color: Color) {
    canvas.line((cx - 4, cy - 4), (cx + 4, cy + 4), 2, color);
    canvas.line((cx - 4, cy + 4), (cx + 4, cy - 4), 2, color);
}

/// A field with `text` and, when typed in, its caret (in characters) and
/// whether all of it is selected.
fn field(canvas: &mut Canvas, r: Rect, text: &str, caret: Option<(usize, bool)>, f: &Face, placeholder: &str) {
    canvas.fill_rounded(r, (r.3 - r.1) / 2, FIELD);
    if caret.is_some() {
        canvas.ring(r, (r.3 - r.1) / 2, 2, ACCENT);
    }
    let (_, middle) = center(r);
    let x0 = r.0 + 12;
    let room = r.2 - r.0 - 24;
    let old = canvas.set_clip((r.0 + 8, r.1, r.2 - 8, r.3));
    if text.is_empty() {
        canvas.text(x0, f.top_for(middle), placeholder, f, FAINT, false);
        if caret.is_some() {
            canvas.fill((x0, middle - 8, x0 + 2, middle + 8), ACCENT);
        }
    } else {
        let at = caret.map_or(0, |(c, _)| f.width(&text[..crate::edit::byte_at(text, c)]));
        let shift = (at - room + 4).max(0);
        let x = x0 - shift;
        if let Some((_, true)) = caret {
            let w = f.width(text);
            canvas.fill_rounded((x - 2, middle - 10, x + w + 2, middle + 10), 3, ACCENT.with_alpha(0x70));
        }
        canvas.text(x, f.top_for(middle), text, f, TEXT, false);
        if let Some((_, false)) = caret {
            canvas.fill((x + at, middle - 8, x + at + 2, middle + 8), ACCENT);
        }
    }
    canvas.restore_clip(old);
}

// Le P'tit Âne a mis ses oreilles dans les onglets.

pub fn draw(b: &Browser, canvas: &mut Canvas) {
    let g = b.geo();
    let small = face(false, false, 13);
    let bold = face(false, true, 13);
    let text = face(false, false, 14);

    // Tabs.
    canvas.fill((0, 0, g.w, TABS), STRIP);
    for (i, tab) in b.tabs.iter().enumerate() {
        let r = g.tab(i);
        let on = i == b.current;
        if on {
            canvas.fill_rounded((r.0, r.1, r.2, r.3 + 8), 8, BAR);
        } else if hovered(b, crate::chrome::Target::Tab(i)) || hovered(b, Target::CloseTab(i)) {
            canvas.fill_rounded((r.0, r.1 + 2, r.2, r.3 - 2), 8, HOVER);
        } else if i + 1 != b.current && i + 1 < b.tabs.len() {
            canvas.fill((r.2, r.1 + 8, r.2 + 1, r.3 - 6), EDGE);
        }
        let middle = (r.1 + r.3) / 2;
        let mut x = r.0 + 10;
        if let Some((done, total)) = tab.progress() {
            // A spinner-ish dot that fills with the load.
            canvas.ring((x, middle - 5, x + 10, middle + 5), 5, 2, FAINT);
            let filled = 10 * (done as i32 + 1) / (total as i32 + 1).max(1);
            canvas.fill_rounded((x, middle + 5 - filled, x + 10, middle + 5), 3, ACCENT);
            x += 16;
        }
        let close = g.close(i);
        let wide = g.tab_width() > 80;
        let room = if wide { close.0 - x - 4 } else { r.2 - x - 6 };
        label(canvas, x, middle, &tab.title(), if on { &bold } else { &small }, if on { TEXT } else { DIM }, room);
        if wide && (on || hovered(b, Target::Tab(i)) || hovered(b, Target::CloseTab(i))) {
            if hovered(b, Target::CloseTab(i)) {
                canvas.fill_rounded(close, 6, EDGE);
            }
            cross(canvas, center(close), if on { TEXT } else { DIM });
        }
    }
    let n = g.new_tab();
    button_back(canvas, b, Target::NewTab, n);
    plus(canvas, center(n), DIM);

    // Toolbar.
    canvas.fill((0, TABS, g.w, TOP), BAR);
    let icons = [Target::Back, Target::Forward, Target::Reload, Target::Home];
    for (k, t) in icons.into_iter().enumerate() {
        let r = g.button(k as i32);
        let usable = match t {
            Target::Back => b.can(crate::Command::Back),
            Target::Forward => b.can(crate::Command::Forward),
            _ => true,
        };
        if usable {
            button_back(canvas, b, t, r);
        }
        let color = if usable { TEXT } else { FAINT };
        let under = if usable && hovered(b, t) { HOVER } else { BAR };
        match t {
            Target::Back => arrow(canvas, center(r), -1, color),
            Target::Forward => arrow(canvas, center(r), 1, color),
            Target::Reload => reload(canvas, center(r), color, under),
            _ => house(canvas, center(r), color),
        }
    }
    let a = g.address();
    let editing = b.focus == Focus::Address;
    if editing {
        field(
            canvas,
            a,
            &b.address.text,
            Some((b.address.caret, b.address.all)),
            &text,
            "Rechercher ou saisir une adresse",
        );
    } else {
        canvas.fill_rounded(a, (a.3 - a.1) / 2, FIELD);
        if hovered(b, Target::Address) {
            canvas.ring(a, (a.3 - a.1) / 2, 1, EDGE);
        }
        let url = b.tab().url();
        let (_, middle) = center(a);
        let mut x = a.0 + 12;
        if url.scheme == "https" {
            lock(canvas, x, middle + 1, DIM);
            x += 16;
        }
        let room = g.star().0 - x - 4;
        if crate::is_home(url) {
            label(canvas, x, middle, "Rechercher ou saisir une adresse", &text, FAINT, room);
        } else {
            let shown = url.pretty();
            let host_end = if url.is_hierarchical() { shown.find('/').unwrap_or(shown.len()) } else { shown.len() };
            let end = label(canvas, x, middle, &shown[..host_end], &text, TEXT, room);
            if host_end < shown.len() {
                label(canvas, end, middle, &shown[host_end..], &text, DIM, room - (end - x));
            }
        }
        let s = g.star();
        if b.is_bookmarked() {
            star(canvas, center(s), 8, GOLD);
        } else {
            star(canvas, center(s), 8, if hovered(b, Target::Star) { TEXT } else { DIM });
            star(canvas, center(s), 5, FIELD);
        }
    }
    let m = g.menu_button();
    if b.menu {
        canvas.fill_rounded(m, 8, HOVER);
    } else {
        button_back(canvas, b, Target::Menu, m);
    }
    menu_icon(canvas, center(m), TEXT);

    // The load's progress, along the toolbar's bottom.
    if let Some((done, total)) = b.tab().progress() {
        let w = g.w * (done as i32 * 2 + 1) / (total as i32 * 2 + 2).max(1);
        canvas.fill((0, TOP - 2, w, TOP), ACCENT);
    }

    // Scrollbar.
    let sb = g.scrollbar();
    canvas.fill(sb, Color::rgb(0x1d, 0x20, 0x28));
    if let Some((top, size)) = b.thumb() {
        let color = if hovered(b, Target::Scrollbar) { DIM } else { Color::rgb(0x4a, 0x50, 0x60) };
        canvas.fill_rounded((sb.0 + 2, sb.1 + top + 2, sb.2 - 2, sb.1 + top + size - 2), 3, color);
    }

    // Status: the link under the pointer, or what loads.
    let status = match &b.hover {
        Hover::Link(url) => Some(url.clone()),
        _ => b.tab().progress().map(|_| format!("Chargement de {}…", b.tab().url().pretty())),
    };
    if let Some(s) = status {
        let c = g.content();
        let w = small.width(&s).min(c.2 * 2 / 3) + 20;
        let r = (c.0 + 4, c.3 - 26, c.0 + 4 + w, c.3 - 4);
        canvas.fill_rounded(r, 6, BAR);
        canvas.ring(r, 6, 1, EDGE);
        label(canvas, r.0 + 10, center(r).1, &s, &small, TEXT, w - 20);
    }

    // Find bar.
    if b.find_open {
        let y0 = g.h - FIND;
        canvas.fill((0, y0, g.w, g.h), BAR);
        canvas.fill((0, y0, g.w, y0 + 1), EDGE);
        label(canvas, 12, y0 + FIND / 2, "Rechercher", &small, DIM, 90);
        let caret = (b.focus == Focus::Find).then_some((b.find.caret, b.find.all));
        let f = g.find_field();
        field(canvas, f, &b.find.text, caret, &small, "dans la page");
        if !b.find.is_empty() {
            let (at, n) = b.found();
            let count = if n == 0 { String::from("Aucun résultat") } else { format!("{at} sur {n}") };
            let color = if n == 0 { Color::rgb(0xff, 0x9e, 0x8f) } else { DIM };
            label(canvas, f.2 + 10, y0 + FIND / 2, &count, &small, color, g.find_prev().0 - f.2 - 14);
        }
        for (t, r) in
            [(Target::FindPrev, g.find_prev()), (Target::FindNext, g.find_next()), (Target::FindClose, g.find_close())]
        {
            button_back(canvas, b, t, r);
            let (cx, cy) = center(r);
            match t {
                Target::FindPrev => {
                    canvas.line((cx - 5, cy + 3), (cx, cy - 3), 2, TEXT);
                    canvas.line((cx, cy - 3), (cx + 5, cy + 3), 2, TEXT);
                }
                Target::FindNext => {
                    canvas.line((cx - 5, cy - 3), (cx, cy + 3), 2, TEXT);
                    canvas.line((cx, cy + 3), (cx + 5, cy - 3), 2, TEXT);
                }
                _ => cross(canvas, (cx, cy), TEXT),
            }
        }
    }

    // Suggestions under the address bar.
    if editing && !b.suggestions.is_empty() {
        let first = g.suggestion(0);
        let last = g.suggestion(b.suggestions.len() - 1);
        let r = (first.0, first.1 - 2, first.2, last.3 + 4);
        canvas.fill_rounded(r, 10, POPUP);
        canvas.ring(r, 10, 1, EDGE);
        for (i, m) in b.suggestions.iter().enumerate() {
            let s = g.suggestion(i);
            if b.pick == Some(i) || hovered(b, Target::Suggestion(i)) {
                canvas.fill_rounded((s.0 + 4, s.1 + 1, s.2 - 4, s.3 - 1), 6, HOVER);
            }
            let title = if m.title.is_empty() { m.url.pretty() } else { m.title.clone() };
            let w = s.2 - s.0 - 28;
            let middle = center(s).1;
            let end = label(canvas, s.0 + 14, middle, &title, &small, TEXT, w * 3 / 5);
            label(canvas, end + 10, middle, &m.url.pretty(), &small, ACCENT, s.2 - end - 24);
        }
    }

    // The menu.
    if b.menu {
        let r = g.menu();
        canvas.fill_rounded(r, 10, POPUP);
        canvas.ring(r, 10, 1, EDGE);
        for (i, (name, command)) in MENU.iter().enumerate() {
            let it = g.menu_item(i);
            if hovered(b, Target::MenuItem(i)) {
                canvas.fill_rounded(it, 6, HOVER);
            }
            let name =
                if *command == crate::Command::Bookmark && b.is_bookmarked() { "Retirer des favoris" } else { name };
            label(canvas, it.0 + 12, center(it).1, name, &small, TEXT, it.2 - it.0 - 24);
        }
    }
}
