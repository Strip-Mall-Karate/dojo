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

//! The environment a build's programs run with: the builder's own, the
//! settings its dependencies publish, and what the plan sets with `b:env`.

use std::{collections::BTreeMap,
          fs,
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

    /// Takes `name` out of the environment.
    pub fn remove(&mut self, name: &str) {
        if let Some(i) = self.position(name) {
            self.vars.remove(i);
        }
    }

    /// Puts `value` in front of `name`'s list, each entry once.
    pub fn prepend(&mut self, name: &str, value: &str, separator: &str) {
        let joined = push_to_path(value, self.get(name).unwrap_or(""), separator);
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

/// Variables whose values are lists, and their separators. Dependencies'
/// values for these are joined rather than overwritten.
const AGGREGATES: &[(&str, &str)] = &[("PATH", ":"),
                                      ("GOPATH", ":"),
                                      ("CLASSPATH", ";"),
                                      ("NODE_PATH", ":"),
                                      ("PYTHONPATH", ":"),
                                      ("BUNDLE_PATH", ":"),
                                      ("BUNDLE_WITHOUT", ":"),
                                      ("GEM_PATH", ":"),
                                      ("RUBYLIB", ":"),
                                      ("RUBYPATH", ":")];

pub fn aggregate_separator(name: &str) -> Option<&'static str> {
    AGGREGATES.iter()
              .find(|(n, _)| *n == name)
              .map(|(_, separator)| *separator)
}

/// `item` in front of `list`, then each entry once, in order of first
/// appearance.
fn push_to_path(item: &str, list: &str, separator: &str) -> String {
    let mut entries: Vec<&str> = Vec::new();
    for entry in item.split(separator).chain(list.split(separator)) {
        if !entry.is_empty() && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries.join(separator)
}

/// An environment a package passes on, with the ident that set each
/// variable: `RUNTIME_ENVIRONMENT` and its provenance, or the build-time
/// pair.
#[derive(Debug, Default)]
pub struct Inherited {
    pub vars:       BTreeMap<String, String>,
    pub provenance: BTreeMap<String, String>,
}

impl Inherited {
    /// Collects `deps`' `RUNTIME_ENVIRONMENT` files, as the shell builder
    /// did: `PATH` is left out, a list variable gets each value in front of
    /// the last, and any other variable takes the last value.
    pub fn from_deps<'a>(deps: impl IntoIterator<Item = &'a Path>) -> Inherited {
        let mut inherited = Inherited::default();
        for dep in deps {
            let Ok(text) = fs::read_to_string(dep.join("RUNTIME_ENVIRONMENT")) else {
                continue;
            };
            let ident = metadata(dep, "IDENT").unwrap_or_default();
            for line in text.lines() {
                let (name, value) = line.split_once('=').unwrap_or((line, ""));
                if name.is_empty() || name == "PATH" {
                    continue;
                }
                inherited.set_from(name, value, &ident);
            }
        }
        inherited
    }

    fn set_from(&mut self, name: &str, value: &str, ident: &str) {
        let current = self.vars.get(name).map_or("", String::as_str);
        if current.is_empty() {
            self.vars.insert(name.to_string(), value.to_string());
            self.provenance.insert(name.to_string(), ident.to_string());
            return;
        }
        if current == value {
            return;
        }
        if let Some(separator) = aggregate_separator(name) {
            let joined = push_to_path(value, current, separator);
            let from = self.provenance.get(name).map_or("", String::as_str);
            let from = push_to_path(ident, from, " ");
            self.vars.insert(name.to_string(), joined);
            self.provenance.insert(name.to_string(), from);
        } else {
            self.vars.insert(name.to_string(), value.to_string());
            self.provenance.insert(name.to_string(), ident.to_string());
        }
    }

    /// The package's own `value` for `name`, replacing what its
    /// dependencies passed on, or put in front of their list with `push`.
    pub fn export(&mut self, name: &str, value: &str, push: Option<&str>, ident: &str) {
        let current = self.vars.get(name).cloned().unwrap_or_default();
        match push {
            Some(separator) if !current.is_empty() => {
                let from = self.provenance.get(name).cloned().unwrap_or_default();
                self.vars
                    .insert(name.to_string(), push_to_path(value, &current, separator));
                self.provenance
                    .insert(name.to_string(), push_to_path(ident, &from, " "));
            }
            _ => {
                self.vars.insert(name.to_string(), value.to_string());
                self.provenance.insert(name.to_string(), ident.to_string());
            }
        }
    }

    /// Writes the variables to `file` and the provenance to
    /// `file_PROVENANCE` under `prefix`, one `name=value` per line sorted by
    /// name. Nothing is written for an empty environment.
    pub fn write(&self, prefix: &Path, file: &str) -> Result<(), String> {
        for (map, name) in [(&self.vars, file.to_string()),
                            (&self.provenance, format!("{file}_PROVENANCE"))]
        {
            if map.is_empty() {
                continue;
            }
            let text = map.iter().fold(String::new(), |text, (name, value)| {
                                     text + name + "=" + value + "\n"
                                 });
            let path = prefix.join(name);
            fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        Ok(())
    }
}

/// Layers the dependencies' environments over the build's: the run
/// dependencies' values replace the build's, then the build dependencies'
/// values join list variables or replace the rest.
pub fn layer(env: &mut Env, runtime: &Inherited, buildtime: &Inherited) {
    for (name, value) in &runtime.vars {
        env.set(name, value.clone());
    }
    for (name, value) in &buildtime.vars {
        match env.get(name) {
            Some(current) if !current.is_empty() => {
                if current == value {
                    continue;
                }
                let layered = match aggregate_separator(name) {
                    Some(separator) => push_to_path(value, current, separator),
                    None => value.clone(),
                };
                env.set(name, layered);
            }
            _ => env.set(name, value.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_variables_join_and_others_take_the_last_value() {
        let mut inherited = Inherited::default();
        inherited.set_from("GEM_PATH", "/a", "o/a/1/1");
        inherited.set_from("GEM_PATH", "/b:/a", "o/b/1/1");
        inherited.set_from("JAVA_HOME", "/a", "o/a/1/1");
        inherited.set_from("JAVA_HOME", "/b", "o/b/1/1");
        assert_eq!(inherited.vars["GEM_PATH"], "/b:/a");
        assert_eq!(inherited.provenance["GEM_PATH"], "o/b/1/1 o/a/1/1");
        assert_eq!(inherited.vars["JAVA_HOME"], "/b");
        assert_eq!(inherited.provenance["JAVA_HOME"], "o/b/1/1");
    }

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
