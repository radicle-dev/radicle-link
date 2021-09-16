// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use structopt::StructOpt;

use librad::profile::{ProfileId, RAD_PROFILE};

/// `--rad-profile` command line name
pub const RAD_PROFILE_ARG: &str = "--rad-profile";

/// `--rad-quiet` command line name
pub const RAD_QUIET_ARG: &str = "--rad-quiet";

/// `--rad-verbose` command line name
pub const RAD_VERBOSE_ARG: &str = "--rad-verbose";

#[derive(Debug, StructOpt)]
pub struct Args {
    /// The profile identifier, if not given then the currently active profile
    /// is used
    #[structopt(long)]
    pub rad_profile: Option<ProfileId>,

    /// No output printed to stdout
    #[structopt(long)]
    pub rad_quiet: bool,

    /// Use verbose output
    #[structopt(long)]
    pub rad_verbose: bool,

    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(Debug, StructOpt)]
pub enum Command {
    /// Manage your Radicle profiles
    Profile(rad_profile::cli::args::Args),
    #[structopt(external_subcommand)]
    External(Vec<String>),
}

/// If an external subcommand is called, we sanitise the global arguments according to the rules defined in [RFC 698](https://github.com/radicle-dev/radicle-link/blob/master/docs/rfc/0698-cli-infrastructure.adoc#global-parameters).
///
/// The rules are summarised as:
///   * The `rad` global _always_ takes precedence, e.g. `rad --rad-profile deaf
///     xxx --rad-profile beef` will result in `deaf`
///   * If multiple `rad` globals are given, it is an error, i.e. `rad
///     --rad-profile deaf --rad-profile beef`
///   * If multiple globals are given as part of the external command, the last
///     one is take, e.g. `rad xxx --rad-profile deaf --rad-profile beef` will
///     result in `beef`.
///   * Command line arguments take precedence over environment variables, e.g.
///     `RAD_PROFILE=deaf rad --rad-profile def` will result in `beef`
///
/// # Examples
///
/// ```text
/// rad --rad-profile=deaf xxx --rad-profile=beef # deaf
/// rad xxx --rad-profile=beef # beef
/// rad --rad-profile=deaf --rad-profile=beef xxx # beef
/// rad xxx --rad-profile=deaf --rad-profile=beef # beef
/// rad --rad-profile=dead xxx --rad-profile=deaf --rad-profile=beef # dead
/// ```
pub fn sanitise_globals(mut args: Args) -> Args {
    match &mut args.command {
        Command::External(external) => {
            sanitise_option(
                RAD_PROFILE_ARG,
                RAD_PROFILE,
                args.rad_profile.clone().map(|id| id.to_string()),
                external,
            );

            sanitise_flag(RAD_QUIET_ARG, "RAD_QUIET", args.rad_quiet, external);

            sanitise_flag(RAD_VERBOSE_ARG, "RAD_VERBOSE", args.rad_verbose, external);

            args
        },
        _ => args,
    }
}

fn sanitise_option(arg: &str, env: &str, global: Option<String>, external: &mut Vec<String>) {
    let env = env::var(env).ok();
    let ex_arg = {
        let mut value = None;
        while let Some(index) = find_arg(arg, external) {
            external.remove(index);
            value = Some(external.remove(index))
        }
        value
    };
    let value = global.or(ex_arg).or(env);
    if let Some(value) = value {
        external.extend_from_slice(&[arg.to_string(), value]);
    }
}

fn sanitise_flag(arg: &str, env: &str, val: bool, external: &mut Vec<String>) {
    let env = env::var(env).ok();
    let ex_arg = {
        let mut value = None;
        while let Some(index) = find_arg(arg, external) {
            external.remove(index);
            value = Some(true)
        }
        value
    };
    let value = val || ex_arg.is_some() || env.is_some();
    if value {
        external.extend_from_slice(&[arg.to_string()]);
    }
}

/// Get the position of an argument name, if present.
pub fn find_arg(needle: &str, external: &[String]) -> Option<usize> {
    external
        .iter()
        .enumerate()
        .find(|(_, arg)| arg.as_str() == needle)
        .map(|(i, _)| i)
}
