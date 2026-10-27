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

//! `doj shell` as a person runs it: the built program, its own argument parsing, in a directory.

use std::process::Command;

use tempfile::TempDir;

/// Runs `doj shell <subcommand>` in an empty directory, with a home of its own.
fn shell_in_an_empty_directory(subcommand: &str) -> (Option<i32>, String) {
    let dir = TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_doj")).args(["shell", subcommand])
                                                     .current_dir(dir.path())
                                                     .env("HOME", dir.path())
                                                     .env_remove("DOJ_BLDR_URL")
                                                     .output()
                                                     .unwrap();
    (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn each_shell_subcommand_says_a_directory_declares_no_shell() {
    for subcommand in ["env", "lock"] {
        let (code, stderr) = shell_in_an_empty_directory(subcommand);
        assert_eq!(Some(1), code, "doj shell {}: {}", subcommand, stderr);
        assert!(stderr.contains("the directory declares no Dojo shell"),
                "doj shell {}: {}",
                subcommand,
                stderr);
    }
}
