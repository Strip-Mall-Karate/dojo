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

use crate::hcore::package::{PackageIdent,
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

fn render_environment(env: &BTreeMap<String, String>) -> Result<String> {
    let mut text = String::new();
    for (name, value) in env {
        if !is_shell_name(name) {
            return Err(Error::InvalidEnvironmentName(name.clone()));
        }
        text.push_str(&format!("export {}={}\n", name, single_quoted(value)));
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
    fn a_name_a_shell_cannot_assign_is_an_error() {
        for name in ["FOO-BAR", "1FOO", "", "A B", "X;id"] {
            assert!(render_environment(&env(&[(name, "v")])).is_err(),
                    "{:?} should be refused",
                    name);
        }
        assert!(render_environment(&env(&[("_A1", "v")])).is_ok());
    }
}
