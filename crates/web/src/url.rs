//! Web addresses: reading them, resolving a link against its page, and
//! turning what the person types in the address bar into an address.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

/// An absolute address. `http`, `https` and `file` addresses have a host
/// part and a path that starts with `/`; others (`about:`, `data:`,
/// `mailto:`, `javascript:`) keep everything after the colon as their
/// path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

fn special(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "file" | "ftp" | "ws" | "wss")
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

/// Percent-encodes what may not stand in an address as it is (spaces,
/// controls, and everything beyond ASCII, as UTF-8).
fn encode(text: &str, keep: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b > 0x20 && b < 0x7f && !b"\"<>`{}".contains(&b) || keep.as_bytes().contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Reads `%XX` escapes back into bytes, then text (invalid UTF-8 replaced).
pub fn decode(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// Encodes a form value (`application/x-www-form-urlencoded`).
pub fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b' ' => out.push('+'),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Removes `.` and `..` segments from a path starting with `/`.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let n = segments.len();
    for (i, s) in segments.iter().enumerate() {
        let last = i + 1 == n;
        match *s {
            "." | "%2e" | "%2E" => {
                if last {
                    out.push("");
                }
            }
            ".." | ".%2e" | "%2e." | "%2e%2e" | ".%2E" | "%2E." | "%2E%2E" => {
                out.pop();
                if last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let mut result = String::with_capacity(path.len());
    for s in &out {
        result.push('/');
        result.push_str(s);
    }
    if result.is_empty() {
        result.push('/');
    }
    result
}

impl Url {
    /// Reads an absolute address; `None` if it is not one.
    pub fn parse(input: &str) -> Option<Url> {
        let input = input.trim_matches(|c: char| c <= ' ');
        let input: String = input.chars().filter(|&c| c != '\t' && c != '\n' && c != '\r').collect();
        let colon = input.find(':')?;
        let scheme = &input[..colon];
        let mut chars = scheme.chars();
        if !chars.next()?.is_ascii_alphabetic()
            || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return None;
        }
        let scheme = scheme.to_ascii_lowercase();
        let rest = &input[colon + 1..];
        let (rest, fragment) = match rest.find('#') {
            Some(i) => (&rest[..i], Some(rest[i + 1..].to_string())),
            None => (rest, None),
        };
        if !special(&scheme) {
            let (path, query) = match rest.find('?') {
                Some(i) if scheme != "data" && scheme != "javascript" => (&rest[..i], Some(rest[i + 1..].to_string())),
                _ => (rest, None),
            };
            return Some(Url { scheme, host: String::new(), port: None, path: path.to_string(), query, fragment });
        }
        // `file:///chemin`: no host.
        let rest = if scheme == "file" && rest.starts_with("///") { &rest[2..] } else { rest };
        let rest = if scheme == "file" && rest.starts_with('/') && !rest.starts_with("//") {
            rest
        } else {
            rest.trim_start_matches(['/', '\\'])
        };
        let end = if scheme == "file" && rest.starts_with('/') {
            0
        } else {
            rest.find(['/', '\\', '?']).unwrap_or(rest.len())
        };
        let authority = &rest[..end];
        let rest = &rest[end..];
        // user:password@ is dropped: the browser never sends it.
        let host_port = authority.rsplit('@').next().unwrap_or("");
        let (host, port) = match host_port.rfind(':') {
            Some(i) if !host_port[i..].contains(']') => {
                let digits = &host_port[i + 1..];
                let port = if digits.is_empty() { None } else { Some(digits.parse::<u16>().ok()?) };
                (&host_port[..i], port)
            }
            _ => (host_port, None),
        };
        if host.is_empty() && scheme != "file" {
            return None;
        }
        if host.chars().any(|c| matches!(c, ' ' | '<' | '>' | '^' | '|' | '%')) {
            return None;
        }
        let host = host.to_ascii_lowercase();
        let port = port.filter(|&p| Some(p) != default_port(&scheme));
        let (path, query) = match rest.find('?') {
            Some(i) => (&rest[..i], Some(encode(&rest[i + 1..], ""))),
            None => (rest, None),
        };
        let path = path.replace('\\', "/");
        let path = if path.is_empty() { String::from("/") } else { normalize(&encode(&path, "")) };
        Some(Url { scheme, host, port, path, query, fragment: fragment.map(|f| encode(&f, "")) })
    }

    /// Whether it has a host and a path (`http`, `https`, `file`...).
    pub fn is_hierarchical(&self) -> bool {
        special(&self.scheme)
    }

    /// `reference` resolved against this address (a link on this page).
    pub fn join(&self, reference: &str) -> Option<Url> {
        let reference = reference.trim_matches(|c: char| c <= ' ');
        let reference: String = reference.chars().filter(|&c| c != '\t' && c != '\n' && c != '\r').collect();
        let reference = reference.as_str();
        if let Some(colon) = reference.find(':') {
            let scheme = &reference[..colon];
            let is_scheme = !scheme.is_empty()
                && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
            if is_scheme {
                // "http:foo" relative to an http page is a relative path.
                let same = scheme.eq_ignore_ascii_case(&self.scheme) && special(&self.scheme);
                let rest = &reference[colon + 1..];
                if !(same && !rest.starts_with('/')) {
                    return Url::parse(reference);
                }
                return self.join(rest);
            }
        }
        if !self.is_hierarchical() {
            return if let Some(fragment) = reference.strip_prefix('#') {
                let mut url = self.clone();
                url.fragment = Some(fragment.to_string());
                Some(url)
            } else {
                None
            };
        }
        if reference.starts_with("//") || reference.starts_with("\\\\") {
            return Url::parse(&format!("{}:{}", self.scheme, reference));
        }
        let mut url = self.clone();
        let (rest, fragment) = match reference.find('#') {
            Some(i) => (&reference[..i], Some(encode(&reference[i + 1..], ""))),
            None => (reference, None),
        };
        url.fragment = fragment;
        if rest.is_empty() {
            return Some(url);
        }
        let (path, query) = match rest.find('?') {
            Some(i) => (&rest[..i], Some(encode(&rest[i + 1..], ""))),
            None => (rest, None),
        };
        if path.is_empty() {
            url.query = query;
            return Some(url);
        }
        url.query = query;
        let path = path.replace('\\', "/");
        let path = encode(&path, "");
        url.path = if path.starts_with('/') {
            normalize(&path)
        } else {
            let dir = &self.path[..self.path.rfind('/').map_or(0, |i| i + 1)];
            normalize(&format!("{}{}", if dir.is_empty() { "/" } else { dir }, path))
        };
        Some(url)
    }

    /// The address without its fragment.
    pub fn without_fragment(&self) -> Url {
        Url { fragment: None, ..self.clone() }
    }

    /// `host` or `host:port`.
    pub fn authority(&self) -> String {
        match self.port {
            Some(p) => format!("{}:{p}", self.host),
            None => self.host.clone(),
        }
    }

    /// Whether both are the same document (they may differ by fragment).
    pub fn same_document(&self, other: &Url) -> bool {
        self.without_fragment() == other.without_fragment()
    }

    /// The address as shown to the person: no `https://`, no trailing `/`,
    /// escapes read back. Exact form stays in [`Url`]'s `Display`.
    pub fn pretty(&self) -> String {
        let full = self.to_string();
        let mut s = full.as_str();
        for prefix in ["https://", "http://"] {
            if let Some(rest) = s.strip_prefix(prefix) {
                s = rest;
            }
        }
        let s = s.strip_prefix("www.").unwrap_or(s);
        let s = s.strip_suffix('/').unwrap_or(s);
        decode(s)
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.scheme)?;
        if self.is_hierarchical() {
            write!(f, "//{}", self.authority())?;
        }
        f.write_str(&self.path)?;
        if let Some(q) = &self.query {
            write!(f, "?{q}")?;
        }
        if let Some(fr) = &self.fragment {
            write!(f, "#{fr}")?;
        }
        Ok(())
    }
}

/// What the address bar makes of what was typed: an address as it is, a
/// host name (`exemple.fr`, `localhost:8080`) taken as `https`, or else a
/// search with `search` (its address, the words appended).
pub fn from_typed(typed: &str, search: &str) -> Option<Url> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    if !typed.contains(' ') {
        let lower = typed.to_ascii_lowercase();
        let has_scheme =
            ["http:", "https:", "file:", "about:", "data:", "view-source:"].iter().any(|s| lower.starts_with(s));
        if has_scheme && let Some(url) = Url::parse(typed) {
            return Some(url);
        }
        let host = typed.split(['/', '?', '#']).next().unwrap_or("");
        let host_only = host.split(':').next().unwrap_or("");
        let looks_like_host = host_only == "localhost"
            || host_only.contains('.')
                && !host_only.starts_with('.')
                && !host_only.ends_with('.')
                && host_only.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.');
        if looks_like_host && let Some(url) = Url::parse(&format!("https://{typed}")) {
            return Some(url);
        }
    }
    Url::parse(&format!("{search}{}", form_encode(typed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn parses_and_prints() {
        let url = u("HTTPS://Exemple.FR:443/a/./b/../c?x=1 2#haut");
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "exemple.fr");
        assert_eq!(url.port, None);
        assert_eq!(url.path, "/a/c");
        assert_eq!(url.to_string(), "https://exemple.fr/a/c?x=1%202#haut");
        assert_eq!(u("http://h:8080").to_string(), "http://h:8080/");
        assert_eq!(u("about:accueil").path, "accueil");
        assert_eq!(u("file:///tmp/a.html").to_string(), "file:///tmp/a.html");
        assert_eq!(u("file:///tmp/a.html").join("b.css").unwrap().path, "/tmp/b.css");
        assert_eq!(u("http://h/é").path, "/%C3%A9");
        assert!(Url::parse("pas une adresse").is_none());
        assert!(Url::parse("http://").is_none());
        assert!(Url::parse("http://h:99999/").is_none());
    }

    #[test]
    fn joins_like_browsers() {
        let base = u("http://a/b/c/d;p?q");
        for (r, want) in [
            ("g", "http://a/b/c/g"),
            ("./g", "http://a/b/c/g"),
            ("g/", "http://a/b/c/g/"),
            ("/g", "http://a/g"),
            ("//g", "http://g/"),
            ("?y", "http://a/b/c/d;p?y"),
            ("g?y", "http://a/b/c/g?y"),
            ("#s", "http://a/b/c/d;p?q#s"),
            ("..", "http://a/b/"),
            ("../g", "http://a/b/g"),
            ("../../../g", "http://a/g"),
            ("", "http://a/b/c/d;p?q"),
            ("https://autre/x", "https://autre/x"),
            ("http:g", "http://a/b/c/g"),
        ] {
            assert_eq!(base.join(r).unwrap().to_string(), want, "{r}");
        }
        assert_eq!(base.join("mailto:x@y").unwrap().scheme, "mailto");
    }

    #[test]
    fn typed_addresses() {
        let search = "https://html.duckduckgo.com/html/?q=";
        assert_eq!(from_typed("exemple.fr", search).unwrap().to_string(), "https://exemple.fr/");
        assert_eq!(from_typed("localhost:8080/x", search).unwrap().to_string(), "https://localhost:8080/x");
        assert_eq!(from_typed("about:aide", search).unwrap().path, "aide");
        assert_eq!(
            from_typed("le p'tit âne", search).unwrap().to_string(),
            "https://html.duckduckgo.com/html/?q=le+p%27tit+%C3%A2ne"
        );
        assert_eq!(from_typed("rust", search).unwrap().query.as_deref(), Some("q=rust"));
        assert_eq!(u("https://www.exemple.fr/").pretty(), "exemple.fr");
        assert_eq!(decode("%C3%A9t%C3%A9"), "été");
    }
}
