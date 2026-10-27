// Copyright (c) 2016 Chef Software Inc. and/or applicable contributors
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

use std::{collections::BTreeMap,
          path::Path};

use crate::hcore::package::{install::list_separator,
                            PackageIdent,
                            PackageInstall};

use crate::error::{Error,
                   Result};

/// Prints the packages' environment as `export` lines a POSIX shell can `eval`, sorted by name so
/// the same packages always print the same text.
pub fn start(idents: &[PackageIdent], fs_root_path: &Path) -> Result<()> {
    let installs = idents.iter()
                         .map(|ident| PackageInstall::load(ident, Some(fs_root_path)))
                         .collect::<std::result::Result<Vec<_>, _>>()?;
    let env = PackageInstall::environment_for_commands(&installs)?;
    print!("{}", render_environment(&env)?);
    Ok(())
}

pub(crate) fn render_environment(env: &BTreeMap<String, String>) -> Result<String> {
    render(env, false)
}

/// The environment as `render_environment` prints it, but laid over the one the shell has: a
/// list variable's entries go in front of what it already holds, so `PATH` keeps the shell's own
/// directories after the packages'. A list with no entries prints nothing, rather than an export
/// whose empty first entry would put the current directory on `PATH`.
pub(crate) fn render_overlay(env: &BTreeMap<String, String>) -> Result<String> { render(env, true) }

fn render(env: &BTreeMap<String, String>, overlay: bool) -> Result<String> {
    let mut text = String::new();
    for (name, value) in env {
        if !is_shell_name(name) {
            return Err(Error::InvalidEnvironmentName(name.clone()));
        }
        match list_separator(name) {
            Some(separator) if overlay => {
                if value.is_empty() {
                    continue;
                }
                text.push_str(&format!("export {name}={}\"${{{name}:+{separator}${{{name}}}}}\"\n",
                                       single_quoted(value),
                                       name = name,
                                       separator = separator));
            }
            _ => text.push_str(&format!("export {}={}\n", name, single_quoted(value))),
        }
    }
    Ok(text)
}

/// A name a POSIX shell can assign: a letter or underscore, then letters, digits and underscores.
fn is_shell_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

/// `value` in single quotes, inside which a shell expands nothing; each `'` in it closes the
/// quotes, is escaped and opens them again.
fn single_quoted(value: &str) -> String { format!("'{}'", value.replace('\'', r"'\''")) }

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> BTreeMap<String, String> {
        vars.iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn exports_are_sorted_by_name() {
        let text =
            render_environment(&env(&[("PATH", "/doj/pkgs/a/bin"), ("FOO", "bar")])).unwrap();
        assert_eq!("export FOO='bar'\nexport PATH='/doj/pkgs/a/bin'\n", text);
    }

    #[test]
    fn values_are_quoted_against_expansion() {
        let text = render_environment(&env(&[("V", "it's $HOME `id` \"x\"\nnext")])).unwrap();
        assert_eq!("export V='it'\\''s $HOME `id` \"x\"\nnext'\n", text);
    }

    #[test]
    fn an_overlay_puts_list_entries_in_front_of_the_shells_own() {
        let text = render_overlay(&env(&[("PATH", "/doj/pkgs/a/bin"),
                                         ("CLASSPATH", "/a.jar"),
                                         ("PYTHONPATH", ""),
                                         ("FOO", "bar")])).unwrap();
        assert_eq!("export CLASSPATH='/a.jar'\"${CLASSPATH:+;${CLASSPATH}}\"\nexport \
                    FOO='bar'\nexport PATH='/doj/pkgs/a/bin'\"${PATH:+:${PATH}}\"\n",
                   text);
    }

    #[test]
    fn a_shell_evaluating_an_overlay_keeps_its_own_path_after_the_packages() {
        let text = render_overlay(&env(&[("PATH", "/doj/pkgs/a/bin"),
                                         ("PYTHONPATH", "/doj/pkgs/a/lib")])).unwrap();
        let script = format!("{}printf '%s|%s' \"$PATH\" \"$PYTHONPATH\"", text);
        let out = std::process::Command::new("/bin/sh").arg("-c")
                                                       .arg(&script)
                                                       .env_clear()
                                                       .env("PATH", "/usr/bin:/bin")
                                                       .output()
                                                       .unwrap();
        assert_eq!("/doj/pkgs/a/bin:/usr/bin:/bin|/doj/pkgs/a/lib",
                   String::from_utf8(out.stdout).unwrap());
    }

    #[test]
    fn a_name_a_shell_cannot_assign_is_an_error() {
        for name in ["FOO-BAR", "1FOO", "", "A B", "X;id"] {
            assert!(render_environment(&env(&[(name, "v")])).is_err(),
                    "{:?} should be refused",
                    name);
        }
        assert!(render_environment(&env(&[("_A1", "v")])).is_ok());
    }
}
