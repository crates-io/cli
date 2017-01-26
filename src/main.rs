#[macro_use]
extern crate clap;
extern crate crates_io_cli;

use crates_io_cli::{ok_or_exit, handle_interactive_search, handle_recent_changes, handle_list,
                    by_user, OutputKind};

use std::env;
use std::path::PathBuf;
use clap::{Arg, SubCommand, App};

const CHANGES_SUBCOMMAND_DESCRIPTION: &'static str = r##"
The output of this command is based on the state of the current crates.io repository clone.
It will remember the last result, so that the next invocation might yield different (or no)
changed crates at all.
Please note that the first query is likely to yield more than 40000 results!
The first invocation may be slow as it might have to clone the crates.io index.
"##;


fn default_repository_dir() -> PathBuf {
    let mut p = env::temp_dir();
    p.push("crates-io-bare-clone_for-cli");
    p
}

fn invalid_subcommand(matches: &clap::ArgMatches) -> ! {
    print!("{}\n", matches.usage());
    std::process::exit(1);
}

fn main() {
    let temp_dir = default_repository_dir();
    let temp_dir_str = temp_dir.to_string_lossy();
    let human_output = format!("{}", OutputKind::human);
    let app = App::new("crates.io interface")
        .version(crate_version!())
        .author("Sebastian Thiel <byronimo@gmail.com>")
        .about("Interact with the https://crates.io index via the command-line")
        .arg(Arg::with_name("repository")
            .short("r")
            .long("repository")
            .value_name("REPO")
            .help("Path to the possibly existing crates.io repository clone.")
            .default_value(&temp_dir_str)
            .required(false)
            .takes_value(true))
        .subcommand(SubCommand::with_name("recent-changes")
            .about("show all recently changed crates")
            .display_order(1)
            .arg(Arg::with_name("format")
                .short("o")
                .long("output")
                .required(false)
                .takes_value(true)
                .default_value(&human_output)
                .possible_values(&OutputKind::variants())
                .help("The type of output to produce."))
            .after_help(CHANGES_SUBCOMMAND_DESCRIPTION))
        .subcommand(SubCommand::with_name("search")
            .display_order(2)
            .about("search crates interactively"))
        .subcommand(SubCommand::with_name("list")
            .display_order(3)
            .subcommand(SubCommand::with_name("by-user")
                .arg(Arg::with_name("user-id")
                    .required(true)
                    .takes_value(true)
                    .help("The numerical id of your user, e.g. 980. Currently there is no way \
                           to easily obtain it though, so you will have to debug actual \
                           crates.io calls in your browser - the /me response contains all \
                           user data. Use any string to receive *all* crates!"))
                .about("crates for the given username"))
            .about("list crates by a particular criterion"));


    let matches = app.get_matches();
    let repo_path = matches.value_of("repository").expect("default to be set");

    match matches.subcommand() {
        ("recent-changes", Some(args)) => ok_or_exit(handle_recent_changes(repo_path, args)),
        ("search", Some(args)) => ok_or_exit(handle_interactive_search(args)),
        ("list", Some(list_args)) => {
            let (subcommand_handler, subcommand_args) = match list_args.subcommand() {
                ("by-user", Some(args)) => (by_user, args),
                _ => invalid_subcommand(list_args),
            };
            ok_or_exit(handle_list(list_args, subcommand_args, subcommand_handler));
        }
        _ => invalid_subcommand(&matches),
    }
}
