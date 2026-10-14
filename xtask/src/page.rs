//! `cargo xtask page <file.html | url> [out.png] [--size WxH] [--full]`:
//! renders a page with the engine alone (no browser around it), into a
//! PNG, to look at the layout. Addresses are fetched with `curl`.

use crate::{Result, png, root};
use hln_web::page::Page;
use hln_web::paint::{self, Canvas, Overlay};
use hln_web::url::Url;
use std::fs;
use std::process::Command;
use std::time::Instant;

/// Fetches an address with curl: the final address, its content type and
/// its body.
pub fn fetch(url: &str) -> Result<(String, Option<String>, Vec<u8>)> {
    let out = Command::new("curl")
        .args(["-sSL", "--max-time", "20", "-A", "Mozilla/5.0 (compatible; hln-web)", "-w", "\n%{url_effective}\n%{content_type}\n%{http_code}", url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let mut body = out.stdout;
    // The last three lines are ours.
    let mut tail = Vec::new();
    for _ in 0..3 {
        let end = body.len();
        let start = body[..end].iter().rposition(|&b| b == b'\n').unwrap_or(0);
        tail.push(String::from_utf8_lossy(&body[start + 1..end]).into_owned());
        body.truncate(start);
    }
    let (code, ctype, url) = (tail[0].clone(), tail[1].clone(), tail[2].clone());
    if code.starts_with('4') || code.starts_with('5') {
        eprintln!("xtask: HTTP {code} for {url}");
    }
    Ok((url, (!ctype.is_empty()).then_some(ctype), body))
}

pub fn run(args: &[String]) -> Result {
    let mut size = (1024, 768);
    let mut full = false;
    let mut dump = false;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let s = args.get(i + 1).ok_or("--size WxH")?;
                let (w, h) = s.split_once('x').ok_or("--size WxH")?;
                size = (w.parse().map_err(|_| "--size")?, h.parse().map_err(|_| "--size")?);
                i += 1;
            }
            "--full" => full = true,
            "--dump" => dump = true,
            a => rest.push(a.to_string()),
        }
        i += 1;
    }
    let source = rest.first().ok_or("usage: cargo xtask page <file.html | url> [out.png] [--size WxH] [--full]")?;
    let out = rest.get(1).cloned().unwrap_or_else(|| root().join("build/page.png").display().to_string());
    let t0 = Instant::now();
    let mut page = if source.starts_with("http://") || source.starts_with("https://") {
        let (url, ctype, body) = fetch(source)?;
        let url = Url::parse(&url).ok_or("bad address")?;
        let mut page = Page::new(url, &body, ctype.as_deref());
        for (node, sheet) in page.stylesheet_links() {
            match fetch(&sheet.to_string()) {
                Ok((_, _, css)) => page.add_sheet(node, String::from_utf8_lossy(&css).into_owned(), sheet),
                Err(e) => eprintln!("xtask: {sheet}: {e}"),
            }
        }
        page
    } else {
        let text = fs::read(source).map_err(|e| format!("{source}: {e}"))?;
        let url = Url::parse(&format!("file://{}", fs::canonicalize(source).map_err(|e| e.to_string())?.display()))
            .ok_or("bad path")?;
        let mut page = Page::new(url, &text, Some("text/html"));
        for (node, sheet) in page.stylesheet_links() {
            if sheet.scheme == "file" {
                match fs::read(hln_web::url::decode(&sheet.path)) {
                    Ok(css) => page.add_sheet(node, String::from_utf8_lossy(&css).into_owned(), sheet),
                    Err(e) => eprintln!("xtask: {sheet}: {e}"),
                }
            }
        }
        page
    };
    let t1 = Instant::now();
    let height = page.lay_out(size.0, size.1).height;
    let t2 = Instant::now();
    if dump {
        let layout = page.layout.as_ref().ok_or("no layout")?;
        print_frag(&page, &layout.root, 0);
    }
    let h = if full { height.min(20_000) } else { size.1 };
    let mut canvas = Canvas::new(size.0, h);
    let layout = page.layout.as_ref().ok_or("no layout")?;
    paint::paint(layout, &mut canvas, (0, 0, size.0, h), 0, &Overlay::default());
    let t3 = Instant::now();
    if let Some(parent) = std::path::Path::new(&out).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&out, png::encode(size.0 as u32, h as u32, &canvas.pixels)).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "xtask: « {} », {} nodes, page {}px high; load {:?}, layout {:?}, paint {:?} -> {out}",
        page.title(),
        page.doc.nodes.len(),
        height,
        t1 - t0,
        t2 - t1,
        t3 - t2
    );
    Ok(())
}

/// The fragment tree, one line each: what it is, where, how large.
fn print_frag(page: &Page, f: &hln_web::layout::Frag, depth: usize) {
    use hln_web::layout::FragKind;
    let what = match (&f.kind, f.node.and_then(|n| page.doc.element(n))) {
        (FragKind::Text { text, .. }, _) => format!("{:?}", text.chars().take(40).collect::<String>()),
        (_, Some(e)) => {
            let mut s = e.name.clone();
            if let Some(id) = e.id() {
                s += &format!("#{id}");
            }
            for c in e.classes().take(3) {
                s += &format!(".{c}");
            }
            s
        }
        (FragKind::Marker { text, .. }, _) => format!("marker {text}"),
        _ => String::from("(anonymous)"),
    };
    println!("{:indent$}{what} {},{} {}x{}{}", "", f.x, f.y, f.w, f.h, if f.positioned { " positioned" } else { "" }, indent = depth * 2);
    if depth < 14 {
        for c in &f.children {
            print_frag(page, c, depth + 1);
        }
    }
}
