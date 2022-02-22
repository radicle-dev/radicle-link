// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use structopt::StructOpt;

use librad::profile::{ProfileId, LNK_PROFILE};
use lnk_clib::keys::ssh::SshAuthSock;

/// `--lnk-profile` command line name
pub const LNK_PROFILE_ARG: &str = "--lnk-profile";

/// `--lnk-quiet` command line name
pub const LNK_QUIET_ARG: &str = "--lnk-quiet";

/// `--lnk-verbose` command line name
pub const LNK_VERBOSE_ARG: &str = "--lnk-verbose";

#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    pub global: Global,

    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(Debug, StructOpt)]
pub struct Global {
    /// The profile identifier, if not given then the currently active profile
    /// is used
    #[structopt(long)]
    pub lnk_profile: Option<ProfileId>,

    /// Which unix domain socket to use for connecting to the ssh-agent. The
    /// default will defer to SSH_AUTH_SOCK, otherwise the value given should be
    /// a valid path.
    #[structopt(long, default_value)]
    pub lnk_ssh_auth_sock: SshAuthSock,

    /// No output printed to stdout
    #[structopt(long)]
    pub lnk_quiet: bool,

    /// Use verbose output
    #[structopt(long)]
    pub lnk_verbose: bool,
}

#[derive(Debug, StructOpt)]
pub enum Command {
    /// Manage Radicle Identities
    Identities(lnk_identities::cli::args::Args),
    /// Manage your Radicle profiles
    Profile(lnk_profile::cli::args::Args),
    #[structopt(external_subcommand)]
    External(Vec<String>),
}

/// If an external subcommand is called, we sanitise the global arguments according to the rules defined in [RFC 698](https://github.com/radicle-dev/radicle-link/blob/master/docs/rfc/0698-cli-infrastructure.adoc#global-parameters).
///
/// The rules are summarised as:
///   * The `lnk` global _always_ takes precedence, e.g. `lnk --lnk-profile deaf
///     xxx --lnk-profile beef` will result in `deaf`
///   * If multiple `lnk` globals are given, it is an error, i.e. `lnk
///     --lnk-profile deaf --lnk-profile beef`
///   * If multiple globals are given as part of the external command, the last
///     one is take, e.g. `lnk xxx --lnk-profile deaf --lnk-profile beef` will
///     result in `beef`.
///   * Command line arguments take precedence over environment variables, e.g.
///     `LNK_PROFILE=deaf lnk --lnk-profile def` will result in `beef`
///
/// # Examples
///
/// ```text
/// lnk --lnk-profile=deaf xxx --lnk-profile=beef # deaf
/// lnk xxx --lnk-profile=beef # beef
/// lnk --lnk-profile=deaf --lnk-profile=beef xxx # beef
/// lnk xxx --lnk-profile=deaf --lnk-profile=beef # beef
/// lnk --lnk-profile=dead xxx --lnk-profile=deaf --lnk-profile=beef # dead
/// ```
pub fn sanitise_globals(mut args: Args) -> Args {
    match &mut args.command {
        Command::External(external) => {
            sanitise_option(
                LNK_PROFILE_ARG,
                LNK_PROFILE,
                args.global.lnk_profile.clone().map(|id| id.to_string()),
                external,
            );

            sanitise_flag(LNK_QUIET_ARG, "LNK_QUIET", args.global.lnk_quiet, external);

            sanitise_flag(
                LNK_VERBOSE_ARG,
                "LNK_VERBOSE",
                args.global.lnk_verbose,
                external,
            );

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
