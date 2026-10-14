//! The browser's engine: from a page's bytes to its image.
//!
//! [`html`] reads the page into a [`dom`] tree, [`css`] reads its style
//! sheets, [`style`] gives every element its computed style, [`layout`]
//! places the boxes and lines, and [`paint`] draws them. Everything is
//! written from scratch, `no_std`, with integers only.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod css;
pub mod dom;
mod entities;
pub mod html;
pub mod layout;
pub mod page;
pub mod paint;
pub mod style;
pub mod url;
