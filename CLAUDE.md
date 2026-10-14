# Conventions for this repository

This repository holds the web browser of the system (provisionally
named Holarch), developed apart and delivered to the system as a holon.
The system lives in `Holarch-system-alpha-2`: read its `FICHE-PROJET.md`
(what the system is, in French) and its `docs/delivering-a-holon.md` (the
contract a holon meets). This repository's handoff log is
`docs/JOURNAL.md`.

- Code, identifiers, comments, commit messages and commands are in
  **English**. Only what the user sees (the browser's pages and chrome,
  messages, `README.md`, `docs/JOURNAL.md`) is in **French**.
- The product name is **provisional** and lives only in the system's
  `hln-brand`. Never write it in code or in text shown to the user here;
  use generic terms (`browser`, `engine`, `holon`, `system`) and the `hln`
  prefix (crates `hln-*`).
- The author's signature, "Le P'tit Âne", is slipped discreetly into the
  code as a comment, a wink: it never has a function. Keep adding a
  discreet one here and there.
- The holon runs in the system's WebAssembly interpreter: **integers
  only** (floating point traps there; `cargo xtask holon` checks the
  module), `#![no_std]`, imports only from module `hln`.
- Rust version is pinned in `rust-toolchain.toml`; edition 2024; `unsafe`
  blocks carry a `// SAFETY:` comment.
- No external crate in the engine, the browser or the holon (everything
  is written from scratch); `xtask` runs on the developer's machine only
  and may use some.
- Before pushing: `cargo fmt --all --check`, the clippy commands of
  `.github/workflows/check.yml`, `cargo test`, `cargo xtask test`.
- Never commit private keys or signing material.
- Update `docs/JOURNAL.md` (French) at the end of each step.
