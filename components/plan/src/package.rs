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

//! Turns an installed build into a package: the plan's hooks and
//! configuration, the metadata files, MANIFEST, the signed FILES list and
//! the signed `.hart`, written as the shell builder wrote them.

use std::{fs,
          path::{Path,
                 PathBuf}};

use serde_json::Value as Json;

use crate::{build::{Options,
                    State},
            builtin,
            deps::Resolved,
            env::Inherited};

/// File names the builder leaves out of `config`, unless
/// `DOJ_CONFIG_EXCLUDE` lists others.
const CONFIG_EXCLUDE: &[&str] = &["*.sw?", "*~", "*.bak"];

/// The service user, and the group, when the plan sets none.
const SVC_USER: &str = "doj";

/// A signed package and where it was copied.
#[derive(Clone, Debug)]
pub struct Artifact {
    /// The `.hart` in the output directory.
    pub path:    PathBuf,
    pub sha256:  String,
    pub blake2b: String,
}

/// Fails unless a secret signing key for `origin` is in `keys`, before any
/// work is done.
pub fn check_key(keys: &Path, origin: &str) -> Result<(), String> {
    let found = fs::read_dir(keys).into_iter()
                                  .flatten()
                                  .flatten()
                                  .any(|entry| {
                                      let name = entry.file_name();
                                      let name = name.to_string_lossy();
                                      name.starts_with(&format!("{origin}-"))
                                      && name.ends_with(".sig.key")
                                  });
    if found {
        Ok(())
    } else {
        Err(format!("no signing key for origin `{origin}` in {}; \
                     generate one with `doj origin key generate \
                     {origin}`",
                    keys.display()))
    }
}

/// Fails if a hook file has Windows line endings, which break its `#!`
/// line.
pub fn check_hooks(plan_dir: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(plan_dir.join("hooks")) else {
        return Ok(());
    };
    let mut bad: Vec<String> =
        entries.flatten()
               .filter(|entry| fs::read(entry.path()).is_ok_and(|b| b.contains(&b'\r')))
               .map(|entry| entry.path().display().to_string())
               .collect();
    if bad.is_empty() {
        return Ok(());
    }
    bad.sort();
    Err(format!("these hooks have CR+LF line endings; convert \
                 them to LF: {}",
                bad.join(", ")))
}

/// Refuses a service the package could not start: `svc` set without `run`
/// in a plan with no run hook.
pub fn check_service(plan_dir: &Path, svc: &Json) -> Result<(), String> {
    if svc.is_object() && svc.get("run").is_none() && !plan_dir.join("hooks").join("run").is_file()
    {
        return Err("`svc` names no `run`, and the plan has no hooks/run to start the service"
                       .to_string());
    }
    Ok(())
}

/// Writes `pre_build.env` to the output directory so a build worker can
/// name a build that fails.
pub fn write_pre_build(options: &Options, ident: &str) -> Result<(), String> {
    let parts: Vec<&str> = ident.split('/').collect();
    let text = report(&[("pkg_origin", parts[0]),
                        ("pkg_name", parts[1]),
                        ("pkg_version", parts[2]),
                        ("pkg_release", parts[3]),
                        ("pkg_ident", ident)]);
    create_dir(&options.output)?;
    write(&options.output.join("pre_build.env"), &text)
}

/// One build's package, ready to be written once `strip` has run.
pub struct Package<'a> {
    pub state:     &'a State,
    pub options:   &'a Options,
    /// The plan's metadata for the target.
    pub json:      &'a Json,
    pub deps:      &'a Resolved,
    pub ident:     &'a str,
    pub prefix:    &'a Path,
    /// The build's directory in the source cache.
    pub work:      &'a Path,
    pub runtime:   Inherited,
    pub buildtime: &'a Inherited,
}

impl Package<'_> {
    /// Writes the package and its artifact.
    pub fn write(mut self) -> Result<Artifact, String> {
        self.copy_config()?;
        self.write_run()?;
        self.write_metadata()?;
        self.write_files()?;
        self.write_manifest()?;
        let hart = self.write_artifact()?;
        self.copy_outputs(&hart)
    }

    fn origin(&self) -> &str { self.ident.split('/').next().unwrap_or_default() }

    fn text(&self, key: &str) -> &str { self.json[key].as_str().unwrap_or_default() }

    fn strings(&self, key: &str) -> Vec<String> {
        self.json[key].as_array()
                      .into_iter()
                      .flatten()
                      .filter_map(|item| item.as_str().map(String::from))
                      .collect()
    }

    /// `dirs` under the prefix, as text.
    fn under_prefix(&self, dirs: &[String]) -> Vec<String> {
        dirs.iter()
            .map(|dir| self.prefix.join(dir).to_string_lossy().into_owned())
            .collect()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.options
            .env
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value.clone())
    }

    fn put(&self, file: &str, text: &str) -> Result<(), String> {
        write(&self.prefix.join(file), text)
    }

    /// Writes `file` as `lines` joined by `separator` with a final newline,
    /// unless there are none.
    fn put_joined(&self, file: &str, lines: &[String], separator: &str) -> Result<(), String> {
        if lines.is_empty() {
            return Ok(());
        }
        self.put(file, &format!("{}\n", lines.join(separator)))
    }

    /// Copies `config`, `hooks` and `default.toml` from the plan's directory.
    fn copy_config(&self) -> Result<(), String> {
        let plan_dir = &self.options.plan_dir;
        let exclude: Vec<String> = match self.env_var("DOJ_CONFIG_EXCLUDE") {
            Some(list) if !list.is_empty() => list.split(',').map(String::from).collect(),
            _ => CONFIG_EXCLUDE.iter().map(|s| (*s).to_string()).collect(),
        };
        let mut dirs = vec!["config"];
        if self.env_var("DOJ_FEAT_INSTALL_HOOK")
               .is_some_and(|v| !v.is_empty())
        {
            dirs.push("config_install");
        }
        dirs.push("hooks");
        for dir in dirs {
            let from = plan_dir.join(dir);
            if !from.is_dir() {
                continue;
            }
            if dir != "hooks" {
                self.state.log(&format!("Writing {dir}"));
            }
            let skip: &[String] = if dir == "hooks" { &[] } else { &exclude };
            copy_tree(&from, &self.prefix.join(dir), skip)?;
            set_mode(&self.prefix.join(dir), 0o755)?;
        }
        // The plan's defaults, then any the build installed (a scaffolding's,
        // for keys the plan does not set), as the shell builder appended them.
        let defaults = plan_dir.join("default.toml");
        let installed = self.prefix.join("default.toml");
        if defaults.is_file() {
            let own = fs::read_to_string(&defaults).map_err(|e| {
                                                       format!("cannot read {}: {e}",
                                                               defaults.display())
                                                   })?;
            let added = fs::read_to_string(&installed).unwrap_or_default();
            let joined = if added.is_empty() {
                own
            } else {
                format!("{own}\n{added}")
            };
            write(&installed, &joined)?;
        }
        Ok(())
    }

    fn svc(&self, key: &str) -> Option<&str> { self.json["svc"][key].as_str() }

    fn is_service(&self) -> bool {
        self.options.plan_dir.join("hooks").join("run").is_file() || self.svc("run").is_some()
    }

    fn svc_user(&self) -> &str { self.svc("user").unwrap_or(SVC_USER) }

    /// The group is `doj` unless set, even when the user is: the shell
    /// builder took its default from the user before the plan set one.
    fn svc_group(&self) -> &str { self.svc("group").unwrap_or(SVC_USER) }

    /// Writes the `run` script for `svc.run` unless the plan has a run hook.
    fn write_run(&self) -> Result<(), String> {
        let Some(run) = self.svc("run") else {
            return Ok(());
        };
        if self.options.plan_dir.join("hooks").join("run").is_file() {
            self.state.log("Using the run hook");
            return Ok(());
        }
        let svc_path = self.options.root.join("svc").join(&self.state.name);
        let (user, group) = (self.svc_user(), self.svc_group());
        self.state
            .log(&format!("Writing a run script to run {run} as {user}:{group}"));
        let script = format!("#!/bin/sh\nexport HOME={data}\ncd {svc}\n\nif [ \"$(whoami)\" = \
                              \"root\" ]; then\n  exec chpst \\\n    -U {user}:{group} \\\n    \
                              -u {user}:{group} \\\n    {run} 2>&1\nelse\n  exec {run} 2>&1\nfi\n",
                             data = svc_path.join("data").display(),
                             svc = svc_path.display());
        self.put("run", &script)
    }

    /// The idents of `paths` relative to the package directory, sorted.
    fn idents(&self, paths: &[PathBuf]) -> Vec<String> {
        let pkgs = self.options.root.join("pkgs");
        let mut idents: Vec<String> =
            paths.iter()
                 .map(|path| {
                     path.strip_prefix(&pkgs)
                         .unwrap_or(path)
                         .components()
                         .map(|c| c.as_os_str().to_string_lossy().into_owned())
                         .collect::<Vec<_>>()
                         .join("/")
                 })
                 .collect();
        idents.sort();
        idents
    }

    /// The package's bin directories, then each run dependency's `PATH`
    /// entries that lie inside it, direct dependencies first.
    fn runtime_path(&self) -> Vec<String> {
        let mut paths = self.under_prefix(&self.strings("bin_dirs"));
        for dep in &self.deps.tdeps {
            let Ok(data) = fs::read_to_string(dep.join("PATH")) else {
                continue;
            };
            let prefix = dep.to_string_lossy();
            for entry in data.trim().split(':') {
                if entry.starts_with(prefix.as_ref()) && !paths.iter().any(|p| p == entry) {
                    paths.push(entry.to_string());
                }
            }
        }
        paths
    }

    fn write_metadata(&mut self) -> Result<(), String> {
        self.state.log("Building package metadata");
        let lib_dirs = self.under_prefix(&self.strings("lib_dirs"));
        let include_dirs = self.under_prefix(&self.strings("include_dirs"));
        let bin_dirs = self.under_prefix(&self.strings("bin_dirs"));

        self.put_joined("LD_RUN_PATH", &lib_dirs, ":")?;
        let ldflags: Vec<String> = lib_dirs.iter().map(|dir| format!("-L{dir}")).collect();
        self.put_joined("LDFLAGS", &ldflags, " ")?;
        let includes: Vec<String> = include_dirs.iter().map(|dir| format!("-I{dir}")).collect();
        for file in ["CFLAGS", "CPPFLAGS", "CXXFLAGS"] {
            self.put_joined(file, &includes, " ")?;
        }
        let pconfig = self.strings("pconfig_dirs");
        let pconfig = if pconfig.is_empty() {
            ["lib/pkgconfig", "share/pkgconfig"].iter()
                                                .filter(|dir| self.prefix.join(dir).is_dir())
                                                .map(|dir| (*dir).to_string())
                                                .collect()
        } else {
            pconfig
        };
        self.put_joined("PKG_CONFIG_PATH", &self.under_prefix(&pconfig), ":")?;

        self.buildtime.write(self.prefix, "BUILDTIME_ENVIRONMENT")?;
        self.put_joined("PATH", &bin_dirs, ":")?;
        self.write_map("EXPORTS", "exports")?;
        self.write_map("BINDS", "binds")?;
        self.write_map("BINDS_OPTIONAL", "binds_optional")?;
        self.write_exposes()?;
        let interpreters = self.under_prefix(&self.strings("interpreters"));
        self.put_joined("INTERPRETERS", &interpreters, "\n")?;
        for (file, paths) in [("BUILD_DEPS",
                               self.deps
                                   .build_deps
                                   .iter()
                                   .map(|d| d.path.clone())
                                   .collect()),
                              ("BUILD_TDEPS", self.deps.build_tdeps.clone()),
                              ("DEPS", self.deps.deps.iter().map(|d| d.path.clone()).collect()),
                              ("TDEPS", self.deps.tdeps.clone())]
        {
            self.put_joined(file, &self.idents(&paths), "\n")?;
        }
        self.put("TARGET", &format!("{}\n", self.options.target))?;
        self.put("IDENT", &format!("{}\n", self.ident))?;

        let runtime_path = self.runtime_path();
        if !runtime_path.is_empty() {
            let joined = runtime_path.join(":");
            self.put("RUNTIME_PATH", &format!("{joined}\n"))?;
            // Supervisors from before RUNTIME_PATH read `PATH` here.
            self.runtime.vars.insert("PATH".to_string(), joined);
        }
        self.runtime.write(self.prefix, "RUNTIME_ENVIRONMENT")?;

        if self.is_service() {
            self.put("SVC_USER", &format!("{}\n", self.svc_user()))?;
            self.put("SVC_GROUP", &format!("{}\n", self.svc_group()))?;
        }
        Ok(())
    }

    /// Writes a table of the plan's as `key=value` lines sorted by key, a
    /// list value joined by spaces.
    fn write_map(&self, file: &str, key: &str) -> Result<(), String> {
        let Some(map) = self.json[key].as_object() else {
            return Ok(());
        };
        let mut lines: Vec<String> =
            map.iter()
               .map(|(name, value)| {
                   let value = match value {
                       Json::Array(items) => {
                           items.iter()
                                .filter_map(Json::as_str)
                                .collect::<Vec<_>>()
                                .join(" ")
                       }
                       other => other.as_str().unwrap_or_default().to_string(),
                   };
                   format!("{name}={value}")
               })
               .collect();
        lines.sort();
        self.put_joined(file, &lines, "\n")
    }

    /// Writes the ports `exposes` names, read from `default.toml` at the
    /// keys `exports` maps them to.
    fn write_exposes(&self) -> Result<(), String> {
        let exposes = self.strings("exposes");
        if exposes.is_empty() {
            return Ok(());
        }
        let file = self.prefix.join("default.toml");
        let text = fs::read_to_string(&file).map_err(|e| {
                                                format!("`exposes` needs {}: {e}", file.display())
                                            })?;
        let config: toml::Table = text.parse()
                                      .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let mut ports = Vec::new();
        for name in exposes {
            let key = self.json["exports"][&name].as_str().unwrap_or_default();
            let port = lookup(&config, key).and_then(port_of).ok_or_else(|| {
                                                                  format!("`exposes` names \
                                                                           `{name}`, but `{key}` \
                                                                           in {} is not a TCP or \
                                                                           UDP port",
                                                                          file.display())
                                                              })?;
            ports.push(port.to_string());
        }
        self.put_joined("EXPOSES", &ports, " ")
    }

    /// Hashes every file in the package and signs the list as FILES.
    fn write_files(&self) -> Result<(), String> {
        self.state
            .log("Generating blake2b hashes of all files in the package");
        let mut files = Vec::new();
        builtin::walk(self.prefix, &mut files, false)?;
        files.sort();
        let mut sums = String::new();
        for file in &files {
            sums = sums + &builtin::blake2b(file)? + "  " + &file.to_string_lossy() + "\n";
        }
        let list = self.work.join(format!("{}_blake2bsums", self.state.name));
        write(&list, &sums)?;
        self.state.log("Generating signed metadata FILES");
        self.doj_sign(&list, &self.prefix.join("FILES"))
    }

    fn doj_sign(&self, from: &Path, to: &Path) -> Result<(), String> {
        let argv = [self.doj(),
                    "pkg".into(),
                    "sign".into(),
                    "--origin".into(),
                    self.origin().to_string(),
                    from.to_string_lossy().into_owned(),
                    to.to_string_lossy().into_owned()];
        self.state
            .run(&argv, Some(self.work), &self.doj_env(), false)
            .map(drop)
    }

    fn doj(&self) -> String { self.options.doj.to_string_lossy().into_owned() }

    fn doj_env(&self) -> Vec<(String, String)> {
        vec![("DOJ_CACHE_KEY_PATH".to_string(), self.options.keys.to_string_lossy().into_owned())]
    }

    fn write_manifest(&self) -> Result<(), String> {
        self.state.log("Creating manifest");
        let or = |value: String, otherwise: &str| {
            if value.is_empty() {
                otherwise.to_string()
            } else {
                value
            }
        };
        let quoted = |items: Vec<String>| {
            if items.is_empty() {
                String::new()
            } else {
                format!("`{}`", spaced(&items))
            }
        };
        let upstream = self.text("upstream_url");
        let upstream = if upstream.is_empty() {
            String::new()
        } else {
            format!("[{upstream}]({upstream})")
        };
        let source = self.json["source"]["url"].as_str().unwrap_or_default();
        let source = if source.is_empty() {
            String::new()
        } else {
            format!("[{source}]({source})")
        };
        let sha = self.json["source"]["sha256"].as_str().unwrap_or_default();
        let sha = if sha.is_empty() {
            String::new()
        } else {
            format!("`{sha}`")
        };
        let build_deps = self.deps
                             .build_deps
                             .iter()
                             .map(|dep| dep.ident.clone())
                             .collect();
        let env = self.state.env.borrow();
        let flag = |name: &str| {
            or(env.get(name).unwrap_or_default().to_string(),
               &format!("no {name}"))
        };
        let plan = fs::read_to_string(self.options.plan_dir.join(crate::PLAN_FILE)).map_err(|e| {
                                                                                        format!("cannot read the plan: {e}")
                                                                                    })?;
        let manifest =
            format!("# {origin} / {name}\n{description}\n\n* __Maintainer__: {maintainer}\n* \
                     __Version__: {version}\n* __Release__: {release}\n* __Target__: {target}\n* \
                     __Upstream URL__: {upstream}\n* __License__: {license}\n* __Source__: \
                     {source}\n* __SHA__: {sha}\n* __Path__: `{prefix}`\n* __Build \
                     Dependencies__: {build_deps}\n* __Dependencies__: {deps}\n* \
                     __Interpreters__: {interpreters}\n\n# Plan\n\n## Build \
                     Flags\n\n```bash\nCFLAGS: {cflags}\nCPPFLAGS: {cppflags}\nCXXFLAGS: \
                     {cxxflags}\nLDFLAGS: {ldflags}\nLD_RUN_PATH: {ld_run_path}\n```\n\n## Plan \
                     Source\n\n```fennel\n{plan}\n```\n",
                    origin = self.origin(),
                    name = self.state.name,
                    description = self.text("description"),
                    maintainer = self.text("maintainer"),
                    version = self.ident.split('/').nth(2).unwrap_or_default(),
                    release = self.options.release,
                    target = self.options.target,
                    upstream = or(upstream,
                                  "upstream project's website or home page is not defined"),
                    license = spaced(&self.strings("license")),
                    source = or(source, "source URL not provided or required"),
                    sha = or(sha, "SHA256 checksum not provided or required"),
                    prefix = self.prefix.display(),
                    build_deps = or(quoted(build_deps), "no build dependencies or undefined"),
                    deps = or(quoted(self.strings("deps")),
                              "no runtime dependencies or undefined"),
                    interpreters = or(quoted(self.strings("interpreters")),
                                      "no interpreters or undefined"),
                    cflags = flag("CFLAGS"),
                    cppflags = flag("CPPFLAGS"),
                    cxxflags = flag("CXXFLAGS"),
                    ldflags = flag("LDFLAGS"),
                    ld_run_path = flag("LD_RUN_PATH"),
                    plan = plan.trim_end_matches('\n'));
        self.put("MANIFEST", &manifest)
    }

    /// Writes the package as a signed, xz-compressed tar under
    /// `cache/artifacts`.
    fn write_artifact(&self) -> Result<PathBuf, String> {
        self.state.log("Generating package artifact");
        let artifacts = self.options.root.join("cache").join("artifacts");
        create_dir(&artifacts)?;
        let base = self.ident.replace('/', "-");
        let base = format!("{base}-{}", self.options.target);
        let hart = artifacts.join(format!("{base}.hart"));
        let tar = artifacts.join(format!(".{base}.tar"));
        let xz = artifacts.join(format!(".{base}.tar.xz"));
        for file in [&tar, &xz, &hart] {
            remove_file(file)?;
        }
        let level = self.env_var("DOJ_HART_COMPRESSION_LEVEL")
                        .unwrap_or_else(|| "6".to_string());
        let text = |path: &Path| path.to_string_lossy().into_owned();
        // The builder's own tar and xz, from the PATH it was started with: a
        // dependency's tools (busybox's xz, which only decompresses) are the
        // build's, not the packager's.
        let builder_path: Vec<(String, String)> = self.options
                                                      .env
                                                      .iter()
                                                      .filter(|(name, _)| name == "PATH")
                                                      .cloned()
                                                      .collect();
        self.state
            .run(&["tar".into(), "-cf".into(), text(&tar), text(self.prefix)],
                 Some(self.work),
                 &builder_path,
                 false)?;
        self.state.run(&["xz".into(),
                          "--compress".into(),
                          format!("-{level}"),
                          "--threads=0".into(),
                          text(&tar)],
                        Some(self.work),
                        &builder_path,
                        false)?;
        self.doj_sign(&xz, &hart)?;
        remove_file(&tar)?;
        remove_file(&xz)?;
        Ok(hart)
    }

    /// Copies the artifact to the output directory and writes
    /// `last_build.env` there in place of `pre_build.env`.
    fn copy_outputs(&self, hart: &Path) -> Result<Artifact, String> {
        let output = &self.options.output;
        create_dir(output)?;
        let file_name = hart.file_name().unwrap_or_default();
        let copied = output.join(file_name);
        copy_file(hart, &copied)?;
        let sha256 = builtin::sha256(hart)?;
        let blake2b = builtin::blake2b(hart)?;
        remove_file(&output.join("pre_build.env"))?;
        let parts: Vec<&str> = self.ident.split('/').collect();
        let artifact = file_name.to_string_lossy();
        let report = report(&[("pkg_origin", parts[0]),
                              ("pkg_name", parts[1]),
                              ("pkg_version", parts[2]),
                              ("pkg_release", parts[3]),
                              ("pkg_target", &self.options.target),
                              ("pkg_ident", self.ident),
                              ("pkg_artifact", &artifact),
                              ("pkg_sha256sum", &sha256),
                              ("pkg_blake2bsum", &blake2b)]);
        write(&output.join("last_build.env"), &report)?;
        self.state.log(&format!("Artifact: {}", copied.display()));
        self.state.log(&format!("SHA256 Checksum: {sha256}"));
        self.state.log(&format!("Blake2b Checksum: {blake2b}"));
        Ok(Artifact { path: copied,
                      sha256,
                      blake2b })
    }
}

/// A build report: one `name=value` line per pair.
fn report(pairs: &[(&str, &str)]) -> String {
    pairs.iter().fold(String::new(), |text, (name, value)| {
                    text + name + "=" + value + "\n"
                })
}

/// Each item followed by a space, as the shell builder printed lists.
fn spaced(items: &[String]) -> String {
    items.iter()
         .fold(String::new(), |text, item| text + item + " ")
}

/// The value at a dotted `key` in `table`.
fn lookup<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let mut parts = key.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        value = value.as_table()?.get(part)?;
    }
    Some(value)
}

fn port_of(value: &toml::Value) -> Option<u16> {
    let port = match value {
        toml::Value::Integer(port) => *port,
        toml::Value::String(text) => text.trim().parse().ok()?,
        _ => return None,
    };
    u16::try_from(port).ok().filter(|port| *port > 0)
}

/// Whether `name` matches the shell pattern `pattern`, which may use `*`
/// and `?`.
fn matches(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.split_first(), name.split_first()) {
        (None, None) => true,
        (Some((b'*', rest)), _) => {
            matches(rest, name) || name.split_first().is_some_and(|(_, n)| matches(pattern, n))
        }
        (Some((b'?', rest)), Some((_, n))) => matches(rest, n),
        (Some((p, rest)), Some((c, n))) => p == c && matches(rest, n),
        _ => false,
    }
}

/// Copies the tree at `from` to `to`, leaving out files whose names match
/// one of `skip`.
fn copy_tree(from: &Path, to: &Path, skip: &[String]) -> Result<(), String> {
    create_dir(to)?;
    let entries = fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", from.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let target = to.join(&name);
        if path.is_dir() {
            copy_tree(&path, &target, skip)?;
        } else if !skip.iter()
                       .any(|pattern| matches(pattern.as_bytes(), name.as_encoded_bytes()))
        {
            copy_file(&path, &target)?;
        }
    }
    Ok(())
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map(drop).map_err(|e| {
                                    format!("cannot copy {} to {}: {e}",
                                            from.display(),
                                            to.display())
                                })
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn create_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

fn remove_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("cannot remove {}: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| {
                                                                   format!("cannot set the mode \
                                                                            of {}: {e}",
                                                                           path.display())
                                                               })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_patterns_match_whole_names() {
        assert!(matches(b"*.sw?", b"a.swp"));
        assert!(matches(b"*~", b"default.toml~"));
        assert!(!matches(b"*.bak", b"a.bak.toml"));
        assert!(!matches(b"*.sw?", b"a.sw"));
    }

    #[test]
    fn ports_are_found_at_dotted_keys() {
        let config: toml::Table = "port = 80\n[http]\nport = \"8080\"\nname = \"x\"\n".parse()
                                                                                      .unwrap();
        assert_eq!(lookup(&config, "port").and_then(port_of), Some(80));
        assert_eq!(lookup(&config, "http.port").and_then(port_of), Some(8080));
        assert_eq!(lookup(&config, "http.name").and_then(port_of), None);
        assert_eq!(lookup(&config, "missing"), None);
    }
}
