// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::path::PathBuf;

use argh::FromArgs;

use librad::{git::Urn, internal::canonical::Cstring, peer::PeerId};

/// Management of Radicle projects and their working-copies.
///
/// This tools allows you to create projects in your Radicle store and manage
/// the remotes for their working copies.
#[derive(Debug, FromArgs)]
pub struct Args {
    #[argh(subcommand)]
    pub command: Command,
}

#[derive(Debug, FromArgs)]
#[argh(subcommand)]
pub enum Command {
    Existing(Existing),
    Fork(Fork),
    New(New),
    Update(Update),
}

/// 🆙 Update the remotes that exist in the include file for the given project
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "include-update")]
pub struct Update {
    /// the project's URN we are interested in
    #[argh(option, from_str_fn(Urn::try_from))]
    pub urn: Urn,
}

/// 🆕 Creates a fresh, new Radicle project in the provided directory and using
/// the provided name. The final directory must not already exist, i.e.
/// <path>/<name> should not already exist.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "create-new")]
pub struct New {
    /// description of the project we are creating
    #[argh(option, from_str_fn(Cstring::from))]
    pub description: Option<Cstring>,
    /// the default branch name for the project
    #[argh(option, from_str_fn(Cstring::from))]
    pub default_branch: Cstring,
    /// the name of the project
    #[argh(option, from_str_fn(Cstring::from))]
    pub name: Cstring,
    /// the directory where we create the project
    #[argh(option)]
    pub path: PathBuf,
}

/// 🔄 Creates a new Radicle project using an existing git repository as the
/// working copy. The name of the project will be the last component of the
/// directory path, e.g. `~/Developer/radicle-link` will have the name
/// `radicle-link`. The git repository must already exist on your filesystem, if
/// it doesn't use the `new` command instead.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "create-existing")]
pub struct Existing {
    /// description of the project we want to create
    #[argh(option, from_str_fn(Cstring::from))]
    pub description: Option<Cstring>,
    /// the default branch name for the project
    #[argh(option, from_str_fn(Cstring::from))]
    pub default_branch: Cstring,
    /// the directory of the existing git repository
    #[argh(option)]
    pub path: PathBuf,
}

/// 🔀 Creates a working copy on your filesystem based off of a Radicle project.
///   * If no `--peer` is given the working copy will based off of your own view
///     of the project.
///   * If `--peer` is given and it's the same as the current peer, then it's
///     the same as above.
///   * If `--peer` is given and it's not the current peer, then the working
///     copy will be based off
///   of the remote's view of the project.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "create-fork")]
pub struct Fork {
    /// the peer were are forking from
    #[argh(option, from_str_fn(PeerId::try_from))]
    pub peer: Option<PeerId>,
    /// the project's URN
    #[argh(option, from_str_fn(Urn::try_from))]
    pub urn: Urn,
    /// the path where we are creating the working copy
    #[argh(option)]
    pub path: PathBuf,
}
