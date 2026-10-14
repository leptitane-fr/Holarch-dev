//! `cargo xtask holon`: builds the browser's holon for the system's
//! interpreter, without and with the `network` feature, and checks what
//! the system checks: the imports it allows (`hln.log`, `hln.present`,
//! `hln.input`, and `hln.fetch` with the feature), and no floating-point
//! instruction (they trap in the interpreter). The modules land in
//! `build/holon/`.

use crate::{Result, root};
use std::fs;
use std::process::Command;

const TODAY: &[&str] = &["log", "present", "input"];

pub fn run(_args: &[String]) -> Result {
    let out = root().join("build/holon");
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for (features, name, allowed) in [(None, "navigateur", TODAY), (Some("network"), "navigateur-reseau", &[][..])] {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.current_dir(root()).args([
            "build",
            "--release",
            "-p",
            "hln-navigateur",
            "--target",
            "wasm32-unknown-unknown",
        ]);
        if let Some(f) = features {
            cmd.args(["--features", f]);
        }
        let status = cmd.status().map_err(|e| format!("cargo: {e}"))?;
        if !status.success() {
            return Err("the holon does not build".into());
        }
        let built = root().join("target/wasm32-unknown-unknown/release/hln_navigateur.wasm");
        let bytes = fs::read(&built).map_err(|e| format!("{}: {e}", built.display()))?;
        let module = scan(&bytes)?;
        let mut allowed: Vec<&str> = allowed.to_vec();
        if allowed.is_empty() {
            allowed = TODAY.to_vec();
            allowed.push("fetch");
        }
        for (m, f) in &module.imports {
            if m != "hln" || !allowed.contains(&f.as_str()) {
                return Err(format!("{name}: import {m}.{f} is not one the system gives"));
            }
        }
        if module.floats > 0 {
            return Err(format!(
                "{name}: {} floating-point instructions (the interpreter traps on them)",
                module.floats
            ));
        }
        let path = out.join(format!("{name}.wasm"));
        fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        let imports: Vec<String> = module.imports.iter().map(|(m, f)| format!("{m}.{f}")).collect();
        println!(
            "xtask: {} : {} Kio, {} fonctions, imports {}, aucune instruction flottante",
            path.display(),
            bytes.len() / 1024,
            module.functions,
            imports.join(" ")
        );
    }
    Ok(())
}

pub struct Module {
    pub imports: Vec<(String, String)>,
    pub functions: usize,
    pub floats: usize,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8> {
        let v = *self.b.get(self.at).ok_or("the module ends too soon")?;
        self.at += 1;
        Ok(v)
    }

    fn uleb(&mut self) -> Result<u64> {
        let (mut v, mut shift) = (0u64, 0);
        loop {
            let b = self.byte()?;
            v |= u64::from(b & 0x7f) << shift.min(63);
            shift += 7;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
    }

    fn u32(&mut self) -> Result<usize> {
        Ok(self.uleb()? as usize)
    }

    fn skip(&mut self, n: usize) -> Result {
        if self.at + n > self.b.len() {
            return Err("the module ends too soon".into());
        }
        self.at += n;
        Ok(())
    }

    fn name(&mut self) -> Result<String> {
        let n = self.u32()?;
        let start = self.at;
        self.skip(n)?;
        Ok(String::from_utf8_lossy(&self.b[start..start + n]).into_owned())
    }

    fn limits(&mut self) -> Result {
        let flags = self.byte()?;
        self.uleb()?;
        if flags & 1 != 0 {
            self.uleb()?;
        }
        Ok(())
    }
}

fn is_float(op: u8) -> bool {
    matches!(op, 0x2a | 0x2b | 0x38 | 0x39 | 0x43 | 0x44 | 0x5b..=0x66 | 0x8b..=0xa6 | 0xa8..=0xab | 0xae..=0xbf)
}

/// Reads a module's imports and counts its floating-point instructions.
pub fn scan(bytes: &[u8]) -> Result<Module> {
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return Err("not a WebAssembly module".into());
    }
    let mut r = Reader { b: bytes, at: 8 };
    let mut module = Module { imports: Vec::new(), functions: 0, floats: 0 };
    while r.at < bytes.len() {
        let id = r.byte()?;
        let size = r.u32()?;
        let end = r.at + size;
        match id {
            2 => {
                for _ in 0..r.u32()? {
                    let m = r.name()?;
                    let f = r.name()?;
                    match r.byte()? {
                        0 => {
                            r.u32()?;
                        }
                        1 => {
                            r.byte()?;
                            r.limits()?;
                        }
                        2 => r.limits()?,
                        _ => {
                            r.skip(2)?;
                        }
                    }
                    module.imports.push((m, f));
                }
            }
            10 => {
                let n = r.u32()?;
                module.functions = n;
                for _ in 0..n {
                    let len = r.u32()?;
                    let body_end = r.at + len;
                    for _ in 0..r.u32()? {
                        r.u32()?;
                        if matches!(r.byte()?, 0x7c | 0x7d) {
                            module.floats += 1;
                        }
                    }
                    while r.at < body_end {
                        instruction(&mut r, &mut module.floats)?;
                    }
                }
            }
            _ => {}
        }
        r.at = end;
    }
    Ok(module)
}

fn instruction(r: &mut Reader, floats: &mut usize) -> Result {
    let op = r.byte()?;
    if is_float(op) {
        *floats += 1;
    }
    match op {
        0x02..=0x04 => {
            let t = r.b.get(r.at).copied().ok_or("end")?;
            if t == 0x40 || (0x6f..=0x7f).contains(&t) {
                r.at += 1;
            } else {
                r.uleb()?;
            }
        }
        0x0c | 0x0d | 0x10 | 0x20..=0x26 | 0xd2 => {
            r.u32()?;
        }
        0x0e => {
            for _ in 0..=r.u32()? {
                r.u32()?;
            }
        }
        0x11 => {
            r.u32()?;
            r.u32()?;
        }
        0x1c => {
            for _ in 0..r.u32()? {
                r.byte()?;
            }
        }
        0x28..=0x3e => {
            r.u32()?;
            r.u32()?;
        }
        0x3f | 0x40 | 0xd0 => {
            r.byte()?;
        }
        0x41 | 0x42 => {
            r.uleb()?;
        }
        0x43 => r.skip(4)?,
        0x44 => r.skip(8)?,
        0xfc => {
            let sub = r.u32()?;
            match sub {
                0..=7 => *floats += 1,
                8 => {
                    r.u32()?;
                    r.byte()?;
                }
                9 | 13 | 15..=17 => {
                    r.u32()?;
                }
                10 => r.skip(2)?,
                11 => r.skip(1)?,
                12 | 14 => {
                    r.u32()?;
                    r.u32()?;
                }
                _ => return Err(format!("unknown instruction 0xfc {sub}")),
            }
        }
        0xfd => return Err("SIMD instructions: the interpreter has none".into()),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module importing `hln.log`, one function with an i64 local, a
    /// block, a load and an `f32.const`.
    #[test]
    fn finds_imports_and_floats() {
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        m.extend([2, 11, 1, 3, b'h', b'l', b'n', 3, b'l', b'o', b'g', 0, 0]);
        let body =
            [1, 1, 0x7e, 0x02, 0x40, 0x41, 0x00, 0x28, 0x02, 0x00, 0x1a, 0x0b, 0x43, 0, 0, 0x80, 0x3f, 0x1a, 0x0b];
        m.extend([10, body.len() as u8 + 2, 1, body.len() as u8]);
        m.extend(body);
        let module = scan(&m).unwrap();
        assert_eq!(module.imports, [(String::from("hln"), String::from("log"))]);
        assert_eq!((module.functions, module.floats), (1, 1));
    }
}
