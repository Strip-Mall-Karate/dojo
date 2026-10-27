use clap::Command;
use doj::cli::sup_commands;

pub fn cli() -> Command { sup_commands() }

#[cfg(test)]
mod test {
    use super::cli;
    use clap::error::ErrorKind;

    macro_rules! assert_cli_cmd {
        ($test:ident, $cmd:expr, $( $key:expr => $value:tt ),+) => {
            #[test]
            fn $test() {
                assert_cmd!(cli(), $cmd, $( $key => $value ),+ );
            }
        }
    }

    #[test]
    fn sup_help_on_run_subcommand() {
        let r = cli().try_get_matches_from(vec!["doj-sup", "run", "--help"]);
        assert!(r.is_err());
        // not `ErrorKind::InvalidSubcommand`
        assert_eq!(r.unwrap_err().kind(), ErrorKind::DisplayHelp);
    }

    mod sup_run {
        use super::*;
        use std::iter::FromIterator as _;

        assert_cli_cmd!(should_handle_multiple_peer_flags,
                        "doj-sup run --peer 1.1.1.1 --peer 2.2.2.2",
                        "PEER" => ["1.1.1.1", "2.2.2.2"]);

        assert_cli_cmd!(should_handle_single_peer_flag_with_multiple_values,
                        "doj-sup run --peer 1.1.1.1 2.2.2.2",
                        "PEER" => ["1.1.1.1", "2.2.2.2"]);

        assert_cli_cmd!(should_handle_peer_flag_with_arguments,
                        "doj-sup run --peer 1.1.1.1 2.2.2.2 -- karate/redis",
                        "PEER" => ["1.1.1.1", "2.2.2.2"],
                        "PKG_IDENT_OR_ARTIFACT" => "karate/redis");

        assert_cli_cmd!(should_handle_multiple_bind_flags,
                        "doj-sup run --bind service.group1 --bind service.group2",
                        "BIND" => ["service.group1", "service.group2"]);

        assert_cli_cmd!(should_handle_single_bind_flag_with_multiple_values,
                        "doj-sup run --bind service.group1 service.group2",
                        "BIND" => ["service.group1", "service.group2"]);

        assert_cli_cmd!(should_handle_bind_flag_with_arguments,
                        "doj-sup run --bind service.group1 service.group2 -- karate/redis",
                        "BIND" => ["service.group1", "service.group2"],
                        "PKG_IDENT_OR_ARTIFACT" => "karate/redis");

        #[test]
        fn local_gossip_mode_and_listen_gossip_are_mutually_exclusive() {
            let cmd_vec = Vec::from_iter("doj-sup run --listen-gossip 1.1.1.1:1111 \
                                          --local-gossip-mode"
                                                              .split_whitespace());
            assert!(cli().try_get_matches_from(cmd_vec).is_err());
        }

        #[test]
        fn local_gossip_mode_and_peer_are_mutually_exclusive() {
            let cmd_vec = Vec::from_iter(
                "doj-sup run --peer 1.1.1.1:1111 --local-gossip-mode".split_whitespace(),
            );
            assert!(cli().try_get_matches_from(cmd_vec).is_err());
        }

        #[test]
        fn local_gossip_mode_and_peer_watch_file_are_mutually_exclusive() {
            let cmd_vec = Vec::from_iter("doj-sup run --local-gossip-mode --peer-watch-file \
                                          foobar"
                                                 .split_whitespace());
            assert!(cli().try_get_matches_from(cmd_vec).is_err());
        }

        #[test]
        fn peer_watch_file_and_peer_are_mutually_exclusive() {
            let cmd_vec = Vec::from_iter("doj-sup run --peer 1.1.1.1:1111 --peer-watch-file \
                                          foobar"
                                                 .split_whitespace());
            assert!(cli().try_get_matches_from(cmd_vec).is_err());
        }
    }
}
