//! `cargo xtask bench [address…] [--size WxH] [--out dir]`: drives the
//! whole browser (tabs, toolbar, its own pages) as the holon would, with
//! `curl` for the network and the disk for `file:` addresses, and saves a
//! PNG after each step to look at.

use crate::{Result, page::fetch, png, root};
use hln_browser::{Browser, Command, Event, Key, LEFT, Net, NetError, Response, TOP};
use hln_web::paint::Canvas;
use hln_web::url::{self, Url};
use std::fs;
use std::path::PathBuf;

/// The network seen from the development machine.
struct Host;

impl Net for Host {
    fn fetch(&mut self, url: &Url, post: Option<&str>) -> std::result::Result<Response, NetError> {
        match url.scheme.as_str() {
            "file" => {
                let path = url::decode(&url.path);
                let body = fs::read(&path).map_err(|e| NetError::Failed(format!("{path} : {e}")))?;
                let ct = if path.ends_with(".css") {
                    "text/css"
                } else if path.ends_with(".txt") {
                    "text/plain"
                } else {
                    "text/html"
                };
                Ok(Response { url: url.clone(), status: 200, content_type: Some(ct.into()), body })
            }
            "http" | "https" => {
                if post.is_some() {
                    return Err(NetError::Failed("le banc n'envoie pas de formulaire POST".into()));
                }
                let (final_url, ct, body) = fetch(&url.to_string()).map_err(NetError::Failed)?;
                let final_url = Url::parse(&final_url).unwrap_or_else(|| url.clone());
                Ok(Response { url: final_url, status: 200, content_type: ct, body })
            }
            _ => Err(NetError::Unsupported),
        }
    }
}

struct Bench {
    browser: Browser,
    canvas: Canvas,
    out: PathBuf,
    shot: usize,
    size: (i32, i32),
}

impl Bench {
    fn settle(&mut self) {
        let mut steps = 0;
        while self.browser.needs_load() && steps < 200 {
            self.browser.load(&mut Host);
            steps += 1;
        }
    }

    fn shoot(&mut self, name: &str) -> Result {
        self.browser.draw(&mut self.canvas);
        self.shot += 1;
        let path = self.out.join(format!("{:02}-{name}.png", self.shot));
        fs::write(&path, png::encode(self.size.0 as u32, self.size.1 as u32, &self.canvas.pixels))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("xtask: « {} » -> {}", self.browser.title(), path.display());
        Ok(())
    }

    fn click(&mut self, x: i32, y: i32) {
        self.browser.handle(Event::Pointer { x, y, buttons: LEFT });
        self.browser.handle(Event::Pointer { x, y, buttons: 0 });
    }
}

pub fn run(args: &[String]) -> Result {
    let mut size = (1024, 700);
    let mut out = root().join("build/bench");
    let mut addresses = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let s = args.get(i + 1).ok_or("--size WxH")?;
                let (w, h) = s.split_once('x').ok_or("--size WxH")?;
                size = (w.parse().map_err(|_| "--size")?, h.parse().map_err(|_| "--size")?);
                i += 1;
            }
            "--out" => {
                out = PathBuf::from(args.get(i + 1).ok_or("--out dir")?);
                i += 1;
            }
            a if std::path::Path::new(a).exists() => {
                let full = fs::canonicalize(a).map_err(|e| e.to_string())?;
                addresses.push(format!("file://{}", full.display()));
            }
            a => addresses.push(a.to_string()),
        }
        i += 1;
    }
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut b =
        Bench { browser: Browser::new(size.0, size.1), canvas: Canvas::new(size.0, size.1), out, shot: 0, size };
    b.browser.bookmark("Page d'essai", Url::parse("about:essai").ok_or("about")?);
    b.browser.bookmark("Aide", Url::parse("about:aide").ok_or("about")?);
    b.settle();
    b.shoot("accueil")?;
    for c in "pyp".chars() {
        b.browser.handle(Event::Char(c));
    }
    b.shoot("saisie")?;
    b.browser.handle(Event::Key(Key::Escape));
    b.browser.run(Command::Help);
    b.settle();
    b.shoot("aide")?;
    b.browser.open("about:essai");
    b.settle();
    b.shoot("essai")?;
    b.browser.handle(Event::Char('/'));
    for c in "tableau".chars() {
        b.browser.handle(Event::Char(c));
    }
    b.shoot("recherche")?;
    b.browser.handle(Event::Key(Key::Escape));
    for address in &addresses {
        b.browser.run(Command::NewTab);
        b.browser.open(address);
        b.settle();
        b.shoot("page")?;
        b.browser.handle(Event::Wheel(6));
        b.browser.handle(Event::Pointer { x: 200, y: TOP + 200, buttons: 0 });
        b.shoot("defile")?;
    }
    let (x, y) = (size.0 - 20, TOP - 20);
    b.click(x, y);
    b.browser.handle(Event::Pointer { x: size.0 - 120, y: TOP + 50, buttons: 0 });
    b.shoot("menu")?;
    b.browser.handle(Event::Key(Key::Escape));
    b.browser.open("https://exemple.invalid/");
    b.settle();
    b.shoot("erreur")?;
    Ok(())
}
