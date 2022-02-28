// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(rustdoc::private_intra_doc_links)]
#![warn(clippy::extra_unused_lifetimes)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::fmt::Debug;

#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate tracing;

use link_crypto::PeerId;
use radicle_std_ext::prelude::*;

pub mod error;
pub use error::Error;

pub mod fetch;
pub mod internal;
pub mod io;
pub mod peek;
pub mod refs;

mod eval;

mod ids;
pub use ids::{AnyIdentity, Identities, LocalIdentity, Urn, VerifiedIdentity};

mod odb;
pub use odb::Odb;

mod refdb;
pub use refdb::{Applied, Policy, RefScan, Refdb, SymrefTarget, Update, Updated};

mod sigrefs;
pub use sigrefs::{SignedRefs, Sigrefs};

mod state;
use state::FetchState;

mod success;
pub use success::Success;

mod track;
pub use track::{Rel as TrackingRel, Tracking};

mod transmit;
pub use transmit::{FilteredRef, Negotiation, Net, RefPrefix, SkippedFetch, WantsHaves};

mod validation;
pub use validation::validate;

// Re-exports
pub use link_git::{
    protocol::{oid, ObjectId},
    refs::{namespace, Namespace},
};

pub trait LocalPeer {
    fn id(&self) -> &PeerId;
}

#[derive(Clone, Copy, Debug)]
pub struct FetchLimit {
    pub peek: u64,
    pub data: u64,
}

impl Default for FetchLimit {
    fn default() -> Self {
        Self {
            peek: 1024 * 1024 * 5,
            data: 1024 * 1024 * 1024 * 5,
        }
    }
}

#[tracing::instrument(skip(cx, whoami), fields(local_id = %LocalPeer::id(cx)))]
pub fn pull<C>(
    cx: &mut C,
    limit: FetchLimit,
    remote_id: PeerId,
    whoami: Option<LocalIdentity>,
) -> Result<Success<<C as Identities>::Urn>, Error>
where
    C: Identities
        + LocalPeer
        + Net
        + Refdb
        + SignedRefs<Oid = <C as Identities>::Oid>
        + Tracking<Urn = <C as Identities>::Urn>,
    <C as Identities>::Oid: Debug + PartialEq + Send + Sync + 'static,
    <C as Identities>::Urn: Clone + Debug + Ord,
{
    if LocalPeer::id(cx) == &remote_id {
        return Err("cannot replicate from self".into());
    }
    let anchor = ids::current(cx)?.ok_or("pull: missing `rad/id`")?;
    eval::pull(
        &mut FetchState::default(),
        cx,
        limit,
        anchor,
        remote_id,
        whoami,
    )
}

#[tracing::instrument(skip(cx, whoami), fields(local_id = %LocalPeer::id(cx)))]
pub fn clone<C>(
    cx: &mut C,
    limit: FetchLimit,
    remote_id: PeerId,
    whoami: Option<LocalIdentity>,
) -> Result<Success<<C as Identities>::Urn>, Error>
where
    C: Identities
        + LocalPeer
        + Net
        + Refdb
        + SignedRefs<Oid = <C as Identities>::Oid>
        + Tracking<Urn = <C as Identities>::Urn>,
    <C as Identities>::Oid: Debug + PartialEq + Send + Sync + 'static,
    <C as Identities>::Urn: Clone + Debug + Ord,
{
    info!("fetching initial verification refs");
    if LocalPeer::id(cx) == &remote_id {
        return Err("cannot replicate from self".into());
    }
    let mut state = FetchState::default();
    let (_, res) = state.step(
        cx,
        peek::ForClone {
            remote_id,
            limit: limit.peek,
        },
    )?;
    let anchor = match res {
        Some(SkippedFetch::NoMatchingRefs) => {
            return Err("remote did not advertise verification refs".into())
        },
        Some(SkippedFetch::WantNothing) => {
            ids::of(cx, &remote_id)?.expect("BUG: wanted nothing, but don't have it either")
        },
        None => Identities::verify(
            cx,
            state
                .id_tips()
                .get(&remote_id)
                .expect("BUG: peek step must ensure we got a rad/id ref"),
            state.lookup_delegations(&remote_id),
        )?,
    };
    eval::pull(&mut state, cx, limit, anchor, remote_id, whoami)
}
