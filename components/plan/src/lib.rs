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

//! Loads Dojo plans written in Fennel.
//!
//! Loading is the first of a plan's two stages. It compiles `plan.fnl`, runs
//! it in a sandbox that cannot touch the machine, and checks the table it
//! returns. Reading a plan's metadata needs nothing more. The build stage,
//! [`Plan::build`], calls the phase functions the loaded table holds.

// Plans build on Linux and macOS only.
#[cfg(not(unix))]
compile_error!("doj-plan-build builds packages on Linux and macOS");

mod build;
mod builtin;
mod deps;
mod env;
mod fennel;
mod host_paths;
mod package;
mod sandbox;
mod schema;

use std::{fmt,
          fs,
          io,
          path::{Path,
                 PathBuf}};

use mlua::{Lua,
           Table,
           Value};

pub use crate::{build::{Built,
                        Options,
                        host_target,
                        target_os},
                deps::{DojPkgPath,
                       Resolve},
                package::Artifact,
                schema::{Metadata,
                         PHASES,
                         TARGETS}};

/// The file a plan directory holds.
pub const PLAN_FILE: &str = "plan.fnl";

/// Plan formats Dojo no longer reads, named in the error for a directory
/// that still holds one.
const OLD_PLAN_FILES: &[&str] = &["plan.lua", "plan.sh", "plan.ps1"];

/// Why a plan did not load.
#[derive(Debug)]
pub enum Error {
    /// The plan file exists but could not be read.
    Read(PathBuf, io::Error),
    /// The directory holds no `plan.fnl`. `old` names a file in a format
    /// Dojo no longer reads, if there is one.
    Missing {
        dir: PathBuf,
        old: Option<&'static str>,
    },
    /// Fennel refused the source.
    Compile(String),
    /// The compiled plan failed while it ran, or ran past its limits.
    Run(String),
    /// The plan's table does not fit the schema.
    Invalid(String),
    /// A build step or phase failed.
    Build(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read(path, error) => write!(f, "cannot read {}: {error}", path.display()),
            Error::Missing { dir,
                             old: Some(old), } => {
                write!(f,
                       "{} has {old} but no {PLAN_FILE}; plans are written in Fennel now",
                       dir.display())
            }
            Error::Missing { dir, old: None } => {
                write!(f, "{} has no {PLAN_FILE}", dir.display())
            }
            Error::Compile(message) | Error::Run(message) | Error::Build(message) => {
                f.write_str(message)
            }
            Error::Invalid(message) => write!(f, "{PLAN_FILE}: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// A loaded plan: its checked metadata, and the Lua state that holds its
/// phase functions for the build stage.
pub struct Plan {
    metadata: Metadata,
    table:    Table,
    lua:      Lua,
}

/// Where a plan and the source it builds live.
#[derive(Debug)]
pub struct Located {
    /// The directory holding `plan.fnl`.
    pub plan_dir: PathBuf,
    /// The source directory of a plan without `source`.
    pub context:  PathBuf,
}

/// Finds the plan for `dir`, which holds `plan.fnl` itself or in a `dojo`
/// subdirectory. In the second case `dir` is the plan's source, as it was
/// for the shell builder.
#[must_use]
pub fn locate(dir: impl AsRef<Path>) -> Located {
    let dir = dir.as_ref();
    let nested = dir.join("dojo");
    if !dir.join(PLAN_FILE).is_file() && nested.join(PLAN_FILE).is_file() {
        Located { plan_dir: nested,
                  context:  dir.to_path_buf(), }
    } else {
        Located { plan_dir: dir.to_path_buf(),
                  context:  dir.to_path_buf(), }
    }
}

impl Plan {
    /// Loads the plan in `dir`.
    ///
    /// # Errors
    ///
    /// Fails when `dir` has no readable `plan.fnl`, or as [`Plan::from_source`] does.
    pub fn load(dir: impl AsRef<Path>) -> Result<Plan, Error> {
        let dir = dir.as_ref();
        let path = dir.join(PLAN_FILE);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let old = OLD_PLAN_FILES.iter()
                                        .copied()
                                        .find(|old| dir.join(old).is_file());
                return Err(Error::Missing { dir: dir.to_path_buf(),
                                            old });
            }
            Err(error) => return Err(Error::Read(path, error)),
        };
        Plan::from_source(&source)
    }

    /// Loads a plan from its Fennel source.
    ///
    /// # Errors
    ///
    /// [`Error::Compile`] when Fennel refuses the source, [`Error::Run`] when the
    /// compiled plan fails or passes its limits, and [`Error::Invalid`] when its
    /// table does not fit the schema.
    pub fn from_source(source: &str) -> Result<Plan, Error> {
        let lua_source =
            fennel::compile(source, PLAN_FILE).map_err(|e| {
                                                  Error::Compile(message(&e, Stage::Compile))
                                              })?;
        let lua = Lua::new();
        let env = sandbox::environment(&lua).map_err(|e| Error::Run(message(&e, Stage::Run)))?;
        sandbox::limit(&lua,
                       sandbox::PLAN_MEMORY,
                       sandbox::PLAN_INSTRUCTIONS,
                       "the plan").map_err(|e| Error::Run(message(&e, Stage::Run)))?;
        let value: Value = lua.load(lua_source)
                              .set_name(format!("@{PLAN_FILE}"))
                              .set_environment(env)
                              .eval()
                              .map_err(|e| Error::Run(message(&e, Stage::Run)))?;
        let Value::Table(table) = value else {
            return Err(Error::Invalid(format!("returns a {}, not a table", value.type_name())));
        };
        let metadata = Metadata::parse(&table)?;
        Ok(Plan { metadata,
                  table,
                  lua })
    }

    /// The plan's checked metadata.
    pub fn metadata(&self) -> &Metadata { &self.metadata }
}

/// The message of a Lua error without mlua's wrapping or the Lua stack
/// traceback, which shows the sandbox's internals rather than the plan.
/// Memory errors carry no message from Lua, so they get one here.
pub(crate) fn message(error: &mlua::Error, stage: Stage) -> String {
    match error {
        mlua::Error::RuntimeError(message) | mlua::Error::SyntaxError { message, .. } => {
            let message = message.split("\nstack traceback:")
                                 .next()
                                 .unwrap_or_default();
            message.trim_end().to_string()
        }
        mlua::Error::MemoryError(_) => {
            match stage {
                Stage::Compile => {
                    format!("compiling the plan ran out of memory (the limit is {} MiB)",
                            sandbox::COMPILE_MEMORY >> 20)
                }
                Stage::Run => {
                    format!("the plan ran out of memory (the limit is {} MiB)",
                            sandbox::PLAN_MEMORY >> 20)
                }
                Stage::Build => {
                    format!("the plan ran out of memory during the build (the limit is {} MiB)",
                            sandbox::PLAN_MEMORY >> 20)
                }
            }
        }
        mlua::Error::CallbackError { cause, .. } => message(cause, stage),
        other => other.to_string(),
    }
}

/// Which part of loading an error came from.
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Compile,
    Run,
    Build,
}
