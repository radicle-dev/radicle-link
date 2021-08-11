// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use bstr::{BString, ByteVec as _};
use either::Either;
use link_crypto::PeerId;

use crate::{
    error,
    refs,
    Identities,
    LocalIdentity,
    LocalPeer,
    ObjectId,
    Policy,
    Refdb,
    SymrefTarget,
    Update,
    Urn as _,
    VerifiedIdentity as _,
};

pub struct Rad<Urn> {
    pub track: Vec<(PeerId, Option<Urn>)>,
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
    fn no_indirects<Urn: Debug>(urn: &Urn) -> Option<ObjectId> {
        debug_assert!(false, "tried to resolve indirect delegation {:?}", urn);
        None
    }

    let mut track = Vec::new();
    let mut up = Vec::new();
    for urn in newest.delegate_urns() {
        let urn_enc = urn.encode_id();
        let delegate = {
            let mut ids: BString = format!("rad/ids/{}", urn_enc).into();
            let head = match remote {
                Some(remote) => {
                    let refname = refs::remote_tracking(remote, ids);
                    Refdb::refname_to_id(cx, &refname)?
                        .ok_or_else(|| format!("rad::setup: missing {}", refname.as_ref()))?
                },
                None => {
                    ids.insert_str(0, "refs/");
                    Refdb::refname_to_id(cx, &ids)?
                        .ok_or_else(|| format!("rad::setup: missing {}", ids))?
                },
            };
            Identities::verify(cx, head, no_indirects)?
        };
        // Make sure we got 'em tracked
        for id in delegate.delegate_ids() {
            // Track id for the current Urn
            track.push((id, None));
            // And also the delegate Urn
            track.push((id, Some(urn.clone())));
        }
        // Symref `rad/ids/$urn` -> refs/namespaces/$urn/refs/rad/id, creating
        // the target ref if it doesn't exist.
        up.push(Update::Symbolic {
            name: BString::from(format!("refs/rad/ids/{}", urn_enc)).into(),
            target: SymrefTarget {
                name: refs::Namespaced {
                    namespace: Some(BString::from(urn_enc).into()),
                    refname: refs::RadId.into(),
                },
                target: delegate.content_id().as_ref().to_owned(),
            },
            type_change: Policy::Allow,
        });
    }

    // Update `rad/self` in the same transaction
    if let Some(local_id) = whoami {
        up.push(Update::Direct {
            name: refs::RadSelf.into(),
            target: local_id.tip,
            no_ff: Policy::Reject,
        })
    }

    // Lastly, point `rad/id` to `newest.content_id`
    up.push(Update::Direct {
        name: refs::RadId.into(),
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
        }
        // Otherwise, theirs:
        //
        // * `rad/id` does not exist, so no other choice
        // * local peer does not have a say, so we want theirs
        // * the revisions are equal, so it doesn't matter
        _ => Ok(Ok(Right(theirs))),
    }
}
