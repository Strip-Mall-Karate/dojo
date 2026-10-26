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

//! The build stage: resolves a plan's dependencies, loads its scaffolding,
//! and runs the eight phases with the build context `b`, which is the only
//! way plan code touches the machine.
//!
//! Each phase resolves to the plan's function for the target, then the
//! scaffolding's, then the builtin. `b.default.<phase>` runs the next one
//! down from wherever it is called.
//!
//! A scaffolding is a package shipping `lib/scaffolding.fnl`, a module of
//! phase functions. Its `metadata` function, if it has one, gets the plan's
//! metadata, the plan's settings for it (`:scaffolding {:package <ident>
//! :settings <data>}`) and read-only views of the build's source context
//! and the plan's directory, before dependencies resolve. It may return
//! dependencies, directories, `svc`, exports, binds and the descriptive keys
//! to add: lists gain its entries, tables its keys, and whatever the plan sets
//! itself stays the plan's. A `default.toml` the build installs follows the
//! plan's in the package. Phases read the settings as `b.scaffolding_settings` and the
//! plan's source as `b.source`.

use std::{cell::{Cell,
                 RefCell},
          collections::BTreeMap,
          env,
          ffi::OsString,
          fs,
          path::{Component,
                 Path,
                 PathBuf},
          process::{Command,
                    Stdio},
          rc::Rc,
          thread,
          time::{SystemTime,
                 UNIX_EPOCH}};

use mlua::{Function,
           Lua,
           Table,
           Value,
           Variadic};
use serde_json::Value as Json;

use crate::{Error,
            Plan,
            Stage,
            builtin,
            deps::{Dep,
                   Resolve,
                   Resolved,
                   package_path},
            env::{self as build_env,
                  Env,
                  Inherited},
            fennel,
            message,
            package::{self,
                      Artifact,
                      Package},
            sandbox,
            schema::{self,
                     PHASES}};

/// Where Dojo keeps packages and caches when `DOJ_ROOT_PATH` is unset.
const DEFAULT_ROOT: &str = "/doj";

/// What separates the entries of `PATH` and the other list variables.
const SEPARATOR: &str = ":";

/// The file a scaffolding package ships its module in.
const SCAFFOLDING_FILE: &str = "lib/scaffolding.fnl";

/// How one build runs.
#[derive(Clone, Debug)]
pub struct Options {
    /// The target to build for.
    pub target:   String,
    /// Dojo's root: packages install under `pkgs` and sources download to
    /// `cache/src`.
    pub root:     PathBuf,
    /// The release, a UTC timestamp like `20261025143000`.
    pub release:  String,
    /// The directory holding `plan.fnl`.
    pub plan_dir: PathBuf,
    /// The source directory of a plan without `source`.
    pub context:  PathBuf,
    /// Whether to run the `check` phase.
    pub check:    bool,
    /// Whether a Linux package that names files outside the root (a dynamic
    /// linker, a RUNPATH entry, a script's interpreter) fails the build.
    pub refuse_host_paths: bool,
    /// Parallel jobs, `b.jobs`.
    pub jobs:     usize,
    /// The environment the build starts from.
    pub env:      Vec<(String, String)>,
    /// Where origin signing keys are: `DOJ_CACHE_KEY_PATH`, else
    /// `cache/keys` under the root.
    pub keys:     PathBuf,
    /// Where the artifact and build report are copied: `DOJ_OUTPUT_PATH`,
    /// else `results` in the current directory.
    pub output:   PathBuf,
    /// The `doj` program that signs packages: `DOJ_BIN`, else `doj`.
    pub doj:      OsString,
}

impl Options {
    /// Options for building `target` from the plan in `plan_dir` with
    /// sources in `context`, taking the rest from this process: its
    /// environment, `DOJ_ROOT_PATH`, the current time and the machine's
    /// parallelism.
    #[must_use]
    pub fn new(target: &str, plan_dir: &Path, context: &Path) -> Options {
        let root =
            env::var_os("DOJ_ROOT_PATH").map_or_else(|| PathBuf::from(DEFAULT_ROOT), PathBuf::from);
        Options { target: target.to_string(),
                  keys: env::var_os("DOJ_CACHE_KEY_PATH").map_or_else(|| {
                                                                          root.join("cache")
                                                                              .join("keys")
                                                                      },
                                                                      PathBuf::from),
                  output: env::var_os("DOJ_OUTPUT_PATH").map_or_else(|| PathBuf::from("results"),
                                                                     PathBuf::from),
                  doj: env::var_os("DOJ_BIN").unwrap_or_else(|| "doj".into()),
                  root,
                  release: release_now(),
                  plan_dir: plan_dir.to_path_buf(),
                  context: context.to_path_buf(),
                  check: false,
                  refuse_host_paths: false,
                  jobs: thread::available_parallelism().map_or(1, usize::from),
                  env: env::vars().collect() }
    }
}

/// What a build produced.
#[derive(Debug)]
pub struct Built {
    /// `origin/name/version/release`.
    pub ident:    String,
    /// Where the package was installed.
    pub prefix:   PathBuf,
    /// The signed package.
    pub artifact: Artifact,
}

/// The source a plan downloads.
#[derive(Debug)]
pub struct Source {
    pub url:      String,
    pub sha256:   String,
    pub filename: String,
    dirname:      Option<String>,
}

/// The metadata the build stage reads, merged for one target.
#[derive(Debug)]
pub struct Settings {
    pub source:   Option<Source>,
    pub bin_dirs: Vec<String>,
    pub lib_dirs: Vec<String>,
    other_dirs:   Vec<String>,
}

/// The state behind `b`, shared by its functions and the builtin phases.
pub struct State {
    pub name:         String,
    pub os:           &'static str,
    pub settings:     Settings,
    pub cache:        PathBuf,
    context:          PathBuf,
    pub env:          RefCell<Env>,
    prefix:           RefCell<Option<PathBuf>>,
    src:              RefCell<PathBuf>,
    deps:             Vec<Dep>,
    /// The phase running, or `version` while the version function runs.
    phase:            Cell<&'static str>,
    /// Per phase, the plan's function and the scaffolding's.
    chains:           BTreeMap<&'static str, [Option<Function>; 2]>,
    /// The phases running and the level of the chain each is at, innermost
    /// last.
    active:           RefCell<Vec<(&'static str, usize)>>,
    /// The address of the `b` table, to tell `b:run` from `b.run`.
    b:                Cell<usize>,
    /// What the plan passes on to the packages that depend on it, in the
    /// order it said so.
    exports:          RefCell<Vec<Export>>,
    /// `PATH` as the builder was started with it, for the tools that work on
    /// the package rather than build it.
    pub builder_path: Option<String>,
}

/// A variable a plan passes on: at run time (`RUNTIME_ENVIRONMENT`) or only
/// to the builds of packages that depend on it (`BUILDTIME_ENVIRONMENT`),
/// replacing the value or put in front of a list.
struct Export {
    runtime: bool,
    /// The list separator when the value goes in front of a list.
    push:    Option<&'static str>,
    name:    String,
    value:   String,
}

impl State {
    /// The directory the source unpacks into, the build's working directory.
    pub fn src_dir(&self) -> PathBuf { self.src.borrow().clone() }

    /// Prints a build line.
    pub fn log(&self, message: &str) {
        println!("   {}: {message}", self.name);
    }

    pub fn prefix(&self) -> Result<PathBuf, String> {
        self.prefix
            .borrow()
            .clone()
            .ok_or_else(|| "the prefix is not known until the version is".to_string())
    }

    pub fn prefix_text(&self) -> Result<String, String> { text(&self.prefix()?) }

    /// The directory a program runs in unless told otherwise: the plan's
    /// source directory while computing the version, the cache while
    /// fetching and unpacking, and `b.src` after.
    fn default_cwd(&self) -> PathBuf {
        match self.phase.get() {
            "version" => self.context.clone(),
            "download" | "verify" | "unpack" => self.cache.clone(),
            _ => self.src.borrow().clone(),
        }
    }

    /// Starts `argv` with the build environment plus `extra` and waits for
    /// it, collecting stdout when `capture` is set; how it ended is the
    /// caller's to judge.
    fn spawn(&self,
             argv: &[String],
             cwd: Option<&Path>,
             extra: &[(String, String)],
             stdin: Option<&[u8]>,
             capture: bool)
             -> Result<(std::process::Output, String), String> {
        let shown = show_argv(argv);
        let Some((program, rest)) = argv.split_first() else {
            return Err("the argv is empty".to_string());
        };
        let cwd = match cwd {
            Some(cwd) => self.default_cwd().join(cwd),
            None => self.default_cwd(),
        };
        let mut env = self.env.borrow().clone();
        for (name, value) in extra {
            env.set(name, value.clone());
        }
        let mut command = Command::new(program);
        command.args(rest)
               .current_dir(&cwd)
               .env_clear()
               .envs(env.vars().iter().map(|(n, v)| (n, v)))
               .stdin(if stdin.is_some() {
                          Stdio::piped()
                      } else {
                          Stdio::null()
                      })
               .stdout(if capture {
                           Stdio::piped()
                       } else {
                           Stdio::inherit()
                       })
               .stderr(Stdio::inherit());
        let mut child =
            command.spawn()
                   .map_err(|e| format!("`{shown}` could not start in {}: {e}", cwd.display()))?;
        // The program reads its input while it writes its output, so the input
        // goes in from another thread. A program that stops reading early ends
        // the write, which is no error of the build's.
        let feeder = match (stdin, child.stdin.take()) {
            (Some(input), Some(mut pipe)) => {
                let input = input.to_vec();
                Some(std::thread::spawn(move || {
                         use std::io::Write as _;
                         match pipe.write_all(&input) {
                             Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e),
                             _ => Ok(()),
                         }
                     }))
            }
            _ => None,
        };
        let output = child.wait_with_output()
                          .map_err(|e| format!("`{shown}` could not be waited for: {e}"))?;
        if let Some(feeder) = feeder {
            feeder.join()
                  .map_err(|_| format!("`{shown}`: writing its input panicked"))?
                  .map_err(|e| format!("`{shown}`: writing its input failed: {e}"))?;
        }
        Ok((output, shown))
    }

    /// Runs `argv` and returns its exit code, whatever it is. A program that
    /// cannot start, or that a signal ends, is still an error.
    pub fn run_status(&self,
                      argv: &[String],
                      cwd: Option<&Path>,
                      extra: &[(String, String)],
                      stdin: Option<&[u8]>)
                      -> Result<i32, String> {
        let (output, shown) = self.spawn(argv, cwd, extra, stdin, false)?;
        match output.status.code() {
            Some(0) => Ok(0),
            Some(code) => {
                self.log(&format!("`{shown}` exited with code {code}; going on"));
                Ok(code)
            }
            None => Err(format!("`{shown}` was killed by a signal")),
        }
    }

    /// Runs `argv` with the build environment plus `extra`. Returns stdout
    /// without its trailing newline when `capture` is set.
    pub fn run(&self,
               argv: &[String],
               cwd: Option<&Path>,
               extra: &[(String, String)],
               capture: bool)
               -> Result<Option<String>, String> {
        self.run_fed(argv, cwd, extra, None, capture)
    }

    /// [`State::run`] with `stdin` as the program's input; with none, the
    /// program reads an empty input.
    pub fn run_fed(&self,
                   argv: &[String],
                   cwd: Option<&Path>,
                   extra: &[(String, String)],
                   stdin: Option<&[u8]>,
                   capture: bool)
                   -> Result<Option<String>, String> {
        let (output, shown) = self.spawn(argv, cwd, extra, stdin, capture)?;
        if !output.status.success() {
            return Err(match output.status.code() {
                Some(code) => format!("`{shown}` exited with code {code}"),
                None => format!("`{shown}` was killed by a signal"),
            });
        }
        if !capture {
            return Ok(None);
        }
        let mut stdout = String::from_utf8(output.stdout).map_err(|_| {
                                                             format!("`{shown}` printed output \
                                                                      that is not UTF-8")
                                                         })?;
        if stdout.ends_with('\n') {
            stdout.pop();
            if stdout.ends_with('\r') {
                stdout.pop();
            }
        }
        Ok(Some(stdout))
    }

    /// A path a plan may create: inside the prefix, `b.src` or `b.cache`
    /// (where sources are downloaded and unpacked, and a build may keep a
    /// separate build directory) when absolute, and not through a symbolic
    /// link that leads elsewhere. Relative paths are inside the prefix; `..`
    /// is resolved by the path's text.
    fn creatable(&self, path: &str, what: &str) -> Result<PathBuf, String> {
        let prefix = self.prefix()?;
        let src = self.src.borrow().clone();
        let full = lexical(&prefix.join(path)).ok_or_else(|| {
                                                  format!("{what}: {path} climbs above the root")
                                              })?;
        let Some(root) = [&prefix, &src, &self.cache].into_iter()
                                                     .find(|root| full.starts_with(root))
        else {
            return Err(format!("{what}: {} is outside the prefix ({}), the \
                                source directory ({}) and the cache ({})",
                               full.display(),
                               prefix.display(),
                               src.display(),
                               self.cache.display()));
        };
        if root.exists() {
            let mut existing = full.as_path();
            while !existing.exists() {
                existing = existing.parent().unwrap_or(root);
            }
            let inside = match (existing.canonicalize(), root.canonicalize()) {
                (Ok(existing), Ok(root)) => existing.starts_with(root),
                _ => false,
            };
            if !inside {
                return Err(format!("{what}: {} leads outside {} through a symbolic \
                                    link",
                                   full.display(),
                                   root.display()));
            }
        }
        if fs::symlink_metadata(&full).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("{what}: {} is a symbolic link", full.display()));
        }
        Ok(full)
    }

    /// A path a plan may read: inside the prefix, the source directory, the
    /// cache, the plan's directory or a declared dependency. Relative paths
    /// are from the directory programs run in.
    fn readable(&self, path: &str, what: &str) -> Result<PathBuf, String> {
        let full = lexical(&self.default_cwd().join(path)).ok_or_else(|| {
                                                              format!("{what}: {path} climbs \
                                                                       above the root")
                                                          })?;
        let mut roots = vec![self.src.borrow().clone(),
                             self.cache.clone(),
                             self.context.clone()];
        if let Ok(prefix) = self.prefix() {
            roots.push(prefix);
        }
        roots.extend(self.deps.iter().map(|dep| dep.path.clone()));
        if roots.iter().any(|root| full.starts_with(root)) {
            Ok(full)
        } else {
            Err(format!("{what}: {} is outside the prefix, the source \
                         directory, the cache, the plan's directory and \
                         the dependencies",
                        full.display()))
        }
    }

    /// A path a plan may remove or move away: where it may create, a
    /// symbolic link itself included.
    fn removable(&self, path: &str, what: &str) -> Result<PathBuf, String> {
        let full = Path::new(path);
        let (Some(parent), Some(name)) = (full.parent(), full.file_name()) else {
            return Err(format!("{what}: {path} names no file"));
        };
        let parent = match parent.to_str() {
            Some("") => ".".to_string(),
            Some(parent) => parent.to_string(),
            None => return Err(format!("{what}: {path} is not UTF-8")),
        };
        Ok(self.creatable(&parent, what)?.join(name))
    }

    /// Points each file's `#!` line at `interpreter` inside `dep`, whatever
    /// path it named before that ends in the interpreter's name.
    fn fix_interpreter(&self,
                       files: &[String],
                       dep: &str,
                       interpreter: &str)
                       -> Result<(), String> {
        let new = self.dep_path(dep)?.join(interpreter);
        if !new.is_file() {
            return Err(format!("b:fix_interpreter: {} is not a file", new.display()));
        }
        let new = text(&new)?;
        for file in files {
            let path = self.creatable(file, "b:fix_interpreter")?;
            let contents = fs::read(&path).map_err(|e| {
                                              format!("b:fix_interpreter: cannot read {}: {e}",
                                                      path.display())
                                          })?;
            let Some(rest) = contents.strip_prefix(b"#!") else {
                continue;
            };
            let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
            let Ok(line) = std::str::from_utf8(&rest[..end]) else {
                continue;
            };
            let program = line.split_whitespace().next().unwrap_or("");
            if !program.ends_with(interpreter) {
                continue;
            }
            let args = &line.trim_start()[program.len()..];
            let mut fixed = format!("#!{new}{args}").into_bytes();
            fixed.extend_from_slice(&rest[end..]);
            write_replacing(&path, &fixed).map_err(|e| {
                                               format!("b:fix_interpreter: cannot write {}: {e}",
                                                       path.display())
                                           })?;
            self.log(&format!("Interpreter of {} is {new}", path.display()));
        }
        Ok(())
    }

    /// The installed path of a declared dependency, named by its name,
    /// `origin/name` or the ident as declared.
    fn dep_path(&self, name: &str) -> Result<PathBuf, String> {
        let matches = |ident: &str| {
            let parts: Vec<&str> = ident.split('/').collect();
            ident == name
            || (parts.len() >= 2
                && (parts[1] == name || format!("{}/{}", parts[0], parts[1]) == name))
        };
        let mut found: Vec<&Dep> = self.deps.iter().filter(|dep| matches(&dep.ident)).collect();
        found.dedup_by(|a, b| a.path == b.path);
        match found.as_slice() {
            [dep] => Ok(dep.path.clone()),
            [] => Err(format!("b:path_for: `{name}` is not a declared dependency")),
            many => {
                let idents: Vec<&str> = many.iter().map(|dep| dep.ident.as_str()).collect();
                Err(format!("b:path_for: `{name}` matches {}; name one with \
                             its origin",
                            idents.join(", ")))
            }
        }
    }
}

/// A build ready to run its first plan code.
struct Started {
    state:     Rc<State>,
    b:         Table,
    /// The plan's metadata for the target, with its scaffolding's added.
    json:      Json,
    deps:      Resolved,
    runtime:   Inherited,
    buildtime: Inherited,
}

/// Where a build's files go, known once its version is.
struct Layout {
    ident:  String,
    prefix: PathBuf,
    /// `b.src`.
    src:    PathBuf,
    /// The directory under the cache the builder empties before unpacking.
    /// The shell builder emptied it even for plans without a source, and
    /// plans use it as scratch space.
    work:   PathBuf,
}

impl Layout {
    /// Lays out the build for `version` and tells `b` and the build's
    /// `PATH` about it.
    fn place(state: &State, b: &Table, options: &Options, version: &str) -> Result<Layout, Error> {
        let origin: String = b.raw_get("origin").map_err(|e| lua_error(&e))?;
        let ident = format!("{origin}/{}/{version}/{}", state.name, options.release);
        let prefix = package_path(&options.root.join("pkgs"), &ident);
        let work = match &state.settings.source {
            Some(Source { dirname: Some(dirname),
                          .. }) => state.cache.join(dirname),
            _ => state.cache.join(format!("{}-{version}", state.name)),
        };
        let src = if state.settings.source.is_some() {
            work.clone()
        } else {
            options.context.clone()
        };
        state.prefix.replace(Some(prefix.clone()));
        state.src.replace(src.clone());
        b.raw_set("version", version).map_err(|e| lua_error(&e))?;
        for (key, path) in [("prefix", &prefix), ("src", &src)] {
            b.raw_set(key, text(path).map_err(Error::Build)?)
             .map_err(|e| lua_error(&e))?;
        }
        let bin_dirs: Vec<PathBuf> = state.settings
                                          .bin_dirs
                                          .iter()
                                          .map(|dir| prefix.join(dir))
                                          .collect();
        if !bin_dirs.is_empty() {
            let mut env = state.env.borrow_mut();
            let path = env.get("PATH").unwrap_or_default().to_string();
            build_env::set_path(&mut env, &path, &bin_dirs, &Resolved::default(), SEPARATOR);
        }
        Ok(Layout { ident,
                    prefix,
                    src,
                    work })
    }
}

fn create_dir(dir: &Path) -> Result<(), Error> {
    fs::create_dir_all(dir).map_err(|e| {
                               Error::Build(format!("cannot create {}: {e}", dir.display()))
                           })
}

impl Plan {
    /// Builds the plan for `options.target`, resolving dependencies with
    /// `packages`.
    ///
    /// # Errors
    ///
    /// [`Error::Build`] names the step or phase that failed and why;
    /// [`Error::Compile`] when the scaffolding module does not compile.
    pub fn build(&self, options: &Options, packages: &dyn Resolve) -> Result<Built, Error> {
        let Started { state,
                      b,
                      json,
                      deps,
                      mut runtime,
                      mut buildtime, } = self.start(options, packages)?;
        let target = options.target.as_str();
        let version = match &self.metadata().to_json(Some(target))["version"] {
            Json::String(version) => version.clone(),
            _ => self.compute_version(target, &state, &b)?,
        };
        check_version(&version).map_err(Error::Build)?;
        let layout = Layout::place(&state, &b, options, &version)?;
        let prefix = layout.prefix.clone();
        state.log(&format!("Building {} for {target}", layout.ident));
        package::write_pre_build(options, &layout.ident).map_err(Error::Build)?;

        create_dir(&state.cache)?;
        self.run_phase(&state, &b, "download")?;
        self.run_phase(&state, &b, "verify")?;
        builtin::clean(&layout.work).map_err(Error::Build)?;
        self.run_phase(&state, &b, "unpack")?;
        create_dir(&layout.src)?;
        let lib_dirs: Vec<PathBuf> = state.settings
                                          .lib_dirs
                                          .iter()
                                          .map(|dir| prefix.join(dir))
                                          .collect();
        build_env::set_flags(&mut state.env.borrow_mut(),
                             &prefix,
                             &lib_dirs,
                             &deps,
                             SEPARATOR);
        if state.settings.source.is_some() {
            builtin::fix_libtool(&state, &layout.src).map_err(Error::Build)?;
        }
        package::check_hooks(&options.plan_dir).map_err(Error::Build)?;
        package::check_service(&options.plan_dir, &json["svc"]).map_err(Error::Build)?;
        self.run_phase(&state, &b, "prepare")?;
        self.run_phase(&state, &b, "build")?;
        if options.check {
            self.run_phase(&state, &b, "check")?;
        }
        let settings = &state.settings;
        for dir in std::iter::once(&String::new()).chain(&settings.bin_dirs)
                                                  .chain(&settings.lib_dirs)
                                                  .chain(&settings.other_dirs)
        {
            create_dir(&prefix.join(dir))?;
        }
        self.run_phase(&state, &b, "install")?;
        self.run_phase(&state, &b, "strip")?;
        if options.refuse_host_paths && state.os == "linux" {
            crate::host_paths::refuse(&prefix, &options.root).map_err(Error::Build)?;
        }
        state.log(&format!("Installed {} in {}", layout.ident, prefix.display()));
        for export in state.exports.borrow().iter() {
            let to = if export.runtime {
                &mut runtime
            } else {
                &mut buildtime
            };
            to.export(&export.name, &export.value, export.push, &layout.ident);
        }
        let artifact = Package { state: &state,
                                 options,
                                 json: &json,
                                 deps: &deps,
                                 ident: &layout.ident,
                                 prefix: &prefix,
                                 work: &layout.work,
                                 runtime,
                                 buildtime: &buildtime }.write()
                                                        .map_err(Error::Build)?;
        Ok(Built { ident: layout.ident,
                   prefix,
                   artifact })
    }

    /// Checks the target, resolves the dependencies, loads the scaffolding
    /// and builds `b`, all before any plan code runs.
    fn start(&self, options: &Options, packages: &dyn Resolve) -> Result<Started, Error> {
        let target = options.target.as_str();
        if !self.metadata().targets().iter().any(|t| t == target) {
            return Err(Error::Build(format!("the plan does not build for `{target}`")));
        }
        let os = target_os(target);
        let mut json = self.metadata().to_json(Some(target));
        if json["type"] == "composite" {
            return Err(Error::Build("composite plans are not supported".to_string()));
        }
        let name = json["name"].as_str().unwrap_or_default().to_string();
        let origin = json["origin"].as_str().unwrap_or_default().to_string();
        package::check_key(&options.keys, &origin).map_err(Error::Build)?;

        // `scaffolding` is a package ident or `{:package <ident> :settings <data>}`.
        let (scaffolding_ident, scaffolding_settings) = match &json["scaffolding"] {
            Json::String(ident) => (Some(ident.clone()), Json::Null),
            Json::Object(fields) => {
                (fields.get("package")
                       .and_then(Json::as_str)
                       .map(String::from),
                 fields.get("settings").cloned().unwrap_or(Json::Null))
            }
            _ => (None, Json::Null),
        };
        let scaffolding = match &scaffolding_ident {
            Some(ident) => {
                let path = packages.resolve(ident).map_err(Error::Build)?;
                Some(load_scaffolding(&self.lua, &path)?)
            }
            None => None,
        };
        if let Some(module) = &scaffolding {
            add_scaffolding_metadata(&self.lua,
                                     module,
                                     &mut json,
                                     &scaffolding_settings,
                                     &[("src", &options.context), ("plan", &options.plan_dir)])?;
        }
        let deps = Resolved::resolve(&strings(&json["deps"]),
                                     &strings(&json["build_deps"]),
                                     scaffolding_ident.as_deref(),
                                     &options.root.join("pkgs"),
                                     packages).map_err(Error::Build)?;
        let mut chains = BTreeMap::new();
        for phase in PHASES {
            let own = match self.value(target, phase).map_err(|e| lua_error(&e))? {
                Value::Function(function) => Some(function),
                _ => None,
            };
            let scaffolded = match &scaffolding {
                Some(module) => {
                    module.raw_get::<Option<Function>>(*phase)
                          .map_err(|e| lua_error(&e))?
                }
                None => None,
            };
            chains.insert(*phase, [own, scaffolded]);
        }

        let runtime = Inherited::from_deps(deps.deps.iter().map(|dep| dep.path.as_path()));
        let buildtime = Inherited::from_deps(deps.build_deps.iter().map(|dep| dep.path.as_path()));
        let mut env = Env::new(options.env.iter().cloned());
        build_env::layer(&mut env, &runtime, &buildtime);
        let initial_path = env.get("PATH").unwrap_or_default().to_string();
        build_env::set_path(&mut env, &initial_path, &[], &deps, SEPARATOR);
        let state = Rc::new(State { settings: settings(&json, &name)?,
                                    name,
                                    os,
                                    cache: options.root.join("cache").join("src"),
                                    context: options.context.clone(),
                                    env: RefCell::new(env),
                                    prefix: RefCell::new(None),
                                    src: RefCell::new(options.context.clone()),
                                    deps: deps.direct().cloned().collect(),
                                    phase: Cell::new("version"),
                                    chains,
                                    active: RefCell::new(Vec::new()),
                                    b: Cell::new(0),
                                    exports: RefCell::new(Vec::new()),
                                    builder_path: options.env
                                                         .iter()
                                                         .find(|(name, _)| name == "PATH")
                                                         .map(|(_, value)| value.clone()) });
        let b = context(&self.lua,
                        &state,
                        options,
                        &origin,
                        scaffolding,
                        &scaffolding_settings).map_err(|e| lua_error(&e))?;
        state.b.set(b.to_pointer() as usize);
        Ok(Started { state,
                     b,
                     json,
                     deps,
                     runtime,
                     buildtime })
    }

    /// The plan's value for `key` on `target`: its `on` entry's, else the
    /// base table's. Both are read raw.
    fn value(&self, target: &str, key: &str) -> mlua::Result<Value> {
        if let Value::Table(on) = self.table.raw_get::<Value>("on")?
           && let Value::Table(overrides) = on.raw_get::<Value>(target)?
        {
            let value = overrides.raw_get::<Value>(key)?;
            if !value.is_nil() {
                return Ok(value);
            }
        }
        self.table.raw_get(key)
    }

    fn compute_version(&self, target: &str, state: &State, b: &Table) -> Result<String, Error> {
        let failed = |e: &mlua::Error| {
            Error::Build(format!("computing the version failed: {}", message(e, Stage::Build)))
        };
        let Value::Function(function) = self.value(target, "version").map_err(|e| lua_error(&e))?
        else {
            return Err(Error::Build("the plan's `version` is neither a string \
                                     nor a function"
                                                    .to_string()));
        };
        state.phase.set("version");
        sandbox::limit(&self.lua,
                       sandbox::PLAN_MEMORY,
                       sandbox::PLAN_INSTRUCTIONS,
                       "the plan").map_err(|e| failed(&e))?;
        match function.call::<Value>(b).map_err(|e| failed(&e))? {
            Value::String(version) => {
                version.to_str().map(|v| v.to_string()).map_err(|_| {
                                                           Error::Build("the `version` function \
                                                                         returned text that is \
                                                                         not UTF-8"
                                                                                   .to_string())
                                                       })
            }
            other => {
                Err(Error::Build(format!("the `version` function returned a \
                                          value of type {}, not a string",
                                         other.type_name())))
            }
        }
    }

    fn run_phase(&self, state: &Rc<State>, b: &Table, phase: &'static str) -> Result<(), Error> {
        let failed = |e: &mlua::Error| {
            Error::Build(format!("the `{phase}` phase failed: {}", message(e, Stage::Build)))
        };
        state.phase.set(phase);
        sandbox::limit(&self.lua,
                       sandbox::PLAN_MEMORY,
                       sandbox::PLAN_INSTRUCTIONS,
                       "the plan").map_err(|e| failed(&e))?;
        call_level(state, b, phase, 0).map_err(|e| failed(&e))
    }
}

/// Runs the first function of `phase`'s chain at or below `from`: the
/// plan's (0), the scaffolding's (1), then the builtin.
fn call_level(state: &Rc<State>, b: &Table, phase: &'static str, from: usize) -> mlua::Result<()> {
    let chain = &state.chains[phase];
    for (level, function) in chain.iter().enumerate().skip(from) {
        if let Some(function) = function {
            state.active.borrow_mut().push((phase, level));
            let result = function.call::<()>(b);
            state.active.borrow_mut().pop();
            return result;
        }
    }
    builtin::run(state, phase).map_err(mlua::Error::runtime)
}

/// Compiles and runs a scaffolding package's module in an environment of
/// its own with the plan's allow list, and checks the table it returns.
fn load_scaffolding(lua: &Lua, package: &Path) -> Result<Table, Error> {
    let path = package.join(SCAFFOLDING_FILE);
    let name = path.display().to_string();
    let source = fs::read_to_string(&path).map_err(|e| Error::Read(path.clone(), e))?;
    let code =
        fennel::compile(&source, &name).map_err(|e| Error::Compile(message(&e, Stage::Compile)))?;
    let failed = |e: &mlua::Error| {
        Error::Build(format!("loading the scaffolding failed: {}",
                             message(e, Stage::Build)))
    };
    let env = sandbox::environment(lua).map_err(|e| failed(&e))?;
    sandbox::limit(lua,
                   sandbox::PLAN_MEMORY,
                   sandbox::PLAN_INSTRUCTIONS,
                   "the scaffolding").map_err(|e| failed(&e))?;
    let value: Value = lua.load(code)
                          .set_name(format!("@{name}"))
                          .set_environment(env)
                          .eval()
                          .map_err(|e| failed(&e))?;
    let Value::Table(module) = value else {
        return Err(Error::Build(format!("{name} returns a {}, not a table", value.type_name())));
    };
    if module.metatable().is_some() {
        return Err(Error::Build(format!("{name} returns a table with a metatable")));
    }
    for phase in PHASES {
        match module.raw_get::<Value>(*phase).map_err(|e| lua_error(&e))? {
            Value::Nil | Value::Function(_) => {}
            other => {
                return Err(Error::Build(format!("{name}: `{phase}` is a {}, not a \
                                                 function",
                                                other.type_name())));
            }
        }
    }
    Ok(module)
}

/// Calls the scaffolding module's `metadata` function, if it has one, with
/// the plan's metadata, the plan's settings for it and read-only views of
/// `dirs` (the build's source context and the plan's directory), and adds
/// what it returns to `json`.
fn add_scaffolding_metadata(lua: &Lua,
                            module: &Table,
                            json: &mut Json,
                            settings: &Json,
                            dirs: &[(&str, &Path)])
                            -> Result<(), Error> {
    let failed = |e: &mlua::Error| {
        Error::Build(format!("the scaffolding's metadata failed: {}",
                             message(e, Stage::Build)))
    };
    let Some(metadata) = module.raw_get::<Option<Function>>("metadata")
                               .map_err(|e| failed(&e))?
    else {
        return Ok(());
    };
    let plan = json_to_lua(lua, json).map_err(|e| failed(&e))?;
    let settings = json_to_lua(lua, settings).map_err(|e| failed(&e))?;
    let views = lua.create_table().map_err(|e| failed(&e))?;
    for (name, dir) in dirs {
        views.raw_set(*name, dir_view(lua, dir).map_err(|e| failed(&e))?)
             .map_err(|e| failed(&e))?;
    }
    sandbox::limit(lua,
                   sandbox::PLAN_MEMORY,
                   sandbox::PLAN_INSTRUCTIONS,
                   "the scaffolding").map_err(|e| failed(&e))?;
    let added = metadata.call::<Value>((plan, settings, views))
                        .map_err(|e| failed(&e))?;
    let added = schema::parse_contribution(&added)?;
    schema::merge_contribution(json, added)
}

/// A read-only view of `dir` for a scaffolding's `metadata`: `read`, `exists`
/// and `json` take a path inside it. `read` gives a file's bytes and `json`
/// its parsed JSON, nil when there is no such file.
fn dir_view(lua: &Lua, dir: &Path) -> mlua::Result<Table> {
    let inside = {
        let dir = dir.to_path_buf();
        move |path: String| -> mlua::Result<PathBuf> {
            let relative = Path::new(&path);
            if relative.is_absolute()
               || relative.components()
                          .any(|part| part == Component::ParentDir)
            {
                return Err(mlua::Error::runtime(format!("{path} is not a path \
                                                         inside the directory")));
            }
            Ok(dir.join(relative))
        }
    };
    let view = lua.create_table()?;
    let at = inside.clone();
    view.raw_set("exists",
                 lua.create_function(move |_, path: String| Ok(at(path)?.exists()))?)?;
    let at = inside.clone();
    view.raw_set("read",
                 lua.create_function(move |lua, path: String| {
                        match fs::read(at(path)?) {
                            Ok(bytes) => Ok(Value::String(lua.create_string(bytes)?)),
                            Err(_) => Ok(Value::Nil),
                        }
                    })?)?;
    view.raw_set("json",
                 lua.create_function(move |lua, path: String| {
                        let file = inside(path.clone())?;
                        let Ok(text) = fs::read_to_string(&file) else {
                            return Ok(Value::Nil);
                        };
                        let json: Json = serde_json::from_str(&text).map_err(|e| {
                                             mlua::Error::runtime(format!("{path} is not JSON: \
                                                                           {e}"))
                                         })?;
                        json_to_lua(lua, &json)
                    })?)?;
    Ok(view)
}

/// JSON as Lua values: objects and arrays become fresh tables, null is nil.
fn json_to_lua(lua: &Lua, json: &Json) -> mlua::Result<Value> {
    Ok(match json {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => {
            match n.as_i64() {
                Some(i) => Value::Integer(i),
                None => Value::Number(n.as_f64().unwrap_or_default()),
            }
        }
        Json::String(text) => Value::String(lua.create_string(text)?),
        Json::Array(items) => {
            let table = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                table.raw_set(i + 1, json_to_lua(lua, item)?)?;
            }
            Value::Table(table)
        }
        Json::Object(fields) => {
            let table = lua.create_table()?;
            for (key, item) in fields {
                table.raw_set(key.as_str(), json_to_lua(lua, item)?)?;
            }
            Value::Table(table)
        }
    })
}

/// Builds `b`. `version`, `prefix` and `src` are set once the version is
/// known.
fn context(lua: &Lua,
           state: &Rc<State>,
           options: &Options,
           origin: &str,
           scaffolding: Option<Table>,
           scaffolding_settings: &Json)
           -> mlua::Result<Table> {
    let b = lua.create_table()?;
    let target = options.target.as_str();
    b.raw_set("name", state.name.as_str())?;
    b.raw_set("origin", origin)?;
    b.raw_set("release", options.release.as_str())?;
    b.raw_set("target", target)?;
    b.raw_set("os", state.os)?;
    b.raw_set("arch", target.split('-').next().unwrap_or_default())?;
    b.raw_set("cache", text(&state.cache).map_err(mlua::Error::runtime)?)?;
    b.raw_set("plan_dir",
              text(&options.plan_dir).map_err(mlua::Error::runtime)?)?;
    b.raw_set("jobs", options.jobs)?;
    b.raw_set("scaffolding", scaffolding)?;
    // The plan's settings for its scaffolding, and its source, for
    // scaffoldings that work on the source themselves.
    b.raw_set("scaffolding_settings",
              json_to_lua(lua, scaffolding_settings)?)?;
    if let Some(source) = &state.settings.source {
        let table = lua.create_table()?;
        table.raw_set("url", source.url.as_str())?;
        table.raw_set("sha256", source.sha256.as_str())?;
        table.raw_set("filename", source.filename.as_str())?;
        if let Some(dirname) = &source.dirname {
            table.raw_set("dirname", dirname.as_str())?;
        }
        b.raw_set("source", table)?;
    }
    add_processes(lua, &b, state)?;
    add_files(lua, &b, state)?;
    add_file_reads(lua, &b, state)?;
    add_file_changes(lua, &b, state)?;
    add_file_helpers(lua, &b, state)?;
    add_exports(lua, &b, state)?;
    add_defaults(lua, &b, state)?;
    Ok(b)
}

/// `b:run`, `b:capture`, `b:getenv`, `b:env`, `b:unsetenv` and `b:env_append`.
fn add_processes(lua: &Lua, b: &Table, state: &Rc<State>) -> mlua::Result<()> {
    let s = Rc::clone(state);
    b.raw_set("run",
              lua.create_function(move |_, (this, argv, opts): (Value, Value, Value)| {
                     s.check_this(&this, "b:run")?;
                     let argv = texts(&argv, "b:run: argv")?;
                     let opts = run_options(&opts, "b:run", true)?;
                     let stdin = opts.stdin.as_deref();
                     if opts.check {
                         s.run_fed(&argv, opts.cwd.as_deref(), &opts.env, stdin, false)
                          .map_err(mlua::Error::runtime)?;
                         Ok(0)
                     } else {
                         s.run_status(&argv, opts.cwd.as_deref(), &opts.env, stdin)
                          .map_err(mlua::Error::runtime)
                     }
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("capture",
              lua.create_function(move |_, (this, argv, opts): (Value, Value, Value)| {
                     s.check_this(&this, "b:capture")?;
                     let argv = texts(&argv, "b:capture: argv")?;
                     let opts = run_options(&opts, "b:capture", false)?;
                     let stdin = opts.stdin.as_deref();
                     s.run_fed(&argv, opts.cwd.as_deref(), &opts.env, stdin, true)
                      .map(Option::unwrap_or_default)
                      .map_err(mlua::Error::runtime)
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("env",
              lua.create_function(move |_, (this, name, value): (Value, Value, Value)| {
                     s.check_this(&this, "b:env")?;
                     let name = env_name(&name, "b:env")?;
                     let value = text_of(&value, "b:env: the value")?;
                     s.env.borrow_mut().set(&name, value);
                     Ok(())
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("getenv",
              lua.create_function(move |_, (this, name): (Value, Value)| {
                     s.check_this(&this, "b:getenv")?;
                     let name = env_name(&name, "b:getenv")?;
                     Ok(s.env.borrow().get(&name).map(str::to_string))
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("unsetenv",
              lua.create_function(move |_, (this, name): (Value, Value)| {
                     s.check_this(&this, "b:unsetenv")?;
                     let name = env_name(&name, "b:unsetenv")?;
                     s.env.borrow_mut().remove(&name);
                     Ok(())
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("env_append",
              lua.create_function(move |_, (this, name, value): (Value, Value, Value)| {
                     s.check_this(&this, "b:env_append")?;
                     let name = env_name(&name, "b:env_append")?;
                     let value = text_of(&value, "b:env_append: the value")?;
                     s.env.borrow_mut().append(&name, &value, SEPARATOR);
                     Ok(())
                 })?)?;
    Ok(())
}

/// `b:path_for`, `b:join`, `b:glob`, `b:install`, `b:mkdir`, `b:write` and
/// `b:log`.
fn add_files(lua: &Lua, b: &Table, state: &Rc<State>) -> mlua::Result<()> {
    let s = Rc::clone(state);
    b.raw_set("path_for",
              lua.create_function(move |_, (this, dep, parts): (Value, Value, Variadic<Value>)| {
                     s.check_this(&this, "b:path_for")?;
                     let dep = text_of(&dep, "b:path_for: the dependency")?;
                     let path = s.dep_path(&dep).map_err(mlua::Error::runtime)?;
                     join(path, &parts, "b:path_for")
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("join",
              lua.create_function(move |_,
                                        (this, first, rest): (Value,
                                         Value,
                                         Variadic<Value>)| {
                                      s.check_this(&this, "b:join")?;
                                      join(PathBuf::from(text_of(&first,
                                                                 "b:join: the first part")?),
                                           &rest,
                                           "b:join")
                                  })?)?;

    let s = Rc::clone(state);
    b.raw_set("glob",
              lua.create_function(move |lua, (this, dir, pattern): (Value, Value, Value)| {
                     s.check_this(&this, "b:glob")?;
                     let dir = s.default_cwd()
                                .join(text_of(&dir, "b:glob: the directory")?);
                     let pattern = text_of(&pattern, "b:glob: the pattern")?;
                     lua.create_sequence_from(glob(&dir, &pattern).map_err(mlua::Error::runtime)?)
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("install",
              lua.create_function(move |_,
                                        (this, from, dest, opts): (Value,
                                         Value,
                                         Value,
                                         Value)| {
                                      s.check_this(&this, "b:install")?;
                                      let files = match &from {
                                          Value::Table(_) => texts(&from, "b:install: the files")?,
                                          _ => vec![text_of(&from, "b:install: the file")?],
                                      };
                                      let dest = text_of(&dest, "b:install: the destination")?;
                                      let mode = install_mode(&opts)?;
                                      s.install(&files, &dest, mode).map_err(mlua::Error::runtime)
                                  })?)?;

    let s = Rc::clone(state);
    b.raw_set("mkdir",
              lua.create_function(move |_, (this, path): (Value, Value)| {
                     s.check_this(&this, "b:mkdir")?;
                     let path = s.creatable(&text_of(&path, "b:mkdir: the path")?, "b:mkdir")
                                 .map_err(mlua::Error::runtime)?;
                     fs::create_dir_all(&path).map_err(|e| {
                                                  mlua::Error::runtime(format!("b:mkdir: cannot \
                                                                                create {}: {e}",
                                                                               path.display()))
                                              })
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("write",
              lua.create_function(move |_, (this, path, contents): (Value, Value, Value)| {
                     s.check_this(&this, "b:write")?;
                     let path = s.creatable(&text_of(&path, "b:write: the path")?, "b:write")
                                 .map_err(mlua::Error::runtime)?;
                     let contents = bytes_of(&contents, "b:write: the text")?;
                     let written = path.parent()
                                       .map_or(Ok(()), fs::create_dir_all)
                                       .and_then(|()| write_replacing(&path, &contents));
                     written.map_err(|e| {
                                mlua::Error::runtime(format!("b:write: cannot write {}: {e}",
                                                             path.display()))
                            })
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("log",
              lua.create_function(move |_, (this, line): (Value, Value)| {
                     s.check_this(&this, "b:log")?;
                     s.log(&text_of(&line, "b:log: the message")?);
                     Ok(())
                 })?)?;
    Ok(())
}

/// `b:read`, `b:exists` and `b:find`. Reads stay inside the prefix, the
/// source directory, the cache, the plan's directory and the declared
/// dependencies.
fn add_file_reads(lua: &Lua, b: &Table, state: &Rc<State>) -> mlua::Result<()> {
    let s = Rc::clone(state);
    b.raw_set("read",
              lua.create_function(move |lua, (this, path): (Value, Value)| {
                     s.check_this(&this, "b:read")?;
                     let path = s.readable(&text_of(&path, "b:read: the path")?, "b:read")
                                 .map_err(mlua::Error::runtime)?;
                     // A file's bytes as they are: a Lua string holds any.
                     let bytes = fs::read(&path).map_err(|e| {
                                                    mlua::Error::runtime(format!("b:read: cannot \
                                                                                  read {}: {e}",
                                                                                 path.display()))
                                                })?;
                     lua.create_string(bytes)
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("exists",
              lua.create_function(move |_, (this, path): (Value, Value)| {
                     s.check_this(&this, "b:exists")?;
                     let path = s.readable(&text_of(&path, "b:exists: the path")?, "b:exists")
                                 .map_err(mlua::Error::runtime)?;
                     Ok(fs::symlink_metadata(path).is_ok())
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("find",
              lua.create_function(move |lua, (this, dir, pattern): (Value, Value, Value)| {
                     s.check_this(&this, "b:find")?;
                     let dir = s.readable(&text_of(&dir, "b:find: the directory")?, "b:find")
                                .map_err(mlua::Error::runtime)?;
                     let pattern = text_of(&pattern, "b:find: the pattern")?;
                     let mut found = Vec::new();
                     find(&dir, &pattern, &mut found).map_err(mlua::Error::runtime)?;
                     found.sort();
                     lua.create_sequence_from(found)
                 })?)?;

    Ok(())
}

/// `b:copy`, `b:move`, `b:remove`, `b:symlink` and `b:chmod`: what a plan
/// does to files without a shell. Writes stay where `b:write` may write.
fn add_file_changes(lua: &Lua, b: &Table, state: &Rc<State>) -> mlua::Result<()> {
    let s = Rc::clone(state);
    b.raw_set("copy",
              lua.create_function(move |_, (this, from, to): (Value, Value, Value)| {
                     s.check_this(&this, "b:copy")?;
                     let from = s.readable(&text_of(&from, "b:copy: the source")?, "b:copy")
                                 .map_err(mlua::Error::runtime)?;
                     let to = s.creatable(&text_of(&to, "b:copy: the destination")?, "b:copy")
                               .map_err(mlua::Error::runtime)?;
                     copy_tree(&from, &to).map_err(mlua::Error::runtime)
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("move",
              lua.create_function(move |_, (this, from, to): (Value, Value, Value)| {
                     s.check_this(&this, "b:move")?;
                     let from = s.removable(&text_of(&from, "b:move: the source")?, "b:move")
                                 .map_err(mlua::Error::runtime)?;
                     let to = s.creatable(&text_of(&to, "b:move: the destination")?, "b:move")
                               .map_err(mlua::Error::runtime)?;
                     if let Some(parent) = to.parent() {
                         fs::create_dir_all(parent).map_err(|e| {
                             mlua::Error::runtime(format!("b:move: cannot create {}: {e}",
                                                          parent.display()))
                         })?;
                     }
                     fs::rename(&from, &to).map_err(|e| {
                                               mlua::Error::runtime(format!("b:move: cannot move \
                                                                             {} to {}: {e}",
                                                                            from.display(),
                                                                            to.display()))
                                           })
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("remove",
              lua.create_function(move |_, (this, path): (Value, Value)| {
                     s.check_this(&this, "b:remove")?;
                     let path = s.removable(&text_of(&path, "b:remove: the path")?, "b:remove")
                                 .map_err(mlua::Error::runtime)?;
                     remove_path(&path).map_err(mlua::Error::runtime)
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("symlink",
              lua.create_function(move |_, (this, target, link): (Value, Value, Value)| {
                     s.check_this(&this, "b:symlink")?;
                     let target = text_of(&target, "b:symlink: the target")?;
                     let link = s.creatable(&text_of(&link, "b:symlink: the link")?, "b:symlink")
                                 .map_err(mlua::Error::runtime)?;
                     make_symlink(&target, &link).map_err(mlua::Error::runtime)
                 })?)?;

    let s = Rc::clone(state);
    b.raw_set("chmod",
              lua.create_function(move |_, (this, path, mode): (Value, Value, Value)| {
                     s.check_this(&this, "b:chmod")?;
                     let path = s.creatable(&text_of(&path, "b:chmod: the path")?, "b:chmod")
                                 .map_err(mlua::Error::runtime)?;
                     let spec = text_of(&mode, "b:chmod: the mode")?;
                     let mode = if spec.bytes().all(|b| b.is_ascii_digit()) {
                         octal_mode(&spec, "b:chmod")?
                     } else {
                         symbolic_mode(&spec, &path).map_err(mlua::Error::runtime)?
                     };
                     set_mode(&path, Some(mode)).map_err(mlua::Error::runtime)
                 })?)?;

    Ok(())
}

/// `b:fix_interpreter` and `b:download`.
fn add_file_helpers(lua: &Lua, b: &Table, state: &Rc<State>) -> mlua::Result<()> {
    let s = Rc::clone(state);
    b.raw_set("fix_interpreter",
              lua.create_function(move |_,
                                        (this, files, dep, interpreter): (Value,
                                         Value,
                                         Value,
                                         Value)| {
                                      s.check_this(&this, "b:fix_interpreter")?;
                                      let files = match &files {
                                          Value::Table(_) => {
                                              texts(&files, "b:fix_interpreter: the files")?
                                          }
                                          _ => {
                                              vec![text_of(&files, "b:fix_interpreter: the file")?]
                                          }
                                      };
                                      let dep = text_of(&dep, "b:fix_interpreter: the dependency")?;
                                      let interpreter = text_of(&interpreter,
                                                                "b:fix_interpreter: the \
                                                                 interpreter")?;
                                      s.fix_interpreter(&files, &dep, &interpreter)
                                       .map_err(mlua::Error::runtime)
                                  })?)?;

    let s = Rc::clone(state);
    b.raw_set("download",
              lua.create_function(move |_,
                                        (this, url, sha256, filename): (Value,
                                         Value,
                                         Value,
                                         Value)| {
                                      s.check_this(&this, "b:download")?;
                                      let url = text_of(&url, "b:download: the URL")?;
                                      let sha256 = text_of(&sha256, "b:download: the SHA-256")?;
                                      let filename =
                                          text_of(&filename, "b:download: the file name")?;
                                      if filename.contains(['/', '\\']) || filename == ".." {
                                          return Err(mlua::Error::runtime(format!("b:download: \
                                                                                   `{filename}` \
                                                                                   is a path, \
                                                                                   not a file \
                                                                                   name")));
                                      }
                                      let file = builtin::fetch(&s, &url, &sha256, &filename)
                                                     .map_err(mlua::Error::runtime)?;
                                      let computed =
                                          builtin::sha256(&file).map_err(mlua::Error::runtime)?;
                                      if computed != sha256 {
                                          let _ = fs::remove_file(&file);
                                          return Err(mlua::Error::runtime(format!("b:download: \
                                                                                   {url} has \
                                                                                   SHA-256 \
                                                                                   {computed}, \
                                                                                   not {sha256}")));
                                      }
                                      text(&file).map_err(mlua::Error::runtime)
                                  })?)?;
    Ok(())
}

/// `b:runtime_env`, `b:runtime_env_push`, `b:buildtime_env` and
/// `b:buildtime_env_push`: what the plan passes on to the packages that
/// depend on it. Each takes effect in this build too, from when it is
/// called.
fn add_exports(lua: &Lua, b: &Table, state: &Rc<State>) -> mlua::Result<()> {
    for (method, runtime, push) in [("runtime_env", true, false),
                                    ("runtime_env_push", true, true),
                                    ("buildtime_env", false, false),
                                    ("buildtime_env_push", false, true)]
    {
        let s = Rc::clone(state);
        let what = format!("b:{method}");
        b.raw_set(method,
                  lua.create_function(move |_, (this, name, value): (Value, Value, Value)| {
                         s.check_this(&this, &what)?;
                         let name = env_name(&name, &what)?;
                         let value = text_of(&value, &format!("{what}: the value"))?;
                         let push = push.then(|| {
                                        build_env::aggregate_separator(&name).unwrap_or(SEPARATOR)
                                    });
                         if let Some(separator) = push {
                             s.env.borrow_mut().prepend(&name, &value, separator);
                         } else {
                             s.env.borrow_mut().set(&name, value.clone());
                         }
                         s.exports.borrow_mut().push(Export { runtime,
                                                              push,
                                                              name,
                                                              value });
                         Ok(())
                     })?)?;
    }
    Ok(())
}

/// `b.default`, one function per phase that runs the next level of that
/// phase's chain.
fn add_defaults(lua: &Lua, b: &Table, state: &Rc<State>) -> mlua::Result<()> {
    let default = lua.create_table()?;
    for phase in PHASES {
        let phase: &'static str = phase;
        let s = Rc::clone(state);
        let what = format!("b.default.{phase}");
        default.raw_set(phase,
                        lua.create_function(move |_, this: Value| {
                               if this.to_pointer() as usize != s.b.get() {
                                   return Err(mlua::Error::runtime(format!("pass b to {what}, \
                                                                            as ({what} b)")));
                               }
                               let Value::Table(b) = this else {
                                   unreachable!("only the b table has b's address");
                               };
                               let level = s.active
                                            .borrow()
                                            .iter()
                                            .rev()
                                            .find(|(p, _)| *p == phase)
                                            .map(|(_, level)| *level);
                               let Some(level) = level else {
                                   return Err(mlua::Error::runtime(format!("{what} can only \
                                                                            run during the \
                                                                            `{phase}` phase")));
                               };
                               call_level(&s, &b, phase, level + 1)
                           })?)?;
    }
    b.raw_set("default", default)
}

impl State {
    /// Refuses a call made as `b.run(...)` rather than `b:run(...)`, which
    /// would pass the first argument as `b`.
    fn check_this(&self, this: &Value, method: &str) -> mlua::Result<()> {
        if this.to_pointer() as usize == self.b.get() {
            Ok(())
        } else {
            let (_, name) = method.split_once(':').unwrap_or(("b", method));
            Err(mlua::Error::runtime(format!("call {method} with a colon, as (b:{name} ...)")))
        }
    }

    fn install(&self, files: &[String], dest: &str, mode: Option<u32>) -> Result<(), String> {
        let dest_path = Path::new(dest);
        if dest_path.components()
                    .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!("b:install: the destination `{dest}` must be a \
                                relative path inside the prefix"));
        }
        let dir = self.prefix()?.join(dest_path);
        fs::create_dir_all(&dir).map_err(|e| {
                                    format!("b:install: cannot create {}: {e}", dir.display())
                                })?;
        for file in files {
            let from = self.default_cwd().join(file);
            if !from.is_file() {
                return Err(format!("b:install: {} is not a file", from.display()));
            }
            let Some(file_name) = from.file_name() else {
                return Err(format!("b:install: {} has no file name", from.display()));
            };
            let to = dir.join(file_name);
            fs::copy(&from, &to).map_err(|e| {
                                    format!("b:install: cannot copy {} to {}: {e}",
                                            from.display(),
                                            to.display())
                                })?;
            set_mode(&to, mode)?;
            self.log(&format!("Installed {} to {}", from.display(), to.display()));
        }
        Ok(())
    }
}

fn set_mode(path: &Path, mode: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    match mode {
        Some(mode) => {
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| {
                                                                           format!("b:install: \
                                                                                    cannot set \
                                                                                    the mode of \
                                                                                    {}: {e}",
                                                                                   path.display())
                                                                       })
        }
        None => Ok(()),
    }
}

/// `path` with `..` resolved by its text, as the shell resolves `cd ..`;
/// none when `..` climbs above the root. A trailing `/.` or `/` stays, so
/// the path still names only a directory.
fn lexical(path: &Path) -> Option<PathBuf> {
    if !path.components().any(|part| part == Component::ParentDir) {
        return Some(path.to_path_buf());
    }
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() || out.as_os_str().is_empty() {
                    return None;
                }
            }
            other => out.push(other),
        }
    }
    let text = path.as_os_str().as_encoded_bytes();
    if text.ends_with(b"/.") {
        out.push(".");
    } else if text.ends_with(b"/") {
        out.push("");
    }
    Some(out)
}

/// Every file and directory under `dir` whose name matches `pattern`, at any
/// depth. Symbolic links are listed, not followed.
fn find(dir: &Path, pattern: &str, found: &mut Vec<String>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("b:find: cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("b:find: cannot read {}: {e}", dir.display()))?;
        let path = entry.path();
        if entry.file_name()
                .to_str()
                .is_some_and(|name| wildcard(pattern, name))
        {
            found.push(text(&path)?);
        }
        let kind = entry.file_type()
                        .map_err(|e| format!("b:find: cannot read {}: {e}", path.display()))?;
        if kind.is_dir() {
            find(&path, pattern, found)?;
        }
    }
    Ok(())
}

/// Copies a file, a symbolic link or a directory tree to exactly `to`,
/// keeping modes and links.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(from).map_err(|e| {
                                             format!("b:copy: cannot read {}: {e}", from.display())
                                         })?;
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| {
                                      format!("b:copy: cannot create {}: {e}", parent.display())
                                  })?;
    }
    if meta.file_type().is_symlink() {
        let target = fs::read_link(from).map_err(|e| {
                                            format!("b:copy: cannot read the link {}: {e}",
                                                    from.display())
                                        })?;
        return make_symlink(&target.to_string_lossy(), to);
    }
    if meta.is_dir() {
        fs::create_dir_all(to).map_err(|e| format!("b:copy: cannot create {}: {e}", to.display()))?;
        let entries =
            fs::read_dir(from).map_err(|e| format!("b:copy: cannot read {}: {e}", from.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("b:copy: cannot read {}: {e}", from.display()))?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        return fs::set_permissions(to, meta.permissions()).map_err(|e| {
                                                              format!("b:copy: cannot set the \
                                                                       mode of {}: {e}",
                                                                      to.display())
                                                          });
    }
    fs::copy(from, to).map(drop).map_err(|e| {
                                    format!("b:copy: cannot copy {} to {}: {e}",
                                            from.display(),
                                            to.display())
                                })
}

/// Removes a file, a symbolic link or a directory tree; one that is not
/// there is already removed.
fn remove_path(path: &Path) -> Result<(), String> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    let removed = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(|e| format!("b:remove: cannot remove {}: {e}", path.display()))
}

fn make_symlink(target: &str, link: &Path) -> Result<(), String> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).map_err(|e| {
                                      format!("b:symlink: cannot create {}: {e}", parent.display())
                                  })?;
    }
    std::os::unix::fs::symlink(target, link).map_err(|e| {
                                                format!("b:symlink: cannot link {} to {target}: \
                                                         {e}",
                                                        link.display())
                                            })
}

/// A symbolic mode like `chmod` takes (`+x`, `u+w,go-w`, `a=r`), applied to
/// `path`'s mode now. With no who, `+` and `-` act on everyone. `X` is
/// execute for a directory or a file someone may execute already.
fn symbolic_mode(spec: &str, path: &Path) -> Result<u32, String> {
    let meta =
        fs::metadata(path).map_err(|e| format!("b:chmod: cannot read {}: {e}", path.display()))?;
    let mut mode = mode_of(&meta);
    let executable = meta.is_dir() || mode & 0o111 != 0;
    let refuse =
        || format!("b:chmod: the mode is `{spec}`, not octal or symbolic like \"+x\" or \"u+w\"");
    for clause in spec.split(',') {
        let op_at = clause.find(['+', '-', '=']).ok_or_else(refuse)?;
        let (who, rest) = clause.split_at(op_at);
        let mut who_bits = 0;
        for c in who.chars() {
            who_bits |= match c {
                'u' => 0o4700,
                'g' => 0o2070,
                'o' => 0o1007,
                'a' => 0o7777,
                _ => return Err(refuse()),
            };
        }
        if who_bits == 0 {
            who_bits = 0o7777;
        }
        let mut chars = rest.chars();
        let op = chars.next().ok_or_else(refuse)?;
        let mut perm = 0;
        for c in chars {
            perm |= match c {
                'r' => 0o444,
                'w' => 0o222,
                'x' => 0o111,
                'X' if executable => 0o111,
                'X' => 0,
                's' => 0o6000,
                't' => 0o1000,
                _ => return Err(refuse()),
            };
        }
        let bits = perm & who_bits;
        mode = match op {
            '+' => mode | bits,
            '-' => mode & !bits,
            _ => (mode & !who_bits) | bits,
        };
    }
    Ok(mode & 0o7777)
}

fn mode_of(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode()
}

/// Octal permissions like `"755"`.
fn octal_mode(mode: &str, what: &str) -> mlua::Result<u32> {
    match u32::from_str_radix(mode, 8) {
        Ok(bits) if (3..=4).contains(&mode.len()) && bits <= 0o7777 => Ok(bits),
        _ => {
            Err(mlua::Error::runtime(format!("{what}: the mode is \
                                              `{mode}`, not octal \
                                              permissions like \"755\"")))
        }
    }
}

/// Reads `opts.mode`: octal digits in a string, like `"755"`.
fn install_mode(opts: &Value) -> mlua::Result<Option<u32>> {
    let opts = options(opts, "b:install", &["mode"])?;
    let Some(mode) = opts.get("mode") else {
        return Ok(None);
    };
    let mode = text_of(mode, "b:install: `mode`")?;
    octal_mode(&mode, "b:install").map(Some)
}

/// `b:run` and `b:capture` options.
struct RunOptions {
    /// The working directory, relative to the default one.
    cwd:   Option<PathBuf>,
    /// Environment variables set for this program only.
    env:   Vec<(String, String)>,
    /// Whether a nonzero exit stops the build: true unless the plan says false.
    check: bool,
    /// Bytes the program reads as its input.
    stdin: Option<Vec<u8>>,
}

/// Reads `b:run` and `b:capture` options: `cwd`, `env`, a table of names to
/// values, `stdin`, and for `b:run` only `check`.
fn run_options(opts: &Value, what: &str, may_tolerate: bool) -> mlua::Result<RunOptions> {
    let allowed: &[&str] = if may_tolerate {
        &["cwd", "env", "stdin", "check"]
    } else {
        &["cwd", "env", "stdin"]
    };
    let opts = options(opts, what, allowed)?;
    let check = match opts.get("check") {
        None => true,
        Some(Value::Boolean(check)) => *check,
        Some(_) => return Err(mlua::Error::runtime(format!("{what}: `check` is true or false"))),
    };
    let cwd = opts.get("cwd")
                  .map(|cwd| text_of(cwd, &format!("{what}: `cwd`")).map(PathBuf::from))
                  .transpose()?;
    let mut env = Vec::new();
    if let Some(vars) = opts.get("env") {
        for (name, value) in options(vars, &format!("{what}: `env`"), &[])? {
            let value = text_of(&value, &format!("{what}: `env.{name}`"))?;
            env.push((name, value));
        }
    }
    let stdin = opts.get("stdin")
                    .map(|stdin| bytes_of(stdin, &format!("{what}: `stdin`")))
                    .transpose()?;
    Ok(RunOptions { cwd,
                    env,
                    check,
                    stdin })
}

/// A table of string keys, read raw. `allowed` lists the keys it may have;
/// empty allows any. `nil` is an empty table.
fn options(value: &Value, what: &str, allowed: &[&str]) -> mlua::Result<BTreeMap<String, Value>> {
    let table = match value {
        Value::Nil => return Ok(BTreeMap::new()),
        Value::Table(table) if table.metatable().is_none() => table,
        Value::Table(_) => {
            return Err(mlua::Error::runtime(format!("{what}: the options table has a metatable")));
        }
        other => {
            return Err(mlua::Error::runtime(format!("{what}: the options are a \
                                                     {}, not a table",
                                                    other.type_name())));
        }
    };
    let mut out = BTreeMap::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key = text_of(&key, &format!("{what}: an option name"))?;
        if !allowed.is_empty() && !allowed.contains(&key.as_str()) {
            return Err(mlua::Error::runtime(format!("{what}: unknown option \
                                                     `{key}`; known options: {}",
                                                    allowed.join(", "))));
        }
        out.insert(key, value);
    }
    Ok(out)
}

fn env_name(value: &Value, what: &str) -> mlua::Result<String> {
    let name = text_of(value, &format!("{what}: the name"))?;
    if name.is_empty() || name.contains(['=', '\0']) {
        return Err(mlua::Error::runtime(format!("{what}: `{name}` is not a variable name")));
    }
    Ok(name)
}

/// A string, or an integer as its digits.
/// A string's bytes, whatever their encoding, or an integer's digits.
fn bytes_of(value: &Value, what: &str) -> mlua::Result<Vec<u8>> {
    match value {
        Value::String(bytes) => Ok(bytes.as_bytes().to_vec()),
        Value::Integer(i) => Ok(i.to_string().into_bytes()),
        other => {
            Err(mlua::Error::runtime(format!("{what} is a {}, not a \
                                              string",
                                             other.type_name())))
        }
    }
}

fn text_of(value: &Value, what: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => {
            text.to_str()
                .map(|text| text.to_string())
                .map_err(|_| mlua::Error::runtime(format!("{what} is not UTF-8")))
        }
        Value::Integer(i) => Ok(i.to_string()),
        other => {
            Err(mlua::Error::runtime(format!("{what} is a {}, not a \
                                              string",
                                             other.type_name())))
        }
    }
}

/// A list of strings or integers: keys 1 to n and nothing else.
fn texts(value: &Value, what: &str) -> mlua::Result<Vec<String>> {
    let Value::Table(table) = value else {
        return Err(mlua::Error::runtime(format!("{what} is a {}, not a list", value.type_name())));
    };
    let length = table.raw_len();
    if table.metatable().is_some() || table.pairs::<Value, Value>().count() != length {
        return Err(mlua::Error::runtime(format!("{what} is not a list")));
    }
    (1..=length).map(|i| text_of(&table.raw_get::<Value>(i)?, &format!("{what}[{i}]")))
                .collect()
}

/// Joins `parts` onto `base`, refusing absolute parts that would replace
/// it.
fn join(base: PathBuf, parts: &[Value], what: &str) -> mlua::Result<String> {
    let mut path = base;
    for part in parts {
        let part = text_of(part, &format!("{what}: a path part"))?;
        if matches!(Path::new(&part).components().next(),
                    Some(Component::Prefix(_) | Component::RootDir))
        {
            return Err(mlua::Error::runtime(format!("{what}: `{part}` is \
                                                     absolute; only the first \
                                                     part may be")));
        }
        path.push(part);
    }
    text(&path).map_err(mlua::Error::runtime)
}

/// The entries of `dir` whose names match `pattern`, sorted. `*` matches
/// any run of characters and `?` any one.
fn glob(dir: &Path, pattern: &str) -> Result<Vec<String>, String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("b:glob: cannot read {}: {e}", dir.display()))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("b:glob: cannot read {}: {e}", dir.display()))?;
        if let Some(name) = entry.file_name().to_str()
           && wildcard(pattern, name)
        {
            found.push(text(&entry.path())?);
        }
    }
    found.sort();
    Ok(found)
}

fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((star_p, star_n)) = star {
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, star_n + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

fn settings(json: &Json, name: &str) -> Result<Settings, Error> {
    let source = match &json["source"] {
        Json::Object(source) => {
            let url = source["url"].as_str().unwrap_or_default().to_string();
            let filename = match source.get("filename").and_then(Json::as_str) {
                Some(filename) => filename.to_string(),
                None => url.rsplit('/').next().unwrap_or_default().to_string(),
            };
            if filename.is_empty() || filename.contains(['/', '\\']) {
                return Err(Error::Build(format!("`source.url` {url} names no file; \
                                                 set `source.filename`")));
            }
            Some(Source { sha256: source["sha256"].as_str()
                                                  .unwrap_or_default()
                                                  .to_ascii_lowercase(),
                          dirname: source.get("dirname")
                                         .and_then(Json::as_str)
                                         .map(String::from),
                          url,
                          filename })
        }
        _ => None,
    };
    if let Some(Source { dirname: Some(dirname),
                         .. }) = &source
       && (dirname.starts_with(['/', '\\'])
           || dirname.contains(':')
           || dirname.split(['/', '\\'])
                     .any(|part| part == ".." || part.is_empty()))
    {
        return Err(Error::Build(format!("{name}: `source.dirname` \
                                         `{dirname}` must be a relative \
                                         path inside the cache")));
    }
    let mut other_dirs = strings(&json["include_dirs"]);
    other_dirs.extend(strings(&json["pconfig_dirs"]));
    Ok(Settings { source,
                  bin_dirs: strings(&json["bin_dirs"]),
                  lib_dirs: strings(&json["lib_dirs"]),
                  other_dirs })
}

fn strings(value: &Json) -> Vec<String> {
    value.as_array()
         .map(|items| {
             items.iter()
                  .filter_map(|item| item.as_str().map(String::from))
                  .collect()
         })
         .unwrap_or_default()
}

/// A version is part of the package's path and ident, so it may not hold
/// `/` or whitespace.
fn check_version(version: &str) -> Result<(), String> {
    if version.is_empty()
       || version.contains('/')
       || version.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(format!("the version `{version}` is empty or holds `/`, \
                            whitespace or control characters"));
    }
    Ok(())
}

fn text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(String::from)
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

fn show_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains(char::is_whitespace) {
                format!("{arg:?}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn lua_error(error: &mlua::Error) -> Error { Error::Build(message(error, Stage::Build)) }

/// `linux` or `darwin`.
#[must_use]
pub fn target_os(target: &str) -> &'static str {
    if target.ends_with("-darwin") {
        "darwin"
    } else {
        "linux"
    }
}

/// The target this machine builds, if it is one Dojo knows.
#[must_use]
pub fn host_target() -> Option<&'static str> {
    match (env::consts::ARCH, env::consts::OS) {
        ("x86_64", "linux") => Some("x86_64-linux"),
        ("aarch64", "linux") => Some("aarch64-linux"),
        ("x86_64", "macos") => Some("x86_64-darwin"),
        ("aarch64", "macos") => Some("aarch64-darwin"),
        _ => None,
    }
}

/// The current UTC time as a release, `YYYYMMDDhhmmss`.
fn release_now() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH)
                                   .map_or(0, |elapsed| elapsed.as_secs());
    release_at(seconds)
}

/// `seconds` after the Unix epoch as `YYYYMMDDhhmmss` in UTC, by Howard
/// Hinnant's `civil_from_days`.
fn release_at(seconds: u64) -> String {
    let days = seconds / 86_400;
    let time = seconds % 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}{:02}{:02}{:02}",
            time / 3_600,
            time % 3_600 / 60,
            time % 60)
}

/// Writes `contents` to `path`. A file there that cannot be written, as a
/// source archive's read-only files cannot, is replaced by a new one with the
/// same permissions, as `sed -i` does: the plan owns the tree it writes in.
fn write_replacing(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    match fs::write(path, contents) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            let Ok(meta) = fs::symlink_metadata(path) else {
                return Err(error);
            };
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let temporary = path.with_file_name(format!(".{name}.doj-write"));
            fs::write(&temporary, contents)?;
            fs::set_permissions(&temporary, meta.permissions())?;
            fs::rename(&temporary, path)
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_are_utc_timestamps() {
        assert_eq!(release_at(0), "19700101000000");
        assert_eq!(release_at(951_782_400), "20000229000000");
        assert_eq!(release_at(1_792_938_600), "20261025143000");
    }

    #[test]
    fn wildcards_match_whole_names() {
        assert!(wildcard("*.dll", "ssleay32.dll"));
        assert!(!wildcard("*.dll", "ssleay32.dll.manifest"));
        assert!(wildcard("lib?.so*", "libz.so.1"));
        assert!(wildcard("*", ""));
        assert!(!wildcard("a*b", "acb c"));
    }
}
