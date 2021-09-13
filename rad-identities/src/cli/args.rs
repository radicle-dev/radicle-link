// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, path::PathBuf};

use structopt::StructOpt;

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
#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(Debug, StructOpt)]
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
#[derive(Debug, StructOpt)]
pub struct Project {
    #[structopt(subcommand)]
    pub project: project::Options,
}

/// create, get, or modify a Radicle person
#[derive(Debug, StructOpt)]
pub struct Person {
    #[structopt(subcommand)]
    pub person: person::Options,
}

/// get any Radicle identity
#[derive(Debug, StructOpt)]
pub struct Any {
    #[structopt(subcommand)]
    pub any: any::Options,
}

/// get or set a Radicle local identity
#[derive(Debug, StructOpt)]
pub struct Local {
    #[structopt(subcommand)]
    pub local: local::Options,
}

/// get the contents of `rad` references, e.g. `rad/self`, `rad/signed_refs`,
/// etc.
#[derive(Debug, StructOpt)]
pub struct RadRefs {
    #[structopt(subcommand)]
    pub rad_refs: rad_refs::Options,
}

/// list the references under a given category, e.g. `heads`, `tags`, etc.
#[derive(Debug, StructOpt)]
pub struct Refs {
    #[structopt(subcommand)]
    pub refs: refs::Options,
}

pub mod project {
    use super::*;

    fn project_payload(value: &str) -> Result<payload::Project, String> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }

    fn indirect_delegation(value: &str) -> Result<BTreeSet<KeyOrUrn<Revision>>, String> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }

    #[derive(Debug, StructOpt)]
    pub enum Options {
        Create(CreateOptions),
        Get(Get),
        List(List),
        Update(Update),
        Checkout(Checkout),
        Review(Review),
    }

    /// create a new Radicle project, either with a fresh working copy or based
    /// on an existing working copy
    #[derive(Debug, StructOpt)]
    pub struct CreateOptions {
        #[structopt(subcommand)]
        pub create: Create,
    }

    #[derive(Debug, StructOpt)]
    pub enum Create {
        New(New),
        Existing(Existing),
    }

    /// create a new Radicle project along with a working copy if a `path` is
    /// specified
    #[derive(Debug, StructOpt)]
    pub struct New {
        /// the payload to create a project. The `name` field is expected, while
        /// `default_branch` and `description` are optional, along with any
        /// extensions defined by the upstream application.
        #[structopt(long, parse(try_from_str = project_payload))]
        pub payload: payload::Project,
        #[structopt(long, parse(try_from_str = ext_payload))]
        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        pub ext: Vec<payload::Ext<serde_json::Value>>,
        /// the Radicle URN pointing to a local identity that will be used for
        /// setting `rad/self` on this project. If no URN is provided the
        /// default identity will be used instead.
        #[structopt(long)]
        pub whoami: Option<Urn>,
        /// the path where the working copy should be created
        #[structopt(long)]
        pub path: Option<PathBuf>,
    }

    /// create a new Radicle project using an existing working copy
    #[derive(Debug, StructOpt)]
    pub struct Existing {
        /// the payload to create a project. The `name` field is expected, while
        /// `default_branch` and `description` are optional, along with any
        /// extensions defined by the upstream application.
        #[structopt(long, parse(try_from_str = project_payload))]
        pub payload: payload::Project,
        #[structopt(long, parse(try_from_str = ext_payload))]
        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        pub ext: Vec<payload::Ext<serde_json::Value>>,
        /// the Radicle URN pointing to a local identity that will be used for
        /// setting `rad/self` on this project. If no URN is provided the
        /// default identity will be used instead.
        #[structopt(long)]
        pub whoami: Option<Urn>,
        /// the path where the working copy should be created
        #[structopt(long)]
        pub path: PathBuf,
    }

    /// get a Radicle project
    #[derive(Debug, StructOpt)]
    pub struct Get {
        /// the Radicle URN of the project
        #[structopt(long)]
        pub urn: Urn,
        /// the peer's version of the project
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// list all Radicle projects
    #[derive(Debug, StructOpt)]
    pub struct List {}

    /// update a Radicle project
    #[derive(Debug, StructOpt)]
    pub struct Update {
        /// the Radicle URN of the project
        #[structopt(long)]
        pub urn: Urn,
        #[structopt(long)]
        pub whoami: Option<Urn>,
        #[structopt(long, parse(try_from_str = project_payload))]
        pub payload: Option<payload::Project>,
        #[structopt(long, parse(try_from_str = ext_payload))]
        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        pub ext: Vec<payload::Ext<serde_json::Value>>,
        #[structopt(long, parse(try_from_str = indirect_delegation))]
        pub delegations: BTreeSet<KeyOrUrn<Revision>>,
    }

    /// checkout a Radicle project to a working copy
    #[derive(Debug, StructOpt)]
    pub struct Checkout {
        /// the Radicle URN of the project
        #[structopt(long)]
        pub urn: Urn,
        /// the location for creating the working copy in
        #[structopt(long)]
        pub path: PathBuf,
        /// the peer for which the initial working copy is based off. Note that
        /// if this value is not provided, or the value that is provided is the
        /// local peer, then the local version of the project is checked out.
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// review a Radicle project for merging
    #[derive(Debug, StructOpt)]
    pub struct Review {}
}

pub mod person {
    use super::*;

    fn person_payload(value: &str) -> Result<payload::Person, String> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }
    fn direct_delegation(value: &str) -> Result<PublicKey, String> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }

    #[derive(Debug, StructOpt)]
    pub enum Options {
        Create(CreateOptions),
        Get(Get),
        List(List),
        Update(Update),
        Checkout(Checkout),
        Review(Review),
    }

    /// create a new Radicle person, either with a fresh working copy or based
    /// on an existing working copy
    #[derive(Debug, StructOpt)]
    pub struct CreateOptions {
        #[structopt(subcommand)]
        pub create: Create,
    }

    #[derive(Debug, StructOpt)]
    pub enum Create {
        New(New),
        Existing(Existing),
    }

    /// create a new Radicle person along with a working copy if a `path` is
    /// specified
    #[derive(Debug, StructOpt)]
    pub struct New {
        /// the payload to create a person. The `name` field is expected
        #[structopt(long, parse(try_from_str = person_payload))]
        pub payload: payload::Person,
        #[structopt(long, parse(try_from_str = ext_payload))]
        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        pub ext: Vec<payload::Ext<serde_json::Value>>,
        /// the path where the working copy should be created
        #[structopt(long)]
        pub path: Option<PathBuf>,
    }

    /// create a new Radicle person using an existing working copy
    #[derive(Debug, StructOpt)]
    pub struct Existing {
        /// the payload to create a person. The `name` field is expected, along
        /// with any extensions defined by the upstream application.
        #[structopt(long, parse(try_from_str = person_payload))]
        pub payload: payload::Person,
        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[structopt(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,
        /// the path where the working copy should be created
        #[structopt(long)]
        pub path: PathBuf,
    }

    /// get a Radicle person
    #[derive(Debug, StructOpt)]
    pub struct Get {
        /// the Radicle URN of the person
        #[structopt(long)]
        pub urn: Urn,
        /// the peer's version of the person
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// list all Radicle persons
    #[derive(Debug, StructOpt)]
    pub struct List {}

    /// update a Radicle person
    #[derive(Debug, StructOpt)]
    pub struct Update {
        /// the Radicle URN of the person
        #[structopt(long)]
        pub urn: Urn,
        #[structopt(long)]
        pub whoami: Option<Urn>,
        #[structopt(long, parse(try_from_str = person_payload))]
        pub payload: Option<payload::Person>,
        /// provide a list of extensions to extend the payload. The extension
        /// must be a JSON object consisting of a namespace URL and the extended
        /// JSON payload
        #[structopt(long, parse(try_from_str = ext_payload))]
        pub ext: Vec<payload::Ext<serde_json::Value>>,
        #[structopt(long, parse(try_from_str = direct_delegation))]
        pub delegations: Vec<PublicKey>,
    }

    /// checkout a Radicle person to a working copy
    #[derive(Debug, StructOpt)]
    pub struct Checkout {
        /// the Radicle URN of the project
        #[structopt(long)]
        pub urn: Urn,
        /// the location for creating the working copy in
        #[structopt(long)]
        pub path: PathBuf,
        /// the peer for which the initial working copy is based off. Note that
        /// if this value is not provided, or the value that is provided is the
        /// local peer, then the local version of the person is checked out.
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// review a Radicle person for merging    
    #[derive(Debug, StructOpt)]
    pub struct Review {}
}

pub mod any {
    use super::*;

    #[derive(Debug, StructOpt)]
    pub enum Options {
        Get(Get),
        List(List),
    }

    /// get a Radicle identity, where the kind of identity is not known
    #[derive(Debug, StructOpt)]
    pub struct Get {
        /// the Radicle URN of the identity
        #[structopt(long)]
        pub urn: Urn,
    }

    /// list all Radicle identities
    #[derive(Debug, StructOpt)]
    pub struct List {}
}

pub mod local {
    use super::*;

    #[derive(Debug, StructOpt)]
    pub enum Options {
        Set(Set),
        Get(Get),
        Default(Default),
    }

    /// get a Radicle local identity, i.e. a person that is created by the local
    /// user
    #[derive(Debug, StructOpt)]
    pub struct Get {
        /// the Radicle URN of the local identity
        #[structopt(long)]
        pub urn: Urn,
    }

    /// set the default Radicle local identity
    #[derive(Debug, StructOpt)]
    pub struct Set {
        /// the Radicle URN of the local identity
        #[structopt(long)]
        pub urn: Urn,
    }

    /// get the default Radicle local identity
    #[derive(Debug, StructOpt)]
    pub struct Default {}
}

pub mod rad_refs {
    use super::*;

    #[derive(Debug, StructOpt)]
    pub enum Options {
        RadSelf(RadSelf),
        Signed(Signed),
        Delegates(Delegates),
        Delegate(Delegate),
    }

    /// get the contents of `rad/self`
    #[derive(Debug, StructOpt)]
    pub struct RadSelf {
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// get the contents of `rad/signed_refs`
    #[derive(Debug, StructOpt)]
    pub struct Signed {
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// list the delegates under `rad/ids/*`
    #[derive(Debug, StructOpt)]
    pub struct Delegates {
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// get the contents of `rad/id/<delegate>`
    #[derive(Debug, StructOpt)]
    pub struct Delegate {
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the delegate's Radicle URN
        #[structopt(long)]
        pub delegate: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }
}

pub mod refs {
    use super::*;

    #[derive(Debug, StructOpt)]
    pub enum Options {
        Heads(Heads),
        Tags(Tags),
        Notes(Notes),
        Category(Category),
    }

    /// list the heads under a Radicle URN
    #[derive(Debug, StructOpt)]
    pub struct Heads {
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// list the tags under a Radicle URN
    #[derive(Debug, StructOpt)]
    pub struct Tags {
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// list the notes under a Radicle URN
    #[derive(Debug, StructOpt)]
    pub struct Notes {
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }

    /// list the references under a Radicle URN using an arbitrary category
    #[derive(Debug, StructOpt)]
    pub struct Category {
        /// the arbitrary category to look under
        #[structopt(long)]
        pub category: String,
        /// the Radicle URN to look under
        #[structopt(long)]
        pub urn: Urn,
        /// the remote peer to look under
        #[structopt(long)]
        pub peer: Option<PeerId>,
    }
}

pub mod tracking {
    use super::*;

    /// track a peer's gossip for a Radicle URN
    #[derive(Debug, StructOpt)]
    pub struct Track {
        /// the Radicle URN to track
        #[structopt(long)]
        pub urn: Urn,
        /// the peer to track
        #[structopt(long)]
        pub peer: PeerId,
    }

    /// untrack a peer's gossip for a Radicle URN
    #[derive(Debug, StructOpt)]
    pub struct Untrack {
        /// the Radicle URN to untrack
        #[structopt(long)]
        pub urn: Urn,
        /// the peer to untrack
        #[structopt(long)]
        pub peer: PeerId,
    }
}

fn ext_payload(value: &str) -> Result<payload::Ext<serde_json::Value>, String> {
    serde_json::from_str(value).map_err(|err| err.to_string())
}
