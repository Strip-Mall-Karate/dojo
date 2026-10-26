// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the MIT license. See LICENSE in the repository root.

//! The environment a build's programs run with: the builder's own, the
//! settings its dependencies publish, and what the plan sets with `b:env`.

use std::{fs,
          path::{Path,
                 PathBuf}};

use crate::deps::Resolved;

/// Environment variables in the order they were first set. Names ignore
/// case on Windows, as Windows does.
#[derive(Clone, Debug)]
pub struct Env {
    vars:        Vec<(String, String)>,
    ignore_case: bool,
}

impl Env {
    pub fn new(vars: impl IntoIterator<Item = (String, String)>, ignore_case: bool) -> Env {
        let mut env = Env { vars: Vec::new(),
                            ignore_case };
        for (name, value) in vars {
            env.set(&name, value);
        }
        env
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.vars.iter().position(|(n, _)| {
                            if self.ignore_case {
                                n.eq_ignore_ascii_case(name)
                            } else {
                                n == name
                            }
                        })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.position(name).map(|i| self.vars[i].1.as_str())
    }

    pub fn set(&mut self, name: &str, value: String) {
        match self.position(name) {
            Some(i) => self.vars[i].1 = value,
            None => self.vars.push((name.to_string(), value)),
        }
    }

    /// Appends `value` to `name` after `separator`, or sets it when `name`
    /// is unset or empty.
    pub fn append(&mut self, name: &str, value: &str, separator: &str) {
        let joined = match self.get(name) {
            Some(old) if !old.is_empty() => format!("{old}{separator}{value}"),
            _ => value.to_string(),
        };
        self.set(name, joined);
    }

    pub fn vars(&self) -> &[(String, String)] { &self.vars }
}

/// The contents of a package metadata file with surrounding whitespace
/// removed, or `None` when the file is missing.
fn metadata(package: &Path, file: &str) -> Option<String> {
    fs::read_to_string(package.join(file)).ok()
                                          .map(|text| text.trim().to_string())
}

fn text(path: &Path) -> String { path.to_string_lossy().into_owned() }

/// Sets `PATH` to the plan's own `bin_dirs`, then each dependency's `PATH`
/// entries that lie inside that dependency, then `initial`, as the shell
/// builder did.
pub fn set_path(env: &mut Env,
                initial: &str,
                bin_dirs: &[PathBuf],
                deps: &Resolved,
                separator: &str) {
    let mut paths: Vec<String> = bin_dirs.iter().map(|dir| text(dir)).collect();
    for dep in &deps.all_tdeps {
        let Some(data) = metadata(dep, "PATH") else {
            continue;
        };
        let prefix = text(dep);
        for entry in data.split(separator) {
            if entry.starts_with(&prefix) && !paths.iter().any(|p| p == entry) {
                paths.push(entry.to_string());
            }
        }
    }
    paths.push(initial.to_string());
    env.set("PATH", paths.join(separator));
}

/// Sets `LD_RUN_PATH`, the C compiler flags, `PKG_CONFIG_PATH` and
/// `PREFIX` from the plan's `lib_dirs` and its direct dependencies'
/// metadata files, as the shell builder did.
pub fn set_flags(env: &mut Env,
                 prefix: &Path,
                 lib_dirs: &[PathBuf],
                 deps: &Resolved,
                 separator: &str) {
    let mut run_path: Vec<String> = lib_dirs.iter().map(|dir| text(dir)).collect();
    for dep in &deps.deps {
        if let Some(data) = metadata(&dep.path, "LD_RUN_PATH") {
            run_path.push(data);
        }
    }
    if !run_path.is_empty() {
        env.set("LD_RUN_PATH", run_path.join(":"));
    }

    for dep in deps.direct() {
        let path = &dep.path;
        if let Some(cflags) = metadata(path, "CFLAGS") {
            env.append("CFLAGS", &cflags, " ");
            if !path.join("CPPFLAGS").is_file() {
                env.append("CPPFLAGS", &cflags, " ");
            }
            if !path.join("CXXFLAGS").is_file() {
                env.append("CXXFLAGS", &cflags, " ");
            }
        }
        for name in ["CPPFLAGS", "CXXFLAGS", "LDFLAGS"] {
            if let Some(flags) = metadata(path, name) {
                env.append(name, &flags, " ");
            }
        }
        if let Some(paths) = metadata(path, "PKG_CONFIG_PATH") {
            env.append("PKG_CONFIG_PATH", &paths, separator);
        }
    }
    env.set("PREFIX", text(prefix));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_ignore_case_only_when_asked() {
        let mut env = Env::new([("Path".to_string(), "a".to_string())], true);
        env.append("PATH", "b", ";");
        assert_eq!(env.vars(), [("Path".to_string(), "a;b".to_string())]);

        let mut env = Env::new([("Path".to_string(), "a".to_string())], false);
        env.append("PATH", "b", ":");
        assert_eq!(env.get("PATH"), Some("b"));
        assert_eq!(env.get("Path"), Some("a"));
    }
}
