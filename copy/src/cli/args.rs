// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::path::PathBuf;

use argh::FromArgs;

use librad::{git::Urn, internal::canonical::Cstring, peer::PeerId};

/// Management of Radicle projects and their working copies.
///
/// This tools allows you to create projects in your Radicle store and manage
/// the remotes for their working copies.
#[derive(Debug, FromArgs)]
pub struct Args {
    #[argh(subcommand)]
    pub command: Command,
    /// 🔑 If you store multiple private keys, this allows you to specify which
    /// one you wish to use. Note that this is just the file name, and not
    /// the whole path.
    #[argh(option)]
    pub key: Option<PathBuf>,
}

#[derive(Debug, FromArgs)]
#[argh(subcommand)]
pub enum Command {
    Garden(Garden),
    Community(Community),
}

/// 🌍 Commands to help manage the remote community that appear in your working
/// copy. These remotes are the peers that you track with regard to a single
/// project.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "include")]
pub struct Community {
    #[argh(subcommand)]
    pub community: community::Options,
}

pub mod community {
    use super::*;
    #[derive(Debug, FromArgs)]
    #[argh(subcommand)]
    pub enum Options {
        Update(Update),
    }

    /// 🏡 Update your remote community for a given project. This will modify
    /// the include file that is configured in your working copy, i.e.
    /// `.git/config`. Peers come and go, so this helps you keep up-to-date
    /// with your latest community branches.
    #[derive(Debug, FromArgs)]
    #[argh(subcommand, name = "update")]
    pub struct Update {
        /// the project's URN we are interested in
        #[argh(option, from_str_fn(Urn::try_from))]
        pub urn: Urn,
    }
}

/// 🌸 Commands to help manage your Radicle garden of projects. They help you
/// kickoff projects and link them to working copies on your filesystem.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "garden")]
pub struct Garden {
    #[argh(subcommand)]
    pub garden: garden::Options,
}

pub mod garden {
    use std::{convert::TryFrom as _, fs::File, io::BufReader};

    use serde::Deserialize;

    use librad::{
        git::{identities::local::LocalIdentity, storage::Storage},
        git_ext::{OneLevel, RefLike},
        paths::Paths,
        signer::BoxedSigner,
    };

    use super::*;

    #[derive(Debug, FromArgs)]
    #[argh(subcommand)]
    pub enum Options {
        Bed(Bed),
        Plant(Plant),
        Repot(Repot),
        Graft(Graft),
    }

    /// 🌱 Plants a fresh, new Radicle project in the provided directory and
    /// using the provided name. The final directory must not already exist,
    /// i.e. <path>/<name> should not already exist.
    #[derive(Debug, Deserialize, FromArgs)]
    #[argh(subcommand, name = "plant")]
    #[serde(rename_all = "camelCase")]
    pub struct Plant {
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

    impl Plant {
        pub fn cultivate(
            self,
            storage: &Storage,
            signer: BoxedSigner,
            paths: Paths,
            whoami: LocalIdentity,
        ) -> anyhow::Result<()> {
            use crate::garden::{plant, plant::Plant};

            let default_branch = OneLevel::from(RefLike::try_from(self.default_branch.as_str())?);
            let raw = Plant::new(self.description, default_branch, self.name, self.path);
            let valid = Plant::validate(raw)?;
            let path = valid.path();
            let project = plant(paths, signer, &storage, whoami, valid)?;

            project_success(&project.urn(), path);
            Ok(())
        }
    }

    /// 🪴 Repots a new Radicle project using an existing git repository as
    /// the working copy. The name of the project will be the last component
    /// of the directory path, e.g. `~/Developer/radicle-link` will have the
    /// name `radicle-link`. The git repository must already exist on your
    /// filesystem, if it doesn't use the `new` command instead.
    #[derive(Debug, Deserialize, FromArgs)]
    #[argh(subcommand, name = "repot")]
    #[serde(rename_all = "camelCase")]
    pub struct Repot {
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

    impl Repot {
        pub fn cultivate(
            self,
            storage: &Storage,
            signer: BoxedSigner,
            paths: Paths,
            whoami: LocalIdentity,
        ) -> anyhow::Result<()> {
            use crate::garden::{repot, repot::Repot};

            let default_branch = OneLevel::from(RefLike::try_from(self.default_branch.as_str())?);
            let raw = Repot::new(self.description, default_branch, self.path.clone())?;
            let valid = Repot::validate(raw)?;
            let project = repot(paths, signer, &storage, whoami, valid)?;

            project_success(&project.urn(), self.path);
            Ok(())
        }
    }

    /// 🍄 Grafts a new working copy on your filesystem based off of a Radicle
    /// project. This is useful when you've replicated a new project from a peer
    /// and you want to start contributing to it, or maybe even just have a
    /// peek around. You can think of this as similar to cloning, where it's
    /// cloning from your Radicle storage.
    #[derive(Debug, FromArgs)]
    #[argh(subcommand, name = "graft")]
    pub struct Graft {
        /// the peer were are forking from. If none is given the working copy
        /// will based off of your own view of the project. If it is
        /// given and it's the same as the current peer, then it's
        /// the same as the previous explanation. If it is given and it's not
        /// the current peer, then the working copy will be based off of
        /// the remote's view of the project.
        #[argh(option, from_str_fn(PeerId::try_from))]
        pub peer: Option<PeerId>,
        /// the project's URN
        #[argh(option, from_str_fn(Urn::try_from))]
        pub urn: Urn,
        /// the path where we are creating the working copy
        #[argh(option)]
        pub path: PathBuf,
    }

    /// 🌷🌹 Build a garden of projects based off of a manifest file.
    #[derive(Debug, FromArgs)]
    #[argh(subcommand, name = "bed")]
    pub struct Bed {
        /// the manifest file that contains JSON descriptions of `plant` and
        /// `repot`.
        #[argh(option)]
        pub manifest: PathBuf,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type")]
    pub enum BagOfSeeds {
        Plant(Plant),
        Repot(Repot),
    }

    impl Bed {
        pub fn load(&self) -> anyhow::Result<Vec<BagOfSeeds>> {
            let file = File::open(&self.manifest)?;
            let reader = BufReader::new(file);

            Ok(serde_json::from_reader(reader)?)
        }
    }

    fn project_success(urn: &Urn, path: PathBuf) {
        println!("Your project was created 🎉");
        println!("The project's URN is `{}`", urn);
        println!("The working copy exists at `{}`", path.display());
    }
}
