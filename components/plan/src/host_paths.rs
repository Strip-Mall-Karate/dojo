// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Finds what in a built package names the build machine instead of Dojo's
//! root: an ELF program's dynamic linker, a RUNPATH or RPATH entry, a
//! script's interpreter. A package that does would run only where the build
//! machine's files are.

use std::{fmt::Write as _,
          fs,
          path::Path};

/// Interpreters a script may name outside Dojo's root, as the shell builder
/// allowed: every system has them.
const SHARED_INTERPRETERS: [&str; 2] = ["/bin/sh", "/usr/bin/env"];

/// How many findings the error lists before it counts the rest.
const SHOWN: usize = 20;

/// Fails when a file under `prefix` names a path outside `root` as its
/// dynamic linker, in its RUNPATH or RPATH, or as its script interpreter.
pub fn refuse(prefix: &Path, root: &Path) -> Result<(), String> {
    let root = root.to_string_lossy().into_owned();
    let mut findings = Vec::new();
    walk(prefix, prefix, &root, &mut findings)?;
    if findings.is_empty() {
        return Ok(());
    }
    findings.sort();
    let mut message = format!("{} name paths outside {root}:", plural(findings.len()));
    for finding in findings.iter().take(SHOWN) {
        message.push_str("\n  ");
        message.push_str(finding);
    }
    if findings.len() > SHOWN {
        let _ = write!(message, "\n  and {} more", findings.len() - SHOWN);
    }
    message.push_str("\nPoint them at packages: b:fix_interpreter for scripts, patchelf for programs");
    Err(message)
}

fn plural(n: usize) -> String { if n == 1 { "1 file does".into() } else { format!("{n} files") } }

fn walk(prefix: &Path, dir: &Path, root: &str, findings: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = entry.path();
        let kind = entry.file_type()
                        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if kind.is_dir() {
            walk(prefix, &path, root, findings)?;
        } else if kind.is_file() {
            let shown = path.strip_prefix(prefix).unwrap_or(&path).display().to_string();
            for (what, named) in host_paths(&path, root)? {
                findings.push(format!("{shown}: {what} {named}"));
            }
        }
    }
    Ok(())
}

/// What `path` names outside `root`, as (what, path) pairs.
fn host_paths(path: &Path, root: &str) -> Result<Vec<(&'static str, String)>, String> {
    let outside = |named: &str| !named.starts_with(root);
    let mut head = [0_u8; 4];
    let read = fs::File::open(path).and_then(|mut f| {
                                      use std::io::Read as _;
                                      f.read(&mut head)
                                  })
                                  .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if read == 4 && head == *b"\x7fELF" {
        let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let Some(elf) = Elf::parse(&bytes) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        if let Some(interpreter) = elf.interpreter.filter(|i| outside(i)) {
            found.push(("dynamic linker", interpreter));
        }
        for entry in elf.search_path.iter().flat_map(|p| p.split(':')) {
            if !entry.is_empty() && !entry.starts_with("$ORIGIN") && outside(entry) {
                found.push(("runpath", entry.to_string()));
            }
        }
        return Ok(found);
    }
    if read >= 2 && head.starts_with(b"#!") {
        let text = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let line = text[2..].split(|&b| b == b'\n').next().unwrap_or_default();
        let line = String::from_utf8_lossy(line);
        if let Some(interpreter) = line.split_whitespace().next()
           && outside(interpreter)
           && !SHARED_INTERPRETERS.contains(&interpreter)
        {
            return Ok(vec![("interpreter", interpreter.to_string())]);
        }
    }
    Ok(Vec::new())
}

/// The parts of a 64-bit little-endian ELF file that name other files.
struct Elf {
    interpreter: Option<String>,
    /// `DT_RUNPATH`, else `DT_RPATH`.
    search_path: Option<String>,
}

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const DT_STRTAB: u64 = 5;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;

impl Elf {
    /// None for anything but a well-formed 64-bit little-endian ELF file.
    fn parse(bytes: &[u8]) -> Option<Elf> {
        if bytes.get(4) != Some(&2) || bytes.get(5) != Some(&1) {
            return None;
        }
        let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
        let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        let u64_at = |at: usize| Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?));
        let string_at = |at: usize| {
            let rest = bytes.get(at..)?;
            let end = rest.iter().position(|&b| b == 0)?;
            Some(String::from_utf8_lossy(&rest[..end]).into_owned())
        };
        let phoff = usize::try_from(u64_at(0x20)?).ok()?;
        let phentsize = usize::from(u16_at(0x36)?);
        let phnum = usize::from(u16_at(0x38)?);
        let mut interpreter = None;
        let mut dynamic = None;
        let mut loads = Vec::new();
        for i in 0..phnum {
            let at = phoff.checked_add(i.checked_mul(phentsize)?)?;
            let offset = u64_at(at + 8)?;
            match u32_at(at)? {
                PT_INTERP => interpreter = string_at(usize::try_from(offset).ok()?),
                PT_DYNAMIC => dynamic = Some((offset, u64_at(at + 32)?)),
                PT_LOAD => loads.push((u64_at(at + 16)?, offset, u64_at(at + 32)?)),
                // Other segments name no files.
                _ => {}
            }
        }
        // The dynamic section's strings are found by address; a loaded
        // segment says where that address is in the file.
        let file_offset = |address: u64| {
            loads.iter()
                 .find(|&&(vaddr, _, size)| {
                     address >= vaddr && vaddr.checked_add(size).is_some_and(|end| address < end)
                 })
                 .and_then(|&(vaddr, offset, _)| (address - vaddr).checked_add(offset))
        };
        let mut search_path = None;
        if let Some((offset, size)) = dynamic {
            let (mut strtab, mut runpath, mut rpath) = (None, None, None);
            let mut at = usize::try_from(offset).ok()?;
            let end = at.checked_add(usize::try_from(size).ok()?)?;
            while at + 16 <= end {
                let (tag, value) = (u64_at(at)?, u64_at(at + 8)?);
                match tag {
                    0 => break,
                    DT_STRTAB => strtab = Some(value),
                    DT_RUNPATH => runpath = Some(value),
                    DT_RPATH => rpath = Some(value),
                    // Other entries name no search path.
                    _ => {}
                }
                at += 16;
            }
            if let Some(table) = strtab.and_then(file_offset) {
                search_path = runpath.or(rpath)
                                     .and_then(|name| {
                                         string_at(usize::try_from(table.checked_add(name)?).ok()?)
                                     });
            }
        }
        Some(Elf { interpreter, search_path })
    }
}
