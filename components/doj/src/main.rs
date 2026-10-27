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

#![recursion_limit = "128"]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use crate::{common::{cli::{cache_key_path_from_matches,
                           DEFAULT_BINLINK_DIR,
                           FS_ROOT},
                     command::package::install::{InstallHookMode,
                                                 InstallMode,
                                                 InstallSource,
                                                 LocalPackageUsage},
                     output,
                     types::ListenCtlAddr,
                     ui::{Status,
                          UIWriter,
                          NONINTERACTIVE_ENVVAR,
                          UI}},
            hcore::{crypto::{keys::PairType,
                             BoxKeyPair,
                             SigKeyPair},
                    env as henv,
                    env::Config as EnvConfig,
                    fs::{cache_artifact_path,
                         launcher_root_path},
                    package::{target,
                              PackageIdent,
                              PackageTarget},
                    service::{HealthCheckInterval,
                              ServiceGroup},
                    url::{bldr_url_from_env,
                          default_bldr_url},
                    ChannelIdent},
            protocol::{codec::*,
                       ctl::ServiceBindList,
                       net::ErrCode,
                       types::*},
            sup_client::{SrvClient,
                         SrvClientError}};

use clap::ArgMatches;
use clap_complete::Shell;
use doj::{cli,
          command::{self,
                    pkg::list::ListingType},
          config::{self,
                   Config},
          error::{Error,
                  Result},
          scaffolding,
          AUTH_TOKEN_ENVVAR,
          BLDR_URL_ENVVAR,
          CTL_SECRET_ENVVAR,
          ORIGIN_ENVVAR,
          PRODUCT,
          VERSION};
use dojo_common::{self as common,
                  FeatureFlag};
use dojo_core as hcore;
use dojo_sup_client as sup_client;
use dojo_sup_protocol as protocol;
use env_logger;
use pbr;
use std::{convert::TryFrom,
          env,
          ffi::OsString,
          fmt,
          fs::File,
          io::{self,
               prelude::*,
               Read},
          net::ToSocketAddrs,
          path::Path,
          process,
          result,
          str::FromStr,
          thread};
use tabwriter::TabWriter;
use termcolor::{self,
                Color,
                ColorSpec};

/// Makes the --org CLI param optional when this env var is set
const DOJO_ORG_ENVVAR: &str = "DOJ_ORG";
/// Makes the --user CLI param optional when this env var is set
const DOJO_USER_ENVVAR: &str = "DOJ_USER";

lazy_static! {
    static ref STATUS_HEADER: Vec<&'static str> = {
        vec!["package",
             "type",
             "desired",
             "state",
             "elapsed (s)",
             "pid",
             "group",]
    };
}

fn main() {
    env_logger::init();
    let mut ui = UI::default_with_env();
    let flags = FeatureFlag::from_env(&mut ui);
    if let Err(e) = start(&mut ui, flags) {
        ui.fatal(e).unwrap();
        std::process::exit(1)
    }
}

fn start(ui: &mut UI, feature_flags: FeatureFlag) -> Result<()> {
    exec_subcommand_if_called(ui)?;

    let (args, remaining_args) = raw_parse_args();
    debug!("clap cli args: {:?}", &args);
    debug!("remaining cli args: {:?}", &remaining_args);

    // We build the command tree in a separate thread to eliminate
    // possible stack overflow crashes at runtime. OSX, for instance,
    // will crash with our large tree. This is a known issue:
    // https://github.com/kbknapp/clap-rs/issues/86
    let child = thread::Builder::new().stack_size(8 * 1024 * 1024)
                                      .spawn(move || {
                                          cli::get(feature_flags).try_get_matches_from(args.iter())
                                                                 .unwrap_or_else(|e| e.exit())
                                      })
                                      .unwrap();
    let app_matches = child.join().unwrap();

    match app_matches.subcommand() {
        Some(("apply", m)) => sub_svc_set(m)?,
        Some(("cli", matches)) => {
            match matches.subcommand() {
                Some(("setup", m)) => sub_cli_setup(ui, m)?,
                Some(("completers", m)) => sub_cli_completers(m, feature_flags)?,
                _ => unreachable!(),
            }
        }
        Some(("config", m)) => {
            match m.subcommand() {
                Some(("apply", m)) => sub_svc_set(m)?,
                Some(("show", m)) => sub_svc_config(m)?,
                _ => unreachable!(),
            }
        }
        Some(("file", m)) => {
            match m.subcommand() {
                Some(("upload", m)) => sub_file_put(m)?,
                _ => unreachable!(),
            }
        }
        Some(("install", m)) => sub_pkg_install(ui, m, feature_flags)?,
        Some(("origin", matches)) => {
            match matches.subcommand() {
                Some(("key", m)) => {
                    match m.subcommand() {
                        Some(("download", sc)) => sub_origin_key_download(ui, sc)?,
                        Some(("export", sc)) => sub_origin_key_export(sc)?,
                        Some(("generate", sc)) => sub_origin_key_generate(ui, sc)?,
                        Some(("import", sc)) => sub_origin_key_import(ui, sc)?,
                        Some(("upload", sc)) => sub_origin_key_upload(ui, sc)?,
                        _ => unreachable!(),
                    }
                }
                Some(("secret", m)) => {
                    match m.subcommand() {
                        Some(("upload", sc)) => sub_origin_secret_upload(ui, sc)?,
                        Some(("delete", sc)) => sub_origin_secret_delete(ui, sc)?,
                        Some(("list", sc)) => sub_origin_secret_list(ui, sc)?,
                        _ => unreachable!(),
                    }
                }
                Some(("delete", m)) => sub_origin_delete(ui, m)?,
                _ => unreachable!(),
            }
        }
        Some(("bldr", matches)) => {
            match matches.subcommand() {
                Some(("job", m)) => {
                    match m.subcommand() {
                        Some(("start", m)) => sub_bldr_job_start(ui, m)?,
                        Some(("cancel", m)) => sub_bldr_job_cancel(ui, m)?,
                        Some(("promote", m)) => sub_bldr_job_promote_or_demote(ui, m, true)?,
                        Some(("demote", m)) => sub_bldr_job_promote_or_demote(ui, m, false)?,
                        Some(("status", m)) => sub_bldr_job_status(ui, m)?,
                        _ => unreachable!(),
                    }
                }
                Some(("channel", m)) => {
                    match m.subcommand() {
                        Some(("create", m)) => sub_bldr_channel_create(ui, m)?,
                        Some(("destroy", m)) => sub_bldr_channel_destroy(ui, m)?,
                        Some(("list", m)) => sub_bldr_channel_list(ui, m)?,
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!(),
            }
        }
        Some(("pkg", matches)) => {
            match matches.subcommand() {
                Some(("binds", m)) => sub_pkg_binds(m)?,
                Some(("binlink", m)) => sub_pkg_binlink(ui, m)?,
                Some(("build", m)) => sub_pkg_build(ui, m)?,
                Some(("channels", m)) => sub_pkg_channels(ui, m)?,
                Some(("config", m)) => sub_pkg_config(m)?,
                Some(("dependencies", m)) => sub_pkg_dependencies(m)?,
                Some(("env", m)) => sub_pkg_env(m)?,
                Some(("exec", m)) => sub_pkg_exec(m, &remaining_args)?,
                Some(("export", m)) => sub_pkg_export(ui, m)?,
                Some(("hash", m)) => sub_pkg_hash(m)?,
                Some(("install", m)) => sub_pkg_install(ui, m, feature_flags)?,
                Some(("list", m)) => sub_pkg_list(m)?,
                Some(("path", m)) => sub_pkg_path(m)?,
                Some(("provides", m)) => sub_pkg_provides(m)?,
                Some(("search", m)) => sub_pkg_search(m)?,
                Some(("sign", m)) => sub_pkg_sign(ui, m)?,
                Some(("uninstall", m)) => sub_pkg_uninstall(ui, m)?,
                Some(("upload", m)) => sub_pkg_upload(ui, m)?,
                Some(("delete", m)) => sub_pkg_delete(ui, m)?,
                Some(("verify", m)) => sub_pkg_verify(ui, m)?,
                Some(("header", m)) => sub_pkg_header(ui, m)?,
                Some(("info", m)) => sub_pkg_info(ui, m)?,
                Some(("promote", m)) => sub_pkg_promote(ui, m)?,
                Some(("demote", m)) => sub_pkg_demote(ui, m)?,
                _ => unreachable!(),
            }
        }
        Some(("plan", matches)) => {
            match matches.subcommand() {
                Some(("init", m)) => sub_plan_init(ui, m)?,
                Some(("render", m)) => sub_plan_render(ui, m)?,
                _ => unreachable!(),
            }
        }
        Some(("ring", matches)) => {
            match matches.subcommand() {
                Some(("key", m)) => {
                    match m.subcommand() {
                        Some(("export", sc)) => sub_ring_key_export(sc)?,
                        Some(("import", sc)) => sub_ring_key_import(ui, sc)?,
                        Some(("generate", sc)) => sub_ring_key_generate(ui, sc)?,
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!(),
            }
        }
        Some(("shell", matches)) => {
            match matches.subcommand() {
                Some(("lock", m)) => sub_shell_lock(m)?,
                Some(("env", m)) => sub_shell_env(m)?,
                _ => unreachable!(),
            }
        }
        Some(("svc", matches)) => {
            match matches.subcommand() {
                Some(("key", m)) => {
                    match m.subcommand() {
                        Some(("generate", sc)) => sub_service_key_generate(ui, sc)?,
                        _ => unreachable!(),
                    }
                }
                Some(("load", m)) => sub_svc_load(m)?,
                Some(("unload", m)) => sub_svc_unload(m)?,
                Some(("start", m)) => sub_svc_start(m)?,
                Some(("stop", m)) => sub_svc_stop(m)?,
                Some(("status", m)) => sub_svc_status(m)?,
                _ => unreachable!(),
            }
        }
        Some(("sup", m)) => {
            match m.subcommand() {
                Some(("depart", m)) => sub_sup_depart(m)?,
                Some(("secret", m)) => {
                    match m.subcommand() {
                        Some(("generate", _)) => sub_sup_secret_generate()?,
                        _ => unreachable!(),
                    }
                }
                // this is effectively an alias of `doj svc status`
                Some(("status", m)) => sub_svc_status(m)?,
                _ => unreachable!(),
            }
        }
        Some(("supportbundle", _)) => sub_supportbundle(ui)?,
        Some(("setup", m)) => sub_cli_setup(ui, m)?,
        Some(("start", m)) => sub_svc_start(m)?,
        Some(("stop", m)) => sub_svc_stop(m)?,
        Some(("user", matches)) => {
            match matches.subcommand() {
                Some(("key", m)) => {
                    match m.subcommand() {
                        Some(("generate", sc)) => sub_user_key_generate(ui, sc)?,
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    };
    Ok(())
}

fn sub_cli_setup(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let cache_key_path = cache_key_path_from_matches(&m);

    command::cli::setup::start(ui, &cache_key_path)
}

fn sub_cli_completers(m: &ArgMatches, feature_flags: FeatureFlag) -> Result<()> {
    let shell = m.get_one::<String>("SHELL")
                 .map(String::as_str)
                 .expect("Missing Shell; A shell is required");

    // TODO (CM): Interesting... the completions generated can depend
    // on what feature flags happen to be enabled at the time you
    // generated the completions
    let shell = shell.parse::<Shell>()
                     .expect("SHELL is one of the shells clap_complete supports");
    clap_complete::generate(shell,
                            &mut cli::get(feature_flags),
                            "doj",
                            &mut io::stdout());
    Ok(())
}

fn sub_origin_key_download(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let origin = m.get_one::<String>("ORIGIN").map(String::as_str).unwrap(); // Required via clap
    let revision = m.get_one::<String>("REVISION").map(String::as_str);
    let with_secret = m.get_flag("WITH_SECRET");
    let with_encryption = m.get_flag("WITH_ENCRYPTION");
    let token = maybe_auth_token(&m);
    let url = bldr_url_from_matches(&m)?;
    let cache_key_path = cache_key_path_from_matches(&m);

    command::origin::key::download::start(ui,
                                          &url,
                                          &origin,
                                          revision,
                                          with_secret,
                                          with_encryption,
                                          token.as_ref().map(String::as_str),
                                          &cache_key_path)
}

fn sub_origin_key_export(m: &ArgMatches) -> Result<()> {
    let origin = m.get_one::<String>("ORIGIN").map(String::as_str).unwrap(); // Required via clap
    let pair_type = PairType::from_str(m.get_one::<String>("PAIR_TYPE")
                                        .map(String::as_str)
                                        .unwrap_or("public"))?;
    let cache_key_path = cache_key_path_from_matches(&m);

    command::origin::key::export::start(origin, pair_type, &cache_key_path)
}

fn sub_origin_key_generate(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let origin = origin_param_or_env(&m)?;
    let cache_key_path = cache_key_path_from_matches(&m);

    command::origin::key::generate::start(ui, &origin, &cache_key_path)
}

fn sub_origin_key_import(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let mut content = String::new();
    let cache_key_path = cache_key_path_from_matches(&m);
    io::stdin().read_to_string(&mut content)?;

    // Trim the content to lose line feeds added by Powershell pipeline
    command::origin::key::import::start(ui, content.trim(), &cache_key_path)
}

fn sub_origin_key_upload(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let token = auth_token_param_or_env(&m)?;
    let cache_key_path = cache_key_path_from_matches(&m);

    if m.contains_id("ORIGIN") {
        let origin = m.get_one::<String>("ORIGIN").map(String::as_str).unwrap();
        // you can either specify files, or infer the latest key names
        let with_secret = m.get_flag("WITH_SECRET");
        command::origin::key::upload_latest::start(ui,
                                                   &url,
                                                   &token,
                                                   origin,
                                                   with_secret,
                                                   &cache_key_path)
    } else {
        let keyfile = Path::new(m.get_one::<String>("PUBLIC_FILE")
                                 .map(String::as_str)
                                 .unwrap());
        let secret_keyfile = m.get_one::<String>("SECRET_FILE")
                              .map(String::as_str)
                              .map(|f| Path::new(f));
        command::origin::key::upload::start(ui, &url, &token, &keyfile, secret_keyfile)
    }
}

fn sub_origin_secret_upload(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let token = auth_token_param_or_env(&m)?;
    let origin = origin_param_or_env(&m)?;
    let key = m.get_one::<String>("KEY_NAME").map(String::as_str).unwrap();
    let secret = m.get_one::<String>("SECRET").map(String::as_str).unwrap();
    let cache_key_path = cache_key_path_from_matches(&m);
    command::origin::secret::upload::start(ui,
                                           &url,
                                           &token,
                                           &origin,
                                           &key,
                                           &secret,
                                           &cache_key_path)
}

fn sub_origin_secret_delete(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let token = auth_token_param_or_env(&m)?;
    let origin = origin_param_or_env(&m)?;
    let key = m.get_one::<String>("KEY_NAME").map(String::as_str).unwrap();
    command::origin::secret::delete::start(ui, &url, &token, &origin, &key)
}

fn sub_origin_secret_list(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let token = auth_token_param_or_env(&m)?;
    let origin = origin_param_or_env(&m)?;
    command::origin::secret::list::start(ui, &url, &token, &origin)
}

fn sub_origin_delete(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let origin = origin_param_or_env(&m)?;
    let url = bldr_url_from_matches(&m)?;
    let token = auth_token_param_or_env(&m)?;
    command::origin::delete::start(ui, &url, &token, &origin)
}

fn sub_pkg_binlink(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let dest_dir = Path::new(m.get_one::<String>("DEST_DIR").map(String::as_str).unwrap()); // required by clap
    let force = m.get_flag("FORCE");
    match m.get_one::<String>("BINARY").map(String::as_str) {
        Some(binary) => {
            command::pkg::binlink::start(ui, &ident, &binary, dest_dir, &FS_ROOT, force)
        }
        None => command::pkg::binlink::binlink_all_in_pkg(ui, &ident, dest_dir, &FS_ROOT, force),
    }
}

fn sub_pkg_build(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let plan_context = m.get_one::<String>("PLAN_CONTEXT")
                        .map(String::as_str)
                        .unwrap(); // Required via clap
    let root = m.get_one::<String>("DOJ_STUDIO_ROOT").map(String::as_str);
    let src = m.get_one::<String>("SRC_PATH").map(String::as_str);
    let keys_string = match m.get_many::<String>("DOJ_ORIGIN_KEYS")
                             .map(|v| v.map(String::as_str))
    {
        Some(keys) => {
            let cache_key_path = cache_key_path_from_matches(&m);
            for key in keys.clone() {
                // Validate that all secret keys are present
                let pair = SigKeyPair::get_latest_pair_for(key, &cache_key_path, None)?;
                let _ = pair.secret();
            }
            Some(keys.collect::<Vec<_>>().join(","))
        }
        None => None,
    };
    let keys: Option<&str> = match keys_string.as_ref() {
        Some(s) => Some(s),
        None => None,
    };
    let docker = m.get_flag("DOCKER");
    let reuse = m.get_flag("REUSE");

    command::pkg::build::start(ui, plan_context, root, src, keys, reuse, docker)
}

fn sub_pkg_config(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;

    common::command::package::config::start(&ident, &*FS_ROOT)?;
    Ok(())
}

fn sub_pkg_binds(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;

    common::command::package::binds::start(&ident, &*FS_ROOT)?;
    Ok(())
}

fn sub_pkg_dependencies(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let scope = if m.get_flag("TRANSITIVE") {
        command::pkg::Scope::PackageAndDependencies
    } else {
        command::pkg::Scope::Package
    };

    let direction = if m.get_flag("REVERSE") {
        command::pkg::DependencyRelation::Supports
    } else {
        command::pkg::DependencyRelation::Requires
    };
    command::pkg::dependencies::start(&ident, scope, direction, &*FS_ROOT)
}

fn sub_shell_lock(m: &ArgMatches) -> Result<()> {
    command::shell::lock(&env::current_dir()?, &*FS_ROOT, m.get_flag("UPDATE"))
}

fn sub_shell_env(m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(m)?;
    command::shell::env(&env::current_dir()?, &*FS_ROOT, &url)
}

fn sub_pkg_env(m: &ArgMatches) -> Result<()> {
    let idents = m.get_many::<String>("PKG_IDENT").map(|v| v.map(String::as_str))
                  .unwrap() // Required via clap
                  .map(PackageIdent::from_str)
                  .collect::<std::result::Result<Vec<_>, _>>()?;

    command::pkg::env::start(&idents, &*FS_ROOT)
}

fn sub_pkg_exec(m: &ArgMatches, cmd_args: &[OsString]) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?; // Required via clap
    let cmd = m.get_one::<String>("CMD").map(String::as_str).unwrap(); // Required via clap

    command::pkg::exec::start(&ident, cmd, cmd_args)
}

fn sub_pkg_export(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let format = &m.get_one::<String>("FORMAT").map(String::as_str).unwrap();
    let url = bldr_url_from_matches(&m)?;
    let channel = channel_from_matches_or_default(&m);
    let export_fmt = command::pkg::export::format_for(ui, &format)?;
    command::pkg::export::start(ui, &url, &channel, &ident, &export_fmt)
}

fn sub_pkg_hash(m: &ArgMatches) -> Result<()> {
    match m.get_one::<String>("SOURCE").map(String::as_str) {
        Some(source) => {
            // hash single file
            command::pkg::hash::start(&source)
        }
        None => {
            // read files from stdin
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let file = line?;
                command::pkg::hash::start(file.trim_end())?;
            }
            Ok(())
        }
    }
}

fn sub_pkg_uninstall(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let execute_strategy = if m.get_flag("DRYRUN") {
        command::pkg::ExecutionStrategy::DryRun
    } else {
        command::pkg::ExecutionStrategy::Run
    };
    let scope = if m.get_flag("NO_DEPS") {
        command::pkg::Scope::Package
    } else {
        command::pkg::Scope::PackageAndDependencies
    };
    let excludes = excludes_from_matches(&m);

    let services = supervisor_services()?;

    command::pkg::uninstall::start(ui,
                                   &ident,
                                   &*FS_ROOT,
                                   execute_strategy,
                                   scope,
                                   &excludes,
                                   &services)
}
fn sub_bldr_channel_create(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let origin = origin_param_or_env(&m)?;
    let channel = required_channel_from_matches(&m);
    let token = auth_token_param_or_env(&m)?;
    command::bldr::channel::create::start(ui, &url, &token, &origin, &channel)
}

fn sub_bldr_channel_destroy(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let origin = origin_param_or_env(&m)?;
    let channel = required_channel_from_matches(&m);
    let token = auth_token_param_or_env(&m)?;
    command::bldr::channel::destroy::start(ui, &url, &token, &origin, &channel)
}

fn sub_bldr_channel_list(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let origin = origin_param_or_env(&m)?;
    command::bldr::channel::list::start(ui, &url, &origin)
}

fn sub_bldr_job_start(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?; // Required via clap
    let url = bldr_url_from_matches(&m)?;
    let target = target_from_matches(m)?;
    let group = m.get_flag("GROUP");
    let token = auth_token_param_or_env(&m)?;
    command::bldr::job::start::start(ui, &url, (&ident, target), &token, group)
}

fn sub_bldr_job_cancel(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let group_id = m.get_one::<String>("GROUP_ID").map(String::as_str).unwrap(); // Required via clap
    let token = auth_token_param_or_env(&m)?;
    let force = m.get_flag("FORCE");
    command::bldr::job::cancel::start(ui, &url, &group_id, &token, force)
}

fn sub_bldr_job_promote_or_demote(ui: &mut UI, m: &ArgMatches, promote: bool) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let group_id = m.get_one::<String>("GROUP_ID").map(String::as_str).unwrap(); // Required via clap
    let channel = required_channel_from_matches(&m);
    let origin = m.get_one::<String>("ORIGIN").map(String::as_str);
    let interactive = m.get_flag("INTERACTIVE");
    let verbose = m.get_flag("VERBOSE");
    let token = auth_token_param_or_env(&m)?;
    command::bldr::job::promote::start(ui,
                                       &url,
                                       &group_id,
                                       &channel,
                                       origin,
                                       interactive,
                                       verbose,
                                       &token,
                                       promote)
}

fn sub_bldr_job_status(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let group_id = m.get_one::<String>("GROUP_ID").map(String::as_str);
    let origin = m.get_one::<String>("ORIGIN").map(String::as_str);
    let limit = m.get_one::<String>("LIMIT")
                 .map(String::as_str)
                 .unwrap_or("10")
                 .parse::<usize>()
                 .unwrap();
    let show_jobs = m.get_flag("SHOW_JOBS");

    command::bldr::job::status::start(ui, &url, group_id, origin, limit, show_jobs)
}

fn sub_plan_init(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let name = m.get_one::<String>("PKG_NAME")
                .map(String::as_str)
                .map(String::from);
    let origin = origin_param_or_env(&m)?;
    let with_docs = m.get_flag("WITH_DOCS");
    let with_callbacks = m.get_flag("WITH_CALLBACKS");
    let with_all = m.get_flag("WITH_ALL");
    let scaffolding_ident =
        scaffolding::scaffold_check(ui, m.get_one::<String>("SCAFFOLDING").map(String::as_str))?;

    command::plan::init::start(ui,
                               origin,
                               with_docs,
                               with_callbacks,
                               with_all,
                               scaffolding_ident,
                               name)
}

fn sub_plan_render(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let template_path = Path::new(m.get_one::<String>("TEMPLATE_PATH")
                                   .map(String::as_str)
                                   .unwrap());

    let default_toml_path = Path::new(m.get_one::<String>("DEFAULT_TOML")
                                       .map(String::as_str)
                                       .unwrap());

    let user_toml_path = m.get_one::<String>("USER_TOML")
                          .map(String::as_str)
                          .map(Path::new);

    let mock_data_path = m.get_one::<String>("MOCK_DATA")
                          .map(String::as_str)
                          .map(Path::new);

    let print = m.get_flag("PRINT");
    let render = !m.get_flag("NO_RENDER");
    let quiet = m.get_flag("QUIET");

    let render_dir = Path::new(m.get_one::<String>("RENDER_DIR")
                                .map(String::as_str)
                                .unwrap());

    command::plan::render::start(ui,
                                 template_path,
                                 default_toml_path,
                                 user_toml_path,
                                 mock_data_path,
                                 print,
                                 render,
                                 render_dir,
                                 quiet)
}

fn sub_pkg_install(ui: &mut UI, m: &ArgMatches, feature_flags: FeatureFlag) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let channel = channel_from_matches_or_default(m);
    let install_sources = install_sources_from_matches(m)?;
    let token = maybe_auth_token(&m);
    let install_mode =
        if feature_flags.contains(FeatureFlag::OFFLINE_INSTALL) && m.get_flag("OFFLINE") {
            InstallMode::Offline
        } else {
            InstallMode::default()
        };

    let local_package_usage =
        if feature_flags.contains(FeatureFlag::IGNORE_LOCAL) && m.get_flag("IGNORE_LOCAL") {
            LocalPackageUsage::Ignore
        } else {
            LocalPackageUsage::default()
        };

    let install_hook_mode = if !feature_flags.contains(FeatureFlag::INSTALL_HOOK)
                               || m.get_flag("IGNORE_INSTALL_HOOK")
    {
        InstallHookMode::Ignore
    } else {
        InstallHookMode::default()
    };

    for install_source in install_sources.iter() {
        let pkg_install =
            common::command::package::install::start(ui,
                                                     &url,
                                                     &channel,
                                                     install_source,
                                                     PRODUCT,
                                                     VERSION,
                                                     &*FS_ROOT,
                                                     &cache_artifact_path(Some(&*FS_ROOT)),
                                                     token.as_ref().map(String::as_str),
                                                     &install_mode,
                                                     &local_package_usage,
                                                     install_hook_mode)?;

        if m.get_flag("BINLINK") {
            let force = m.get_flag("FORCE");
            command::pkg::binlink::binlink_all_in_pkg(ui,
                                                      pkg_install.ident(),
                                                      Path::new(DEFAULT_BINLINK_DIR),
                                                      &FS_ROOT,
                                                      force)?;
        }
    }
    Ok(())
}

fn sub_pkg_path(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;

    command::pkg::path::start(&ident, &*FS_ROOT)
}

fn sub_pkg_list(m: &ArgMatches) -> Result<()> {
    let listing_type = ListingType::from(m);

    command::pkg::list::start(&listing_type, &*FS_ROOT)
}

fn sub_pkg_provides(m: &ArgMatches) -> Result<()> {
    let filename = m.get_one::<String>("FILE").map(String::as_str).unwrap(); // Required via clap

    let full_releases = m.get_flag("FULL_RELEASES");
    let full_paths = m.get_flag("FULL_PATHS");

    command::pkg::provides::start(&filename, &*FS_ROOT, full_releases, full_paths)
}

fn sub_pkg_search(m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let search_term = m.get_one::<String>("SEARCH_TERM")
                       .map(String::as_str)
                       .unwrap(); // Required via clap
    let token = maybe_auth_token(&m);
    command::pkg::search::start(&search_term, &url, token.as_ref().map(String::as_str))
}

fn sub_pkg_sign(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let src = Path::new(m.get_one::<String>("SOURCE").map(String::as_str).unwrap()); // Required via clap
    let dst = Path::new(m.get_one::<String>("DEST").map(String::as_str).unwrap()); // Required via clap
    let cache_key_path = cache_key_path_from_matches(&m);
    let pair = SigKeyPair::get_latest_pair_for(&origin_param_or_env(&m)?,
                                               &cache_key_path,
                                               Some(&PairType::Secret))?;

    command::pkg::sign::start(ui, &pair, &src, &dst)
}

fn sub_pkg_upload(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let key_path = cache_key_path_from_matches(&m);
    let url = bldr_url_from_matches(&m)?;

    // When packages are uploaded, they *always* go to `unstable`;
    // they can optionally get added to another channel, too.
    let additional_release_channel = channel_from_matches(&m);

    // When packages are uploaded we check if they exist in the db
    // before allowing a write to the backend, this bypasses the check
    let force_upload = m.get_flag("FORCE");

    let token = auth_token_param_or_env(&m)?;
    let artifact_paths = m.get_many::<String>("HART_FILE")
                          .map(|v| v.map(String::as_str))
                          .unwrap(); // Required via clap
    for artifact_path in artifact_paths.map(Path::new) {
        command::pkg::upload::start(ui,
                                    &url,
                                    &additional_release_channel,
                                    &token,
                                    artifact_path,
                                    force_upload,
                                    &key_path)?;
    }
    Ok(())
}

fn sub_pkg_delete(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let token = auth_token_param_or_env(&m)?;
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let target = target_from_matches(m)?;

    command::pkg::delete::start(ui, &url, (&ident, target), &token)?;

    Ok(())
}

fn sub_pkg_verify(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let src = Path::new(m.get_one::<String>("SOURCE").map(String::as_str).unwrap()); // Required via clap
    let cache_key_path = cache_key_path_from_matches(&m);

    command::pkg::verify::start(ui, &src, &cache_key_path)
}

fn sub_pkg_header(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let src = Path::new(m.get_one::<String>("SOURCE").map(String::as_str).unwrap()); // Required via clap

    command::pkg::header::start(ui, &src)
}

fn sub_pkg_info(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let src = Path::new(m.get_one::<String>("SOURCE").map(String::as_str).unwrap()); // Required via clap
    let to_json = m.get_flag("TO_JSON");

    command::pkg::info::start(ui, &src, to_json)
}

fn sub_pkg_promote(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let channel = required_channel_from_matches(&m);
    let token = auth_token_param_or_env(&m)?;
    let target = target_from_matches(m)?;
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?; // Required via clap
    command::pkg::promote::start(ui, &url, (&ident, target), &channel, &token)
}

fn sub_pkg_demote(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let channel = required_channel_from_matches(&m);
    let token = auth_token_param_or_env(&m)?;
    let target = target_from_matches(m)?;
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?; // Required via clap
    command::pkg::demote::start(ui, &url, (&ident, target), &channel, &token)
}

fn sub_pkg_channels(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let url = bldr_url_from_matches(&m)?;
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?; // Required via clap
    let token = maybe_auth_token(&m);
    let target = target_from_matches(m)?;

    command::pkg::channels::start(ui,
                                  &url,
                                  (&ident, target),
                                  token.as_ref().map(String::as_str))
}

fn sub_svc_set(m: &ArgMatches) -> Result<()> {
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let service_group = ServiceGroup::from_str(m.get_one::<String>("SERVICE_GROUP")
                                                .map(String::as_str)
                                                .unwrap())?;
    let mut ui = ui();
    let mut validate = protocol::ctl::SvcValidateCfg::default();
    validate.service_group = Some(service_group.clone().into());
    let mut buf = Vec::with_capacity(protocol::butterfly::MAX_SVC_CFG_SIZE);
    let cfg_len = match m.get_one::<String>("FILE").map(String::as_str) {
        Some("-") | None => io::stdin().read_to_end(&mut buf)?,
        Some(f) => {
            let mut file = File::open(f)?;
            file.read_to_end(&mut buf)?
        }
    };
    if cfg_len > protocol::butterfly::MAX_SVC_CFG_SIZE {
        ui.fatal(format!("Configuration too large. Maximum size allowed is {} bytes.",
                         protocol::butterfly::MAX_SVC_CFG_SIZE))?;
        process::exit(1);
    }
    validate.cfg = Some(buf.clone());
    let cache = cache_key_path_from_matches(&m);
    let mut set = protocol::ctl::SvcSetCfg::default();
    match (service_group.org(), user_param_or_env(&m)) {
        (Some(_org), Some(username)) => {
            let user_pair = BoxKeyPair::get_latest_pair_for(username, &cache)?;
            let service_pair = BoxKeyPair::get_latest_pair_for(&service_group, &cache)?;
            ui.status(Status::Encrypting,
                      format!("TOML as {} for {}",
                              user_pair.name_with_rev(),
                              service_pair.name_with_rev()))?;
            set.cfg = Some(user_pair.encrypt(&buf, Some(&service_pair))?.into_bytes());
            set.is_encrypted = Some(true);
        }
        _ => set.cfg = Some(buf.to_vec()),
    }
    set.service_group = Some(service_group.into());
    set.version = Some(m.get_one::<String>("VERSION_NUMBER")
                        .expect(concat!("VERSION_NUMBER", " is required"))
                        .parse::<u64>()
                        .unwrap());
    ui.begin(format!("Setting new configuration version {} for {}",
                     set.version
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "UNKNOWN".to_string()),
                     set.service_group
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "UNKNOWN".to_string()),))?;
    ui.status(Status::Creating, "service configuration")?;
    ctl_call(&listen_ctl_addr, &secret_key, validate, |reply| {
        match reply.message_id() {
            "NetOk" => Ok(()),
            "NetErr" => {
                let m = reply.parse::<protocol::net::NetErr>()
                             .map_err(SrvClientError::Decode)?;
                match ErrCode::try_from(m.code).ok() {
                    Some(ErrCode::InvalidPayload) => {
                        ui.warn(m)?;
                        Ok(())
                    }
                    _ => Err(SrvClientError::from(m)),
                }
            }
            _ => Err(SrvClientError::from(io::Error::from(io::ErrorKind::UnexpectedEof))),
        }
    })?;
    ui.status(Status::Applying, format!("via peer {}", listen_ctl_addr))?;
    ctl_call(&listen_ctl_addr, &secret_key, set, |reply| {
        match reply.message_id() {
            "NetOk" => Ok(()),
            "NetErr" => {
                let m = reply.parse::<protocol::net::NetErr>()
                             .map_err(SrvClientError::Decode)?;
                Err(SrvClientError::from(m))
            }
            _ => Err(SrvClientError::from(io::Error::from(io::ErrorKind::UnexpectedEof))),
        }
    })?;
    ui.end("Applied configuration")?;
    Ok(())
}

fn sub_svc_config(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut msg = protocol::ctl::SvcGetDefaultCfg::default();
    msg.ident = Some(ident.into());
    ctl_call(&listen_ctl_addr, &secret_key, msg, |reply| {
        match reply.message_id() {
            "ServiceCfg" => {
                let m = reply.parse::<protocol::types::ServiceCfg>()
                             .map_err(SrvClientError::Decode)?;
                println!("{}", m.default.unwrap_or_default());
                Ok(())
            }
            "NetErr" => {
                let m = reply.parse::<protocol::net::NetErr>()
                             .map_err(SrvClientError::Decode)?;
                Err(SrvClientError::from(m))
            }
            _ => Err(SrvClientError::from(io::Error::from(io::ErrorKind::UnexpectedEof))),
        }
    })?;
    Ok(())
}

fn sub_svc_load(m: &ArgMatches) -> Result<()> {
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut msg = protocol::ctl::SvcLoad::default();
    update_svc_load_from_input(m, &mut msg)?;
    let ident: PackageIdent = m.get_one::<String>("PKG_IDENT")
                               .map(String::as_str)
                               .unwrap()
                               .parse()?;
    msg.ident = Some(ident.into());
    ctl_call(&listen_ctl_addr, &secret_key, msg, |m| handle_ctl_reply(&m))?;
    Ok(())
}

fn sub_svc_unload(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut msg = protocol::ctl::SvcUnload::default();
    msg.ident = Some(ident.into());
    ctl_call(&listen_ctl_addr, &secret_key, msg, |m| handle_ctl_reply(&m))?;
    Ok(())
}

fn sub_svc_start(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut msg = protocol::ctl::SvcStart::default();
    msg.ident = Some(ident.into());
    ctl_call(&listen_ctl_addr, &secret_key, msg, |m| handle_ctl_reply(&m))?;
    Ok(())
}

fn sub_svc_status(m: &ArgMatches) -> Result<()> {
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut msg = protocol::ctl::SvcStatus::default();
    if let Some(pkg) = m.get_one::<String>("PKG_IDENT").map(String::as_str) {
        msg.ident = Some(PackageIdent::from_str(pkg)?.into());
    }

    let mut out = TabWriter::new(io::stdout());
    let mut first = true;
    ctl_call(&listen_ctl_addr, &secret_key, msg, |reply| {
        print_svc_status(&mut out, reply, first)?;
        first = false;
        Ok(())
    })?;
    out.flush()?;
    Ok(())
}

fn sub_svc_stop(m: &ArgMatches) -> Result<()> {
    let ident = PackageIdent::from_str(m.get_one::<String>("PKG_IDENT")
                                        .map(String::as_str)
                                        .unwrap())?;
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut msg = protocol::ctl::SvcStop::default();
    msg.ident = Some(ident.into());
    ctl_call(&listen_ctl_addr, &secret_key, msg, |m| handle_ctl_reply(&m))?;
    Ok(())
}

fn sub_file_put(m: &ArgMatches) -> Result<()> {
    let service_group = ServiceGroup::from_str(m.get_one::<String>("SERVICE_GROUP")
                                                .map(String::as_str)
                                                .unwrap())?;
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut ui = ui();
    let mut msg = protocol::ctl::SvcFilePut::default();
    let file = Path::new(m.get_one::<String>("FILE").map(String::as_str).unwrap());
    if file.metadata()?.len() > protocol::butterfly::MAX_FILE_PUT_SIZE_BYTES as u64 {
        ui.fatal(format!("File too large. Maximum size allowed is {} bytes.",
                         protocol::butterfly::MAX_FILE_PUT_SIZE_BYTES))?;
        process::exit(1);
    };
    msg.service_group = Some(service_group.clone().into());
    msg.version = Some(m.get_one::<String>("VERSION_NUMBER")
                        .expect(concat!("VERSION_NUMBER", " is required"))
                        .parse::<u64>()
                        .unwrap());
    msg.filename = Some(file.file_name().unwrap().to_string_lossy().into_owned());
    let mut buf = Vec::with_capacity(protocol::butterfly::MAX_FILE_PUT_SIZE_BYTES);
    let cache = cache_key_path_from_matches(&m);
    ui.begin(format!("Uploading file {} to {} incarnation {}",
                     file.display(),
                     msg.version
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "UNKNOWN".to_string()),
                     msg.service_group
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "UKNOWN".to_string()),))?;
    ui.status(Status::Creating, "service file")?;
    File::open(&file)?.read_to_end(&mut buf)?;
    match (service_group.org(), user_param_or_env(&m)) {
        (Some(_org), Some(username)) => {
            let user_pair = BoxKeyPair::get_latest_pair_for(username, &cache)?;
            let service_pair = BoxKeyPair::get_latest_pair_for(&service_group, &cache)?;
            ui.status(Status::Encrypting,
                      format!("file as {} for {}",
                              user_pair.name_with_rev(),
                              service_pair.name_with_rev()))?;
            msg.content = Some(user_pair.encrypt(&buf, Some(&service_pair))?.into_bytes());
            msg.is_encrypted = Some(true);
        }
        _ => msg.content = Some(buf.to_vec()),
    }
    ui.status(Status::Applying, format!("via peer {}", listen_ctl_addr))?;
    ctl_call(&listen_ctl_addr, &secret_key, msg, |reply| {
        match reply.message_id() {
            "NetOk" => Ok(()),
            "NetErr" => {
                let m = reply.parse::<protocol::net::NetErr>()
                             .map_err(SrvClientError::Decode)?;
                match ErrCode::try_from(m.code).ok() {
                    Some(ErrCode::InvalidPayload) => {
                        ui.warn(m)?;
                        Ok(())
                    }
                    _ => Err(SrvClientError::from(m)),
                }
            }
            _ => Err(SrvClientError::from(io::Error::from(io::ErrorKind::UnexpectedEof))),
        }
    })?;
    ui.end("Uploaded file")?;
    Ok(())
}

fn sub_sup_depart(m: &ArgMatches) -> Result<()> {
    let cfg = config::load()?;
    let listen_ctl_addr = listen_ctl_addr_from_input(m)?;
    let secret_key = ctl_secret_key(&cfg)?;
    let mut ui = ui();
    let mut msg = protocol::ctl::SupDepart::default();
    msg.member_id = Some(m.get_one::<String>("MEMBER_ID")
                          .map(String::as_str)
                          .unwrap()
                          .to_string());
    ui.begin(format!("Permanently marking {} as departed",
                     msg.member_id
                        .as_ref()
                        .map(String::as_str)
                        .unwrap_or("UNKNOWN")))?;
    ui.status(Status::Applying, format!("via peer {}", listen_ctl_addr))?;
    ctl_call(&listen_ctl_addr, &secret_key, msg, |reply| {
        match reply.message_id() {
            "NetOk" => Ok(()),
            "NetErr" => {
                let m = reply.parse::<protocol::net::NetErr>()
                             .map_err(SrvClientError::Decode)?;
                Err(SrvClientError::from(m))
            }
            _ => Err(SrvClientError::from(io::Error::from(io::ErrorKind::UnexpectedEof))),
        }
    })?;
    ui.end("Departure recorded.")?;
    Ok(())
}

fn sub_sup_secret_generate() -> Result<()> {
    let mut ui = ui();
    let mut buf = String::new();
    protocol::generate_secret_key(&mut buf);
    ui.info(buf)?;
    Ok(())
}

fn sub_supportbundle(ui: &mut UI) -> Result<()> { command::supportbundle::start(ui) }

fn sub_ring_key_export(m: &ArgMatches) -> Result<()> {
    let ring = m.get_one::<String>("RING").map(String::as_str).unwrap(); // Required via clap
    let cache_key_path = cache_key_path_from_matches(&m);

    command::ring::key::export::start(ring, &cache_key_path)
}

fn sub_ring_key_generate(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let ring = m.get_one::<String>("RING").map(String::as_str).unwrap(); // Required via clap
    let cache_key_path = cache_key_path_from_matches(&m);

    command::ring::key::generate::start(ui, ring, &cache_key_path)
}

fn sub_ring_key_import(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let mut content = String::new();
    let cache_key_path = cache_key_path_from_matches(&m);
    io::stdin().read_to_string(&mut content)?;

    // Trim the content to lose line feeds added by Powershell pipeline
    command::ring::key::import::start(ui, content.trim(), &cache_key_path)
}

fn sub_service_key_generate(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let org = org_param_or_env(&m)?;
    let service_group = ServiceGroup::from_str(m.get_one::<String>("SERVICE_GROUP")
                                                .map(String::as_str)
                                                .unwrap())?;
    let cache_key_path = cache_key_path_from_matches(&m);

    command::service::key::generate::start(ui, &org, &service_group, &cache_key_path)
}

fn sub_user_key_generate(ui: &mut UI, m: &ArgMatches) -> Result<()> {
    let user = m.get_one::<String>("USER").map(String::as_str).unwrap(); // Required via clap
    let cache_key_path = cache_key_path_from_matches(&m);

    command::user::key::generate::start(ui, user, &cache_key_path)
}

fn args_after_first(args_to_skip: usize) -> Vec<OsString> {
    env::args_os().skip(args_to_skip).collect()
}

fn exec_subcommand_if_called(ui: &mut UI) -> Result<()> {
    let mut args = env::args();
    match (args.nth(1).unwrap_or_default().as_str(),
           args.next().unwrap_or_default().as_str(),
           args.next().unwrap_or_default().as_str())
    {
        ("pkg", "export", "docker") => {
            command::pkg::export::docker::start(ui, &args_after_first(4))
        }
        ("pkg", "export", "tar") => command::pkg::export::tar::start(ui, &args_after_first(4)),
        ("run", ..) => command::launcher::start(ui, &args_after_first(1)),
        ("stu", ..) | ("stud", ..) | ("studi", ..) | ("studio", ..) => {
            command::studio::enter::start(ui, &args_after_first(2))
        }
        // Skip invoking the `doj-sup` binary for sup cli help messages;
        // handle these from within `doj`
        ("help", "sup", _)
        | ("sup", _, "help")
        | ("sup", "help", _)
        | ("sup", _, "-h")
        | ("sup", "-h", _)
        | ("sup", _, "--help")
        | ("sup", "--help", _) => Ok(()),
        // Delegate remaining Supervisor subcommands to `doj-sup`
        ("sup", "", "")
        | ("sup", "term", _)
        | ("sup", "bash", _)
        | ("sup", "sh", _)
        | ("sup", "-V", _)
        | ("sup", "--version", _) => command::sup::start(ui, &args_after_first(2)),
        ("term", ..) => command::sup::start(ui, &args_after_first(1)),
        // Delegate `doj sup run *` to the Launcher
        ("sup", "run", _) => command::launcher::start(ui, &args_after_first(2)),
        _ => Ok(()),
    }
}

/// Parse the raw program arguments and split off any arguments that will skip clap's parsing.
///
/// **Note** with the current version of clap there is no clean way to ignore arguments after a
/// certain point, especially if those arguments look like further options and flags.
fn raw_parse_args() -> (Vec<OsString>, Vec<OsString>) {
    let mut args = env::args();
    match (args.nth(1).unwrap_or_default().as_str(), args.next().unwrap_or_default().as_str()) {
        ("pkg", "exec") => {
            if args.by_ref().count() > 2 {
                (env::args_os().take(5).collect(), env::args_os().skip(5).collect())
            } else {
                (env::args_os().collect(), Vec::new())
            }
        }
        _ => (env::args_os().collect(), Vec::new()),
    }
}

/// Check to see if the user has passed in an AUTH_TOKEN param. If not, check the
/// DOJ_AUTH_TOKEN env var. If not, check the CLI config to see if there is a default auth
/// token set. If that's empty too, then error.
fn auth_token_param_or_env(m: &ArgMatches) -> Result<String> {
    match m.get_one::<String>("AUTH_TOKEN").map(String::as_str) {
        Some(o) => Ok(o.to_string()),
        None => {
            match henv::var(AUTH_TOKEN_ENVVAR) {
                Ok(v) => Ok(v),
                Err(_) => {
                    config::load()?.auth_token
                                   .ok_or(Error::ArgumentError("No auth token specified"))
                }
            }
        }
    }
}

/// Check if the DOJ_CTL_SECRET env var. If not, check the CLI config to see if there is a ctl
/// secret set and return a copy of that value.
fn ctl_secret_key(config: &Config) -> Result<String> {
    match henv::var(CTL_SECRET_ENVVAR) {
        Ok(v) => Ok(v.to_string()),
        Err(_) => {
            match config.ctl_secret {
                Some(ref v) => Ok(v.to_string()),
                None => SrvClient::read_secret_key().map_err(Error::from),
            }
        }
    }
}

/// Check to see if an auth token exists and convert it to a string slice if it does. Unlike
/// auth_token_param_or_env, it's ok for no auth token to be present here. This is useful for
/// commands that can optionally take an auth token for operating on private packages.
fn maybe_auth_token(m: &ArgMatches) -> Option<String> {
    match auth_token_param_or_env(&m) {
        Ok(t) => Some(t),
        Err(_) => None,
    }
}

/// Check to see if the user has passed in an ORIGIN param.  If not, check the DOJO_ORIGIN env
/// var. If not, check the CLI config to see if there is a default origin set. If that's empty too,
/// then error.
fn origin_param_or_env(m: &ArgMatches) -> Result<String> {
    match m.get_one::<String>("ORIGIN").map(String::as_str) {
        Some(o) => Ok(o.to_string()),
        None => {
            match henv::var(ORIGIN_ENVVAR) {
                Ok(v) => Ok(v),
                Err(_) => {
                    config::load()?.origin.ok_or_else(|| {
                                              Error::CryptoCLI("No origin specified".to_string())
                                          })
                }
            }
        }
    }
}

/// Check to see if the user has passed in an ORG param.
/// If not, check the DOJO_ORG env var. If that's
/// empty too, then error.
fn org_param_or_env(m: &ArgMatches) -> Result<String> {
    match m.get_one::<String>("ORG").map(String::as_str) {
        Some(o) => Ok(o.to_string()),
        None => {
            henv::var(DOJO_ORG_ENVVAR).map_err(|_| {
                                          Error::CryptoCLI("No organization specified".to_string())
                                      })
        }
    }
}

/// Check to see if the user has passed in a Builder URL param.  If not, check the DOJ_BLDR_URL env
/// var. If not, check the CLI config to see if there is a default url set. If that's empty too,
/// then we'll use the default (https://bldr.karate.company).
fn bldr_url_from_matches(matches: &ArgMatches) -> Result<String> {
    match matches.get_one::<String>("BLDR_URL").map(String::as_str) {
        Some(url) => Ok(url.to_string()),
        None => {
            match henv::var(BLDR_URL_ENVVAR) {
                Ok(v) => Ok(v),
                Err(_) => {
                    let config = config::load()?;
                    match config.bldr_url {
                        Some(v) => Ok(v),
                        None => Ok(default_bldr_url()),
                    }
                }
            }
        }
    }
}

/// Resolve a channel. Taken from the environment or from CLI args, if
/// given.
fn channel_from_matches(matches: &ArgMatches) -> Option<ChannelIdent> {
    matches.get_one::<String>("CHANNEL")
           .map(String::as_str)
           .map(ChannelIdent::from)
}

/// Resolve a channel. Taken from the environment or from CLI args. This
/// should only be called when the argument is required by the CLAP config,
/// otherwise this would panic.
fn required_channel_from_matches(matches: &ArgMatches) -> ChannelIdent {
    channel_from_matches(matches).unwrap()
}

/// Resolve a channel. Taken from the environment or from CLI args, if
/// given or return the default channel value.
fn channel_from_matches_or_default(matches: &ArgMatches) -> ChannelIdent {
    channel_from_matches(matches).unwrap_or_else(ChannelIdent::configured_value)
}

/// Resolve a target. Default to x86_64-linux if none specified
fn target_from_matches(matches: &ArgMatches) -> Result<PackageTarget> {
    matches.get_one::<String>("PKG_TARGET")
           .map(String::as_str)
           .map(PackageTarget::from_str)
           .unwrap_or_else(|| Ok(active_target()))
           .map_err(Error::DojoCore)
}

/// Helper function to determine active package target.
/// It overrides x86_64-darwin to be x86_64-linux in order
/// to provide a better user experience (ie, for the 99% case)
fn active_target() -> PackageTarget {
    match PackageTarget::active_target() {
        target::X86_64_DARWIN => target::X86_64_LINUX,
        t => t,
    }
}

fn install_sources_from_matches(matches: &ArgMatches) -> Result<Vec<InstallSource>> {
    matches
        .get_many::<String>("PKG_IDENT_OR_ARTIFACT").map(|v| v.map(String::as_str))
        .unwrap() // Required via clap
        .map(|t| t.parse().map_err(Error::from))
        .collect()
}

fn excludes_from_matches(matches: &ArgMatches) -> Vec<PackageIdent> {
    matches
        .get_many::<String>("EXCLUDE")
        .into_iter()
        .flatten()
        .map(|i| PackageIdent::from_str(i).unwrap()) // unwrap safe as we've validated the input
        .collect()
}

/// Connect to a Supervisor, send it one request and hand each reply to `on_reply` until the last.
fn ctl_call<T, F>(listen_ctl_addr: &ListenCtlAddr,
                  secret_key: &str,
                  request: T,
                  mut on_reply: F)
                  -> Result<()>
    where T: Into<SrvMessage> + fmt::Debug,
          F: FnMut(&SrvMessage) -> result::Result<(), SrvClientError>
{
    let mut conn = SrvClient::connect(listen_ctl_addr, secret_key)?;
    for reply in conn.call(request)? {
        on_reply(&reply?)?;
    }
    Ok(())
}

fn handle_ctl_reply(reply: &SrvMessage) -> result::Result<(), SrvClientError> {
    let mut progress_bar = pbr::ProgressBar::<io::Stdout>::new(0);
    progress_bar.set_units(pbr::Units::Bytes);
    progress_bar.show_tick = true;
    progress_bar.message("    ");
    match reply.message_id() {
        "ConsoleLine" => {
            let m = reply.parse::<protocol::ctl::ConsoleLine>()
                         .map_err(SrvClientError::Decode)?;
            let mut new_spec = ColorSpec::new();
            let msg_spec = match m.color {
                Some(color) => {
                    new_spec.set_fg(Some(Color::from_str(&color)?))
                            .set_bold(m.bold)
                }
                None => new_spec.set_bold(m.bold),
            };
            common::ui::print(UI::default_with_env().out(), m.line.as_bytes(), msg_spec)?;
        }
        "NetProgress" => {
            let m = reply.parse::<protocol::ctl::NetProgress>()
                         .map_err(SrvClientError::Decode)?;
            progress_bar.total = m.total;
            if progress_bar.set(m.position) >= m.total {
                progress_bar.finish();
            }
        }
        "NetErr" => {
            let m = reply.parse::<protocol::net::NetErr>()
                         .map_err(SrvClientError::Decode)?;
            return Err(SrvClientError::from(m));
        }
        _ => (),
    }
    Ok(())
}

fn print_svc_status<T>(out: &mut T,
                       reply: &SrvMessage,
                       print_header: bool)
                       -> result::Result<(), SrvClientError>
    where T: io::Write
{
    let status = match reply.message_id() {
        "ServiceStatus" => {
            reply.parse::<protocol::types::ServiceStatus>()
                 .map_err(SrvClientError::Decode)?
        }
        "NetOk" => {
            println!("No services loaded.");
            return Ok(());
        }
        "NetErr" => {
            let err = reply.parse::<protocol::net::NetErr>()
                           .map_err(SrvClientError::Decode)?;
            return Err(SrvClientError::from(err));
        }
        _ => {
            warn!("Unexpected status message, {:?}", reply);
            return Ok(());
        }
    };
    let svc_desired_state = status.desired_state
                                  .map_or("<none>".to_string(), |s| s.to_string());
    let (svc_state, svc_pid, svc_elapsed) = {
        match status.process {
            Some(process) => {
                (process.state.to_string(),
                 process.pid
                        .map_or_else(|| "<none>".to_string(), |p| p.to_string()),
                 process.elapsed.unwrap_or_default().to_string())
            }
            None => {
                (ProcessState::default().to_string(), "<none>".to_string(), "<none>".to_string())
            }
        }
    };
    if print_header {
        writeln!(out, "{}", STATUS_HEADER.join("\t")).unwrap();
    }
    // Composites were removed in 0.75 but people could be
    // depending on the exact format of this output even if they
    // never used composites. We don't want to break their tooling
    // so we hardcode in 'standalone' as it's the only supported
    // package type
    //
    // TODO: Remove this when we have a stable machine-readable alternative
    // that scripts could depend on
    writeln!(out,
             "{}\tstandalone\t{}\t{}\t{}\t{}\t{}",
             status.ident,
             DesiredState::from_str(&svc_desired_state)?,
             ProcessState::from_str(&svc_state)?,
             svc_elapsed,
             svc_pid,
             status.service_group,)?;
    Ok(())
}

/// Check if we have a launcher/supervisor running out of this dojo root.
/// If the launcher PID file exists then the supervisor is up and running
fn launcher_is_running(fs_root_path: &Path) -> bool {
    let launcher_root = launcher_root_path(Some(fs_root_path));
    let pid_file_path = launcher_root.join("PID");

    pid_file_path.is_file()
}

fn supervisor_services() -> Result<Vec<PackageIdent>> {
    if !launcher_is_running(&*FS_ROOT) {
        return Ok(vec![]);
    }

    let cfg = config::load()?;
    let secret_key = ctl_secret_key(&cfg)?;
    let listen_ctl_addr = ListenCtlAddr::default();
    let msg = protocol::ctl::SvcStatus::default();

    let mut out: Vec<PackageIdent> = vec![];
    ctl_call(&listen_ctl_addr, &secret_key, msg, |reply| {
        match reply.message_id() {
            "ServiceStatus" => {
                let m = reply.parse::<protocol::types::ServiceStatus>()
                             .map_err(SrvClientError::Decode)?;
                out.push(m.ident.into());
                Ok(())
            }
            "NetOk" => Ok(()),
            "NetErr" => {
                let err = reply.parse::<protocol::net::NetErr>()
                               .map_err(SrvClientError::Decode)?;
                Err(SrvClientError::from(err))
            }
            _ => {
                warn!("Unexpected status message, {:?}", reply);
                Ok(())
            }
        }
    })?;
    Ok(out)
}

/// A Builder URL, but *only* if the user specified it via CLI args or
/// the environment
fn bldr_url_from_input(m: &ArgMatches) -> Option<String> {
    m.get_one::<String>("BLDR_URL")
     .map(String::as_str)
     .and_then(|u| Some(u.to_string()))
     .or_else(bldr_url_from_env)
}

/// If the user provides both --application and --environment options,
/// parse and set the value on the spec.
fn get_app_env_from_input(m: &ArgMatches) -> Result<Option<ApplicationEnvironment>> {
    if let (Some(app), Some(env)) = (m.get_one::<String>("APPLICATION").map(String::as_str),
                                     m.get_one::<String>("ENVIRONMENT").map(String::as_str))
    {
        Ok(Some(ApplicationEnvironment { application: app.to_string(),
                                         environment: env.to_string(), }))
    } else {
        Ok(None)
    }
}

fn get_binds_from_input(m: &ArgMatches) -> Result<Option<ServiceBindList>> {
    match m.get_many::<String>("BIND").map(|v| v.map(String::as_str)) {
        Some(bind_strs) => {
            let mut list = ServiceBindList::default();
            for bind_str in bind_strs {
                list.binds.push(ServiceBind::from_str(bind_str)?);
            }
            Ok(Some(list))
        }
        None => Ok(None),
    }
}

fn get_binding_mode_from_input(m: &ArgMatches) -> Option<protocol::types::BindingMode> {
    // There won't be errors, because we validate with `valid_binding_mode`
    m.get_one::<String>("BINDING_MODE")
     .map(String::as_str)
     .and_then(|b| BindingMode::from_str(b).ok())
}

fn get_group_from_input(m: &ArgMatches) -> Option<String> {
    m.get_one::<String>("GROUP")
     .map(String::as_str)
     .map(ToString::to_string)
}

fn get_health_check_interval_from_input(m: &ArgMatches)
                                        -> Option<protocol::types::HealthCheckInterval> {
    // Value will have already been validated by `cli::valid_health_check_interval`
    m.get_one::<String>("HEALTH_CHECK_INTERVAL")
     .map(String::as_str)
     .and_then(|s| HealthCheckInterval::from_str(s).ok())
     .map(|s| s.into())
}

fn get_topology_from_input(m: &ArgMatches) -> Option<Topology> {
    m.get_one::<String>("TOPOLOGY")
     .map(String::as_str)
     .and_then(|f| Topology::from_str(f).ok())
}

fn get_strategy_from_input(m: &ArgMatches) -> Option<UpdateStrategy> {
    m.get_one::<String>("STRATEGY")
     .map(String::as_str)
     .and_then(|f| UpdateStrategy::from_str(f).ok())
}

fn listen_ctl_addr_from_input(m: &ArgMatches) -> Result<ListenCtlAddr> {
    m.get_one::<String>("REMOTE_SUP")
     .map(String::as_str)
     .map_or(Ok(ListenCtlAddr::default()), resolve_listen_ctl_addr)
}

fn resolve_listen_ctl_addr(input: &str) -> Result<ListenCtlAddr> {
    let listen_ctl_addr = if input.find(':').is_some() {
        input.to_string()
    } else {
        format!("{}:{}", input, ListenCtlAddr::DEFAULT_PORT)
    };

    listen_ctl_addr.to_socket_addrs()
                   .and_then(|mut addrs| {
                       addrs.find(std::net::SocketAddr::is_ipv4).ok_or_else(|| {
                                                                    io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    "Address could not be resolved.",
                )
                                                                })
                   })
                   .map(ListenCtlAddr::from)
                   .map_err(|e| Error::RemoteSupResolutionError(listen_ctl_addr, e))
}

/// Check to see if the user has passed in a USER param.
/// If not, check the DOJ_USER env var. If that's
/// empty too, then return an error.
fn user_param_or_env(m: &ArgMatches) -> Option<String> {
    match m.get_one::<String>("USER").map(String::as_str) {
        Some(u) => Some(u.to_string()),
        None => {
            match env::var(DOJO_USER_ENVVAR) {
                Ok(v) => Some(v),
                Err(_) => None,
            }
        }
    }
}

// Based on UI::default_with_env, but taking into account the setting
// of the global color variable.
//
// TODO: Ideally we'd have a unified way of setting color, so this
// function wouldn't be necessary. In the meantime, though, it'll keep
// the scope of change contained.
fn ui() -> UI {
    let isatty = if env::var(NONINTERACTIVE_ENVVAR).map(|val| val == "1" || val == "true")
                                                   .unwrap_or(false)
    {
        Some(false)
    } else {
        None
    };
    UI::default_with(output::get_format().color_choice(), isatty)
}

/// Set all fields for an `SvcLoad` message that we can from the given opts. This function
/// populates all *shared* options between `run` and `load`.
fn update_svc_load_from_input(m: &ArgMatches, msg: &mut protocol::ctl::SvcLoad) -> Result<()> {
    msg.bldr_url = bldr_url_from_input(m);
    msg.bldr_channel = channel_from_matches(m).map(|c| c.to_string());
    msg.application_environment = get_app_env_from_input(m)?;
    msg.binds = get_binds_from_input(m)?;
    if m.get_flag("FORCE") {
        msg.force = Some(true);
    }
    msg.group = get_group_from_input(m);
    msg.health_check_interval = get_health_check_interval_from_input(m);
    msg.binding_mode = get_binding_mode_from_input(m).map(|v| v as i32);
    msg.topology = get_topology_from_input(m).map(|v| v as i32);
    msg.update_strategy = get_strategy_from_input(m).map(|v| v as i32);
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    mod resolve_listen_ctl_addr {
        use super::*;

        #[test]
        fn ip_is_resolved() {
            let expected =
                ListenCtlAddr::from_str("127.0.0.1:8080").expect("Could not create ListenCtlAddr");
            let actual =
                resolve_listen_ctl_addr("127.0.0.1:8080").expect("Could not resolve string");

            assert_eq!(expected, actual);
        }

        #[test]
        fn localhost_is_resolved() {
            let expected =
                ListenCtlAddr::from_str("127.0.0.1:8080").expect("Could not create ListenCtlAddr");
            let actual =
                resolve_listen_ctl_addr("localhost:8080").expect("Could not resolve string");

            assert_eq!(expected, actual);
        }

        #[test]
        fn port_is_set_to_default_if_not_specified() {
            let expected =
                ListenCtlAddr::from_str(&format!("127.0.0.1:{}", ListenCtlAddr::DEFAULT_PORT))
                    .expect("Could not create ListenCtlAddr");
            let actual = resolve_listen_ctl_addr("localhost").expect("Could not resolve string");

            assert_eq!(expected, actual);
        }
    }
}
