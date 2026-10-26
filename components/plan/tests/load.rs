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

//! Loads every plan under `tests/fixtures`. A `fail-*` plan must be refused
//! with the message listed here, and every other plan must load.

use std::{fs,
          path::{Path,
                 PathBuf}};

use dojo_plan::Plan;
use serde_json::json;

/// Each refused plan and a piece of the message it must be refused with.
const REFUSED: &[(&str, &str)] = &[("fail-bad-ident", "`zlib` is not a package ident"),
                                   ("fail-exposes",
                                    "`exposes` names `admin`, which is not in `exports`"),
                                   ("fail-global-table", "unknown identifier: _G"),
                                   ("fail-import-macros", "extra module not found"),
                                   ("fail-include", "module not found extra"),
                                   ("fail-io", "unknown identifier: io"),
                                   ("fail-loop", "the plan ran past its budget"),
                                   ("fail-lua-plan", "has plan.lua but no plan.fnl"),
                                   ("fail-macro-io", "unknown identifier: io"),
                                   ("fail-macro-loop", "compiling the plan ran past its budget"),
                                   ("fail-macro-memory", "not enough memory"),
                                   ("fail-memory", "the plan ran out of memory"),
                                   ("fail-missing-name", "does not set `name`"),
                                   ("fail-not-table", "returns a string, not a table"),
                                   ("fail-on-name", "`on.x86_64-linux` may not override `name`"),
                                   ("fail-pcall", "unknown identifier: pcall"),
                                   ("fail-phase-not-fn", "`build` is a string, not a function"),
                                   ("fail-raw-lua", "attempt to index a nil value (global 'os')"),
                                   ("fail-string-metatable", "unknown identifier: getmetatable"),
                                   ("fail-unknown-key", "unknown key `nmae`"),
                                   ("fail-unknown-target", "`x86_64-windows` is not in `targets`")];

fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut dirs: Vec<PathBuf> = fs::read_dir(root).expect("tests/fixtures is readable")
                                                   .map(|entry| entry.expect("entry").path())
                                                   .filter(|path| path.is_dir())
                                                   .collect();
    dirs.sort();
    dirs
}

fn name(dir: &Path) -> &str {
    dir.file_name()
       .and_then(|n| n.to_str())
       .expect("UTF-8 name")
}

#[test]
fn every_failing_fixture_is_listed() {
    let failing: Vec<&str> = REFUSED.iter().map(|(name, _)| *name).collect();
    for dir in fixtures() {
        let name = name(&dir);
        if name.starts_with("fail-") {
            assert!(failing.contains(&name),
                    "{name} has no expected message in REFUSED");
        }
    }
}

#[test]
fn refused_plans_fail_with_their_reason() {
    let mut wrong = Vec::new();
    for (fixture, expected) in REFUSED {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
                                                       .join(fixture);
        match Plan::load(&dir) {
            Ok(_) => {
                wrong.push(format!("{fixture} loaded, but should be refused with `{expected}`"));
            }
            Err(error) => {
                let message = error.to_string();
                if !message.contains(expected) {
                    wrong.push(format!("{fixture} was refused with `{message}`, not `{expected}`"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn other_plans_load() {
    for dir in fixtures() {
        if !name(&dir).starts_with("fail-")
           && let Err(error) = Plan::load(&dir)
        {
            panic!("{} did not load: {error}", name(&dir));
        }
    }
}

#[test]
fn launcher_merges_the_windows_overrides() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/launcher");
    let plan = Plan::load(dir).expect("the launcher plan loads");
    let linux = plan.metadata().to_json(Some("x86_64-linux"));
    let windows = plan.metadata().to_json(Some("x86_64-windows"));
    assert_eq!(linux["deps"][0], json!("karate/glibc"));
    assert_eq!(windows["deps"][0], json!("karate/openssl"));
    assert_eq!(windows["name"], json!("doj-launcher"));
    assert_eq!(windows["version_computed"], json!(true));
    assert_eq!(windows["phases"], json!(["prepare", "install"]));
    assert!(windows.get("on").is_none());
}

#[test]
fn metadata_is_the_same_every_load() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/launcher");
    let first = Plan::load(&dir).expect("loads").metadata().to_json(None);
    for _ in 0..5 {
        assert_eq!(Plan::load(&dir).expect("loads").metadata().to_json(None),
                   first);
    }
}
