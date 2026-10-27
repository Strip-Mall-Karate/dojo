use dojo_common as common;
use dojo_pkg_export_tar as export_tar;
use env_logger;
#[macro_use]
extern crate log;

use crate::{common::{ui::{UIWriter,
                          UI},
                     PROGRAM_NAME},
            export_tar::{Cli,
                         Result}};
use clap::Command;

fn main() {
    let mut ui = UI::default_with_env();
    if let Err(e) = start(&mut ui) {
        ui.fatal(e).unwrap();
        std::process::exit(1)
    }
}

fn start(ui: &mut UI) -> Result<()> {
    env_logger::init();
    let cli = cli();
    let m = cli.get_matches();
    debug!("clap cli args: {:?}", m);

    export_tar::export_for_cli_matches(ui, &m)
}

fn cli() -> Command {
    let name: &str = &*PROGRAM_NAME;
    let about = "Creates a tar package from a Dojo package";
    Cli::new(name, about).add_base_packages_args()
                         .add_builder_args()
                         .add_pkg_ident_arg()
                         .app
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_cli_is_well_formed() { super::cli().debug_assert(); }

    #[test]
    fn help_puts_the_maintainer_on_its_own_line() {
        let help = super::cli().render_help().to_string();
        let mut lines = help.lines();
        assert!(lines.next().unwrap().ends_with(super::export_tar::VERSION.trim()));
        assert_eq!(lines.next(), Some("Maintainer: Strip Mall Karate Company"));
    }

    #[test]
    fn the_package_and_builder_are_read() {
        let m = super::cli().try_get_matches_from(["export",
                                                   "--url",
                                                   "https://bldr.example.com",
                                                   "--channel",
                                                   "unstable",
                                                   "core/redis"])
                            .unwrap();
        assert_eq!(m.get_one::<String>("PKG_IDENT_OR_ARTIFACT").unwrap(),
                   "core/redis");
        assert_eq!(m.get_one::<String>("BLDR_URL").unwrap(),
                   "https://bldr.example.com");
        assert!(super::cli().try_get_matches_from(["export", "--url", "not a url", "core/redis"])
                            .is_err());
    }
}
