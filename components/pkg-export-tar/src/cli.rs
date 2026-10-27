use clap::{Arg,
           Command};
use dojo_common::cli::HELP_TEMPLATE;
use std::{result,
          str::FromStr};

use crate::common::command::package::install::InstallSource;
use url::Url;

/// The version of this library and program when built.
pub const VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/VERSION"));

#[derive(Clone)]
pub struct Cli {
    pub app: Command,
}

impl Cli {
    pub fn new(name: &str, about: &str) -> Self {
        Cli { app: Command::new(name.to_string()).about(about.to_string())
                                                 .version(VERSION)
                                                 .author("Maintainer: Strip Mall Karate Company")
                                                 .help_template(HELP_TEMPLATE), }
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
            );

        Cli { app }
    }

    pub fn add_pkg_ident_arg(self) -> Self {
        let help = "A Dojo package identifier (ex: acme/redis) and/or filepath to a Dojo Artifact \
                    (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)";

        let app = self.app
                      .arg(Arg::new("PKG_IDENT_OR_ARTIFACT").value_name("PKG_IDENT_OR_ARTIFACT")
                                                            .required(true)
                                                            .help(help));

        Cli { app }
    }
}

fn valid_ident_or_hart(val: &str) -> result::Result<String, String> {
    match InstallSource::from_str(val) {
        Ok(_) => Ok(val.to_string()),
        Err(e) => Err(format!("{}", e)),
    }
}

fn valid_url(val: &str) -> result::Result<String, String> {
    match Url::parse(val) {
        Ok(_) => Ok(val.to_string()),
        Err(_) => Err(format!("URL: '{}' is not valid", &val)),
    }
}
