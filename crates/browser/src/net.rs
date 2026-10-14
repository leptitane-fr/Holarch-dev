//! What the browser asks of the network: one address, its answer. The
//! holon gets it from the system (`hln.fetch`, once holons have the
//! network right), the bench from `curl`.

use alloc::string::String;
use alloc::vec::Vec;
use hln_web::url::Url;

/// An answer.
pub struct Response {
    /// Where the answer came from, after redirections.
    pub url: Url,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// The holon may not reach the network (no `network` right yet).
    NoRight,
    /// An address this browser does not fetch (`ftp:`, `mailto:`...).
    Unsupported,
    /// It failed: why, in French, for the person.
    Failed(String),
}

pub trait Net {
    /// Fetches `url`: with `post`, sends it as a form's body.
    fn fetch(&mut self, url: &Url, post: Option<&str>) -> Result<Response, NetError>;
}

/// A network that has none: every request is refused with [`NetError::NoRight`].
pub struct Offline;

impl Net for Offline {
    fn fetch(&mut self, _: &Url, _: Option<&str>) -> Result<Response, NetError> {
        Err(NetError::NoRight)
    }
}
