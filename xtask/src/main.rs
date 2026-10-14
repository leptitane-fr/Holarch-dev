//! The development tasks (see `cargo xtask help`).

mod bench;
mod faces;
mod page;
mod png;

use std::path::PathBuf;
use std::process::ExitCode;

pub type Result<T = ()> = std::result::Result<T, String>;

/// The workspace's root.
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask is in the workspace").to_path_buf()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("bench") => bench::run(&args[1..]),
        Some("faces") => faces::generate(&args[1..]),
        Some("page") => page::run(&args[1..]),
        _ => Err("usage: cargo xtask <bench … | faces … | page …>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
