#[macro_use]
extern crate clap;
use dojo_common as common;
use dojo_core as hcore;

#[macro_use]
extern crate failure_derive;

#[macro_use]
extern crate log;

mod build;
pub mod cli;
mod error;
mod rootfs;

pub use crate::{cli::Cli,
                error::{Error,
                        Result}};
use crate::{common::ui::UI,
            hcore::{package::{PackageIdent,
                              PackageInstall},
                    url as hurl}};
use flate2::{write::GzEncoder,
             Compression};
use std::{fs::File,
          path::{Path,
                 PathBuf},
          str::FromStr};
use tar::Builder;

pub use crate::build::BuildSpec;

/// The version of this library and program when built.
pub const VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/VERSION"));
/// The Dojo Package Identifier string for a Busybox package.
const BUSYBOX_IDENT: &str = "karate/busybox-static";

pub fn export_for_cli_matches(ui: &mut UI, matches: &clap::ArgMatches<'_>) -> Result<()> {
    let default_url = hurl::default_bldr_url();
    let spec = BuildSpec::new_from_cli_matches(&matches, &default_url);
    export(ui, spec)?;

    Ok(())
}

pub fn export(ui: &mut UI, build_spec: BuildSpec<'_>) -> Result<()> {
    let doj_pkg = build_spec.doj;
    let build_result = build_spec.create(ui).unwrap();
    let builder_dir_path = build_result.0.path();
    let pkg_ident = build_result.1;

    tar_command(builder_dir_path, pkg_ident, doj_pkg);
    Ok(())
}

#[allow(unused_must_use)]
fn tar_command(temp_dir_path: &Path, pkg_ident: PackageIdent, doj_pkg: &str) {
    let tarball_name = format_tar_name(pkg_ident);

    let tarball = File::create(tarball_name).unwrap();
    let enc = GzEncoder::new(tarball, Compression::default());
    let mut tar_builder = Builder::new(enc);
    tar_builder.follow_symlinks(false);

    let root_fs = temp_dir_path.join("rootfs");
    let doj_pkgs_path = temp_dir_path.join("rootfs/doj");

    // Although this line of code DOES work (it adds the required directories
    // and subdirectories to the tarball), it also returns an error
    // thread 'main' panicked at 'could not export.: "Is a directory (os error 21)"'
    // , /checkout/src/libcore/result.rs:906:4
    // An issue re: this error has been opened in the github repo of tar-rs
    // https://github.com/alexcrichton/tar-rs/issues/147
    // Until this is sorted out, I am not doing anything with the result
    // that is returned by this command -NSH
    tar_builder.append_dir_all("doj", doj_pkgs_path);

    // Find the path to the doj binary
    let mut doj_pkg_binary_path = doj_install_path(&doj_package_ident(doj_pkg), &root_fs);
    doj_pkg_binary_path.push("bin");

    // Append the doj binary to the tar ball
    tar_builder.append_dir_all("doj/bin", doj_pkg_binary_path);
}

fn format_tar_name(ident: PackageIdent) -> String {
    format!("{}-{}-{}-{}.tar.gz",
            ident.origin,
            ident.name,
            ident.version.unwrap(),
            ident.release.unwrap())
}

fn doj_package_ident(doj_pkg: &str) -> PackageIdent { PackageIdent::from_str(doj_pkg).unwrap() }

fn doj_install_path(doj_ident: &PackageIdent, root_fs_path: &Path) -> PathBuf {
    let root_fs_path = Path::new(&root_fs_path);
    PackageInstall::load(&doj_ident, Some(root_fs_path)).unwrap()
                                                        .installed_path
}
