// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use argh::FromArgs;

use librad::profile::ProfileId;

/// Management of Radicle profiles and their associated configuration data.
#[derive(Debug, FromArgs)]
pub struct Args {
    #[argh(subcommand)]
    pub command: Command,
}

#[derive(Debug, FromArgs)]
#[argh(subcommand)]
pub enum Command {
    Create(Create),
    Get(Get),
    Set(Set),
    List(List),
    Peer(GetPeerId),
    Paths(GetPaths),
    Ssh(SshAdd),
}

/// Create a new profile, generating a new secret key and initialising
/// configurations and storage.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "create")]
pub struct Create {}

/// Get the currently active profile.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "get")]
pub struct Get {}

/// Set the active profile.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "set")]
pub struct Set {
    /// the identifier to set the active profile to
    #[argh(option)]
    pub id: ProfileId,
}

/// List all profiles that have been created
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "list")]
pub struct List {}

/// Get the peer identifier associated with the provided profile identfier. If
/// no profile was provided, then the active one is used.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "peer-id")]
pub struct GetPeerId {
    /// the identifier to look up
    #[argh(option)]
    pub id: Option<ProfileId>,
}

/// Get the paths associated with the provided profile identfier. If no profile
/// was provided, then the active one is used.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "paths")]
pub struct GetPaths {
    /// the identifier to look up    
    #[argh(option)]
    pub id: Option<ProfileId>,
}

/// Add the profile's associated secrety key to the ssh-agent. If no profile was
/// provided, then the active one is used.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "ssh-add")]
pub struct SshAdd {
    /// the identifier to look up    
    #[argh(option)]
    pub id: Option<ProfileId>,
}
