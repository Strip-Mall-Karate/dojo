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

use crate::command::studio;
use clap::{Arg,
           ArgAction,
           ArgGroup,
           Command};
use dojo_common::{cli::{BINLINK_DIR_ENVVAR,
                        DEFAULT_BINLINK_DIR,
                        GOSSIP_DEFAULT_ADDR,
                        GOSSIP_LISTEN_ADDRESS_ENVVAR,
                        HELP_TEMPLATE,
                        LISTEN_CTL_DEFAULT_ADDR_STRING,
                        LISTEN_HTTP_ADDRESS_ENVVAR,
                        LISTEN_HTTP_DEFAULT_ADDR,
                        PACKAGE_TARGET_ENVVAR,
                        RING_ENVVAR,
                        RING_KEY_ENVVAR},
                  types::ListenCtlAddr,
                  FeatureFlag};
use dojo_core::{crypto::{keys::PairType,
                         CACHE_KEY_PATH_ENV_VAR},
                env::Config,
                fs::CACHE_KEY_PATH,
                package::{ident,
                          Identifiable,
                          PackageIdent,
                          PackageTarget},
                service::{HealthCheckInterval,
                          ServiceGroup},
                ChannelIdent};
use dojo_sup_protocol;
use std::{net::SocketAddr,
          path::Path,
          result,
          str::FromStr};
use url::Url;

pub fn get(feature_flags: FeatureFlag) -> Command {
    let alias_apply = sub_config_apply().about("Alias for 'config apply'")
                                        .aliases(["ap", "app", "appl"])
                                        .hide(true);
    let alias_install =
        sub_pkg_install(feature_flags).about("Alias for 'pkg install'")
                                      .aliases(["i", "in", "ins", "inst", "insta", "instal"])
                                      .hide(true);
    let alias_setup = sub_cli_setup().about("Alias for 'cli setup'")
                                     .aliases(["set", "setu"])
                                     .hide(true);
    let alias_start = sub_svc_start().about("Alias for 'svc start'")
                                     .aliases(["sta", "star"])
                                     .hide(true);
    let alias_stop = sub_svc_stop().about("Alias for 'svc stop'")
                                   .aliases(["sto"])
                                   .hide(true);

    Command::new("doj")
.about("\"A Dojo is the natural environment for your services\" - Alan Turing")
.version(super::VERSION)
.author("Maintainer: Strip Mall Karate Company")
.help_template(HELP_TEMPLATE)
.propagate_version(true)
.arg_required_else_help(true)
.subcommand(Command::new("cli")
.about("Commands relating to Dojo runtime config")
.aliases(["cl"])
.arg_required_else_help(true)
.subcommand(sub_cli_setup().aliases(["s", "se", "set", "setu"]))
.subcommand(sub_cli_completers().aliases(["c", "co", "com", "comp"])))
.subcommand(Command::new("config")
.about("Commands relating to a Service's runtime config")
.aliases(["co", "con", "conf", "confi"])
.arg_required_else_help(true)
.subcommand(sub_config_apply().aliases(["ap", "app", "appl"]))
.subcommand(Command::new("show")
.about("Displays the default configuration options for a service")
.aliases(["sh", "sho"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2)"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))))
.subcommand(Command::new("file")
.about("Commands relating to Dojo files")
.aliases(["f", "fi", "fil"])
.arg_required_else_help(true)
.subcommand(Command::new("upload")
.about("Uploads a file to be shared between members of a Service Group")
.aliases(["u", "up", "upl", "uplo", "uploa"])
.arg(Arg::new("SERVICE_GROUP").value_parser(checked(valid_service_group)).required(true).help("Target service group service.group[@organization] (ex: redis.default or foo.default@bazcorp)"))
.arg(Arg::new("VERSION_NUMBER").required(true).help("A version number (positive integer) for this configuration (ex: 42)"))
.arg(Arg::new("FILE").value_parser(checked(file_exists)).required(true).help("Path to local file on disk"))
.arg(Arg::new("USER").long("user").short('u').help("Name of the user key"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))
.arg(arg_cache_key_path("Path to search for encryption keys. \
                    Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                    directory otherwise."))))
.subcommand(Command::new("bldr")
.about("Commands relating to Dojo Builder")
.aliases(["b", "bl", "bld"])
.arg_required_else_help(true)
.subcommand(Command::new("job")
.about("Commands relating to Dojo Builder jobs")
.aliases(["j", "jo"])
.arg_required_else_help(true)
.subcommand(Command::new("start")
.about("Schedule a build job or group of jobs")
.aliases(["s", "st", "sta", "star"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("The origin and name of the package to schedule a job for (eg: karate/redis)"))
.arg(arg_target())
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the cli.toml or DOJ_BLDR_URL environment variable if defined. \
                         (default: https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder"))
.arg(Arg::new("GROUP").long("group").short('g').action(ArgAction::SetTrue).help("Schedule jobs for this package and all of its reverse \
                        dependencies")))
.subcommand(Command::new("cancel")
.about("Cancel a build job group and any in-progress builds")
.aliases(["c", "ca", "can", "cance"])
.arg(Arg::new("GROUP_ID").required(true).help("The job group id that was returned from \"doj bldr job start\" \
                        (ex: 771100000000000000)"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("FORCE").long("force").short('f').action(ArgAction::SetTrue).help("Don't prompt for confirmation"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("promote")
.about("Promote packages from a completed build job to a specified channel")
.aliases(["p", "pr", "pro", "prom", "promo", "promot"])
.arg(Arg::new("GROUP_ID").required(true).help("The job id that was returned from \"doj bldr job start\" \
                        (ex: 771100000000000000)"))
.arg(Arg::new("CHANNEL").required(true).help("The target channel name"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("Limit the promotable packages to the specified origin"))
.arg(Arg::new("INTERACTIVE").long("interactive").short('i').action(ArgAction::SetTrue).help("Allow editing the list of promotable packages"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("demote")
.about("Demote packages from a completed build job to a specified channel")
.aliases(["d", "de", "dem", "demo", "demot"])
.arg(Arg::new("GROUP_ID").required(true).help("The job id that was returned from \"doj bldr start\" \
                        (ex: 771100000000000000)"))
.arg(Arg::new("CHANNEL").required(true).help("The target channel name"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("Limit the demotable packages to the specified origin"))
.arg(Arg::new("INTERACTIVE").long("interactive").short('i').action(ArgAction::SetTrue).help("Allow editing the list of demotable packages"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("status")
.about("Get the status of one or more job groups")
.aliases(["stat", "statu"])
.arg(Arg::new("GROUP_ID").help("The group id that was returned from \"doj bldr job start\" \
                            (ex: 771100000000000000)"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("Show the status of recent job groups created in this origin \
                            (default: 10 most recent)"))
.group(ArgGroup::new("status").required(true).args(["GROUP_ID", "ORIGIN"]))
.arg(Arg::new("LIMIT").long("limit").short('l').value_parser(checked(valid_numeric::<usize>)).help("Limit how many job groups to retrieve, ordered by most recent \
                        (default: 10)"))
.arg(Arg::new("SHOW_JOBS").long("showjobs").short('s').action(ArgAction::SetTrue).help("Show the status of all build jobs for a retrieved job group"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))))
.subcommand(Command::new("channel")
.about("Commands relating to Dojo Builder channels")
.aliases(["c", "ch", "cha", "chan", "chann", "channe"])
.arg_required_else_help(true)
.subcommand(Command::new("create")
.about("Creates a new channel")
.aliases(["c", "cr", "cre", "crea", "creat"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("CHANNEL").required(true).help("The channel name"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("Sets the origin to which the channel will belong. Default is from \
                        'DOJ_ORIGIN' or cli.toml")))
.subcommand(Command::new("destroy")
.about("Destroys a channel")
.aliases(["d", "de", "des", "dest", "destr", "destro"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("CHANNEL").required(true).help("The channel name"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("Sets the origin to which the channel belongs. Default is from 'DOJ_ORIGIN'\
                        or cli.toml")))
.subcommand(Command::new("list")
.about("Lists origin channels")
.aliases(["l", "li", "lis"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("ORIGIN").value_parser(checked(valid_origin)).help("The origin for which channels will be listed. Default is from 'DOJ_ORIGIN'\
                        or cli.toml")))))
.subcommand(Command::new("origin")
.about("Commands relating to Dojo origin keys")
.aliases(["o", "or", "ori", "orig", "origi"])
.arg_required_else_help(true)
.subcommand(Command::new("delete")
.about("Removes an unused/empty origin")
.aliases(["del", "dele"])
.arg(Arg::new("ORIGIN").value_parser(checked(valid_origin)).required(true).help("The origin name"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                     be taken from the `DOJ_BLDR_URL environment variable if defined. (default: \
                     https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("key")
.about("Commands relating to Dojo origin key maintenance")
.aliases(["k", "ke"])
.arg_required_else_help(true)
.subcommand(Command::new("download")
.about("Download origin key(s)")
.aliases(["d", "do", "dow", "down", "downl", "downlo", "downloa"])
.arg(arg_cache_key_path("Path to download keys to. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise."))
.arg(Arg::new("ORIGIN").value_parser(checked(valid_origin)).required(true).help("The origin name"))
.arg(Arg::new("REVISION").help("The key revision"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("WITH_SECRET").long("secret").short('s').action(ArgAction::SetTrue).help("Download secret signing key instead of public signing key"))
.arg(Arg::new("WITH_ENCRYPTION").long("encryption").short('e').action(ArgAction::SetTrue).help("Download public encryption key instead of public signing key"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder \
                        (required for downloading secret keys)")))
.subcommand(Command::new("export")
.about("Outputs the latest origin key contents to stdout")
.aliases(["e", "ex", "exp", "expo", "expor"])
.arg(Arg::new("ORIGIN").value_parser(checked(valid_origin)).required(true))
.arg(Arg::new("PAIR_TYPE").long("type").short('t').value_parser(checked(valid_pair_type)).help("Export either the 'public' or 'secret' key"))
.arg(arg_cache_key_path("Path to export keys from. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))
.subcommand(Command::new("generate")
.about("Generates a Dojo origin key")
.aliases(["g", "ge", "gen", "gene", "gener", "genera", "generat"])
.arg(Arg::new("ORIGIN").value_parser(checked(valid_origin)).help("The origin name"))
.arg(arg_cache_key_path("Path to store generated keys. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))
.subcommand(Command::new("import")
.about("Reads a stdin stream containing a public or secret origin key \
                        contents and writes the key to disk")
.aliases(["i", "im", "imp", "impo", "impor"])
.arg(arg_cache_key_path("Path to import keys to. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))
.subcommand(Command::new("upload")
.arg(Arg::new("ORIGIN").value_parser(checked(valid_origin)).help("The origin name"))
.arg(Arg::new("PUBLIC_FILE").long("pubfile").value_parser(checked(file_exists)).help("Path to a local public origin key file on disk"))
.group(ArgGroup::new("upload").required(true).args(["ORIGIN", "PUBLIC_FILE"]))
.about("Upload origin keys to Builder")
.aliases(["u", "up", "upl", "uplo", "uploa"])
.arg(arg_cache_key_path("Path to upload keys from. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise."))
.arg(Arg::new("WITH_SECRET").long("secret").short('s').conflicts_with("PUBLIC_FILE").action(ArgAction::SetTrue).help("Upload secret key in addition to the public key"))
.arg(Arg::new("SECRET_FILE").long("secfile").conflicts_with("ORIGIN").value_parser(checked(file_exists)).help("Path to a local secret origin key file on disk"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder"))))
.subcommand(Command::new("secret")
.about("Commands related to secret management")
.arg_required_else_help(true)
.subcommand(Command::new("upload")
.about("Create and upload a secret for your origin.")
.arg(Arg::new("KEY_NAME").required(true).help("The name of the variable key to be injected into the studio. \
                        Ex: KEY=\"some_value\""))
.arg(Arg::new("SECRET").required(true).help("The contents of the variable to be injected into the studio."))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("The origin for which the secret will be uploaded. Default is from \
                        'DOJ_ORIGIN' or cli.toml"))
.arg(arg_cache_key_path("Path to public encryption key. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))
.subcommand(Command::new("delete")
.about("Delete a secret for your origin")
.arg(Arg::new("KEY_NAME").required(true).help("The name of the variable key to be injected into the studio."))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("The origin for which the secret will be deleted. Default is from \
                        'DOJ_ORIGIN' or cli.toml")))
.subcommand(Command::new("list")
.about("List all secrets for your origin")
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("The origin for which secrets will be listed. Default is from 'DOJ_ORIGIN' \
                        or cli.toml")))))
.subcommand(Command::new("pkg")
.about("Commands relating to Dojo packages")
.aliases(["p", "pk", "package"])
.arg_required_else_help(true)
.subcommand(Command::new("binds")
.about("Displays the binds for a service")
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-statis/1.42.2)")))
.subcommand(Command::new("binlink")
.about("Creates a binlink for a package binary in a common 'PATH' location")
.aliases(["bi", "bin", "binl", "binli", "binlin"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2)"))
.arg(Arg::new("BINARY").help("The command to binlink (ex: bash)"))
.arg(Arg::new("DEST_DIR").long("dest").short('d').env(BINLINK_DIR_ENVVAR).default_value(DEFAULT_BINLINK_DIR).value_parser(checked(non_empty)).help("Sets the destination directory"))
.arg(Arg::new("FORCE").long("force").short('f').action(ArgAction::SetTrue).help("Overwrite existing binlinks")))
.subcommand(Command::new("config")
.about("Displays the default configuration options for a service")
.aliases(["conf", "cfg"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2)")))
.subcommand(sub_pkg_build())
.subcommand(Command::new("env")
.about("Prints the runtime environment of installed packages as shell exports, \
                    the first package named taking precedence")
.arg(Arg::new("PKG_IDENT").num_args(1..).value_parser(checked(valid_ident)).required(true).help("Package identifiers (ex: karate/redis, karate/busybox-static/1.42.2)")))
.subcommand(Command::new("exec")
.about("Executes a command using the 'PATH' context of an installed package")
.aliases(["exe"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2)"))
.arg(Arg::new("CMD").required(true).help("The command to execute (ex: ls)"))
.arg(Arg::new("ARGS").num_args(1..).help("Arguments to the command (ex: -l /tmp)")))
.subcommand(Command::new("export")
.about("Exports the package to the specified format")
.aliases(["exp"])
.arg(Arg::new("FORMAT").required(true).help("The export format (ex: docker or tar)"))
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2) or \
                    filepath to a Dojo Artifact \
                    (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                     be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                     https://bldr.karate.company)"))
.arg(Arg::new("CHANNEL").long("channel").short('c').default_value("stable").env(ChannelIdent::ENVVAR).help("Retrieve the container's package from the specified release channel")))
.subcommand(Command::new("hash")
.about("Generates a blake2b hashsum from a target at any given filepath")
.aliases(["ha", "has"])
.arg(Arg::new("SOURCE").value_parser(checked(file_exists)).help("A filepath of the target")))
.subcommand(sub_pkg_install(feature_flags).aliases(
                &["i", "in", "ins", "inst", "insta", "instal"]))
.subcommand(Command::new("path")
.about("Prints the path to a specific installed release of a package")
.aliases(["p", "pa", "pat"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2)")))
.subcommand(Command::new("list")
.about("List all versions of installed packages")
.aliases(["li"])
.arg(Arg::new("ALL").long("all").short('a').action(ArgAction::SetTrue).help("List all installed packages"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("An origin to list"))
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2)."))
.group(ArgGroup::new("prefix").required(true).args(["ALL", "ORIGIN", "PKG_IDENT"])))
.subcommand(Command::new("provides")
.about("Search installed Dojo packages for a given file")
.arg(Arg::new("FILE").required(true).help("File name to find"))
.arg(Arg::new("FULL_RELEASES").short('r').action(ArgAction::SetTrue).help("Show fully qualified package names \
                    (ex: karate/busybox-static/1.24.2/20160708162350)"))
.arg(Arg::new("FULL_PATHS").short('p').action(ArgAction::SetTrue).help("Show full path to file")))
.subcommand(Command::new("search")
.about("Search for a package in Builder")
.arg(Arg::new("SEARCH_TERM").required(true).help("Search term"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder \
                    endpoint. If not specified, the value will be taken from the DOJ_BLDR_URL \
                    environment variable if defined. (default: https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("sign")
.about("Signs an archive with an origin key, generating a Dojo Artifact")
.aliases(["s", "si", "sig"])
.arg(Arg::new("ORIGIN").long("origin").value_parser(checked(valid_origin)).help("Origin key used to create signature"))
.arg(Arg::new("SOURCE").value_parser(checked(file_exists)).required(true).help("A path to a source archive file \
                    (ex: /home/acme-redis-3.0.7-21120102031201.tar.xz)"))
.arg(Arg::new("DEST").required(true).help("The destination path to the signed Dojo Artifact \
                    (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)"))
.arg(arg_cache_key_path("Path to search for origin keys. \
                    Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                    directory otherwise.")))
.subcommand(Command::new("uninstall")
.about("Safely uninstall a package and dependencies from the local filesystem")
.aliases(["un", "unin"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2/21120102031201)"))
.arg(Arg::new("DRYRUN").long("dryrun").short('d').action(ArgAction::SetTrue).help("Just show what would be uninstalled, don't actually do it"))
.arg(Arg::new("EXCLUDE").long("exclude").num_args(1..).action(ArgAction::Append).value_parser(checked(valid_ident)).help("Identifier of one or more packages that should not be uninstalled. \
                    (ex: karate/redis, karate/busybox-static/1.42.2/21120102031201)"))
.arg(Arg::new("NO_DEPS").long("no-deps").action(ArgAction::SetTrue).help("Don't uninstall dependencies")))
.subcommand(Command::new("upload")
.about("Uploads a local Dojo Artifact to Builder")
.aliases(["u", "up", "upl", "uplo", "uploa"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder \
                    endpoint. If not specified, the value will be taken from the DOJ_BLDR_URL \
                    environment variable if defined. (default: https://bldr.karate.company)"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder"))
.arg(Arg::new("CHANNEL").long("channel").short('c').help("Optional additional release channel to upload package to. \
                     Packages are always uploaded to `unstable`, regardless \
                     of the value of this option."))
.arg(Arg::new("FORCE").long("force").action(ArgAction::SetTrue).help("Skips checking availability of package and \
                    force uploads, potentially overwriting a stored copy of a package. \
                    (default: false)"))
.arg(Arg::new("HART_FILE").num_args(1..).value_parser(checked(file_exists)).required(true).help("One or more filepaths to a Dojo Artifact \
                    (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)"))
.arg(arg_cache_key_path("Path to search for public origin keys to upload. \
                    Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                    directory otherwise.")))
.subcommand(Command::new("delete")
.about("Removes a package from Builder")
.aliases(["del", "dele"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder \
                    endpoint. If not specified, the value will be taken from the DOJ_BLDR_URL \
                    environment variable if defined. (default: https://bldr.karate.company)"))
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_fully_qualified_ident)).required(true).help("A fully qualified package identifier \
                    (ex: karate/busybox-static/1.42.2/20170513215502)"))
.arg(arg_target())
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("promote")
.about("Promote a package to a specified channel")
.aliases(["pr", "pro", "promo", "promot"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder \
                    endpoint. If not specified, the value will be taken from the DOJ_BLDR_URL \
                    environment variable if defined. (default: https://bldr.karate.company)"))
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_fully_qualified_ident)).required(true).help("A fully qualified package identifier \
                    (ex: karate/busybox-static/1.42.2/20170513215502)"))
.arg(Arg::new("CHANNEL").required(true).help("Promote to the specified release channel"))
.arg(arg_target())
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("demote")
.about("Demote a package from a specified channel")
.aliases(["de", "dem", "demo", "demot"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder \
                    endpoint. If not specified, the value will be taken from the DOJ_BLDR_URL \
                    environment variable if defined. (default: https://bldr.karate.company)"))
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_fully_qualified_ident)).required(true).help("A fully qualified package identifier \
                    (ex: karate/busybox-static/1.42.2/20170513215502)"))
.arg(Arg::new("CHANNEL").required(true).help("Demote from the specified release channel"))
.arg(arg_target())
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("channels")
.about("Find out what channels a package belongs to")
.aliases(["ch", "cha", "chan", "chann", "channe", "channel"])
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder \
                    endpoint. If not specified, the value will be taken from the DOJ_BLDR_URL \
                    environment variable if defined. (default: https://bldr.karate.company)"))
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_fully_qualified_ident)).required(true).help("A fully qualified package identifier \
                    (ex: karate/busybox-static/1.42.2/20170513215502)"))
.arg(arg_target())
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder")))
.subcommand(Command::new("verify")
.about("Verifies a Dojo Artifact with an origin key")
.aliases(["v", "ve", "ver", "veri", "verif"])
.arg(Arg::new("SOURCE").value_parser(checked(file_exists)).required(true).help("A path to a Dojo Artifact \
                    (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)"))
.arg(arg_cache_key_path("Path to search for public origin keys for verification. \
                    Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                    directory otherwise.")))
.subcommand(Command::new("header")
.about("Returns the Dojo Artifact header")
.aliases(["hea", "head", "heade"])
.hide(true)
.arg(Arg::new("SOURCE").value_parser(checked(file_exists)).required(true).help("A path to a Dojo Artifact \
                    (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)")))
.subcommand(Command::new("info")
.about("Returns the Dojo Artifact information")
.aliases(["inf"])
.arg(Arg::new("TO_JSON").long("json").short('j').action(ArgAction::SetTrue).help("Output will be rendered in json"))
.arg(Arg::new("SOURCE").value_parser(checked(file_exists)).required(true).help("A path to a Dojo Artifact \
                    (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)")))
.subcommand(Command::new("dependencies")
.about("Returns the Dojo Artifact dependencies. By default it will return \
                    the direct dependencies of the package")
.aliases(["dep", "deps"])
.arg(Arg::new("TRANSITIVE").long("transitive").short('t').action(ArgAction::SetTrue).help("Show transitive dependencies"))
.arg(Arg::new("REVERSE").long("reverse").short('r').action(ArgAction::SetTrue).help("Show packages which are dependant on this one"))
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A package identifier (ex: karate/redis, karate/busybox-static/1.42.2)"))))
.subcommand(Command::new("plan")
.about("Commands relating to plans and other app-specific configuration.")
.aliases(["pl", "pla"])
.arg_required_else_help(true)
.subcommand(Command::new("init")
.about("Generates common package specific configuration files. Executing without \
                    argument will create a `dojo` directory in your current folder for the \
                    plan. If `PKG_NAME` is specified it will create a folder with that name. \
                    Environment variables (those starting with 'pkg_') that are set will be used \
                    in the generated plan")
.aliases(["i", "in", "ini"])
.arg(Arg::new("PKG_NAME").help("Name for the new app"))
.arg(Arg::new("ORIGIN").long("origin").short('o').value_parser(checked(valid_origin)).help("Origin for the new app"))
.arg(Arg::new("WITH_DOCS").long("with-docs").action(ArgAction::SetTrue).help("Include plan options documentation"))
.arg(Arg::new("WITH_CALLBACKS").long("with-callbacks").action(ArgAction::SetTrue).help("Include callback functions in template"))
.arg(Arg::new("WITH_ALL").long("with-all").action(ArgAction::SetTrue).help("Generate omnibus plan with all available plan options"))
.arg(Arg::new("SCAFFOLDING").long("scaffolding").short('s').help("Specify explicit Scaffolding for your app (ex: node, ruby)")))
.subcommand(Command::new("render")
.about("Renders plan config files")
.aliases(["r", "re", "ren", "rend", "rende"])
.arg(Arg::new("TEMPLATE_PATH").value_parser(checked(file_exists)).required(true).help("Path to config to render"))
.arg(Arg::new("DEFAULT_TOML").long("default-toml").short('d').default_value("./default.toml").help("Path to default.toml"))
.arg(Arg::new("USER_TOML").long("user-toml").short('u').help("Path to user.toml, defaults to none"))
.arg(Arg::new("MOCK_DATA").long("mock-data").short('m').help("Path to json file with mock data for template, defaults to none"))
.arg(Arg::new("PRINT").long("print").short('p').action(ArgAction::SetTrue).help("Prints config to STDOUT"))
.arg(Arg::new("RENDER_DIR").long("render-dir").short('r').default_value("./results").help("Path to render templates"))
.arg(Arg::new("NO_RENDER").long("no-render").short('n').action(ArgAction::SetTrue).help("Don't write anything to disk, ignores --render-dir"))
.arg(Arg::new("QUIET").long("quiet").short('q').action(ArgAction::SetTrue).help("Don't print any helper messages.  When used with `--print` will only print config file"))))
.subcommand(Command::new("ring")
.about("Commands relating to Dojo rings")
.aliases(["r", "ri", "rin"])
.arg_required_else_help(true)
.subcommand(Command::new("key")
.about("Commands relating to Dojo ring keys")
.aliases(["k", "ke"])
.arg_required_else_help(true)
.subcommand(Command::new("export")
.about("Outputs the latest ring key contents to stdout")
.aliases(["e", "ex", "exp", "expo", "expor"])
.arg(Arg::new("RING").required(true).help("Ring key name"))
.arg(arg_cache_key_path("Path to search for keys. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))
.subcommand(Command::new("import")
.about("Reads a stdin stream containing ring key contents and writes \
                    the key to disk")
.aliases(["i", "im", "imp", "impo", "impor"])
.arg(arg_cache_key_path("Path to store imported keys. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))
.subcommand(Command::new("generate")
.about("Generates a Dojo ring key")
.aliases(["g", "ge", "gen", "gene", "gener", "genera", "generat"])
.arg(Arg::new("RING").required(true).help("Ring key name"))
.arg(arg_cache_key_path("Path to store generated keys. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))))
.subcommand(Command::new("shell")
.about("Commands relating to a project's dev shell: the packages its shell.toml \
                names, pinned in its shell.lock")
.arg_required_else_help(true)
.subcommand(Command::new("lock")
.about("Pins each package shell.toml names to a release in the archive cache, \
                    for this machine's target, in shell.lock. A package already pinned keeps its \
                    release")
.arg(Arg::new("UPDATE").long("update").action(ArgAction::SetTrue).help("Pins each package to the newest release in the cache, \
                    even one already pinned")))
.subcommand(Command::new("env")
.about("Installs what shell.lock pins, from the archive cache, and prints the \
                    shell's environment as shell exports")))
.subcommand(sup_commands())
.subcommand(Command::new("svc")
.about("Commands relating to Dojo services")
.aliases(["sv", "ser", "serv", "service"])
.arg_required_else_help(true)
.subcommand(Command::new("key")
.about("Commands relating to Dojo service keys")
.aliases(["k", "ke"])
.arg_required_else_help(true)
.subcommand(Command::new("generate")
.about("Generates a Dojo service key")
.aliases(["g", "ge", "gen", "gene", "gener", "genera", "generat"])
.arg(Arg::new("SERVICE_GROUP").value_parser(checked(valid_service_group)).required(true).help("Target service group service.group[@organization] (ex: redis.default or foo.default@bazcorp)"))
.arg(Arg::new("ORG").help("The service organization"))
.arg(arg_cache_key_path("Path to store generated keys. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise."))))
.subcommand(sub_svc_load().aliases(["l", "lo", "loa"]))
.subcommand(sub_svc_start().aliases(["star"]))
.subcommand(sub_svc_status().aliases(["stat", "statu"]))
.subcommand(sub_svc_stop().aliases(["sto"]))
.subcommand(Command::new("unload")
.about("Unload a service loaded by the Dojo Supervisor. If the service is \
                    running it will additionally be stopped.")
.aliases(["u", "un", "unl", "unlo", "unloa"])
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A Dojo package identifier (ex: karate/redis)"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))))
.subcommand(Command::new("studio")
.about("Commands relating to Dojo Studios")
.aliases(["stu", "stud", "studi"]))
.subcommand(Command::new("supportbundle")
.about("Create a tarball of Dojo Supervisor data to send to support")
.aliases(["supp", "suppo", "suppor", "support-bundle"]))
.subcommand(Command::new("user")
.about("Commands relating to Dojo users")
.aliases(["u", "us", "use"])
.arg_required_else_help(true)
.subcommand(Command::new("key")
.about("Commands relating to Dojo user keys")
.aliases(["k", "ke"])
.arg_required_else_help(true)
.subcommand(Command::new("generate")
.about("Generates a Dojo user key")
.aliases(["g", "ge", "gen", "gene", "gener", "genera", "generat"])
.arg(Arg::new("USER").required(true).help("Name of the user key"))
.arg(arg_cache_key_path("Path to store generated keys. \
                        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
                        directory otherwise.")))))
.subcommand(alias_apply)
.subcommand(alias_install)
.subcommand(alias_run())
.subcommand(alias_setup)
.subcommand(alias_start)
.subcommand(alias_stop)
.subcommand(alias_term())
.after_help("\nALIASES:\
            \n    apply      Alias for: 'config apply'\
            \n    install    Alias for: 'pkg install'\
            \n    run        Alias for: 'sup run'\
            \n    setup      Alias for: 'cli setup'\
            \n    start      Alias for: 'svc start'\
            \n    stop       Alias for: 'svc stop'\
            \n    term       Alias for: 'sup term'\
            \n")
}

fn alias_run() -> Command {
    Command::new("run").about("Run the Dojo Supervisor")
                       .hide(true)
}

fn alias_term() -> Command {
    Command::new("term")
.about("Gracefully terminate the Dojo Supervisor and all of its running services")
.hide(true)
}

fn sub_cli_setup() -> Command {
    Command::new("setup").about("Sets up the CLI with reasonable defaults.")
                         .arg(arg_cache_key_path(
        "Path to search for or create keys in. \
        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
        directory otherwise.",
    ))
}

pub fn sup_commands() -> Command {
    // Define all of the `doj sup *` subcommands in one place.
    // This removes the need to duplicate this in `doj-sup`.
    // The 'sup' App name here is significant for the `doj` binary as it
    // is inserted as a named subcommand. For the `doj-sup` binary, it is
    // the top-level App name (not a named subcommand) and therefore is not
    // significant since we override `usage` below.
    Command::new("sup").about("The Dojo Supervisor")
                       .version(super::VERSION)
                       .author("Maintainer: Strip Mall Karate Company")
                       .help_template(HELP_TEMPLATE)
                       .override_usage("doj sup <SUBCOMMAND>")
                       .subcommand_required(true)
                       .arg_required_else_help(true)
                       .subcommand(sub_sup_bash().aliases(["b", "ba", "bas"]))
                       .subcommand(sub_sup_depart().aliases(["d", "de", "dep", "depa"]))
                       .subcommand(sub_sup_run().aliases(["r", "ru"]))
                       .subcommand(sub_sup_secret().aliases(["sec", "secr"]))
                       .subcommand(sub_sup_sh())
                       .subcommand(sub_svc_status().aliases(["stat", "statu"]))
                       .subcommand(sub_sup_term().aliases(["ter"]))
}

fn sub_cli_completers() -> Command {
    let sub = Command::new("completers").about("Creates command-line completers for your shell.");

    let supported_shells = ["bash", "fish", "zsh", "powershell"];

    sub.arg(Arg::new("SHELL").help("The name of the shell you want to generate the \
                                          command-completion. Supported Shells: bash, fish, zsh, \
                                          powershell")
                             .short('s')
                             .long("shell")
                             .required(true)
                             .value_parser(supported_shells))
}

// The default value makes the argument always present, and clap checks a default as it checks
// a given value. We hide the default because it's a special value that will be internally mapped
// according to the user type. This is to allow us to apply consistent validation to the env var
// override.
fn arg_cache_key_path(help_text: &'static str) -> Arg {
    Arg::new("CACHE_KEY_PATH").long("cache-key-path")
                              .value_parser(checked(non_empty))
                              .env(CACHE_KEY_PATH_ENV_VAR)
                              .default_value(CACHE_KEY_PATH)
                              .hide_default_value(true)
                              .help(help_text)
}

fn arg_target() -> Arg {
    Arg::new("PKG_TARGET").required(false)
                          .value_parser(checked(valid_target))
                          .env(PACKAGE_TARGET_ENVVAR)
                          .help("A package target (ex: aarch64-darwin) (default: system \
                                       appropriate target")
}

fn sub_pkg_build() -> Command {
    let mut sub = Command::new("build")
.about("Builds a Plan using a Studio")
.arg(Arg::new("DOJ_ORIGIN_KEYS").long("keys").short('k').help("Installs secret origin keys (ex: \"unicorn\", \"acme,other,acme-ops\")"))
.arg(Arg::new("DOJ_STUDIO_ROOT").long("root").short('r').help("Sets the Studio root (default: /doj/studios/<DIR_NAME>)"))
.arg(Arg::new("SRC_PATH").long("src").short('s').help("Sets the source path (default: $PWD)"))
.arg(Arg::new("PLAN_CONTEXT").required(true).help("A directory containing a plan file \
        or a `dojo/` directory which contains the plan file"))
.arg(arg_cache_key_path("Path to search for origin keys. \
        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
        directory otherwise."));
    // Only a truly native/local Studio can be reused--the Docker implementation will always be
    // ephemeral
    if studio::native_studio_support() {
        sub = sub.arg(Arg::new("REUSE").action(ArgAction::SetTrue)
                                       .help("Reuses a previous Studio for the build \
                                                    (default: clean up before building)")
                                       .short('R')
                                       .long("reuse"))
                 .arg(Arg::new("DOCKER").action(ArgAction::SetTrue)
                                        .help("Uses a Dockerized Studio for the build")
                                        .short('D')
                                        .long("docker"));
    }

    sub
}

fn sub_pkg_install(feature_flags: FeatureFlag) -> Command {
    let mut sub = Command::new("install")
.about("Installs a Dojo package from Builder or locally from a Dojo Artifact")
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
                         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
                         https://bldr.karate.company)"))
.arg(Arg::new("CHANNEL").long("channel").short('c').default_value("stable").env(ChannelIdent::ENVVAR).help("Install from the specified release channel"))
.arg(Arg::new("PKG_IDENT_OR_ARTIFACT").num_args(1..).required(true).help("One or more Dojo package identifiers (ex: acme/redis) and/or filepaths \
            to a Dojo Artifact (ex: /home/acme-redis-3.0.7-21120102031201-x86_64-linux.hart)"))
.arg(Arg::new("BINLINK").long("binlink").short('b').action(ArgAction::SetTrue).help("Binlink all binaries from installed package(s)"))
.arg(Arg::new("FORCE").long("force").short('f').action(ArgAction::SetTrue).help("Overwrite existing binlinks"))
.arg(Arg::new("AUTH_TOKEN").long("auth").short('z').help("Authentication token for Builder"));
    if feature_flags.contains(FeatureFlag::OFFLINE_INSTALL) {
        sub = sub.arg(Arg::new("OFFLINE").action(ArgAction::SetTrue)
                                         .help("Install packages in offline mode")
                                         .long("offline"));
    };
    if feature_flags.contains(FeatureFlag::IGNORE_LOCAL) {
        sub = sub.arg(Arg::new("IGNORE_LOCAL").action(ArgAction::SetTrue)
                                              .help("Do not use locally-installed \
                                                           packages when a corresponding \
                                                           package cannot be installed from \
                                                           Builder")
                                              .long("ignore-local"));
    };
    if feature_flags.contains(FeatureFlag::INSTALL_HOOK) {
        sub = sub.arg(Arg::new("IGNORE_INSTALL_HOOK").action(ArgAction::SetTrue)
                                                     .help("Do not run any install hooks")
                                                     .long("ignore-install-hook"));
    };
    sub
}

fn sub_config_apply() -> Command {
    Command::new("apply")
.about("Sets a configuration to be shared by members of a Service Group")
.arg(Arg::new("SERVICE_GROUP").value_parser(checked(valid_service_group)).required(true).help("Target service group service.group[@organization] (ex: redis.default or foo.default@bazcorp)"))
.arg(Arg::new("VERSION_NUMBER").required(true).help("A version number (positive integer) for this configuration (ex: 42)"))
.arg(Arg::new("FILE").value_parser(checked(file_exists_or_stdin)).help("Path to local file on disk (ex: /tmp/config.toml, default: <stdin>)"))
.arg(Arg::new("USER").long("user").short('u').help("Name of a user key to use for encryption"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))
.arg(arg_cache_key_path("Path to search for encryption keys. \
        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
        directory otherwise."))
}

// the following sup related functions are
// public due to their utilization in `doj-sup`
// for consistency, all supervisor related clap subcommands are defined in this module
pub fn sub_sup_depart() -> Command {
    Command::new("depart")
.about("Depart a Supervisor from the gossip ring; kicking and banning the target \
            from joining again with the same member-id")
.arg(Arg::new("MEMBER_ID").required(true).help("The member-id of the Supervisor to depart"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))
}

pub fn sub_sup_secret() -> Command {
    Command::new("secret")
.about("Commands relating to a Dojo Supervisor's Control Gateway secret")
.arg_required_else_help(true)
.subcommand(Command::new("generate")
.about("Generate a secret key to use as a Supervisor's Control Gateway secret"))
}

pub fn sub_sup_bash() -> Command {
    Command::new("bash").about("Start an interactive Bash-like shell")
                        .override_usage("doj sup bash")
}

pub fn sub_sup_run() -> Command {
    Command::new("run")
.about("Run the Dojo Supervisor")
.override_usage("doj sup run [FLAGS] [OPTIONS] [--] [PKG_IDENT_OR_ARTIFACT]")
.arg(Arg::new("LISTEN_GOSSIP").long("listen-gossip").env(GOSSIP_LISTEN_ADDRESS_ENVVAR).default_value(GOSSIP_DEFAULT_ADDR.as_str()).value_parser(checked(valid_socket_addr)).help("The listen address for the Gossip System Gateway."))
.arg(Arg::new("LOCAL_GOSSIP_MODE").long("local-gossip-mode").conflicts_with("LISTEN_GOSSIP").conflicts_with("PEER").conflicts_with("PEER_WATCH_FILE").action(ArgAction::SetTrue).help("Start the supervisor in local mode."))
.arg(Arg::new("LISTEN_HTTP").long("listen-http").env(LISTEN_HTTP_ADDRESS_ENVVAR).default_value(LISTEN_HTTP_DEFAULT_ADDR.as_str()).value_parser(checked(valid_socket_addr)).help("The listen address for the HTTP Gateway."))
.arg(Arg::new("HTTP_DISABLE").long("http-disable").short('D').action(ArgAction::SetTrue).help("Disable the HTTP Gateway completely [default: false]"))
.arg(Arg::new("LISTEN_CTL").long("listen-ctl").env(ListenCtlAddr::ENVVAR).default_value(LISTEN_CTL_DEFAULT_ADDR_STRING.as_str()).value_parser(checked(valid_socket_addr)).help("The listen address for the Control Gateway. If not specified, the value will \
        be taken from the DOJ_LISTEN_CTL environment variable if defined. [default: 127.0.0.1:9632]"))
.arg(Arg::new("ORGANIZATION").long("org").help("The organization that the Supervisor and its subsequent services are part of."))
.arg(Arg::new("PEER").long("peer").num_args(1..).action(ArgAction::Append).help("The listen address of one or more initial peers (IP[:PORT])"))
.arg(Arg::new("PERMANENT_PEER").long("permanent-peer").short('I').action(ArgAction::SetTrue).help("If this Supervisor is a permanent peer"))
.arg(Arg::new("PEER_WATCH_FILE").long("peer-watch-file").conflicts_with("PEER").help("Watch this file for connecting to the ring"))
.arg(arg_cache_key_path("Path to search for encryption keys. \
        Default value is doj/cache/keys if root and .doj/cache/keys under the home \
        directory otherwise."))
.arg(Arg::new("RING").long("ring").short('r').env(RING_ENVVAR).conflicts_with("RING_KEY").value_parser(checked(non_empty)).help("The name of the ring used by the Supervisor when running with wire encryption. \
         (ex: doj sup run --ring myring)"))
.arg(Arg::new("RING_KEY").long("ring-key").env(RING_KEY_ENVVAR).conflicts_with("RING").value_parser(checked(non_empty)).hide(true).help("The contents of the ring key when running with wire encryption. \
             (Note: This option is explicitly undocumented and for testing purposes only. Do not use it in a production system. Use the corresponding environment variable instead.)
             (ex: doj sup run --ring-key 'SYM-SEC-1 \
              foo-20181113185935 \

                  GCrBOW6CCN75LMl0j2V5QqQ6nNzWm6and9hkKBSUFPI=')"))
.arg(Arg::new("CHANNEL").long("channel").default_value("stable").help("Receive Supervisor updates from the specified release channel"))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
         be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
         https://bldr.karate.company)"))
.arg(Arg::new("CONFIG_DIR").long("config-from").value_parser(checked(dir_exists)).help("Use package config from this path, rather than the package itself"))
.arg(Arg::new("AUTO_UPDATE").long("auto-update").short('A').action(ArgAction::SetTrue).help("Enable automatic updates for the Supervisor \
        itself"))
.arg(Arg::new("KEY_FILE").long("key").requires("CERT_FILE").value_parser(checked(file_exists)).help("Used for enabling TLS for the HTTP gateway. Read private key from KEY_FILE. \
         This should be a RSA private key or PKCS8-encoded private key, in PEM format."))
.arg(Arg::new("CERT_FILE").long("certs").requires("KEY_FILE").value_parser(checked(file_exists)).help("Used for enabling TLS for the HTTP gateway. Read server certificates from CERT_FILE. \
         This should contain PEM-format certificates in the right order (the first certificate \
         should certify KEY_FILE, the last should be a root CA)."))
.arg(Arg::new("CA_CERT_FILE").long("ca-certs").requires("CERT_FILE").requires("KEY_FILE").value_parser(checked(file_exists)).help("Used for enabling client-authentication with TLS for the HTTP gateway. Read CA certificate from CA_CERT_FILE. \
         This should contain PEM-format certificate that can be used to validate client requests."))
.arg(Arg::new("PKG_IDENT_OR_ARTIFACT").help("Load the given Dojo package as part of \
        the Supervisor startup specified by a package identifier \
        (ex: karate/redis) or filepath to a Dojo Artifact \
        (ex: /home/core-redis-3.0.7-21120102031201-x86_64-linux.hart)."))
.arg(Arg::new("APPLICATION").long("application").short('a').requires("ENVIRONMENT").help("Application name; [default: not set]."))
.arg(Arg::new("ENVIRONMENT").long("environment").short('e').requires("APPLICATION").help("Environment name; [default: not set]."))
.arg(Arg::new("GROUP").long("group").help("The service group; shared config and topology [default: default]."))
.arg(Arg::new("TOPOLOGY").long("topology").short('t').value_parser(["standalone", "leader"]).help("Service topology; [default: none]"))
.arg(Arg::new("STRATEGY").long("strategy").short('s').value_parser(checked(valid_update_strategy)).help("The update strategy; [default: none] [values: none, at-once, rolling]"))
.arg(Arg::new("BIND").long("bind").num_args(1..).action(ArgAction::Append).help("One or more service groups to bind to a configuration"))
.arg(Arg::new("BINDING_MODE").long("binding-mode").value_parser(checked(valid_binding_mode)).help("Governs how the presence or absence of binds affects service startup. `strict` blocks \
         startup until all binds are present. [default: strict] [values: relaxed, strict]"))
.arg(Arg::new("VERBOSE").short('v').action(ArgAction::SetTrue).help("Verbose output; shows file and line/column numbers"))
.arg(Arg::new("NO_COLOR").long("no-color").action(ArgAction::SetTrue).help("Turn ANSI color off"))
.arg(Arg::new("JSON").long("json-logging").action(ArgAction::SetTrue).help("Use structured JSON logging for the Supervisor. \
        Implies NO_COLOR"))
.arg(Arg::new("HEALTH_CHECK_INTERVAL").long("health-check-interval").short('i').value_parser(checked(valid_health_check_interval)).help("The interval (seconds) on which to run health checks [default: 30]"))
}

pub fn sub_sup_sh() -> Command {
    Command::new("sh").about("Start an interactive Bourne-like shell")
                      .override_usage("doj sup sh")
}

pub fn sub_sup_term() -> Command {
    Command::new("term")
.about("Gracefully terminate the Dojo Supervisor and all of its running services")
.override_usage("doj sup term [OPTIONS]")
}

fn sub_svc_start() -> Command {
    Command::new("start")
.about("Start a loaded, but stopped, Dojo service.")
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A Dojo package identifier (ex: karate/redis)"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))
}

// `doj svc status` is the canonical location for this command, but we
// have historically used `doj sup status` as an alias.
pub fn sub_svc_status() -> Command {
    Command::new("status")
.about("Query the status of Dojo services.")
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).help("A Dojo package identifier (ex: karate/redis)"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))
}

fn sub_svc_stop() -> Command {
    Command::new("stop")
.about("Stop a running Dojo service.")
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A Dojo package identifier (ex: karate/redis)"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))
}

fn sub_svc_load() -> Command {
    let sub = Command::new("load")
.about("Load a service to be started and supervised by Dojo from a package \
            identifier. If an installed package doesn't satisfy the given package \
            identifier, a suitable package will be installed from Builder.")
.arg(Arg::new("PKG_IDENT").value_parser(checked(valid_ident)).required(true).help("A Dojo package identifier (ex: karate/redis)"))
.arg(Arg::new("APPLICATION").long("application").short('a').requires("ENVIRONMENT").help("Application name; [default: not set]."))
.arg(Arg::new("ENVIRONMENT").long("environment").short('e').requires("APPLICATION").help("Environment name; [default: not set]."))
.arg(Arg::new("CHANNEL").long("channel").default_value("stable").help("Receive package updates from the specified release channel"))
.arg(Arg::new("GROUP").long("group").help("The service group; shared config and topology [default: default]."))
.arg(Arg::new("BLDR_URL").long("url").short('u').value_parser(checked(valid_url)).help("Specify an alternate Builder endpoint. If not specified, the value will \
             be taken from the DOJ_BLDR_URL environment variable if defined. (default: \
             https://bldr.karate.company)"))
.arg(Arg::new("TOPOLOGY").long("topology").short('t').value_parser(["standalone", "leader"]).help("Service topology; [default: none]"))
.arg(Arg::new("STRATEGY").long("strategy").short('s').value_parser(checked(valid_update_strategy)).help("The update strategy; [default: none] [values: none, at-once, rolling]"))
.arg(Arg::new("BIND").long("bind").num_args(1..).action(ArgAction::Append).help("One or more service groups to bind to a configuration"))
.arg(Arg::new("BINDING_MODE").long("binding-mode").value_parser(checked(valid_binding_mode)).help("Governs how the presence or absence of binds affects service startup. `strict` blocks \
              startup until all binds are present. [default: strict] [values: relaxed, strict]"))
.arg(Arg::new("FORCE").long("force").short('f').action(ArgAction::SetTrue).help("Load or reload an already loaded service. If the service \
            was previously loaded and running this operation will also restart the service"))
.arg(Arg::new("REMOTE_SUP").long("remote-sup").short('r').help("Address to a remote Supervisor's Control Gateway [default: 127.0.0.1:9632]"))
.arg(Arg::new("HEALTH_CHECK_INTERVAL").long("health-check-interval").short('i').value_parser(checked(valid_health_check_interval)).help("The interval (seconds) on which to run health checks [default: 30]"));

    sub
}

// CLAP Validation Functions
////////////////////////////////////////////////////////////////////////

/// Turns one of the checks below into a clap value parser that keeps the value as given.
fn checked(check: fn(String) -> result::Result<(), String>)
           -> impl Fn(&str) -> result::Result<String, String> + Clone + Send + Sync + 'static {
    move |val| check(val.to_string()).map(|()| val.to_string())
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_binding_mode(val: String) -> result::Result<(), String> {
    match dojo_sup_protocol::types::BindingMode::from_str(&val) {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("Binding mode: '{}' is not valid", &val)),
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_pair_type(val: String) -> result::Result<(), String> {
    match PairType::from_str(&val) {
        Ok(_) => Ok(()),
        Err(_) => {
            Err(format!("PAIR_TYPE: {} is invalid, must be one of \
                         (public, secret)",
                        &val))
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_service_group(val: String) -> result::Result<(), String> {
    ServiceGroup::validate(&val).map_err(|e| e.to_string())
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn dir_exists(val: String) -> result::Result<(), String> {
    if Path::new(&val).is_dir() {
        Ok(())
    } else {
        Err(format!("Directory: '{}' cannot be found", &val))
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn file_exists(val: String) -> result::Result<(), String> {
    if Path::new(&val).is_file() {
        Ok(())
    } else {
        Err(format!("File: '{}' cannot be found", &val))
    }
}

fn file_exists_or_stdin(val: String) -> result::Result<(), String> {
    if val == "-" {
        Ok(())
    } else {
        file_exists(val)
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_socket_addr(val: String) -> result::Result<(), String> {
    match SocketAddr::from_str(&val) {
        Ok(_) => Ok(()),
        Err(_) => {
            Err("Socket address should include both IP and port, eg: '0.0.0.0:9700'".to_string())
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_url(val: String) -> result::Result<(), String> {
    match Url::parse(&val) {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("URL: '{}' is not valid", &val)),
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_numeric<T: FromStr>(val: String) -> result::Result<(), String> {
    match val.parse::<T>() {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("'{}' is not a valid number", &val)),
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_health_check_interval(val: String) -> result::Result<(), String> {
    match HealthCheckInterval::from_str(&val) {
        Ok(_) => Ok(()),
        Err(e) => {
            Err(format!("'{}' is not a valid value for health check \
                         interval: {}",
                        val, e))
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_update_strategy(val: String) -> result::Result<(), String> {
    match dojo_sup_protocol::types::UpdateStrategy::from_str(&val) {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("Update strategy: '{}' is not valid", &val)),
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_ident(val: String) -> result::Result<(), String> {
    match PackageIdent::from_str(&val) {
        Ok(_) => Ok(()),
        Err(_) => {
            Err(format!("'{}' is not valid. Package identifiers have the \
                         form origin/name[/version[/release]]",
                        &val))
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_target(val: String) -> result::Result<(), String> {
    match PackageTarget::from_str(&val) {
        Ok(_) => Ok(()),
        Err(_) => {
            Err(format!("'{}' is not valid. A valid target is in the form \
                         architecture-platform (example: x86_64-linux)",
                        &val))
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_fully_qualified_ident(val: String) -> result::Result<(), String> {
    match PackageIdent::from_str(&val) {
        Ok(ref ident) if ident.fully_qualified() => Ok(()),
        _ => {
            Err(format!("'{}' is not valid. Fully qualified package \
                         identifiers have the form \
                         origin/name/version/release",
                        &val))
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn valid_origin(val: String) -> result::Result<(), String> {
    if ident::is_valid_origin_name(&val) {
        Ok(())
    } else {
        Err(format!("'{}' is not valid. A valid origin contains a-z, \
                     0-9, and _ or - after the first character",
                    &val))
    }
}

#[allow(clippy::needless_pass_by_value)] // Signature required by CLAP
fn non_empty(val: String) -> result::Result<(), String> {
    if val.is_empty() {
        Err("must not be empty (check env overrides)".to_string())
    } else {
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {

    fn no_feature_flags() -> FeatureFlag { FeatureFlag::empty() }

    use super::*;

    mod sup_commands {

        use super::*;
        use clap::error::ErrorKind;

        #[test]
        fn the_cli_is_well_formed() {
            get(no_feature_flags()).debug_assert();
            get(FeatureFlag::all()).debug_assert();
        }

        #[test]
        fn sup_subcommand_short_help() {
            let r = get(no_feature_flags()).try_get_matches_from(vec!["doj", "sup", "-h"]);
            assert!(r.is_err());
            // not `ErrorKind::InvalidSubcommand`
            assert_eq!(r.unwrap_err().kind(), ErrorKind::DisplayHelp);
        }

        #[test]
        fn sup_subcommand_run_with_peer() {
            let r = get(no_feature_flags()).try_get_matches_from(vec!["doj", "sup", "run",
                                                                      "--peer", "1.1.1.1"]);
            assert!(r.is_ok());
            let matches = r.expect("Error while getting matches");
            // validate `sup` subcommand
            assert_eq!(matches.subcommand_name(), Some("sup"));
            let (_, sup_matches) = matches.subcommand()
                                          .expect("Error while getting sup matches");
            assert_eq!(sup_matches.subcommand_name(), Some("run"));
            let (_, run_matches) = sup_matches.subcommand()
                                              .expect("Error while getting run matches");
            assert_eq!(run_matches.get_one::<String>("PEER").map(String::as_str),
                       Some("1.1.1.1"));
        }
    }
}
