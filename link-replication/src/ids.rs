// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, fmt::Debug};

use link_crypto::PeerId;
use link_git::protocol::{oid, ObjectId};
use radicle_data::NonEmpty;

use crate::{error, refs, Error, LocalPeer, Refdb};

pub trait VerifiedIdentity: Sized {
    type Rev: Eq;
    type Oid: AsRef<oid>;
    type Urn: Urn + Ord;

    fn revision(&self) -> Self::Rev;
    fn content_id(&self) -> Self::Oid;
    fn urn(&self) -> Self::Urn;

    /// Set of all [`PeerId`]s this identity delegates to, directly and
    /// indirectly.
    fn delegate_ids(&self) -> NonEmpty<BTreeSet<PeerId>>;

    /// Set of all URNs this identity delegates to (ie. indirect delegations).
    /// Possibly empty.
    fn delegate_urns(&self) -> BTreeSet<Self::Urn>;
}

pub trait Urn: Sized {
    type Error: std::error::Error + Send + Sync + 'static;

    fn try_from_id(s: impl AsRef<str>) -> Result<Self, Self::Error>;
    fn encode_id(&self) -> String;
}

pub trait Identities {
    type Urn: Urn;
    type Oid: AsRef<oid>;

    type VerifiedIdentity: VerifiedIdentity<Oid = Self::Oid, Urn = Self::Urn>
        + Debug
        + Send
        + Sync
        + 'static;
    type VerificationError: std::error::Error + Send + Sync + 'static;

    /// Verify the identity history with tip `head`.
    fn verify<H, F, T>(
        &self,
        head: H,
        resolve: F,
    ) -> Result<Self::VerifiedIdentity, Self::VerificationError>
    where
        H: AsRef<oid>,
        F: Fn(&Self::Urn) -> Option<T>,
        T: AsRef<oid>;

    /// Return the more recent of identities `a` and `b`, or an error if their
    /// histories are unrelated.
    fn newer(
        &self,
        a: Self::VerifiedIdentity,
        b: Self::VerifiedIdentity,
    ) -> Result<Self::VerifiedIdentity, error::IdentityHistory<Self::VerifiedIdentity>>;
}

/// The identity the local peer wishes to identify as.
///
/// The local peer id must be in the delegation `ids`.
pub struct LocalIdentity {
    pub tip: ObjectId,
    pub ids: BTreeSet<PeerId>,
}

/// Read and verify the identity at `refs/rad/id` of the current namespace.
///
/// If the `refs/rad/id` ref is not found, `None` is returned. Indirect
/// delegations are resolved relative to `refs/rad/ids/`.
#[tracing::instrument(level = "debug", skip(cx), err)]
pub fn current<C>(cx: &C) -> Result<Option<C::VerifiedIdentity>, Error>
where
    C: Identities + Refdb,
{
    let resolve = |urn: &<C as Identities>::Urn| -> Option<<C as Refdb>::Oid> {
        let name = refs::rad_ids(urn);
        debug!("resolving {}", name);
        Refdb::refname_to_id(cx, name).ok().flatten()
    };
    let id = Refdb::refname_to_id(cx, refs::Qualified::from(refs::REFS_RAD_ID))?
        .map(|tip| Identities::verify(cx, tip, resolve))
        .transpose()?;
    Ok(id)
}

/// Read and verify the identity at `refs/remotes/<remote>/rad/id` if the
/// current namespace.
///
/// If the ref `refs/remotes/<remote>/rad/id` is not found, `None` is returned.
/// Indirect delegations are resolved relative to
/// `refs/remote/<remote>/rad/ids/`.
#[tracing::instrument(level = "debug", skip(cx), err)]
pub fn of<C>(cx: &C, remote: &PeerId) -> Result<Option<C::VerifiedIdentity>, Error>
where
    C: Identities + Refdb,
{
    let id_ref = refs::remote_tracking(remote, refs::REFS_RAD_ID).expect("const name known valid");
    let resolve = |urn: &<C as Identities>::Urn| -> Option<<C as Refdb>::Oid> {
        let name = refs::remote_tracking(remote, refs::rad_ids(urn))?;
        Refdb::refname_to_id(cx, name).ok().flatten()
    };
    let id = Refdb::refname_to_id(cx, id_ref)?
        .map(|tip| Identities::verify(cx, tip, resolve))
        .transpose()?;
    Ok(id)
}

/// Read and verify the identities `of` peers relative to the current namespace.
/// Also determine which one is the most recent, or report an error if their
/// histories diverge.
///
/// If one of the remote tracking branches is not found, an error is returned.
/// If the id is equal to [`LocalPeer::id`], the [`VerifiedIdentity`] is read
/// via [`current`], otherwise via [`of`].
///
/// If the iterator `of` is empty, `None` is returned.
#[tracing::instrument(level = "debug", skip(cx, of), err)]
pub fn newest<'a, C, I>(cx: &C, of: I) -> Result<Option<(&'a PeerId, C::VerifiedIdentity)>, Error>
where
    C: Identities + LocalPeer + Refdb,
    <C as Identities>::Oid: PartialEq,
    I: IntoIterator<Item = &'a PeerId>,
{
    let ours = LocalPeer::id(cx);
    let mut newest = None;
    for id in of {
        let a = if id == ours {
            self::current(cx)?.ok_or("`newest: missing `refs/rad/id`")?
        } else {
            self::of(cx, id)?.ok_or(format!(
                "newest: missing delegation id ref `refs/remotes/{}/rad/id`",
                id
            ))?
        };
        match newest {
            None => newest = Some((id, a)),
            Some((id_b, b)) => {
                let oid_b = b.content_id();
                let newer = Identities::newer(cx, a, b)?;
                if newer.content_id() != oid_b {
                    newest = Some((id, newer));
                } else {
                    newest = Some((id_b, newer));
                }
            },
        }
    }

    Ok(newest)
}
