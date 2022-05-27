// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use librad::git::Urn;

use crate::Mode;

#[derive(Clone, Debug, clap::Subcommand)]
pub enum Args {
    /// Synchronise local state with configured seeds
    Sync {
        /// The URN we will synchronise
        #[clap(long)]
        urn: Urn,
        /// Whether to fetch,push or both to seeds
        #[clap(long, default_value_t)]
        mode: Mode,
    },
    /// Attempt to clone a project URN into a local working directory
    ///
    /// This will first track the URN and attempt to fetch it from your
    /// configured seeds. If any data is found it will be checked out to a
    /// local working directory. The checked out working copy will have
    /// remotes set up in the form rad://<handle>@<peer id> for each delegate of
    /// the URN.
    ///
    /// # Choosing a peer
    ///
    /// If you run clone without a peer selected (the --peer argument) then this
    /// will attempt to determine what the default branch of the project
    /// should be by examining all the delegates and seeing if they agree on
    /// a common OID for the default branch. If the delegates agree then the
    /// default branch will be checked out. If they do not an error message will
    /// be displayed which should give you more information to help choose a
    /// peer to clone from.
    Clone {
        /// The URN of the project to clone
        #[clap(long)]
        urn: Urn,
        /// The path to check the project out into. If this is not specified
        /// then the project will be checked out into $PWD/<project
        /// name>, if this is specified then the project will be checked
        /// out into the specified directory - throwing an error if the
        /// directory is not empty.
        #[clap(long)]
        path: Option<std::path::PathBuf>,
        /// A specific peer to clone from
        #[clap(long)]
        peer: Option<librad::PeerId>,
    },
}
