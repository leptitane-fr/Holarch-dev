//! The faces' files (written by `cargo xtask faces`).

pub(crate) const SIZES: [i32; 12] = [11, 12, 13, 14, 15, 16, 18, 20, 24, 28, 32, 40];
pub(crate) const MONO_SIZES: [i32; 4] = [12, 13, 14, 16];

pub(crate) static SANS: [&[u8]; 12] = [
    include_bytes!("faces/sans-11.bin"),
    include_bytes!("faces/sans-12.bin"),
    include_bytes!("faces/sans-13.bin"),
    include_bytes!("faces/sans-14.bin"),
    include_bytes!("faces/sans-15.bin"),
    include_bytes!("faces/sans-16.bin"),
    include_bytes!("faces/sans-18.bin"),
    include_bytes!("faces/sans-20.bin"),
    include_bytes!("faces/sans-24.bin"),
    include_bytes!("faces/sans-28.bin"),
    include_bytes!("faces/sans-32.bin"),
    include_bytes!("faces/sans-40.bin"),
];
pub(crate) static SANS_BOLD: [&[u8]; 12] = [
    include_bytes!("faces/sans-bold-11.bin"),
    include_bytes!("faces/sans-bold-12.bin"),
    include_bytes!("faces/sans-bold-13.bin"),
    include_bytes!("faces/sans-bold-14.bin"),
    include_bytes!("faces/sans-bold-15.bin"),
    include_bytes!("faces/sans-bold-16.bin"),
    include_bytes!("faces/sans-bold-18.bin"),
    include_bytes!("faces/sans-bold-20.bin"),
    include_bytes!("faces/sans-bold-24.bin"),
    include_bytes!("faces/sans-bold-28.bin"),
    include_bytes!("faces/sans-bold-32.bin"),
    include_bytes!("faces/sans-bold-40.bin"),
];
pub(crate) static MONO: [&[u8]; 4] = [
    include_bytes!("faces/mono-12.bin"),
    include_bytes!("faces/mono-13.bin"),
    include_bytes!("faces/mono-14.bin"),
    include_bytes!("faces/mono-16.bin"),
];
pub(crate) static MONO_BOLD: [&[u8]; 4] = [
    include_bytes!("faces/mono-bold-12.bin"),
    include_bytes!("faces/mono-bold-13.bin"),
    include_bytes!("faces/mono-bold-14.bin"),
    include_bytes!("faces/mono-bold-16.bin"),
];
