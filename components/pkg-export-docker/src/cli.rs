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

use std::{path::Path,
          result,
          str::FromStr};

use crate::hcore::package::PackageIdent;
use clap::{builder::ValueRange,
           Arg,
           ArgAction,
           Command};
use dojo_common::cli::HELP_TEMPLATE;
use url::Url;

use crate::RegistryType;

/// The version of this library and program when built.
pub const VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/VERSION"));

/// A Docker-specific clap:App wrapper
#[derive(Clone)]
pub struct Cli {
    pub app: Command,
}

#[derive(Clone, Copy)]
pub struct PkgIdentArgOptions {
    pub multiple: bool,
}

impl Cli {
    pub fn new(name: &str, about: &str) -> Self {
        let image_name =
            Arg::new("IMAGE_NAME").long("image-name")
                                  .short('i')
                                  .value_name("IMAGE_NAME")
                                  .help("Image name (default: \"{{pkg_origin}}/{{pkg_name}}\" supports: \
                                         {{pkg_origin}}, {{pkg_name}}, {{pkg_version}}, \
                                         {{pkg_release}}, {{channel}})");
        Cli { app: Command::new(name.to_string()).about(about.to_string())
                                                 .version(VERSION)
                                                 .author("Maintainer: Strip Mall Karate Company")
                                                 .help_template(HELP_TEMPLATE)
                                                 .arg(image_name), }
    }

    pub fn add_base_packages_args(self) -> Self {
        let app = self
            .app
            .arg(
                Arg::new("DOJ_PKG")
                    .long("doj-pkg")
                    .value_name("DOJ_PKG")
                    .value_parser(valid_ident_or_hart)
                    .help(
                        "Dojo CLI package identifier (ex: acme/redis) or filepath to a Dojo \
                         artifact (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart) \
                         to install (default: karate/doj)",
                    ),
            )
            .arg(
                Arg::new("DOJ_LAUNCHER_PKG")
                    .long("launcher-pkg")
                    .value_name("DOJ_LAUNCHER_PKG")
                    .value_parser(valid_ident_or_hart)
                    .help(
                        "Launcher package identifier (ex: karate/doj-launcher) or filepath to a \
                         Dojo artifact (ex: \
                         /home/core-doj-launcher-6083-20171101045646-x86_64-linux.hart) to \
                         install (default: karate/doj-launcher)",
                    ),
            )
            .arg(
                Arg::new("DOJ_SUP_PKG")
                    .long("sup-pkg")
                    .value_name("DOJ_SUP_PKG")
                    .value_parser(valid_ident_or_hart)
                    .help(
                        "Supervisor package identifier (ex: karate/doj-sup) or filepath to a \
                         Dojo artifact (ex: \
                         /home/ore-doj-sup-0.39.1-20171118011657-x86_64-linux.hart) to install \
                         (default: karate/doj-sup)",
                    ),
            );

        Cli { app }
    }

    pub fn add_builder_args(self) -> Self {
        let app = self
            .app
            .arg(
                Arg::new("BLDR_URL")
                    .long("url")
                    .short('u')
                    .value_name("BLDR_URL")
                    .value_parser(valid_url)
                    .help(
                        "Install packages from Builder at the specified URL \
                         (default: https://bldr.karate.company)",
                    ),
            )
            .arg(
                Arg::new("CHANNEL")
                    .long("channel")
                    .short('c')
                    .value_name("CHANNEL")
                    .help("Install packages from the specified release channel (default: stable)"),
            )
            .arg(
                Arg::new("BASE_PKGS_BLDR_URL")
                    .long("base-pkgs-url")
                    .value_name("BASE_PKGS_BLDR_URL")
                    .value_parser(valid_url)
                    .help(
                        "Install base packages from Builder at the specified URL \
                         (default: https://bldr.karate.company)",
                    ),
            )
            .arg(
                Arg::new("BASE_PKGS_CHANNEL")
                    .long("base-pkgs-channel")
                    .value_name("BASE_PKGS_CHANNEL")
                    .help(
                        "Install base packages from the specified release channel \
                         (default: stable)",
                    ),
            )
            .arg(
                Arg::new("BLDR_AUTH_TOKEN")
                    .long("auth")
                    .short('z')
                    .value_name("BLDR_AUTH_TOKEN")
                    .help("Provide a Builder auth token for private pkg export"),
            );

        Cli { app }
    }

    pub fn add_tagging_args(self) -> Self {
        let app = self
            .app
            .arg(
                Arg::new("TAG_VERSION_RELEASE")
                    .action(ArgAction::SetTrue)
                    .long("tag-version-release")
                    .conflicts_with("NO_TAG_VERSION_RELEASE")
                    .help("Tag image with :\"{{pkg_version}}-{{pkg_release}}\" (default: yes)"),
            )
            .arg(
                Arg::new("NO_TAG_VERSION_RELEASE")
                    .action(ArgAction::SetTrue)
                    .long("no-tag-version-release")
                    .conflicts_with("TAG_VERSION_RELEASE")
                    .help(
                        "Do not tag image with :\"{{pkg_version}}-{{pkg_release}}\" (default: no)",
                    ),
            )
            .arg(
                Arg::new("TAG_VERSION")
                    .action(ArgAction::SetTrue)
                    .long("tag-version")
                    .conflicts_with("NO_TAG_VERSION")
                    .help("Tag image with :\"{{pkg_version}}\" (default: yes)"),
            )
            .arg(
                Arg::new("NO_TAG_VERSION")
                    .action(ArgAction::SetTrue)
                    .long("no-tag-version")
                    .conflicts_with("TAG_VERSION")
                    .help("Do not tag image with :\"{{pkg_version}}\" (default: no)"),
            )
            .arg(
                Arg::new("TAG_LATEST")
                    .action(ArgAction::SetTrue)
                    .long("tag-latest")
                    .conflicts_with("NO_TAG_LATEST")
                    .help("Tag image with :\"latest\" (default: yes)"),
            )
            .arg(
                Arg::new("NO_TAG_LATEST")
                    .action(ArgAction::SetTrue)
                    .long("no-tag-latest")
                    .conflicts_with("TAG_LATEST")
                    .help("Do not tag image with :\"latest\" (default: no)"),
            )
            .arg(
                Arg::new("TAG_CUSTOM")
                    .long("tag-custom")
                    .value_name("TAG_CUSTOM")
                    .help(
                        "Tag image with additional custom tag (supports: {{pkg_origin}}, \
                         {{pkg_name}}, {{pkg_version}}, {{pkg_release}}, {{channel}})",
                    ),
            );

        Cli { app }
    }

    pub fn add_publishing_args(self) -> Self {
        let app = self
            .app
            .arg(
                Arg::new("PUSH_IMAGE")
                    .action(ArgAction::SetTrue)
                    .long("push-image")
                    .conflicts_with("NO_PUSH_IMAGE")
                    .requires("REGISTRY_USERNAME")
                    .requires("REGISTRY_PASSWORD")
                    .help("Push image to remote registry (default: no)"),
            )
            .arg(
                Arg::new("NO_PUSH_IMAGE")
                    .action(ArgAction::SetTrue)
                    .long("no-push-image")
                    .conflicts_with("PUSH_IMAGE")
                    .help("Do not push image to remote registry (default: yes)"),
            )
            .arg(
                Arg::new("REGISTRY_USERNAME")
                    .long("username")
                    .short('U')
                    .value_name("REGISTRY_USERNAME")
                    .requires("REGISTRY_PASSWORD")
                    .help(
                        "Remote registry username, required for pushing image to remote registry",
                    ),
            )
            .arg(
                Arg::new("REGISTRY_PASSWORD")
                    .long("password")
                    .short('P')
                    .value_name("REGISTRY_PASSWORD")
                    .requires("REGISTRY_USERNAME")
                    .help(
                        "Remote registry password, required for pushing image to remote registry",
                    ),
            )
            .arg(
                Arg::new("REGISTRY_TYPE")
                    .value_parser(clap::builder::PossibleValuesParser::new(RegistryType::variants()))
                    .long("registry-type")
                    .short('R')
                    .value_name("REGISTRY_TYPE")
                    .help("Remote registry type (default: docker)"),
            )
            .arg(
                Arg::new("REGISTRY_URL")
                    // This is not strictly a requirement but will keep someone from
                    // making a mistake when inputing an ECR URL
                    .required_if_eq_any([("REGISTRY_TYPE", "amazon"), ("REGISTRY_TYPE", "azure")])
                    .long("registry-url")
                    .short('G')
                    .value_name("REGISTRY_URL")
                    .help("Remote registry url"),
            )
            // Cleanup
            .arg(
                Arg::new("RM_IMAGE")
                    .action(ArgAction::SetTrue)
                    .long("rm-image")
                    .help("Remove local image from engine after build and/or push (default: no)"),
            );

        Cli { app }
    }

    pub fn add_pkg_ident_arg(self, options: PkgIdentArgOptions) -> Self {
        let help = if options.multiple {
            "One or more Dojo package identifiers (ex: acme/redis) and/or filepaths to a Dojo \
             Artifact (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)"
        } else {
            "A Dojo package identifier (ex: acme/redis) and/or filepath to a Dojo Artifact (ex: \
             /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)"
        };

        let app = self.app
                      .arg(Arg::new("PKG_IDENT_OR_ARTIFACT").value_name("PKG_IDENT_OR_ARTIFACT")
                                                            .required(true)
                                                            .num_args(if options.multiple {
                                                                          ValueRange::new(1..)
                                                                      } else {
                                                                          ValueRange::SINGLE
                                                                      })
                                                            .help(help));

        Cli { app }
    }

    pub fn add_memory_arg(self) -> Self {
        let app = self.app
                      .arg(Arg::new("MEMORY_LIMIT").value_name("MEMORY_LIMIT")
                                                   .long("memory")
                                                   .short('m')
                                                   .help("Memory limit passed to docker \
                                                                build's --memory arg (ex: 2bg)"));

        Cli { app }
    }
}

fn valid_ident_or_hart(val: &str) -> result::Result<String, String> {
    if Path::new(val).is_file() {
        Ok(val.to_string())
    } else if val.ends_with(".hart") {
        Err(format!("Dojo artifact file: '{}' not found", val))
    } else {
        match PackageIdent::from_str(val) {
            Ok(_) => Ok(val.to_string()),
            Err(e) => Err(format!("{}", e)),
        }
    }
}

fn valid_url(val: &str) -> result::Result<String, String> {
    match Url::parse(val) {
        Ok(_) => Ok(val.to_string()),
        Err(_) => Err(format!("URL: '{}' is not valid", &val)),
    }
}
