//! `cargo xtask bench [address] [out.png] [--size WxH] [--offline] [steps…]`:
//! runs the whole browser (tabs, bar, pages) as the holon would, on this
//! machine's network (`curl`) or none (`--offline`), plays the steps, and
//! writes what the window shows to a PNG.
//!
//! Steps, played in order once the address has loaded:
//! `--type TEXT` types in what has the keys, `--key NAME` presses a key
//! (`enter`, `tab`, `escape`, `down`, `up`, `pagedown`, `backspace`, or
//! `ctrl+l`, `ctrl+t`, `ctrl+f`, `ctrl+d`, `alt+left`…), `--click X,Y`
//! clicks there, `--hover X,Y` moves the pointer, `--wheel N` turns the
//! wheel, `--open ADDRESS` opens an address. The browser loads whatever
//! a step started before the next one.

use crate::page::fetch_with;
use crate::{Result, png, root};
use hln_browser::net::{Net, NetError, Response};
use hln_browser::{Browser, Event, Key, Mods};
use hln_web::paint::Canvas;
use hln_web::url::{self, Url};
use std::fs;
use std::time::Instant;

/// The network of this machine: `curl`, and files.
struct Curl;

impl Net for Curl {
    fn fetch(&mut self, url: &Url, post: Option<&str>) -> std::result::Result<Response, NetError> {
        match url.scheme.as_str() {
            "file" => {
                let path = url::decode(&url.path);
                let body = fs::read(&path)
                    .map_err(|e| NetError::Failed(format!("Le fichier {path} ne s'ouvre pas ({e}).")))?;
                let html = path.ends_with(".html") || path.ends_with(".htm");
                let content_type = Some(String::from(if html {
                    "text/html"
                } else if path.ends_with(".css") {
                    "text/css"
                } else {
                    ""
                }));
                Ok(Response { url: url.clone(), status: 200, content_type, body })
            }
            "http" | "https" => {
                let (final_url, content_type, status, body) = fetch_with(&url.to_string(), post)
                    .map_err(|e| NetError::Failed(format!("Le serveur n'a pas pu être joint ({e}).")))?;
                let url = Url::parse(&final_url).unwrap_or_else(|| url.clone());
                Ok(Response { url, status, content_type, body })
            }
            _ => Err(NetError::Unsupported),
        }
    }
}

fn settle(browser: &mut Browser, net: &mut dyn Net) {
    for _ in 0..200 {
        if !browser.step(net) {
            return;
        }
    }
}

fn point(text: &str) -> Result<(i32, i32)> {
    let (x, y) = text.split_once(',').ok_or("X,Y")?;
    Ok((x.trim().parse().map_err(|_| "X,Y")?, y.trim().parse().map_err(|_| "X,Y")?))
}

fn key(name: &str) -> Result<(Key, Mods)> {
    let mut mods = Mods::NONE;
    let mut rest = name;
    loop {
        if let Some(r) = rest.strip_prefix("ctrl+") {
            mods.ctrl = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("alt+") {
            mods.alt = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("shift+") {
            mods.shift = true;
            rest = r;
        } else {
            break;
        }
    }
    let key = match rest {
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "space" => Key::Char(' '),
        k if k.chars().count() == 1 => Key::Char(k.chars().next().unwrap_or(' ')),
        k => return Err(format!("unknown key {k}")),
    };
    Ok((key, mods))
}

pub fn run(args: &[String]) -> Result {
    let mut size = (1280, 800);
    let mut offline = false;
    let mut rest = Vec::new();
    let mut steps: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--size" => {
                let s = args.get(i + 1).ok_or("--size WxH")?;
                let (w, h) = s.split_once('x').ok_or("--size WxH")?;
                size = (w.parse().map_err(|_| "--size")?, h.parse().map_err(|_| "--size")?);
                i += 1;
            }
            "--offline" => offline = true,
            "--type" | "--key" | "--click" | "--hover" | "--wheel" | "--open" => {
                let v = args.get(i + 1).ok_or(format!("{a} needs a value"))?;
                steps.push((a.to_string(), v.clone()));
                i += 1;
            }
            _ if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ => rest.push(a.to_string()),
        }
        i += 1;
    }
    let out = rest.get(1).cloned().unwrap_or_else(|| root().join("build/bench.png").display().to_string());
    let mut net: Box<dyn Net> = if offline { Box::new(hln_browser::net::Offline) } else { Box::new(Curl) };
    let t0 = Instant::now();
    let mut browser = Browser::new(size.0, size.1);
    if let Some(address) = rest.first() {
        let address = if fs::metadata(address).is_ok() {
            format!("file://{}", fs::canonicalize(address).map_err(|e| e.to_string())?.display())
        } else {
            address.clone()
        };
        browser.open(&address);
        settle(&mut browser, net.as_mut());
    }
    for (step, value) in &steps {
        match step.as_str() {
            "--type" => {
                for c in value.chars() {
                    browser.handle(Event::Key(Key::Char(c), Mods::NONE));
                }
            }
            "--key" => {
                let (k, m) = key(value)?;
                browser.handle(Event::Key(k, m));
            }
            "--click" => {
                let (x, y) = point(value)?;
                browser.handle(Event::Pointer { x, y, left: true });
                browser.handle(Event::Pointer { x, y, left: false });
            }
            "--hover" => {
                let (x, y) = point(value)?;
                browser.handle(Event::Pointer { x, y, left: false });
            }
            "--wheel" => {
                browser.handle(Event::Wheel(value.parse().map_err(|_| "--wheel N")?));
            }
            _ => browser.open(value),
        }
        settle(&mut browser, net.as_mut());
    }
    let t1 = Instant::now();
    let mut canvas = Canvas::new(size.0, size.1);
    browser.draw(&mut canvas);
    let t2 = Instant::now();
    if let Some(parent) = std::path::Path::new(&out).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&out, png::encode(size.0 as u32, size.1 as u32, &canvas.pixels)).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "xtask: « {} », {} tab(s); load and steps {:?}, draw {:?} -> {out}",
        browser.title(),
        browser.tabs.len(),
        t1 - t0,
        t2 - t1
    );
    Ok(())
}
