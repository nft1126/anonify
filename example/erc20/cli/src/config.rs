use super::term;
use clap::{Arg, ArgMatches};
use dirs;
use std::path::PathBuf;

const APPLICATION_DIRECTORY_NAME: &'static str = "anonify";
const APPLICATION_ENVIRONMENT_ROOT_DIR: &'static str = "ANONIFY_ROOT_DIR";

pub const VERSION: u32 = 1;
pub const ITERS: u32 = 1024;

/// root directory configuration
pub(crate) fn get_default_root_dir() -> PathBuf {
    match dirs::data_local_dir() {
        Some(dir) => dir.join(APPLICATION_DIRECTORY_NAME),
        None => panic!("Undefined the local data directory."),
    }
}

pub(crate) fn global_rootdir_definition<'a, 'b>(default: &'a PathBuf) -> Arg<'a, 'b> {
    Arg::with_name("ROOT_DIR")
        .long("root_dir")
        .help("the zface root direction")
        .default_value(default.to_str().unwrap())
        .env(APPLICATION_ENVIRONMENT_ROOT_DIR)
}

pub(crate) fn global_rootdir_match<'a>(default: &'a PathBuf, matches: &ArgMatches<'a>) -> PathBuf {
    match matches.value_of("ROOT_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(default),
    }
}

// quiet configuration

pub(crate) fn global_quiet_definition<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("QUIET")
        .long("quiet")
        .global(true)
        .help("run the command quietly, do not print anything to the command line output")
}

pub(crate) fn global_quiet_option(matches: &ArgMatches) -> bool {
    matches.is_present("QUIET")
}

// color configuration

pub(crate) fn global_color_definition<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("COLOR")
        .long("color")
        .takes_value(true)
        .default_value("auto")
        .possible_values(&["auto", "always", "never"])
        .global(true)
        .help("enable output colors or not")
}

pub(crate) fn global_color_option(matches: &ArgMatches) -> term::ColorChoice {
    match matches.value_of("COLOR") {
        None => term::ColorChoice::Auto,
        Some("auto") => term::ColorChoice::Auto,
        Some("always") => term::ColorChoice::Always,
        Some("never") => term::ColorChoice::Never,
        Some(&_) => unreachable!(),
    }
}

// verbosity configuration

pub(crate) fn global_verbose_definition<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name("VERBOSITY")
        .long("verbose")
        .short("v")
        .multiple(true)
        .global(true)
        .help("set the verbosity mode, multiple occurrences means more verbosity")
}

pub(crate) fn global_verbose_option<'a>(matches: &ArgMatches<'a>) -> u64 {
    matches.occurrences_of("VERBOSITY")
}

pub(crate) fn config_terminal(matches: &ArgMatches) -> term::Config {
    let quiet = global_quiet_option(matches);
    let color = global_color_option(matches);
    let verbosity = global_verbose_option(matches);

    if !quiet {
        let log_level = match verbosity {
            0 => log::LevelFilter::Warn,
            1 => log::LevelFilter::Info,
            2 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        };

        env_logger::Builder::from_default_env()
            .filter_level(log_level)
            .init();
    }

    term::Config { color, quiet }
}
