// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use clap::Parser;

use librad::profile::ProfileId;
use lnk_clib::keys::ssh::SshAuthSock;

/// `--lnk-profile` command line name
pub const LNK_PROFILE_ARG: &str = "--lnk-profile";

/// `--lnk-quiet` command line name
pub const LNK_QUIET_ARG: &str = "--lnk-quiet";

/// `--lnk-verbose` command line name
pub const LNK_VERBOSE_ARG: &str = "--lnk-verbose";

#[derive(Debug, Parser)]
#[clap(author, version, about)]
pub struct Args {
    #[clap(flatten)]
    pub global: Global,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Parser)]
pub struct Global {
    /// The profile identifier, if not given then the currently active profile
    /// is used
    #[clap(global = true, long, env = "LNK_PROFILE")]
    pub lnk_profile: Option<ProfileId>,

    /// Which unix domain socket to use for connecting to the ssh-agent. The
    /// default will defer to SSH_AUTH_SOCK, otherwise the value given should be
    /// a valid path.
    #[clap(global = true, long, default_value_t)]
    pub lnk_ssh_auth_sock: SshAuthSock,

    /// No output printed to stdout
    #[clap(global = true, long)]
    pub lnk_quiet: bool,

    /// Use verbose output
    #[clap(global = true, long)]
    pub lnk_verbose: bool,
}

#[derive(Debug, Parser)]
pub enum Command {
    /// Manage Radicle Identities
    Identities(lnk_identities::cli::args::Args),
    /// Manage your Radicle profiles
    Profile(lnk_profile::cli::args::Args),
    #[clap(external_subcommand)]
    External(Vec<String>),
}
}
