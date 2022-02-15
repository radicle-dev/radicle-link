// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use either::Either;
use link_crypto::PeerId;

use crate::{
    error,
    refs,
    track,
    Identities,
    LocalIdentity,
    LocalPeer,
    ObjectId,
    Policy,
    Refdb,
    SymrefTarget,
    Update,
    VerifiedIdentity as _,
};

pub struct Rad<T> {
    pub track: Vec<track::Rel<T>>,
    pub up: Vec<Update<'static>>,
}

pub fn setup<C>(
    cx: &C,
    remote: Option<&PeerId>,
    newest: &C::VerifiedIdentity,
    whoami: Option<LocalIdentity>,
) -> Result<Rad<C::Urn>, error::Error>
where
    C: Identities + Refdb,
    <C as Identities>::Urn: Clone + Debug,
{
    use Either::*;

    fn no_indirects<Urn: Debug>(urn: &Urn) -> Option<ObjectId> {
        debug_assert!(false, "tried to resolve indirect delegation {:?}", urn);
        None
    }

    let mut track = Vec::new();
    let mut up = Vec::new();
    for urn in newest.delegate_urns() {
        let name = refs::rad_ids(&urn).to_owned();
        let delegate = {
            let head = match remote {
                Some(remote) => {
                    let refname = refs::remote_tracking(remote, name.clone())
                        .expect("`name` is guaranteed to be valid");
                    Refdb::refname_to_id(cx, &refname)?
                        .ok_or_else(|| format!("rad::setup: missing {}", refname))?
                },
                None => Refdb::refname_to_id(cx, name.clone())?
                    .ok_or_else(|| format!("rad::setup: missing {}", name))?,
            };
            Identities::verify(cx, head, no_indirects)?
        };
        // Make sure we track the delegate's URN
        track.push(track::Rel::Delegation(Right(urn.clone())));
        // Symref `rad/ids/$urn` -> refs/namespaces/$urn/refs/rad/id, creating
        // the target ref if it doesn't exist.
        up.push(Update::Symbolic {
            name,
            target: SymrefTarget {
                name: refs::namespaced(&urn, refs::REFS_RAD_ID),
                target: delegate.content_id().as_ref().to_owned(),
            },
            type_change: Policy::Allow,
        });
    }

    // Track all peers in the delegations for the current URN
    for id in newest.delegate_ids() {
        track.push(track::Rel::Delegation(Left(id)));
    }

    // Update `rad/self` in the same transaction
    if let Some(local_id) = whoami {
        up.push(Update::Direct {
            name: refs::REFS_RAD_SELF.into(),
            target: local_id.tip,
            no_ff: Policy::Reject,
        })
    }

    // Lastly, point `rad/id` to `newest.content_id`
    up.push(Update::Direct {
        name: refs::REFS_RAD_ID.into(),
        target: newest.content_id().as_ref().to_owned(),
        no_ff: Policy::Reject,
    });

    Ok(Rad { track, up })
}

#[allow(clippy::type_complexity)]
pub fn newer<C>(
    cx: &C,
    ours: Option<C::VerifiedIdentity>,
    theirs: C::VerifiedIdentity,
) -> Result<
    Result<Either<C::VerifiedIdentity, C::VerifiedIdentity>, error::ConfirmationRequired>,
    error::IdentityHistory<C::VerifiedIdentity>,
>
where
    C: Identities + LocalPeer,
{
    use Either::*;

    match ours {
        // `rad/id` exists, delegates to the local peer id, and is not at the
        // same revision as `theirs`
        Some(ours)
            if ours.delegate_ids().contains(LocalPeer::id(cx))
                && ours.revision() != theirs.revision() =>
        {
            // Check which one is more recent
            let tip = ours.content_id();
            let newer = Identities::newer(cx, ours, theirs)?;
            // Theirs is ahead, so we need to confirm
            if newer.content_id().as_ref() != tip.as_ref() {
                Ok(Err(error::ConfirmationRequired))
            }
            // Ours is ahead, so use that
            else {
                Ok(Ok(Left(newer)))
            }
        },
        // Otherwise, theirs:
        //
        // * `rad/id` does not exist, so no other choice
        // * local peer does not have a say, so we want theirs
        // * the revisions are equal, so it doesn't matter
        _ => Ok(Ok(Right(theirs))),
    }
}
