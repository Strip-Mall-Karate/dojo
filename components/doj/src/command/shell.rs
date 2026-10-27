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

//! A project's dev shell. `shell.toml` names the packages it wants; `doj shell lock` pins each
//! to one release per target in `shell.lock`, with the checksum of every archive those releases
//! and their dependencies are installed from; `doj shell env` installs what the lock pins and
//! prints its environment for a shell to `eval`.
//!
//! Releases resolve from the archive cache, where an archive's checksum can be taken, and the
//! lock pins only the target `doj` runs on, leaving the other targets' pins as they are.

use std::{collections::BTreeMap,
          fs,
          io,
          path::{Path,
                 PathBuf},
          str::FromStr};

use crate::{common::{command::package::install::{self,
                                                 InstallHookMode,
                                                 InstallMode,
                                                 InstallSource,
                                                 LocalPackageUsage},
                     ui::UI},
            hcore::{fs::cache_artifact_path,
                    package::{Identifiable,
                              PackageArchive,
                              PackageIdent,
                              PackageInstall,
                              PackageTarget},
                    ChannelIdent},
            PRODUCT,
            VERSION};
use termcolor::ColorChoice;

use super::pkg::env::render_overlay;
use crate::error::{Error,
                   Result};

pub const DECLARATION: &str = "shell.toml";
pub const LOCK: &str = "shell.lock";
const LOCK_VERSION: u32 = 1;
const LOCK_HEADER: &str =
    "# Written by `doj shell lock` from shell.toml: the release each package \
                           resolved to, per target, and\n# the BLAKE2b checksum of every archive \
                           they and their dependencies install from.\n\n";

/// What `shell.toml` declares: packages for every target, packages for one target, and variables
/// the shell sets over the packages' own.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    #[serde(default)]
    packages: Vec<String>,
    #[serde(default)]
    env:      BTreeMap<String, String>,
    #[serde(default)]
    target:   BTreeMap<String, TargetDeclaration>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetDeclaration {
    #[serde(default)]
    packages: Vec<String>,
}

impl Declaration {
    fn load(dir: &Path) -> Result<Declaration> {
        let path = dir.join(DECLARATION);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(Error::ShellError(format!("no {} in {}: the directory declares no \
                                                      Dojo shell",
                                                     DECLARATION,
                                                     dir.display())))
            }
            Err(e) => {
                return Err(Error::ShellError(format!("cannot read {}: {}",
                                                     path.display(),
                                                     e)))
            }
        };
        let declaration: Declaration = toml::from_str(&text).map_err(|e| {
                                           Error::ShellError(format!("{}: {}", path.display(), e))
                                       })?;
        for name in declaration.target.keys() {
            PackageTarget::from_str(name).map_err(|_| {
                                             Error::ShellError(format!("{}: `{}' is not a \
                                                                        package target",
                                                                       path.display(),
                                                                       name))
                                         })?;
        }
        Ok(declaration)
    }

    /// The packages `target` gets: those for every target, then its own.
    fn packages_for(&self, target: PackageTarget) -> Result<Vec<PackageIdent>> {
        let own = self.target
                      .get(&target.to_string())
                      .map_or(&[][..], |t| &t.packages[..]);
        self.packages
            .iter()
            .chain(own)
            .map(|ident| {
                PackageIdent::from_str(ident).map_err(|e| {
                                                 Error::ShellError(format!("{}: {}",
                                                                           DECLARATION, e))
                                             })
            })
            .collect()
    }
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Lock {
    version: u32,
    #[serde(default)]
    targets: BTreeMap<String, TargetLock>,
}

/// One target's pins.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct TargetLock {
    /// The release each declared package resolved to, in the order declared.
    packages:  Vec<String>,
    /// The checksum of each archive those releases and their dependencies install from.
    checksums: BTreeMap<String, String>,
}

impl Lock {
    fn load(dir: &Path) -> Result<Option<Lock>> {
        let path = dir.join(LOCK);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(Error::ShellError(format!("cannot read {}: {}",
                                                     path.display(),
                                                     e)))
            }
        };
        let lock: Lock =
            toml::from_str(&text).map_err(|e| {
                                     Error::ShellError(format!("{}: {}", path.display(), e))
                                 })?;
        if lock.version != LOCK_VERSION {
            return Err(Error::ShellError(format!("{} is version {}; this doj reads version {}",
                                                 path.display(),
                                                 lock.version,
                                                 LOCK_VERSION)));
        }
        Ok(Some(lock))
    }

    fn to_text(&self) -> Result<String> {
        let body = toml::to_string_pretty(self).map_err(|e| {
                                                   Error::ShellError(format!("cannot write {}: \
                                                                              {}",
                                                                             LOCK, e))
                                               })?;
        Ok(format!("{}{}", LOCK_HEADER, body))
    }

    /// Writes the lock beside the declaration, replacing the old one whole.
    fn write(&self, dir: &Path) -> Result<()> {
        let path = dir.join(LOCK);
        let partial = dir.join(format!(".{}.partial", LOCK));
        fs::write(&partial, self.to_text()?)?;
        fs::rename(&partial, &path)?;
        Ok(())
    }
}

impl TargetLock {
    fn packages(&self) -> Result<Vec<PackageIdent>> {
        self.packages
            .iter()
            .map(|ident| pinned_ident(ident))
            .collect()
    }

    /// The release already pinned for `wanted`, if one satisfies it.
    fn pinned(&self, wanted: &PackageIdent) -> Result<Option<PackageIdent>> {
        Ok(self.packages()?
               .into_iter()
               .find(|ident| ident.satisfies(wanted)))
    }
}

fn pinned_ident(ident: &str) -> Result<PackageIdent> {
    let parsed =
        PackageIdent::from_str(ident).map_err(|e| Error::ShellError(format!("{}: {}", LOCK, e)))?;
    if !parsed.fully_qualified() {
        return Err(Error::ShellError(format!("{}: `{}' is not one release", LOCK, ident)));
    }
    Ok(parsed)
}

/// Where a target's packages are: installed under Dojo's root, and as archives in its cache.
struct Store<'a> {
    fs_root: &'a Path,
    cache:   PathBuf,
    target:  PackageTarget,
}

impl<'a> Store<'a> {
    /// The store `doj` uses on this machine: Dojo's root, its archive cache, and its target.
    fn local(fs_root: &'a Path) -> Store<'a> {
        Store { fs_root,
                cache: cache_artifact_path(Some(fs_root)),
                target: PackageTarget::active_target() }
    }

    fn cached(&self, ident: &PackageIdent) -> Result<Option<PathBuf>> {
        let path = self.cache
                       .join(ident.archive_name_with_target(self.target)?);
        Ok(if path.is_file() { Some(path) } else { None })
    }

    fn installed(&self, ident: &PackageIdent) -> Option<PackageInstall> {
        PackageInstall::load(ident, Some(self.fs_root)).ok()
    }

    /// The newest release in the cache that satisfies `wanted` and is built for the target.
    fn latest_cached(&self, wanted: &PackageIdent) -> Result<Option<PackageIdent>> {
        let entries = match fs::read_dir(&self.cache) {
            Ok(entries) => entries,
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        // An archive is named origin-name-version-release-target.hart, and a name may hold
        // dashes, so the name only narrows the search: the archive's own metadata decides.
        let prefix = format!("{}-{}-", wanted.origin(), wanted.name());
        let suffix = format!("-{}.hart", self.target);
        let mut latest: Option<PackageIdent> = None;
        for entry in entries {
            let path = entry?.path();
            let file = path.file_name().map(|f| f.to_string_lossy().into_owned());
            if !file.map_or(false, |f| f.starts_with(&prefix) && f.ends_with(&suffix)) {
                continue;
            }
            let mut archive = PackageArchive::new(&path);
            let (ident, target) = match (archive.ident(), archive.target()) {
                (Ok(ident), Ok(target)) => (ident, target),
                _ => continue,
            };
            if target != self.target || !ident.satisfies(wanted) {
                continue;
            }
            if latest.as_ref().map_or(true, |l| ident > *l) {
                latest = Some(ident);
            }
        }
        Ok(latest)
    }

    /// The releases `ident` depends on, all of them, from its archive or its install.
    fn tdeps(&self, ident: &PackageIdent) -> Result<Vec<PackageIdent>> {
        if let Some(path) = self.cached(ident)? {
            return Ok(PackageArchive::new(path).tdeps()?);
        }
        if let Some(install) = self.installed(ident) {
            return Ok(install.tdeps()?);
        }
        Err(Error::ShellError(format!("{} is neither installed nor in {}",
                                      ident,
                                      self.cache.display())))
    }

    /// The checksum of `ident`'s archive: taken from the cache, and compared with what the lock
    /// recorded when it holds both; the recorded one alone when the archive has left the cache.
    fn checksum(&self, ident: &PackageIdent, recorded: Option<&String>) -> Result<String> {
        match (self.cached(ident)?, recorded) {
            (Some(path), recorded) => {
                let actual = PackageArchive::new(&path).checksum()?;
                match recorded {
                    Some(recorded) if *recorded != actual => {
                        Err(Error::ShellError(format!("{} has checksum {}, but {} records {}",
                                                      path.display(),
                                                      actual,
                                                      LOCK,
                                                      recorded)))
                    }
                    _ => Ok(actual),
                }
            }
            (None, Some(recorded)) => Ok(recorded.clone()),
            (None, None) => {
                Err(Error::ShellError(format!("no archive of {} in {} to take its checksum \
                                               from",
                                              ident,
                                              self.cache.display())))
            }
        }
    }
}

/// Pins the declaration's packages for the target `doj` runs on. A package already pinned to a
/// release that satisfies it keeps that release unless `update`; the rest resolve to the newest
/// release in the archive cache.
pub fn lock(dir: &Path, fs_root: &Path, update: bool) -> Result<()> {
    let store = Store::local(fs_root);
    let lock = locked(dir, &store, update)?;
    lock.write(dir)?;
    let target = store.target;
    let pins = &lock.targets[&target.to_string()];
    eprintln!("{}: {} packages and {} archives for {}",
              LOCK,
              pins.packages.len(),
              pins.checksums.len(),
              target);
    Ok(())
}

fn locked(dir: &Path, store: &Store<'_>, update: bool) -> Result<Lock> {
    let declaration = Declaration::load(dir)?;
    let target = store.target;
    let mut lock = Lock::load(dir)?.unwrap_or(Lock { version: LOCK_VERSION,
                                                     targets: BTreeMap::new(), });
    let old = lock.targets.remove(&target.to_string());
    let old = if update { None } else { old };

    let mut packages = Vec::new();
    for wanted in declaration.packages_for(target)? {
        let kept = match old {
            Some(ref old) => old.pinned(&wanted)?,
            None => None,
        };
        let ident = match kept {
            Some(ident) => ident,
            None => {
                store.latest_cached(&wanted)?.ok_or_else(|| {
                    Error::ShellError(format!("no archive of {} for {} in {}",
                                              wanted,
                                              target,
                                              store.cache.display()))
                })?
            }
        };
        packages.push(ident);
    }

    let mut checksums = BTreeMap::new();
    for ident in &packages {
        for each in Some(ident.clone()).into_iter().chain(store.tdeps(ident)?) {
            let name = each.to_string();
            if checksums.contains_key(&name) {
                continue;
            }
            let recorded = old.as_ref().and_then(|o| o.checksums.get(&name));
            let checksum = store.checksum(&each, recorded)?;
            checksums.insert(name, checksum);
        }
    }

    lock.targets.insert(target.to_string(),
                        TargetLock { packages: packages.iter().map(|i| i.to_string()).collect(),
                                     checksums });
    Ok(lock)
}

/// Installs what the lock pins for the target `doj` runs on, from the archive cache after
/// checking each archive's checksum, and prints the packages' environment, the declaration's
/// variables over it, as `export` lines that lay it over the shell's own: a list variable's
/// entries go in front of what the shell already has.
pub fn env(dir: &Path, fs_root: &Path, bldr_url: &str) -> Result<()> {
    let text = environment(dir, &Store::local(fs_root), bldr_url)?;
    print!("{}", text);
    Ok(())
}

fn environment(dir: &Path, store: &Store<'_>, bldr_url: &str) -> Result<String> {
    let declaration = Declaration::load(dir)?;
    let fs_root = store.fs_root;
    let pins = matching_pins(&declaration, Lock::load(dir)?, store)?;
    let packages = pins.packages()?;

    let mut missing = Vec::new();
    for (ident, checksum) in &pins.checksums {
        let ident = pinned_ident(ident)?;
        if store.installed(&ident).is_some() {
            continue;
        }
        let path = store.cached(&ident)?.ok_or_else(|| {
                                             Error::ShellError(format!("{} is not installed, \
                                                                       and its archive is not \
                                                                       in {}",
                                                                       ident,
                                                                       store.cache.display()))
                                         })?;
        store.checksum(&ident, Some(checksum))?;
        missing.push((ident, path));
    }
    if !missing.is_empty() {
        // Install progress goes to stderr: stdout is the environment a shell evaluates.
        let mut ui = UI::with_streams(Box::new(io::empty()),
                                      || Box::new(io::stderr()),
                                      || Box::new(io::stderr()),
                                      ColorChoice::Never,
                                      false);
        for (ident, path) in missing {
            // Installing one archive installs its dependencies too.
            if store.installed(&ident).is_some() {
                continue;
            }
            let source = InstallSource::from_str(&path.to_string_lossy())?;
            install::start(&mut ui,
                           bldr_url,
                           &ChannelIdent::stable(),
                           &source,
                           PRODUCT,
                           VERSION,
                           fs_root,
                           &store.cache,
                           None,
                           &InstallMode::Offline,
                           &LocalPackageUsage::Prefer,
                           InstallHookMode::Run)?;
        }
    }

    let installs = packages.iter()
                           .map(|ident| PackageInstall::load(ident, Some(fs_root)))
                           .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut env = PackageInstall::environment_for_commands(&installs)?;
    env.extend(declaration.env.clone());
    render_overlay(&env)
}

/// The lock's pins for the store's target, when they still pin what the declaration asks for,
/// in its order, with a checksum for every archive they install.
fn matching_pins(declaration: &Declaration,
                 lock: Option<Lock>,
                 store: &Store<'_>)
                 -> Result<TargetLock> {
    let target = store.target;
    let stale = || {
        Error::ShellError(format!("{} does not pin what {} asks for on {}; run `doj shell lock`",
                                  LOCK, DECLARATION, target))
    };
    let lock = lock.ok_or_else(|| Error::ShellError(format!("no {}; run `doj shell lock`", LOCK)))?;
    let pins = lock.targets
                   .get(&target.to_string())
                   .cloned()
                   .ok_or_else(stale)?;
    let wanted = declaration.packages_for(target)?;
    let packages = pins.packages()?;
    if wanted.len() != packages.len()
       || !packages.iter()
                   .zip(&wanted)
                   .all(|(pinned, wanted)| pinned.satisfies(wanted))
    {
        return Err(stale());
    }
    for ident in &packages {
        for each in Some(ident.clone()).into_iter().chain(store.tdeps(ident)?) {
            if !pins.checksums.contains_key(&each.to_string()) {
                return Err(Error::ShellError(format!("{} has no checksum for {}; run `doj \
                                                      shell lock`",
                                                     LOCK, each)));
            }
        }
    }
    Ok(pins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hcore::crypto::{artifact,
                               SigKeyPair};
    use std::fs::File;
    use tempfile::TempDir;

    /// A project directory and a Dojo root with an archive cache of its own, and an origin key
    /// to sign archives with. Installing from an archive checks its signature against a key
    /// cache core puts in the user's home unless they are root, so these tests install by
    /// unpacking.
    struct World {
        project: TempDir,
        root:    TempDir,
        key:     SigKeyPair,
    }

    impl World {
        fn new() -> World {
            World { project: TempDir::new().unwrap(),
                    root:    TempDir::new().unwrap(),
                    key:     SigKeyPair::generate_pair_for_origin("acme").unwrap(), }
        }

        fn store(&self) -> Store<'_> {
            Store { fs_root: self.root.path(),
                    cache:   self.cache(),
                    target:  target(), }
        }

        fn declare(&self, text: &str) {
            fs::write(self.project.path().join(DECLARATION), text).unwrap();
        }

        fn lock(&self, update: bool) -> Result<()> {
            let lock = locked(self.project.path(), &self.store(), update)?;
            lock.write(self.project.path())
        }

        fn pins(&self) -> TargetLock {
            Lock::load(self.project.path()).unwrap().unwrap().targets[&target().to_string()].clone()
        }

        fn env(&self) -> Result<String> {
            environment(self.project.path(), &self.store(), "http://bldr.invalid")
        }

        fn cache(&self) -> PathBuf { self.root.path().join("cache") }

        fn install(&self, archive: &Path) {
            PackageArchive::new(archive).unpack(Some(self.root.path()))
                                        .unwrap();
        }

        /// Signs an archive of `ident`, with `bin` on its runtime `PATH` and `tdeps` as its
        /// dependencies, into the cache.
        fn archive(&self, ident: &str, tdeps: &[&str], env: &str) -> PathBuf {
            let ident = PackageIdent::from_str(ident).unwrap();
            let staging = TempDir::new().unwrap();
            let prefix = format!("doj/pkgs/{}", ident);
            let dir = staging.path().join(&prefix);
            fs::create_dir_all(dir.join("bin")).unwrap();
            let runtime_path =
                Some(format!("/{}/bin", prefix)).into_iter()
                                                .chain(tdeps.iter()
                                                            .map(|d| {
                                                                format!("/doj/pkgs/{}/bin", d)
                                                            }))
                                                .collect::<Vec<_>>()
                                                .join(":");
            for (name, text) in [("IDENT", format!("{}\n", ident)),
                                 ("TARGET", format!("{}\n", target())),
                                 ("TDEPS", tdeps.iter().map(|d| format!("{}\n", d)).collect()),
                                 ("RUNTIME_PATH", runtime_path),
                                 ("RUNTIME_ENVIRONMENT", env.to_string())]
            {
                fs::write(dir.join(name), text).unwrap();
            }
            let tar = staging.path().join("payload.tar");
            let mut builder = tar::Builder::new(File::create(&tar).unwrap());
            builder.append_dir_all(&prefix, &dir).unwrap();
            builder.finish().unwrap();
            drop(builder);
            fs::create_dir_all(self.cache()).unwrap();
            let hart = self.cache()
                           .join(ident.archive_name_with_target(target()).unwrap());
            artifact::sign(&tar, &hart, &self.key).unwrap();
            hart
        }
    }

    fn target() -> PackageTarget { PackageTarget::active_target() }

    #[test]
    fn a_declaration_puts_a_targets_own_packages_last() {
        let world = World::new();
        world.declare(&format!("packages = [\"acme/a\"]\n[target.{}]\npackages = \
                                [\"acme/b\"]\n",
                               target()));
        let declaration = Declaration::load(world.project.path()).unwrap();
        let names: Vec<String> = declaration.packages_for(target())
                                            .unwrap()
                                            .iter()
                                            .map(|i| i.to_string())
                                            .collect();
        assert_eq!(vec!["acme/a", "acme/b"], names);
    }

    #[test]
    fn a_directory_with_no_declaration_says_it_declares_no_shell() {
        let world = World::new();
        let error = Declaration::load(world.project.path()).unwrap_err()
                                                           .to_string();
        assert_eq!(format!("no shell.toml in {}: the directory declares no Dojo shell",
                           world.project.path().display()),
                   error);
    }

    #[test]
    fn a_declaration_refuses_unknown_keys_and_targets() {
        let world = World::new();
        world.declare("channel = \"stable\"\npackages = []\n");
        assert!(Declaration::load(world.project.path()).is_err());
        world.declare("[target.x86_64-plan9]\npackages = []\n");
        assert!(Declaration::load(world.project.path()).is_err());
    }

    #[test]
    fn the_lock_text_is_the_same_for_the_same_pins() {
        let mut targets = BTreeMap::new();
        let mut checksums = BTreeMap::new();
        checksums.insert("acme/b/1/2".to_string(), "bb".to_string());
        checksums.insert("acme/a/1/2".to_string(), "aa".to_string());
        targets.insert("x86_64-linux".to_string(),
                       TargetLock { packages: vec!["acme/b/1/2".to_string()],
                                    checksums });
        let lock = Lock { version: LOCK_VERSION,
                          targets };
        let text = lock.to_text().unwrap();
        assert!(text.starts_with(LOCK_HEADER));
        assert!(text.find("acme/a/1/2").unwrap() < text.find("\"acme/b/1/2\" =").unwrap());
        assert_eq!(lock, toml::from_str(&text).unwrap());
    }

    #[test]
    fn lock_pins_the_newest_cached_release_and_checksums_its_dependencies() {
        let world = World::new();
        let dep = world.archive("acme/dep/1.0.0/20260101000000", &[], "");
        world.archive("acme/tool/1.0.0/20260101000000", &[], "");
        let newest = world.archive("acme/tool/1.1.0/20260201000000",
                                   &["acme/dep/1.0.0/20260101000000"],
                                   "");
        world.declare("packages = [\"acme/tool\"]\n");

        world.lock(false).unwrap();

        let pins = world.pins();
        assert_eq!(vec!["acme/tool/1.1.0/20260201000000"], pins.packages);
        let mut expected = BTreeMap::new();
        expected.insert("acme/dep/1.0.0/20260101000000".to_string(),
                        PackageArchive::new(dep).checksum().unwrap());
        expected.insert("acme/tool/1.1.0/20260201000000".to_string(),
                        PackageArchive::new(newest).checksum().unwrap());
        assert_eq!(expected, pins.checksums);
    }

    #[test]
    fn lock_keeps_a_pin_until_updated() {
        let world = World::new();
        world.archive("acme/tool/1.0.0/20260101000000", &[], "");
        world.declare("packages = [\"acme/tool\"]\n");
        world.lock(false).unwrap();
        world.archive("acme/tool/1.1.0/20260201000000", &[], "");

        world.lock(false).unwrap();
        assert_eq!(vec!["acme/tool/1.0.0/20260101000000"],
                   world.pins().packages);

        world.lock(true).unwrap();
        assert_eq!(vec!["acme/tool/1.1.0/20260201000000"],
                   world.pins().packages);
    }

    #[test]
    fn lock_leaves_other_targets_pins_alone() {
        let world = World::new();
        world.archive("acme/tool/1.0.0/20260101000000", &[], "");
        world.declare("packages = [\"acme/tool\"]\n");
        let other = TargetLock { packages:  vec!["acme/tool/0.9.0/20251201000000".to_string()],
                                 checksums: BTreeMap::new(), };
        let mut targets = BTreeMap::new();
        targets.insert("some-other-target".to_string(), other.clone());
        Lock { version: LOCK_VERSION,
               targets }.write(world.project.path())
                        .unwrap();

        world.lock(false).unwrap();

        let lock = Lock::load(world.project.path()).unwrap().unwrap();
        assert_eq!(Some(&other), lock.targets.get("some-other-target"));
    }

    #[test]
    fn lock_refuses_an_archive_whose_checksum_changed() {
        let world = World::new();
        world.archive("acme/tool/1.0.0/20260101000000", &[], "");
        world.declare("packages = [\"acme/tool\"]\n");
        world.lock(false).unwrap();
        // The same release, signed again: another archive under the same name.
        world.archive("acme/tool/1.0.0/20260101000000", &[], "FOO=other\n");

        assert!(world.lock(false).is_err());
    }

    #[test]
    fn lock_fails_for_a_package_with_no_archive() {
        let world = World::new();
        world.declare("packages = [\"acme/missing\"]\n");
        assert!(world.lock(false).is_err());
    }

    #[test]
    fn env_prints_the_pins_environment_with_the_declarations_variables_over_it() {
        let world = World::new();
        let dep = world.archive("acme/dep/1.0.0/20260101000000", &[], "");
        let tool = world.archive("acme/tool/1.0.0/20260101000000",
                                 &["acme/dep/1.0.0/20260101000000"],
                                 "GREETING=hello\n");
        world.install(&dep);
        world.install(&tool);
        world.declare("packages = [\"acme/tool\"]\n[env]\nGREETING = \"from shell.toml\"\n");
        world.lock(false).unwrap();

        let text = world.env().unwrap();

        assert_eq!("export GREETING='from shell.toml'\nexport \
                    PATH='/doj/pkgs/acme/tool/1.0.0/20260101000000/bin:/doj/pkgs/acme/dep/1.0.\
                    0/20260101000000/bin'\"${PATH:+:${PATH}}\"\n",
                   text);
    }

    #[test]
    fn env_refuses_a_lock_that_no_longer_matches_the_declaration() {
        let world = World::new();
        let tool = world.archive("acme/tool/1.0.0/20260101000000", &[], "");
        let other = world.archive("acme/other/1.0.0/20260101000000", &[], "");
        world.install(&tool);
        world.install(&other);
        world.declare("packages = [\"acme/tool\"]\n");
        world.lock(false).unwrap();

        world.declare("packages = [\"acme/tool\", \"acme/other\"]\n");
        let error = world.env().unwrap_err().to_string();
        assert!(error.contains("does not pin what shell.toml asks for"),
                "{}",
                error);
    }

    #[test]
    fn env_refuses_an_archive_whose_checksum_changed_before_installing_it() {
        let world = World::new();
        world.archive("acme/tool/1.0.0/20260101000000", &[], "");
        world.declare("packages = [\"acme/tool\"]\n");
        world.lock(false).unwrap();
        world.archive("acme/tool/1.0.0/20260101000000", &[], "FOO=other\n");

        let error = world.env().unwrap_err().to_string();
        assert!(error.contains("but shell.lock records"), "{}", error);
        assert!(PackageInstall::load(&PackageIdent::from_str("acme/tool").unwrap(),
                                     Some(world.root.path())).is_err());
    }
}
