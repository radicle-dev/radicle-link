// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::path::PathBuf;

use clap::Parser;
use either::Either;

use librad::{
    crypto::PublicKey,
    git::Urn,
    identities::{
        git::Revision,
        payload::{self, KeyOrUrn},
    },
    PeerId,
};

/// Management of Radicle projects and their working copies.
///
/// This tools allows you to create projects in your Radicle store and manage
/// the remotes for their working copies.
#[derive(Debug, Parser)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Parser)]
pub enum Command {
    Project(Project),
    Person(Person),
    Any(Any),
    Local(Local),
    RadRefs(RadRefs),
    Refs(Refs),
    Track(tracking::Track),
    Untrack(tracking::Untrack),
}

/// create, get, or modify a Radicle project
#[derive(Debug, Parser)]
pub struct Project {
    #[clap(subcommand)]
    pub project: project::Options,
}

/// create, get, or modify a Radicle person
#[derive(Debug, Parser)]
pub struct Person {
    #[clap(subcommand)]
    pub person: person::Options,
}

/// get any Radicle identity
#[derive(Debug, Parser)]
pub struct Any {
    #[clap(subcommand)]
    pub any: any::Options,
}

/// get or set a Radicle local identity
#[derive(Debug, Parser)]
pub struct Local {
    #[clap(subcommand)]
    pub local: local::Options,
}

/// get the contents of `rad` references, e.g. `rad/self`, `rad/signed_refs`,
/// etc.
#[derive(Debug, Parser)]
pub struct RadRefs {
    #[clap(subcommand)]
    pub rad_refs: rad_refs::Options,
}

/// list the references under a given category, e.g. `heads`, `tags`, etc.
#[derive(Debug, Parser)]
pub struct Refs {
    #[clap(subcommand)]
    pub refs: refs::Options,
}

pub mod project {
    use super::*;

    fn project_payload(value: &str) -> Result<payload::Project, String> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }

    fn indirect_delegation(value: &str) -> Result<KeyOrUrn<Revision>, String> {
        match value.parse::<Urn>() {
            Ok(urn) => Ok(Either::Right(urn).into()),
            Err(urn_err) => match value.parse::<PeerId>() {
                Ok(key) => Ok(Either::Left(*key.as_public_key()).into()),
                Err(key_err) => Err(format!(
                    "Could not parse URN: \"{}\", nor Peer ID: \"{}\"",
                    urn_err, key_err
                )),
            },
        }
    }

    #[derive(Debug, Parser)]
    pub enum Options {
        Create(CreateOptions),
        Get(Get),
        List(List),
        Update(Update),
        Checkout(Checkout),
        Diff(Diff),
        Accept(Accept),
        Tracked(Tracked),
    }

    /// create a new Radicle project, either with a fresh working copy or based
    /// on an existing working copy
    #[derive(Debug, Parser)]
    pub struct CreateOptions {
        #[clap(subcommand)]
        pub create: Create,
    }

    #[derive(Debug, Parser)]
    pub enum Create {
        New(New),
        Existing(Existing),
    }

    /// create a new Radicle project along with a working copy if a `path` is
    /// specified
    #[derive(Debug, Parser)]
    pub struct New {
        /// the payload to create a project. The `name` field is expected, while
        /// `default_branch` and `description` are optional, along with any
        /// extensions defined by the upstream application.
        #[clap(long, parse(try_from_str = project_payload))]
        pub payload: payload::Project,

        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[clap(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,

        /// the Radicle URN pointing to a local identity that will be used for
        /// setting `rad/self` on this project. If no URN is provided the
        /// default identity will be used instead.
        #[clap(long)]
        pub whoami: Option<Urn>,

        /// the initial set of delegates to initialise the project with. The
        /// delegate can either be a Rad URN or Peer ID.
        /// The local identity is always used as a delegate, so it is not
        /// necessary to include it here.
        /// Note that the delegates must exist within your local store, if in
        /// URN form.
        #[clap(long, parse(try_from_str = indirect_delegation))]
        pub delegations: Vec<KeyOrUrn<Revision>>,

        /// the path where the working copy should be created
        #[clap(long)]
        pub path: Option<PathBuf>,
    }

    /// create a new Radicle project using an existing working copy
    #[derive(Debug, Parser)]
    pub struct Existing {
        /// the payload to create a project. The `name` field is expected, while
        /// `default_branch` and `description` are optional.
        #[clap(long, parse(try_from_str = project_payload))]
        pub payload: payload::Project,

        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[clap(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,

        /// the Radicle URN pointing to a local identity that will be used for
        /// setting `rad/self` on this project. If no URN is provided the
        /// default identity will be used instead.
        #[clap(long)]
        pub whoami: Option<Urn>,

        /// the initial set of delegates to initialise the project with. The
        /// delegate can either be a Rad URN or Peer ID. Note
        /// The local identity is always used as a delegate, so it is not
        /// necessary to include it here.
        /// that the delegates must exist within your local store, if in URN
        /// form.
        #[clap(long, parse(try_from_str = indirect_delegation))]
        pub delegations: Vec<KeyOrUrn<Revision>>,

        /// the path where the working copy should be created
        #[clap(long)]
        pub path: PathBuf,
    }

    /// get a Radicle project
    #[derive(Debug, Parser)]
    pub struct Get {
        /// the Radicle URN of the project
        #[clap(long)]
        pub urn: Urn,

        /// the peer's version of the project
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// list all Radicle projects
    #[derive(Debug, Parser)]
    pub struct List {}

    /// update a Radicle project
    #[derive(Debug, Parser)]
    pub struct Update {
        /// the Radicle URN of the project
        #[clap(long)]
        pub urn: Urn,

        /// the Radicle URN pointing to a local identity that will be used for
        /// setting `rad/self` on this project.
        #[clap(long)]
        pub whoami: Option<Urn>,

        /// the payload to create a project. The `name` field is expected, while
        /// `default_branch` and `description` are optional.
        #[clap(long, parse(try_from_str = project_payload))]
        pub payload: Option<payload::Project>,

        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[clap(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,

        /// the set of delegates to update the project with. This set is
        /// required to be absolute, so if the previous delegates are to be kept
        /// then they MUST be included here. If no delegates are provided then
        /// they will not be updated. The delegate can either be a Rad URN or
        /// Peer ID. Note that the delegates must exist within
        /// your local store, if in URN form.
        #[clap(long, parse(try_from_str = indirect_delegation))]
        pub delegations: Vec<KeyOrUrn<Revision>>,
    }

    /// checkout a Radicle project to a working copy
    #[derive(Debug, Parser)]
    pub struct Checkout {
        /// the Radicle URN of the project
        #[clap(long)]
        pub urn: Urn,

        /// the location for creating the working copy in
        #[clap(long)]
        pub path: PathBuf,

        /// the peer for which the initial working copy is based off. Note that
        /// if this value is not provided, or the value that is provided is the
        /// local peer, then the local version of the project is checked out.
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// review the difference between the local Radicle project and a peer's
    #[derive(Debug, Parser)]
    pub struct Diff {
        /// the Radicle URN of the project
        #[clap(long)]
        pub urn: Urn,
        /// the peer to compare to
        #[clap(long)]
        pub peer: PeerId,
    }

    /// accept the proposed changes between the local Radicle project and a
    /// peer's
    #[derive(Debug, Parser)]
    pub struct Accept {
        /// the Radicle URN of the project
        #[clap(long)]
        pub urn: Urn,
        /// the peer to compare to, and accept from
        #[clap(long)]
        pub peer: PeerId,
        /// skip the prompt to accept the change
        #[clap(long, short)]
        pub force: bool,
    }

    #[derive(Debug, Parser)]
    pub struct Tracked {
        /// the Radicle URN of the project
        #[clap(long)]
        pub urn: Urn,
    }
}

pub mod person {
    use super::*;

    fn person_payload(value: &str) -> Result<payload::Person, String> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }

    fn direct_delegation(value: &str) -> Result<PublicKey, String> {
        value
            .parse::<PeerId>()
            .map(|peer| *peer.as_public_key())
            .map_err(|err| err.to_string())
    }

    #[derive(Debug, Parser)]
    pub enum Options {
        Create(CreateOptions),
        Get(Get),
        List(List),
        Update(Update),
        Checkout(Checkout),
        Diff(Diff),
        Accept(Accept),
        Tracked(Tracked),
    }

    /// create a new Radicle person, either with a fresh working copy or based
    /// on an existing working copy
    #[derive(Debug, Parser)]
    pub struct CreateOptions {
        #[clap(subcommand)]
        pub create: Create,
    }

    #[derive(Debug, Parser)]
    pub enum Create {
        New(New),
        Existing(Existing),
    }

    /// create a new Radicle person along with a working copy if a `path` is
    /// specified
    #[derive(Debug, Parser)]
    pub struct New {
        /// the payload to create a person. The `name` field is expected.
        #[clap(long, parse(try_from_str = person_payload))]
        pub payload: payload::Person,

        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[clap(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,

        /// the initial set of delegates, in Peer ID form, to initialise the
        /// project with. The local key is always used as a delegate, so
        /// it is not necessary to include it here.
        #[clap(long, parse(try_from_str = direct_delegation))]
        pub delegations: Vec<PublicKey>,

        /// the path where the working copy should be created
        #[clap(long)]
        pub path: Option<PathBuf>,
    }

    /// create a new Radicle person using an existing working copy
    #[derive(Debug, Parser)]
    pub struct Existing {
        /// the payload to create a person. The `name` field is expected.
        #[clap(long, parse(try_from_str = person_payload))]
        pub payload: payload::Person,

        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[clap(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,

        /// the initial set of delegates, in Peer ID form, to initialise the
        /// project with. The local key is always used as a delegate, so
        /// it is not necessary to include it here.
        #[clap(long, parse(try_from_str = direct_delegation))]
        pub delegations: Vec<PublicKey>,

        /// the path where the working copy should be created
        #[clap(long)]
        pub path: PathBuf,
    }

    /// get a Radicle person
    #[derive(Debug, Parser)]
    pub struct Get {
        /// the Radicle URN of the person
        #[clap(long)]
        pub urn: Urn,

        /// the peer's version of the person
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// list all Radicle persons
    #[derive(Debug, Parser)]
    pub struct List {}

    /// update a Radicle person
    #[derive(Debug, Parser)]
    pub struct Update {
        /// the Radicle URN of the person
        #[clap(long)]
        pub urn: Urn,

        /// the Radicle URN pointing to a local identity that will be used for
        /// setting `rad/self` on this project.
        #[clap(long)]
        pub whoami: Option<Urn>,

        /// the payload to create a person. The `name` field is expected.
        #[clap(long, parse(try_from_str = person_payload))]
        pub payload: Option<payload::Person>,

        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[clap(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,

        /// the set of delegates, in Peer ID form, to update the project with.
        /// This set is required to be absolute, so if the previous
        /// delegates are to be kept then they MUST be included here. If
        /// no delegates are provided then they will not be updated.
        #[clap(long, parse(try_from_str = direct_delegation))]
        pub delegations: Vec<PublicKey>,
    }

    /// checkout a Radicle person to a working copy
    #[derive(Debug, Parser)]
    pub struct Checkout {
        /// the Radicle URN of the project
        #[clap(long)]
        pub urn: Urn,

        /// the location for creating the working copy in
        #[clap(long)]
        pub path: PathBuf,

        /// the peer for which the initial working copy is based off. Note that
        /// if this value is not provided, or the value that is provided is the
        /// local peer, then the local version of the person is checked out.
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// review the difference between the local Radicle person and a peer's
    #[derive(Debug, Parser)]
    pub struct Diff {
        /// the Radicle URN of the person
        #[clap(long)]
        pub urn: Urn,
        /// the peer to compare to, and accept from
        #[clap(long)]
        pub peer: PeerId,
    }

    /// accept the proposed changes between the local Radicle person and a
    /// peer's
    #[derive(Debug, Parser)]
    pub struct Accept {
        /// the Radicle URN of the person
        #[clap(long)]
        pub urn: Urn,
        /// the peer to compare to, and accept from
        #[clap(long)]
        pub peer: PeerId,
        /// skip the prompt to accept the change
        #[clap(long, short)]
        pub force: bool,
    }

    #[derive(Debug, Parser)]
    pub struct Tracked {
        /// the Radicle URN of the person
        #[clap(long)]
        pub urn: Urn,
    }
}

pub mod any {
    use super::*;

    #[derive(Debug, Parser)]
    pub enum Options {
        Get(Get),
        List(List),
    }

    /// get a Radicle identity, where the kind of identity is not known
    #[derive(Debug, Parser)]
    pub struct Get {
        /// the Radicle URN of the identity
        #[clap(long)]
        pub urn: Urn,
    }

    /// list all Radicle identities
    #[derive(Debug, Parser)]
    pub struct List {}
}

pub mod local {
    use super::*;

    #[derive(Debug, Parser)]
    pub enum Options {
        Set(Set),
        Get(Get),
        Default(Default),
    }

    /// get a Radicle local identity, i.e. a person that is created by the local
    /// user
    #[derive(Debug, Parser)]
    pub struct Get {
        /// the Radicle URN of the local identity
        #[clap(long)]
        pub urn: Urn,
    }

    /// set the default Radicle local identity
    #[derive(Debug, Parser)]
    pub struct Set {
        /// the Radicle URN of the local identity
        #[clap(long)]
        pub urn: Urn,
    }

    /// get the default Radicle local identity
    #[derive(Debug, Parser)]
    pub struct Default {}
}

pub mod rad_refs {
    use super::*;

    #[derive(Debug, Parser)]
    pub enum Options {
        RadSelf(RadSelf),
        Signed(Signed),
        Delegates(Delegates),
        Delegate(Delegate),
    }

    /// get the contents of `rad/self`
    #[derive(Debug, Parser)]
    pub struct RadSelf {
        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// get the contents of `rad/signed_refs`
    #[derive(Debug, Parser)]
    pub struct Signed {
        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// list the delegates under `rad/ids/*`
    #[derive(Debug, Parser)]
    pub struct Delegates {
        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// get the contents of `rad/id/<delegate>`
    #[derive(Debug, Parser)]
    pub struct Delegate {
        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the delegate's Radicle URN
        #[clap(long)]
        pub delegate: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }
}

pub mod refs {
    use super::*;

    #[derive(Debug, Parser)]
    pub enum Options {
        Heads(Heads),
        Tags(Tags),
        Notes(Notes),
        Category(Category),
    }

    /// list the heads under a Radicle URN
    #[derive(Debug, Parser)]
    pub struct Heads {
        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// list the tags under a Radicle URN
    #[derive(Debug, Parser)]
    pub struct Tags {
        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// list the notes under a Radicle URN
    #[derive(Debug, Parser)]
    pub struct Notes {
        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }

    /// list the references under a Radicle URN using an arbitrary category
    #[derive(Debug, Parser)]
    pub struct Category {
        /// the arbitrary category to look under
        #[clap(long)]
        pub category: String,

        /// the Radicle URN to look under
        #[clap(long)]
        pub urn: Urn,

        /// the remote peer to look under
        #[clap(long)]
        pub peer: Option<PeerId>,
    }
}

pub mod tracking {
    use super::*;

    /// track a peer's gossip for a Radicle URN
    #[derive(Debug, Parser)]
    pub struct Track {
        /// the Radicle URN to track
        #[clap(long)]
        pub urn: Urn,

        /// the peer to track
        #[clap(long)]
        pub peer: PeerId,
    }

    /// untrack a peer's gossip for a Radicle URN
    #[derive(Debug, Parser)]
    pub struct Untrack {
        /// the Radicle URN to untrack
        #[clap(long)]
        pub urn: Urn,

        /// the peer to untrack
        #[clap(long)]
        pub peer: PeerId,
    }
}

fn ext_payload(value: &str) -> Result<payload::Ext<serde_json::Value>, String> {
    serde_json::from_str(value).map_err(|err| err.to_string())
}
