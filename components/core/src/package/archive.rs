// Copyright (c) 2016-2017 Chef Software Inc. and/or applicable contributors
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

use std::{collections::HashMap,
          error,
          io::{BufRead,
               BufReader,
               Read},
          path::{Path,
                 PathBuf},
          result,
          str::FromStr,
          sync::LazyLock};

use liblzma::read::XzDecoder;
use regex::Regex;

use super::{metadata::{MetaFile,
                       PackageType},
            Identifiable,
            PackageIdent,
            PackageTarget};
use crate::{crypto::{artifact,
                     hash},
            error::{Error,
                    Result}};

/// The metadata files read from an archive, by file name.
const METAFILES: &[MetaFile] = &[MetaFile::CFlags,
                                 MetaFile::Config,
                                 MetaFile::Deps,
                                 MetaFile::TDeps,
                                 MetaFile::BuildDeps,
                                 MetaFile::BuildTDeps,
                                 MetaFile::Exposes,
                                 MetaFile::Ident,
                                 MetaFile::LdRunPath,
                                 MetaFile::LdFlags,
                                 MetaFile::SvcUser,
                                 MetaFile::Services,
                                 MetaFile::ResolvedServices,
                                 MetaFile::Manifest,
                                 MetaFile::Path,
                                 MetaFile::Target,
                                 MetaFile::Type];

/// A file directly inside a package's directory: `doj/pkgs/<ident>/<file>`.
static METAFILE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^/?doj/pkgs/[^/]+/[^/]+/[^/]+/[^/]+/([^/]+)$").expect("the pattern is valid")
});

type Metadata = HashMap<MetaFile, String>;

#[derive(Debug)]
pub struct PackageArchive {
    pub path: PathBuf,
    metadata: Option<Metadata>,
}

impl PackageArchive {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        PackageArchive { path:     path.into(),
                         metadata: None, }
    }

    /// Calculate and return the checksum of the package archive in base64 format.
    ///
    /// # Failures
    ///
    /// * If the archive cannot be read
    pub fn checksum(&self) -> Result<String> { hash::hash_file(&self.path) }

    pub fn cflags(&mut self) -> Result<Option<String>> {
        match self.read_metadata(MetaFile::CFlags) {
            Ok(data) => Ok(data.cloned()),
            Err(e) => Err(e),
        }
    }

    pub fn config(&mut self) -> Result<Option<String>> {
        match self.read_metadata(MetaFile::Config) {
            Ok(data) => Ok(data.cloned()),
            Err(e) => Err(e),
        }
    }

    // doj-plan-build.sh only generates SVC_USER and SVC_GROUP files if it thinks a package is
    // a service. It determines that by checking for the presence of a run hook file or a
    // pkg_svc_run value. Therefore, if we can detect the presence of a SVC_USER file, we can
    // consider this archive a service.
    //
    // The allow below is necessary because `is_*` functions expect a `&self`, not `&mut self`.
    // It would be good to refactor this struct to do the read_metadata in new and then
    // eliminate the `&mut self`s on all the accessor functions, but that's a more involved
    // change than we want to undertake now.
    //
    // See https://rust-lang.github.io/rust-clippy/master/index.html#wrong_self_convention
    #[allow(clippy::wrong_self_convention)]
    pub fn is_a_service(&mut self) -> bool {
        match self.svc_user() {
            Ok(_) => true,
            _ => false,
        }
    }

    /// Returns a list of package identifiers representing the runtime package dependencies for
    /// this archive.
    ///
    /// # Failures
    ///
    /// * If the archive cannot be read
    /// * If the archive cannot be verified
    pub fn deps(&mut self) -> Result<Vec<PackageIdent>> { self.read_deps(MetaFile::Deps) }

    /// Returns a list of package identifiers representing the transitive runtime package
    /// dependencies for this archive.
    ///
    /// # Failures
    ///
    /// * If the archive cannot be read
    /// * If the archive cannot be verified
    pub fn tdeps(&mut self) -> Result<Vec<PackageIdent>> { self.read_deps(MetaFile::TDeps) }

    /// Returns a list of package identifiers representing the buildtime package dependencies for
    /// this archive.
    ///
    /// # Failures
    ///
    /// * If the archive cannot be read
    /// * If the archive cannot be verified
    pub fn build_deps(&mut self) -> Result<Vec<PackageIdent>> {
        self.read_deps(MetaFile::BuildDeps)
    }

    /// Returns a list of package identifiers representing the transitive buildtime package
    /// dependencies for this archive.
    ///
    /// # Failures
    ///
    /// * If the archive cannot be read
    /// * If the archive cannot be verified
    pub fn build_tdeps(&mut self) -> Result<Vec<PackageIdent>> {
        self.read_deps(MetaFile::BuildTDeps)
    }

    pub fn exposes(&mut self) -> Result<Vec<u16>> {
        match self.read_metadata(MetaFile::Exposes) {
            Ok(Some(data)) => {
                let ports: Vec<u16> = data.split(' ')
                                          .filter_map(|port| port.parse::<u16>().ok())
                                          .collect();
                Ok(ports)
            }
            Ok(None) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    pub fn ident(&mut self) -> Result<PackageIdent> {
        match self.read_metadata(MetaFile::Ident) {
            Ok(None) => Err(Error::MetaFileNotFound(MetaFile::Ident)),
            Ok(Some(data)) => PackageIdent::from_str(&data),
            Err(e) => Err(e),
        }
    }

    pub fn ld_run_path(&mut self) -> Result<Option<String>> {
        match self.read_metadata(MetaFile::LdRunPath) {
            Ok(data) => Ok(data.cloned()),
            Err(e) => Err(e),
        }
    }

    pub fn ldflags(&mut self) -> Result<Option<String>> {
        match self.read_metadata(MetaFile::LdFlags) {
            Ok(data) => Ok(data.cloned()),
            Err(e) => Err(e),
        }
    }

    pub fn svc_user(&mut self) -> Result<String> {
        match self.read_metadata(MetaFile::SvcUser) {
            Ok(Some(data)) => Ok(data.clone()),
            Ok(None) => Err(Error::MetaFileNotFound(MetaFile::SvcUser)),
            Err(e) => Err(e),
        }
    }

    pub fn manifest(&mut self) -> Result<String> {
        match self.read_metadata(MetaFile::Manifest) {
            Ok(None) => Err(Error::MetaFileNotFound(MetaFile::Manifest)),
            Ok(Some(data)) => Ok(data.clone()),
            Err(e) => Err(e),
        }
    }

    pub fn package_type(&mut self) -> Result<PackageType> {
        match self.read_metadata(MetaFile::Type) {
            Ok(None) => Ok(PackageType::Standalone),
            Ok(Some(data)) => PackageType::from_str(&data),
            Err(e) => Err(e),
        }
    }

    pub fn path(&mut self) -> Result<Option<String>> {
        match self.read_metadata(MetaFile::Path) {
            Ok(data) => Ok(data.cloned()),
            Err(e) => Err(e),
        }
    }

    pub fn pkg_services(&mut self) -> Result<Vec<PackageIdent>> {
        self.read_deps(MetaFile::Services)
    }

    pub fn resolved_services(&mut self) -> Result<Vec<PackageIdent>> {
        self.read_deps(MetaFile::ResolvedServices)
    }

    pub fn target(&mut self) -> Result<PackageTarget> {
        match self.read_metadata(MetaFile::Target) {
            Ok(None) => Err(Error::MetaFileNotFound(MetaFile::Target)),
            Ok(Some(data)) => PackageTarget::from_str(&data),
            Err(e) => Err(e),
        }
    }

    /// A plain string representation of the archive's file name.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    /// Given a package name and a path to a file as an `&str`, verify
    /// the files signature.
    ///
    /// # Failures
    ///
    /// * Fails if it cannot verify the signature for any reason
    pub fn verify<P: AsRef<Path>>(&self, cache_key_path: &P) -> Result<(String, String)> {
        artifact::verify(&self.path, cache_key_path)
    }

    /// Given a package name and a path to a file as an `&str`, unpack
    /// the package.
    ///
    /// # Failures
    ///
    /// * If the package cannot be unpacked
    pub fn unpack(&self, fs_root_path: Option<&Path>) -> Result<()> {
        let root = fs_root_path.unwrap_or_else(|| Path::new("/"));
        let mut archive = self.tar()?;
        archive.set_preserve_permissions(true);
        archive.set_preserve_mtime(true);
        archive.unpack(root)?;
        Ok(())
    }

    /// The archive's tar stream, after its signed header. Payloads are
    /// xz-compressed, but a plain tar is read too, as libarchive did.
    fn tar(&self) -> Result<tar::Archive<Box<dyn Read>>> {
        const XZ_MAGIC: &[u8] = b"\xFD7zXZ\0";
        let mut payload = BufReader::new(artifact::get_archive_reader(&self.path)?);
        let compressed = payload.fill_buf()?.starts_with(XZ_MAGIC);
        let stream: Box<dyn Read> = if compressed {
            Box::new(XzDecoder::new(payload))
        } else {
            Box::new(payload)
        };
        Ok(tar::Archive::new(stream))
    }

    fn read_deps(&mut self, file: MetaFile) -> Result<Vec<PackageIdent>> {
        let mut deps: Vec<PackageIdent> = vec![];

        // For now, all deps files but SERVICES need fully-qualified
        // package identifiers
        let must_be_fully_qualified = { file != MetaFile::Services };

        match self.read_metadata(file) {
            Ok(Some(body)) => {
                let ids: Vec<String> = body.lines().map(|d| d.to_string()).collect();
                for id in &ids {
                    let package = PackageIdent::from_str(id)?;
                    if !package.fully_qualified() && must_be_fully_qualified {
                        return Err(Error::FullyQualifiedPackageIdentRequired(package.to_string()));
                    }
                    deps.push(package);
                }
                Ok(deps)
            }
            Ok(None) => Ok(vec![]),
            Err(Error::MetaFileNotFound(_)) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    fn read_metadata(&mut self, file: MetaFile) -> Result<Option<&String>> {
        if self.metadata.is_none() {
            self.metadata = Some(self.read_all_metadata()?);
        }
        Ok(self.metadata.as_ref().and_then(|files| files.get(&file)))
    }

    fn read_all_metadata(&self) -> Result<Metadata> {
        let mut metadata = Metadata::new();
        let mut archive = self.tar()?;
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_string_lossy().into_owned();
            let Some(name) = METAFILE_PATH.captures(&path).and_then(|c| c.get(1)) else {
                continue;
            };
            let Some(file) = METAFILES.iter().find(|f| f.to_string() == name.as_str()) else {
                continue;
            };
            let mut body = String::new();
            entry.read_to_string(&mut body)
                 .map_err(|_| Error::MetaFileMalformed(*file))?;
            // Callers want the value without surrounding whitespace.
            metadata.insert(*file, body.trim().to_string());
            if metadata.len() == METAFILES.len() {
                break;
            }
        }
        Ok(metadata)
    }
}

pub trait FromArchive: Sized {
    type Error: error::Error;

    fn from_archive(archive: &mut PackageArchive) -> result::Result<Self, Self::Error>;
}

#[cfg(test)]
mod test {
    use super::{super::target,
                *};
    use std::path::PathBuf;

    #[test]
    fn reading_artifact_metadata() {
        let mut hart = PackageArchive::new(fixtures().join("happyhumans-possums-8.1.\
                                                            4-20160427165340-x86_64-linux.hart"));
        let ident = hart.ident().unwrap();
        assert_eq!(ident.origin, "happyhumans");
        assert_eq!(ident.name, "possums");
        assert_eq!(ident.version, Some("8.1.4".to_string()));
        assert_eq!(ident.release, Some("20160427165340".to_string()));
    }

    pub fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests") }

    pub fn fixtures() -> PathBuf { root().join("fixtures") }

    #[test]
    fn reading_artifact_deps() {
        let mut hart = PackageArchive::new(fixtures().join("happyhumans-possums-8.1.\
                                                            4-20160427165340-x86_64-linux.hart"));
        let _ = hart.deps().unwrap();
        let _ = hart.tdeps().unwrap();
    }

    #[test]
    fn reading_artifact_large_tdeps() {
        let mut hart = PackageArchive::new(fixtures().join("unhappyhumans-possums-8.1.\
                                                            4-20160427165340-x86_64-linux.hart"));
        let tdeps = hart.tdeps().unwrap();
        assert_eq!(1024, tdeps.len());
    }

    #[test]
    fn unpacking_writes_the_files_with_their_modes_and_times() {
        use std::os::unix::fs::{MetadataExt,
                                PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let hart = PackageArchive::new(fixtures().join("happyhumans-possums-8.1.\
                                                        4-20160427165340-x86_64-linux.hart"));
        hart.unpack(Some(root.path())).unwrap();
        let prefix = root.path()
                         .join("doj/pkgs/happyhumans/possums/8.1.4/20160427165340");
        assert_eq!(std::fs::read_to_string(prefix.join("IDENT")).unwrap()
                                                                .trim(),
                   "happyhumans/possums/8.1.4/20160427165340");
        let signme = std::fs::metadata(prefix.join("share/signme.dat")).unwrap();
        assert_eq!(signme.permissions().mode() & 0o777, 0o755);
        let ident = std::fs::metadata(prefix.join("IDENT")).unwrap();
        assert_eq!(ident.permissions().mode() & 0o777, 0o644);
        // 2016-04-27 09:53, as the archive records it.
        assert!(ident.mtime() < 1_461_800_000 && ident.mtime() > 1_461_700_000,
                "mtime {}",
                ident.mtime());
    }

    #[test]
    fn reading_artifact_target() {
        let mut hart = PackageArchive::new(fixtures().join("unhappyhumans-possums-8.1.\
                                                            4-20160427165340-x86_64-linux.hart"));
        let target = hart.target().unwrap();

        assert_eq!(target::X86_64_LINUX, target);
    }
}
