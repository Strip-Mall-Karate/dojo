// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the MIT license. See LICENSE in the repository root.

//! Finds the installed packages a build depends on, in the order the shell
//! builder used for `PATH` and the compiler flags.

use std::{env,
          ffi::OsString,
          fs,
          path::{Path,
                 PathBuf},
          process::{Command,
                    Stdio}};

/// Finds the installed package for an ident.
pub trait Resolve {
    /// The install directory of the newest installed package matching
    /// `ident`, which may name just `origin/name`.
    ///
    /// # Errors
    ///
    /// A message saying why no package was found.
    fn resolve(&self, ident: &str) -> Result<PathBuf, String>;
}

/// Resolves idents with `doj pkg path`, as the shell builder does. The
/// program is `$DOJ_BIN`, or `doj` from `PATH`.
pub struct DojPkgPath {
    program: OsString,
}

impl DojPkgPath {
    #[must_use]
    pub fn from_env() -> DojPkgPath {
        DojPkgPath { program: env::var_os("DOJ_BIN").unwrap_or_else(|| "doj".into()), }
    }
}

impl Resolve for DojPkgPath {
    fn resolve(&self, ident: &str) -> Result<PathBuf, String> {
        let output = Command::new(&self.program).args(["pkg", "path", ident])
                                                .stdin(Stdio::null())
                                                .stderr(Stdio::null())
                                                .output()
                                                .map_err(|e| {
                                                    format!("cannot run `{} pkg path`: {e}",
                                                            self.program.display())
                                                })?;
        let path = String::from_utf8(output.stdout).unwrap_or_default();
        let path = path.trim();
        if output.status.success() && !path.is_empty() {
            Ok(PathBuf::from(path))
        } else {
            Err(format!("no installed package matches `{ident}`; should it be built first?"))
        }
    }
}

/// A direct dependency: the ident as declared, and where it is installed.
#[derive(Clone, Debug)]
pub struct Dep {
    pub ident: String,
    pub path:  PathBuf,
}

/// Every dependency of a build, resolved.
#[derive(Debug, Default)]
pub struct Resolved {
    /// Direct run dependencies.
    pub deps:        Vec<Dep>,
    /// Direct build dependencies, the scaffolding and its run dependencies
    /// first.
    pub build_deps:  Vec<Dep>,
    /// The run dependencies followed by their transitive run
    /// dependencies, without repeats.
    pub tdeps:       Vec<PathBuf>,
    /// The build dependencies followed by their transitive run
    /// dependencies, without repeats.
    pub build_tdeps: Vec<PathBuf>,
    /// The direct dependencies, run then build, followed by each one's
    /// transitive run dependencies, without repeats.
    pub all_tdeps:   Vec<PathBuf>,
}

impl Resolved {
    /// Resolves the declared dependencies. `pkgs` is the directory packages
    /// are installed under, which `DEPS` and `TDEPS` entries are relative to.
    ///
    /// # Errors
    ///
    /// The first dependency that does not resolve.
    pub fn resolve(deps: &[String],
                   build_deps: &[String],
                   scaffolding: Option<&str>,
                   pkgs: &Path,
                   packages: &dyn Resolve)
                   -> Result<Resolved, String> {
        let mut resolved = Resolved::default();
        if let Some(scaffolding) = scaffolding {
            let path = packages.resolve(scaffolding)?;
            for ident in ident_list(&path, "DEPS") {
                resolved.build_deps
                        .push(Dep { path: package_path(pkgs, &ident),
                                    ident });
            }
            resolved.build_deps.insert(0,
                                       Dep { ident: scaffolding.to_string(),
                                             path });
        }
        for ident in build_deps {
            resolved.build_deps.push(Dep { ident: ident.clone(),
                                           path:  packages.resolve(ident)?, });
        }
        for ident in deps {
            resolved.deps.push(Dep { ident: ident.clone(),
                                     path:  packages.resolve(ident)?, });
        }

        resolved.tdeps = transitive(&resolved.deps, pkgs);
        resolved.build_tdeps = transitive(&resolved.build_deps, pkgs);
        let mut all: Vec<PathBuf> = Vec::new();
        let direct = resolved.direct().map(|dep| &dep.path);
        for path in direct.chain(&resolved.tdeps).chain(&resolved.build_tdeps) {
            add(&mut all, path.clone());
        }
        resolved.all_tdeps = all;
        Ok(resolved)
    }

    /// The direct dependencies, run then build.
    pub fn direct(&self) -> impl Iterator<Item = &Dep> { self.deps.iter().chain(&self.build_deps) }
}

/// `deps` followed by the packages in each one's `TDEPS`, without repeats.
fn transitive(deps: &[Dep], pkgs: &Path) -> Vec<PathBuf> {
    let mut set: Vec<PathBuf> = deps.iter().map(|dep| dep.path.clone()).collect();
    for dep in deps {
        for ident in ident_list(&dep.path, "TDEPS") {
            add(&mut set, package_path(pkgs, &ident));
        }
    }
    set
}

fn add(set: &mut Vec<PathBuf>, path: PathBuf) {
    if !set.contains(&path) {
        set.push(path);
    }
}

/// The idents listed one per line in a package's metadata file, or none
/// when the file is missing.
fn ident_list(package: &Path, file: &str) -> Vec<String> {
    fs::read_to_string(package.join(file)).unwrap_or_default()
                                          .lines()
                                          .map(str::trim)
                                          .filter(|line| !line.is_empty())
                                          .map(String::from)
                                          .collect()
}

/// The install directory of a fully qualified ident.
pub fn package_path(pkgs: &Path, ident: &str) -> PathBuf {
    ident.split('/')
         .fold(pkgs.to_path_buf(), |path, part| path.join(part))
}
