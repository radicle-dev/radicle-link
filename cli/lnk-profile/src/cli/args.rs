// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use structopt::StructOpt;

use librad::profile::ProfileId;

/// Management of Radicle profiles and their associated configuration data.
#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(Debug, StructOpt)]
pub enum Command {
    Create(Create),
    Get(Get),
    Set(Set),
    List(List),
    Peer(GetPeerId),
    Paths(GetPaths),
    Ssh(Ssh),
}

/// Create a new profile, generating a new secret key and initialising
/// configurations and storage.
#[derive(Debug, StructOpt)]
pub struct Create {}

/// Get a profile, defaulting to the active profile if no identifier is given.
#[derive(Debug, StructOpt)]
pub struct Get {
    /// the identifier of the profile requested
    #[structopt(long)]
    pub id: Option<ProfileId>,
}

/// Set the active profile.
#[derive(Debug, StructOpt)]
pub struct Set {
    /// the identifier to set the active profile to
    #[structopt(long)]
    pub id: ProfileId,
}

/// List all profiles that have been created
#[derive(Debug, StructOpt)]
pub struct List {}

/// Get the peer identifier associated with the provided profile identfier. If
/// no profile was provided, then the active one is used.
#[derive(Debug, StructOpt)]
pub struct GetPeerId {
    /// the identifier to look up
    #[structopt(long)]
    pub id: Option<ProfileId>,
}

/// Get the paths associated with the provided profile identfier. If no profile
/// was provided, then the active one is used.
#[derive(Debug, StructOpt)]
pub struct GetPaths {
    /// the identifier to look up    
    #[structopt(long)]
    pub id: Option<ProfileId>,
}

/// Manage the profile's key material on the ssh-agent
#[derive(Debug, StructOpt)]
pub struct Ssh {
    #[structopt(subcommand)]
    pub options: ssh::Options,
}

pub mod ssh {
    use super::*;

    #[derive(Debug, StructOpt)]
    pub enum Options {
        Add(Add),
        Rm(Rm),
        Ready(Ready),
        Sign(Sign),
        Verify(Verify),
    }

    /// Add the profile's associated secret key to the ssh-agent. If no profile
    /// was provided, then the active one is used.
    #[derive(Debug, StructOpt)]
    pub struct Add {
        /// the identifier to look up
        #[structopt(long)]
        pub id: Option<ProfileId>,
        /// the lifetime of the key being added to the ssh-agent, if none is
        /// provided the default lifetime is left to the agent used (for
        /// `ssh-agent` this is forever).
        #[structopt(long, short)]
        pub time: Option<u32>,
    }

    /// Remove the profile's associated secret key from the ssh-agent. If no
    /// profile was provided, then the active one is used.
    #[derive(Debug, StructOpt)]
    pub struct Rm {
        /// the identifier to look up
        #[structopt(long)]
        pub id: Option<ProfileId>,
    }

    /// See if the profile's associated secret key is present in the ssh-agent,
    /// ready for signing. If no profile was provided, then the active one
    /// is used.
    #[derive(Debug, StructOpt)]
    pub struct Ready {
        /// the identifier to look up
        #[structopt(long)]
        pub id: Option<ProfileId>,
    }

    /// Sign a payload with the profile's associated secret key. If no profile
    /// was provided, then the active one is used.
    #[derive(Debug, StructOpt)]
    pub struct Sign {
        /// the identifier to look up
        #[structopt(long)]
        pub id: Option<ProfileId>,
        /// the payload to sign
        #[structopt(long)]
        pub payload: String,
    }

    /// Verify a signature of a payload with the profile's associated public
    /// key. If no profile was provided, then the active one is used.
    #[derive(Debug, StructOpt)]
    pub struct Verify {
        /// the identifier to look up
        #[structopt(long)]
        pub id: Option<ProfileId>,
        /// the payload to be verified. Defaults to "radicle-link.xyz" for
        /// debugging purposes.
        #[structopt(long, default_value = "radicle-link.xyz")]
        pub payload: String,
        /// the expected signature for the signed payload.
        #[structopt(long)]
        pub signature: String,
    }
}
