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

//! The builtin phases, which run when neither the plan nor its scaffolding
//! sets a phase or when either calls `b.default.<phase>`, and the steps the
//! builder takes between phases.

use std::{fmt::Write as _,
          fs,
          io::{self,
               Read},
          path::{Path,
                 PathBuf}};

use blake2::{Blake2b,
             digest::consts::U32};
use sha2::{Digest,
           Sha256};

use crate::build::State;

/// Runs the builtin `phase`.
pub fn run(state: &State, phase: &str) -> Result<(), String> {
    match phase {
        "download" => download(state),
        "verify" => verify(state),
        "unpack" => unpack(state),
        "build" => {
            let prefix = format!("--prefix={}", state.prefix_text()?);
            state.run(&["./configure".into(), prefix], None, &[], false)?;
            state.run(&["make".into()], None, &[], false).map(drop)
        }
        "install" => {
            state.run(&["make".into(), "install".into()], None, &[], false)
                 .map(drop)
        }
        "strip" if state.os == "linux" => strip(state),
        // `prepare` and `check` do nothing, nor do `build`, `install` and
        // `strip` where the arms above do not apply.
        _ => Ok(()),
    }
}

/// Downloads `source.url` into the cache, unless a file there already
/// matches `source.sha256`.
fn download(state: &State) -> Result<(), String> {
    let Some(source) = &state.settings.source else {
        return Ok(());
    };
    fetch(state, &source.url, &source.sha256, &source.filename).map(drop)
}

/// Downloads `url` into the cache as `filename`, unless a file there already
/// has `sha256`, and returns its path. `verify` checks the plan's source;
/// `b:download` checks its own.
pub fn fetch(state: &State, url: &str, sha256: &str, filename: &str) -> Result<PathBuf, String> {
    let file = state.cache.join(filename);
    if file.is_file() {
        state.log(&format!("Found {}, checking it", file.display()));
        if self::sha256(&file)? == sha256 {
            state.log("Using the cached file");
            return Ok(file);
        }
        fs::remove_file(&file).map_err(|e| format!("cannot remove {}: {e}", file.display()))?;
    }
    state.log(&format!("Downloading {url} to {}", file.display()));
    let partial = state.cache.join(format!("{filename}.part"));
    state.run(&["curl".into(),
                "--fail".into(),
                "--location".into(),
                "--silent".into(),
                "--show-error".into(),
                "--output".into(),
                partial.to_string_lossy().into_owned(),
                url.to_string()],
              None,
              &[],
              false)?;
    fs::rename(&partial, &file).map_err(|e| {
                                   format!("cannot move {} to {}: {e}",
                                           partial.display(),
                                           file.display())
                               })?;
    Ok(file)
}

/// Checks the downloaded file against `source.sha256`.
fn verify(state: &State) -> Result<(), String> {
    let Some(source) = &state.settings.source else {
        return Ok(());
    };
    let file = state.cache.join(&source.filename);
    let computed = sha256(&file)?;
    if computed != source.sha256 {
        return Err(format!("{} has SHA-256 {computed}, but `source.sha256` \
                            is {}",
                           file.display(),
                           source.sha256));
    }
    state.log(&format!("Checksum verified for {}", source.filename));
    Ok(())
}

/// Extracts the downloaded file in the cache.
fn unpack(state: &State) -> Result<(), String> {
    let Some(source) = &state.settings.source else {
        return Ok(());
    };
    let file = state.cache
                    .join(&source.filename)
                    .to_string_lossy()
                    .into_owned();
    let name = source.filename.to_ascii_lowercase();
    let is_zip = Path::new(&name).extension()
                                 .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
    let tar = [".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.xz", ".txz"];
    let argv: Vec<String> = if tar.iter().any(|ext| name.ends_with(ext)) {
        vec!["tar".into(), "xf".into(), file, "--no-same-owner".into()]
    } else if is_zip {
        vec!["unzip".into(), "-o".into(), file]
    } else {
        return Err(format!("cannot unpack {}: its format is not one of {} or \
                            .zip; write an `unpack` phase for it",
                           source.filename,
                           tar.join(", ")));
    };
    state.log(&format!("Unpacking {}", source.filename));
    state.run(&argv, None, &[], false)?;
    owner_writable(&state.src_dir())
}

/// Gives the owner write permission on everything under `dir`, which the
/// archive may have shipped read-only: the build owns its source tree, and the
/// shell builder, running as root, never saw the difference. Symbolic links
/// are not followed. A missing `dir` (an archive that unpacks elsewhere) is
/// left alone.
fn owner_writable(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return Ok(());
    };
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    let mode = meta.permissions().mode();
    if mode & 0o200 == 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(mode | 0o200))
            .map_err(|e| format!("cannot make {} writable: {e}", dir.display()))?;
    }
    if meta.is_dir() {
        let entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
            owner_writable(&entry.path())?;
        }
    }
    Ok(())
}

/// Strips binaries under `bin_dirs` and `lib_dirs`: executables of
/// everything, shared objects and position-independent executables of what
/// linking does not need, and static archives of debug information. Files
/// the owner cannot write are left alone, as the shell builder did. The
/// builder's own strip does it, from the PATH it was started with: the
/// package's strip (binutils') cannot rewrite itself while it runs.
fn strip(state: &State) -> Result<(), String> {
    let prefix = state.prefix()?;
    let mut files = Vec::new();
    for dir in state.settings
                    .bin_dirs
                    .iter()
                    .chain(&state.settings.lib_dirs)
    {
        walk(&prefix.join(dir), &mut files, true)?;
    }
    files.sort();
    files.dedup();
    if !files.is_empty() {
        state.log("Stripping unneeded symbols from binaries and libraries");
    }
    let builder_path: Vec<(String, String)> = state.builder_path
                                                   .iter()
                                                   .map(|path| ("PATH".to_string(), path.clone()))
                                                   .collect();
    for file in files {
        let flag = match kind(&file)? {
            Some(Binary::Executable) => "--strip-all",
            Some(Binary::Shared) => "--strip-unneeded",
            Some(Binary::Archive) => "--strip-debug",
            None => continue,
        };
        state.run(&["strip".into(),
                    flag.into(),
                    file.to_string_lossy().into_owned()],
                  None,
                  &builder_path,
                  false)?;
    }
    Ok(())
}

/// Collects the regular files under `dir`, or only those the owner can
/// write, without following symbolic links. A missing `dir` has none.
pub fn walk(dir: &Path, files: &mut Vec<PathBuf>, writable: bool) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot read {}: {error}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| {
                                                      format!("cannot read {}: {e}", path.display())
                                                  })?;
        if metadata.is_dir() {
            walk(&path, files, writable)?;
        } else if metadata.is_file() && (!writable || owner_can_write(&metadata)) {
            files.push(path);
        }
    }
    Ok(())
}

fn owner_can_write(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o200 != 0
}

enum Binary {
    Executable,
    Shared,
    Archive,
}

/// What kind of binary `file` is, from its first bytes.
fn kind(file: &Path) -> Result<Option<Binary>, String> {
    let mut head = Vec::with_capacity(18);
    fs::File::open(file).and_then(|f| f.take(18).read_to_end(&mut head))
                        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    if head.starts_with(b"!<arch>\n") {
        return Ok(Some(Binary::Archive));
    }
    if head.len() < 18 || !head.starts_with(b"\x7fELF") {
        return Ok(None);
    }
    // `e_type` follows the 16-byte identification, in the byte order the
    // identification's sixth byte names: 1 for little-endian, 2 for big.
    let e_type = if head[5] == 2 {
        u16::from_be_bytes([head[16], head[17]])
    } else {
        u16::from_le_bytes([head[16], head[17]])
    };
    Ok(match e_type {
        2 => Some(Binary::Executable),
        3 => Some(Binary::Shared),
        _ => None,
    })
}

/// The SHA-256 of `file` in lowercase hexadecimal.
pub fn sha256(file: &Path) -> Result<String, String> { digest::<Sha256>(file) }

fn digest<D: Digest>(file: &Path) -> Result<String, String> {
    let mut input =
        fs::File::open(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let mut hasher = D::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)
                        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// The BLAKE2b-256 of `file` in lowercase hexadecimal, as `doj pkg hash`
/// prints it.
pub fn blake2b(file: &Path) -> Result<String, String> { digest::<Blake2b<U32>>(file) }

/// Empties `dir`, creating it if needed.
pub fn clean(dir: &Path) -> Result<(), String> {
    match fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot remove {}: {error}", dir.display())),
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

/// Removes the system library search paths from every libtool script in
/// `src`, so libtool links against dependencies rather than the host's
/// libraries.
pub fn fix_libtool(state: &State, src: &Path) -> Result<(), String> {
    let mut scripts = Vec::new();
    find_named(src, "ltmain.sh", &mut scripts)?;
    for script in scripts {
        let text = fs::read_to_string(&script).map_err(|e| {
                                                  format!("cannot read {}: {e}", script.display())
                                              })?;
        let fixed = remove_search_paths(&text);
        if fixed != text {
            state.log(&format!("Fixing libtool script {}", script.display()));
            fs::write(&script, fixed).map_err(|e| {
                                         format!("cannot write {}: {e}", script.display())
                                     })?;
        }
    }
    Ok(())
}

fn find_named(dir: &Path, name: &str, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry.file_type()
                             .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if file_type.is_dir() {
            find_named(&path, name, found)?;
        } else if file_type.is_file()
                  && entry.file_name()
                          .to_string_lossy()
                          .eq_ignore_ascii_case(name)
        {
            found.push(path);
        }
    }
    Ok(())
}

/// What `sed 's^eval sys_lib_.*search_path=.*^^'` did to each line: cuts it
/// at the first `eval sys_lib_` followed later by `search_path=`.
fn remove_search_paths(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let body = line.trim_end_matches('\n');
            let cut = body.match_indices("eval sys_lib_")
                          .map(|(i, _)| i)
                          .find(|&i| body[i + "eval sys_lib_".len()..].contains("search_path="));
            match cut {
                Some(i) => format!("{}{}", &body[..i], &line[body.len()..]),
                None => line.to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libtool_search_paths_are_cut() {
        // What sed does to these lines, checked by running it.
        let text = "a\n  eval sys_lib_search_path=\"$x\"\nkeep eval sys_lib_dlsearch_path=\"y\" \
                    z\nspec eval sys_lib_dlsearch_path_spec=y\nno eval sys_lib_other\n";
        assert_eq!(remove_search_paths(text),
                   "a\n  \nkeep \nspec eval sys_lib_dlsearch_path_spec=y\nno eval sys_lib_other\n");
    }
}
