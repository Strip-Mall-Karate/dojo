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

use dojo_common as common;
use dojo_core as hcore;

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_json;

mod accounts;
mod build;
#[cfg(unix)]
mod chmod;
pub mod cli;
mod docker;
mod error;
#[cfg(unix)]
pub mod rootfs;
mod util;

pub use crate::{build::BuildSpec,
                cli::{Cli,
                      PkgIdentArgOptions},
                docker::{DockerBuildRoot,
                         DockerImage},
                error::{Error,
                        Result}};
use crate::{common::{ui::{UIWriter,
                          UI},
                     PROGRAM_NAME},
            hcore::url as hurl};
use aws_sdk_ecr::config::{BehaviorVersion,
                          Credentials as AwsCredentials,
                          Region};
use base64::Engine;
use clap::Command;
use std::{env,
          fmt,
          result,
          str::FromStr};

/// The version of this library and program when built.
pub const VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/VERSION"));

/// The Dojo Package Identifier string for a Busybox package.
const BUSYBOX_IDENT: &str = "karate/busybox-static";
/// The Dojo Package Identifier string for SSL certificate authorities (CA) certificates package.
const CACERTS_IDENT: &str = "karate/cacerts";

/// An image naming policy.
///
/// This is a value struct which captures the naming and tagging intentions for an image.
#[derive(Debug)]
pub struct Naming<'a> {
    /// An optional custom image name which would override a computed default value.
    pub custom_image_name:   Option<&'a str>,
    /// Whether or not to tag the image with a latest value.
    pub latest_tag:          bool,
    /// Whether or not to tag the image with a value containing a version from a Package
    /// Identifier.
    pub version_tag:         bool,
    /// Whether or not to tag the image with a value containing a version and release from a
    /// Package Identifier.
    pub version_release_tag: bool,
    /// An optional custom tag value for the image.
    pub custom_tag:          Option<&'a str>,
    /// A URL to a custom Docker registry to publish to. This will be used as part of every tag
    /// before pushing.
    pub registry_url:        Option<&'a str>,
    /// The type of registry we're publishing to. Ex: Amazon, Docker, Google, Azure.
    pub registry_type:       RegistryType,
}

impl<'a> Naming<'a> {
    /// Creates a `Naming` from cli arguments.
    pub fn new_from_cli_matches(m: &'a clap::ArgMatches) -> Self {
        let registry_type = m.get_one::<String>("REGISTRY_TYPE")
                             .and_then(|t| t.parse().ok())
                             .unwrap_or(RegistryType::Docker);
        let registry_url = m.get_one::<String>("REGISTRY_URL").map(String::as_str);

        Naming { custom_image_name: m.get_one::<String>("IMAGE_NAME").map(String::as_str),
                 latest_tag: !m.get_flag("NO_TAG_LATEST"),
                 version_tag: !m.get_flag("NO_TAG_VERSION"),
                 version_release_tag: !m.get_flag("NO_TAG_VERSION_RELEASE"),
                 custom_tag: m.get_one::<String>("TAG_CUSTOM").map(String::as_str),
                 registry_url,
                 registry_type }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RegistryType {
    Amazon,
    Azure,
    Docker,
}

impl RegistryType {
    fn variants() -> &'static [&'static str] { &["amazon", "azure", "docker"] }
}

impl FromStr for RegistryType {
    type Err = Error;

    fn from_str(value: &str) -> result::Result<Self, Self::Err> {
        match value {
            "amazon" => Ok(RegistryType::Amazon),
            "azure" => Ok(RegistryType::Azure),
            "docker" => Ok(RegistryType::Docker),
            _ => Err(Error::InvalidRegistryType(String::from(value))),
        }
    }
}

impl fmt::Display for RegistryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let disp = match *self {
            RegistryType::Amazon => "amazon",
            RegistryType::Azure => "azure",
            RegistryType::Docker => "docker",
        };
        write!(f, "{}", disp)
    }
}

/// A credentials username and password pair.
///
/// This is a value struct which references username and password values.
#[derive(Debug)]
pub struct Credentials {
    pub token: String,
}

impl Credentials {
    pub fn new(registry_type: RegistryType, username: &str, password: &str) -> Result<Self> {
        match registry_type {
            RegistryType::Amazon => {
                // The username and password should be valid IAM credentials
                let credentials =
                    AwsCredentials::new(username, password, None, None, "doj-pkg-export-docker");
                // TODO TED: Make the region configurable
                let config =
                    aws_sdk_ecr::Config::builder().behavior_version(BehaviorVersion::latest())
                                                  .region(Region::new("us-west-2"))
                                                  .credentials_provider(credentials)
                                                  .build();
                let client = aws_sdk_ecr::Client::from_conf(config);
                // One request, so a runtime of its own for it.
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all()
                                                                           .build()?;
                let response =
                    runtime.block_on(client.get_authorization_token().send())
                           .map_err(|e| Error::TokenFetchFailed(aws_sdk_ecr::Error::from(e)))?;
                let token = response.authorization_data()
                                    .first()
                                    .and_then(|data| data.authorization_token())
                                    .ok_or(Error::NoECRTokensReturned)?
                                    .to_string();

                Ok(Credentials { token })
            }
            RegistryType::Docker | RegistryType::Azure => {
                Ok(Credentials { token: base64::engine::general_purpose::STANDARD.encode(format!("{}:{}",
                                                                username.to_string(),
                                                                password.to_string())), })
            }
        }
    }
}

/// Exports a Docker image to a Docker engine from a build specification and naming policy.
///
/// # Errors
///
/// * If a generic and temporary build root directory cannot be created containing a root
/// file system
/// * If additional Docker-related files cannot be created in the root file system
/// * If building the Docker image fails
/// * If destroying the temporary build root directory fails
pub fn export(ui: &mut UI,
              build_spec: BuildSpec,
              naming: &Naming,
              memory: Option<&str>)
              -> Result<DockerImage> {
    ui.begin(format!("Building a runnable Docker image with: {}",
                     build_spec.idents_or_archives.join(", ")))?;
    let build_root = DockerBuildRoot::from_build_root(build_spec.create(ui)?, ui)?;
    let image = build_root.export(ui, naming, memory)?;
    build_root.destroy(ui)?;
    ui.end(format!("Docker image '{}' created with tags: {}",
                   image.name(),
                   image.tags().join(", ")))?;

    Ok(image)
}

/// Creates a build specification and naming policy from Cli arguments, and then exports a Docker
/// image to a Docker engine from them.
///
/// # Errors
///
/// * The actual import fails.
/// * Current directory does not exist.
/// * There are insufficient permissions to access the current directory.
/// * Pushing the image to remote registry fails.
/// * Parsing of credentials fails.
/// * The image (tags) cannot be removed.
pub fn export_for_cli_matches(ui: &mut UI,
                              matches: &clap::ArgMatches)
                              -> Result<Option<DockerImage>> {
    let default_url = hurl::default_bldr_url();
    let spec = BuildSpec::new_from_cli_matches(&matches, &default_url);
    let naming = Naming::new_from_cli_matches(&matches);

    let docker_image = export(ui,
                              spec,
                              &naming,
                              matches.get_one::<String>("MEMORY_LIMIT")
                                     .map(String::as_str))?;
    docker_image.create_report(ui, env::current_dir()?.join("results"))?;

    if matches.get_flag("PUSH_IMAGE") {
        let credentials = Credentials::new(naming.registry_type,
                                           matches.get_one::<String>("REGISTRY_USERNAME")
                                                  .map(String::as_str)
                                                  .expect("Username not specified"),
                                           matches.get_one::<String>("REGISTRY_PASSWORD")
                                                  .map(String::as_str)
                                                  .expect("Password not specified"))?;
        docker_image.push(ui, &credentials, naming.registry_url)?;
    }
    if matches.get_flag("RM_IMAGE") {
        docker_image.rm(ui)?;

        Ok(None)
    } else {
        Ok(Some(docker_image))
    }
}

/// Create the Clap CLI for the Docker exporter
pub fn cli() -> Command {
    let name: &str = &*PROGRAM_NAME;
    let about = "Creates (and optionally pushes) a Docker image from a set of Dojo packages";

    Cli::new(name, about).add_base_packages_args()
                         .add_builder_args()
                         .add_tagging_args()
                         .add_publishing_args()
                         .add_memory_arg()
                         .add_pkg_ident_arg(PkgIdentArgOptions { multiple: true })
                         .app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_is_well_formed() { cli().debug_assert(); }

    #[test]
    fn switches_and_the_registry_type_are_read() {
        let m = cli().try_get_matches_from(["export",
                                            "--no-tag-latest",
                                            "--push-image",
                                            "--username",
                                            "u",
                                            "--password",
                                            "p",
                                            "--registry-type",
                                            "amazon",
                                            "--registry-url",
                                            "123.dkr.ecr.us-west-2.amazonaws.com",
                                            "core/redis",
                                            "core/nginx"])
                     .unwrap();
        let naming = Naming::new_from_cli_matches(&m);
        assert!(!naming.latest_tag);
        assert!(naming.version_tag);
        assert!(matches!(naming.registry_type, RegistryType::Amazon));
        assert!(m.get_flag("PUSH_IMAGE"));
        assert!(!m.get_flag("RM_IMAGE"));
        assert_eq!(m.get_many::<String>("PKG_IDENT_OR_ARTIFACT")
                    .unwrap()
                    .count(),
                   2);
    }

    #[test]
    fn an_amazon_registry_needs_its_url() {
        let err = cli().try_get_matches_from(["export", "--registry-type", "amazon", "core/redis"])
                       .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn an_unknown_registry_type_is_refused() {
        let err = cli().try_get_matches_from(["export", "--registry-type", "quay", "core/redis"])
                       .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }
}
