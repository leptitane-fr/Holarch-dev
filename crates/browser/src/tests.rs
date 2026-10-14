use super::*;
use alloc::vec;
use alloc::vec::Vec;

/// A network of a few pages, counting what it was asked.
struct Fake {
    pages: Vec<(&'static str, &'static str, &'static str)>,
    asked: Vec<String>,
}

impl Net for Fake {
    fn fetch(&mut self, url: &Url, post: Option<&str>) -> Result<Response, NetError> {
        let key = url.to_string();
        self.asked.push(match post {
            Some(p) => format!("POST {key} {p}"),
            None => key.clone(),
        });
        let (_, ct, body) = self
            .pages
            .iter()
            .find(|(u, _, _)| *u == key)
            .ok_or_else(|| NetError::Failed(String::from("introuvable")))?;
        Ok(Response {
            url: url.clone(),
            status: 200,
            content_type: Some(String::from(*ct)),
            body: body.as_bytes().to_vec(),
        })
    }
}

fn web() -> Fake {
    Fake {
        pages: vec![
            (
                "https://exemple.fr/",
                "text/html",
                "<title>Accueil</title><link rel=stylesheet href=/s.css>\
                 <p><a href='/deux'>deux</a></p><div style='height:2000px'></div><p id=bas>bas de page</p>",
            ),
            ("https://exemple.fr/s.css", "text/css", "@import 'i.css'; p { margin: 0 }"),
            ("https://exemple.fr/i.css", "text/css", "body { margin: 0 }"),
            (
                "https://exemple.fr/deux",
                "text/html",
                "<title>Deux</title><form method=post action=/envoi><input name=q><button>OK</button></form>",
            ),
            ("https://exemple.fr/envoi", "text/html", "<title>Reçu</title>merci"),
        ],
        asked: Vec::new(),
    }
}

fn settle(b: &mut Browser, net: &mut dyn Net) {
    for _ in 0..50 {
        if !b.needs_load() {
            return;
        }
        b.load(net);
    }
    panic!("the load never ends");
}

fn type_text(b: &mut Browser, text: &str) {
    for c in text.chars() {
        b.handle(Event::Char(c));
    }
}

#[test]
fn starts_on_the_home_page_offline() {
    let mut b = Browser::new(800, 600);
    settle(&mut b, &mut Offline);
    assert_eq!(b.title(), "Nouvel onglet");
    assert_eq!(b.focus(), Focus::Address);
    let mut canvas = Canvas::new(800, 600);
    b.draw(&mut canvas);
    assert!(!b.dirty());
    // No network: the page says so honestly.
    b.handle(Event::Key(Key::Escape));
    b.handle(Event::Key(Key::Escape));
    type_text(&mut b, "exemple.fr");
    b.handle(Event::Key(Key::Enter));
    settle(&mut b, &mut Offline);
    assert_eq!(b.title(), "Pas encore d'accès au réseau");
    assert_eq!(b.url(), "https://exemple.fr/");
}

#[test]
fn loads_a_page_its_sheets_and_imports_then_goes_back() {
    let mut net = web();
    let mut b = Browser::new(800, 600);
    settle(&mut b, &mut net);
    type_text(&mut b, "exemple.fr");
    b.handle(Event::Key(Key::Enter));
    settle(&mut b, &mut net);
    assert_eq!(b.title(), "Accueil");
    assert_eq!(net.asked, ["https://exemple.fr/", "https://exemple.fr/s.css", "https://exemple.fr/i.css"]);
    // An anchor within the page scrolls without loading.
    b.open("https://exemple.fr/#bas");
    assert!(!b.needs_load());
    assert!(b.tab().scroll > 1000);
    b.handle(Event::Key(Key::Home));
    assert_eq!(b.tab().scroll, 0);
    // A click on the link (body margin 0 from the imported sheet).
    b.handle(Event::Pointer { x: 10, y: TOP + 8, buttons: 0 });
    assert!(matches!(&b.hover, Hover::Link(u) if u == "exemple.fr/deux"));
    b.handle(Event::Pointer { x: 10, y: TOP + 8, buttons: LEFT });
    b.handle(Event::Pointer { x: 10, y: TOP + 8, buttons: 0 });
    settle(&mut b, &mut net);
    assert_eq!(b.title(), "Deux");
    // Type in the field, send the form by Entrée.
    b.handle(Event::Key(Key::Tab));
    assert!(matches!(b.focus(), Focus::Field(_)));
    type_text(&mut b, "âne");
    b.handle(Event::Key(Key::Enter));
    settle(&mut b, &mut net);
    assert_eq!(b.title(), "Reçu");
    assert_eq!(net.asked.last().map(String::as_str), Some("POST https://exemple.fr/envoi q=%C3%A2ne"));
    // Back twice: the page with the form, then the first one.
    b.handle(Event::Key(Key::Backspace));
    settle(&mut b, &mut net);
    assert_eq!(b.title(), "Deux");
    b.run(Command::Back);
    b.run(Command::Back);
    settle(&mut b, &mut net);
    assert_eq!(b.title(), "Accueil");
    assert!(b.can(Command::Forward));
}

#[test]
fn tabs_bookmarks_suggestions_and_find() {
    let mut net = web();
    let mut b = Browser::new(800, 600);
    b.open("https://exemple.fr/");
    settle(&mut b, &mut net);
    b.run(Command::Bookmark);
    assert!(b.is_bookmarked());
    b.handle(Event::Ctrl('t'));
    settle(&mut b, &mut net);
    assert_eq!(b.tabs.len(), 2);
    type_text(&mut b, "exem");
    assert_eq!(b.suggestions.len(), 1);
    b.handle(Event::Key(Key::Down));
    b.handle(Event::Key(Key::Enter));
    settle(&mut b, &mut net);
    assert_eq!(b.title(), "Accueil");
    b.handle(Event::Char('/'));
    type_text(&mut b, "BAS DE");
    assert_eq!(b.found(), (1, 1));
    assert!(b.tab().scroll > 1000);
    b.handle(Event::Key(Key::Escape));
    assert!(!b.find_open);
    // Menu, then « Fermer l'onglet ».
    let g = b.geo();
    let (x, y) = center(g.menu_button());
    b.handle(Event::Pointer { x, y, buttons: LEFT });
    b.handle(Event::Pointer { x, y, buttons: 0 });
    assert!(b.menu);
    let close = MENU.iter().position(|(_, c)| *c == Command::CloseTab).unwrap();
    let (x, y) = center(g.menu_item(close));
    b.handle(Event::Pointer { x, y, buttons: LEFT });
    assert_eq!(b.tabs.len(), 1);
    let mut canvas = Canvas::new(800, 600);
    b.draw(&mut canvas);
}

fn center((x0, y0, x1, y1): Rect) -> (i32, i32) {
    ((x0 + x1) / 2, (y0 + y1) / 2)
}
