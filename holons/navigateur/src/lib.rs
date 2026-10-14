//! The web browser, as a holon: it reads the words of its window
//! (`hln.input`: keys with their modifiers, the pointer, the wheel, the
//! room for its image), lets the browser handle them, takes one step of a
//! load at a time when nothing waits, and shows the browser's image
//! (`hln.present`) when it changed, at the size of the window's room.
//!
//! With the `network` feature (the default), pages come through
//! `hln.fetch`, which the system gives a holon holding the `network`
//! right (`docs/delivering-a-holon.md` of the system):
//!
//! `fetch(url: i32, url_len: i32, body: i32, capacity: i32) -> i64`
//!
//! fetches the `https` address (UTF-8) at `url`, follows redirections,
//! writes at most `capacity` bytes of the body at `body`, and answers the
//! word of `hln_user::fetch::Answer`: the body's length in bits 0 to 31,
//! the HTTP status in 32 to 47, why it failed in 48 to 55 (0: it did not).
//! The call blocks until the answer.

#![no_std]

extern crate alloc;

mod heap;

use alloc::string::String;
use hln_browser::{Browser, Event, Key, Mods};
use hln_web::paint::Canvas;

#[link(wasm_import_module = "hln")]
unsafe extern "C" {
    fn log(text: *const u8, len: usize);
    fn present(pixels: *const u32, width: usize, height: usize);
    fn input(wait: i32) -> i64;
}

/// The window's size when it opens, until `RESIZED` says otherwise
/// (`hln_user::window`), and the largest the system gives.
const WIDTH: i32 = 480;
const HEIGHT: i32 = 320;
const ROOM_MAX: i32 = 4095;

/// The words of `hln.input` (`hln_user::key`, `hln_user::window`).
const SPECIAL: i64 = 1 << 32;
const REDRAW: i64 = SPECIAL | 0x100;
const MOUSE: i64 = 1 << 41;
const RESIZED: i64 = 1 << 44;
const WHEEL: i64 = 1 << 45;
const SHIFT: i64 = 1 << 36;
const CTRL: i64 = 1 << 37;
const ALT: i64 = 1 << 38;

// Le P'tit Âne surfe à quatre pattes.

fn say(text: &str) {
    let text = &text.as_bytes()[..text.len().min(200)];
    // SAFETY: the host reads `text.len()` bytes at `text`, in this
    // holon's memory.
    unsafe { log(text.as_ptr(), text.len()) }
}

/// What a word of `hln.input` means for the browser, if anything. The
/// words that are not keys first, as the system asks.
fn event(word: i64) -> Option<Event> {
    if word & MOUSE != 0 {
        let x = (word & 0xffff) as i32;
        let y = (word >> 16 & 0xffff) as i32;
        let left = word >> 32 & 1 != 0;
        return Some(Event::Pointer { x, y, left });
    }
    if word & WHEEL != 0 {
        // The system counts away from the person (up the page), the
        // browser down the page.
        let notches = (word >> 48) as u8 as i8;
        return Some(Event::Wheel(-i32::from(notches)));
    }
    if word & RESIZED != 0 {
        let w = (word & 0xffff) as i32;
        let h = (word >> 16 & 0x7fff) as i32;
        return Some(Event::Resize(w.clamp(1, ROOM_MAX), h.clamp(1, ROOM_MAX)));
    }
    let mods = Mods { ctrl: word & CTRL != 0, shift: word & SHIFT != 0, alt: word & ALT != 0 };
    if word & SPECIAL != 0 {
        let key = match word & 0xfff {
            1 => Key::Enter,
            2 => Key::Backspace,
            3 => Key::Tab,
            4 => Key::Escape,
            5 => Key::Up,
            6 => Key::Down,
            7 => Key::Left,
            8 => Key::Right,
            9 => Key::Home,
            10 => Key::End,
            11 => Key::Delete,
            12 => Key::PageUp,
            13 => Key::PageDown,
            _ => return None,
        };
        return Some(Event::Key(key, mods));
    }
    // A character: Ctrl+L comes as `'l'` with Ctrl, never typed.
    let c = char::from_u32((word & 0xffff_ffff) as u32).filter(|c| !c.is_control())?;
    Some(Event::Key(Key::Char(c), mods))
}

#[cfg(feature = "network")]
mod network {
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use hln_browser::net::{Net, NetError, Response};
    use hln_web::url::Url;

    #[link(wasm_import_module = "hln")]
    unsafe extern "C" {
        fn fetch(url: *const u8, url_len: usize, body: *mut u8, capacity: usize) -> i64;
    }

    /// The most a page or a sheet may weigh.
    const CAPACITY: usize = 8 << 20;

    /// Why a fetch failed, in French (`hln_user::fetch`).
    fn why(code: u8) -> &'static str {
        match code {
            1 => "Le système n'ouvre que les adresses https.",
            2 => "Ce nom de site est introuvable.",
            3 => "Le site ne répond pas.",
            4 => "Le réseau est absent ou a échoué.",
            5 => "Le site a mis trop de temps à répondre.",
            6 => "La connexion sécurisée a échoué (certificat refusé ?).",
            7 => "La réponse du site est illisible.",
            8 => "Le site renvoie d'adresse en adresse sans fin.",
            9 => "La page est trop lourde pour ce navigateur.",
            _ => "Une erreur inconnue est survenue.",
        }
    }

    pub struct System;

    impl Net for System {
        fn fetch(&mut self, url: &Url, post: Option<&str>) -> Result<Response, NetError> {
            if post.is_some() {
                return Err(NetError::Failed(String::from("Les formulaires envoyés (POST) attendent le système.")));
            }
            // The system opens `https` only: a `http` page is asked for
            // over `https`, as most sites answer both.
            let url = match url.scheme.as_str() {
                "https" => url.clone(),
                "http" => {
                    let mut secure = url.clone();
                    secure.scheme = String::from("https");
                    if secure.port == Some(80) {
                        secure.port = None;
                    }
                    secure
                }
                _ => return Err(NetError::Unsupported),
            };
            let address = url.to_string();
            let mut body: Vec<u8> = Vec::with_capacity(CAPACITY);
            // SAFETY: the host reads the address and writes at most
            // `CAPACITY` bytes into `body`'s spare room, in this holon's
            // memory.
            let word = unsafe { fetch(address.as_ptr(), address.len(), body.as_mut_ptr(), CAPACITY) } as u64;
            let (length, status, code) = ((word & 0xffff_ffff) as usize, (word >> 32) as u16, (word >> 48) as u8);
            if code != 0 {
                return Err(NetError::Failed(String::from(why(code))));
            }
            // SAFETY: the host wrote `length` bytes (at most `CAPACITY`).
            unsafe { body.set_len(length.min(CAPACITY)) };
            body.shrink_to_fit();
            Ok(Response { url, status, content_type: None, body })
        }
    }
}

#[cfg(feature = "network")]
fn net() -> network::System {
    network::System
}

#[cfg(not(feature = "network"))]
fn net() -> hln_browser::net::Offline {
    hln_browser::net::Offline
}

#[unsafe(no_mangle)]
pub extern "C" fn hln_main() -> i32 {
    let mut browser = Browser::new(WIDTH, HEIGHT);
    let mut canvas = Canvas::new(WIDTH, HEIGHT);
    let mut net = net();
    let mut dirty = true;
    say("navigateur: prêt");
    loop {
        // Everything that came, then draw once.
        let mut wait = !dirty && !browser.busy();
        loop {
            // SAFETY: a call into the host, no memory shared.
            let word = unsafe { input(i32::from(wait)) };
            if word < 0 {
                return 0;
            }
            if word == 0 {
                break;
            }
            wait = false;
            if word == REDRAW {
                dirty = true;
            } else if let Some(e) = event(word) {
                dirty |= browser.handle(e);
                if let Event::Resize(..) = e {
                    // The browser keeps a least size; the image is its own.
                    canvas = Canvas::new(browser.width, browser.height);
                }
            }
        }
        if dirty {
            dirty = false;
            browser.draw(&mut canvas);
            // SAFETY: the host reads `width` × `height` pixels at
            // `canvas.pixels`, in this holon's memory.
            unsafe { present(canvas.pixels.as_ptr(), canvas.width as usize, canvas.height as usize) };
        } else if browser.busy() {
            dirty = browser.step(&mut net);
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;
    let mut line = String::from("navigateur: panique : ");
    let _ = write!(line, "{}", info.message());
    say(&line);
    core::arch::wasm32::unreachable()
}
