// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{BTreeSet, HashSet};

use bstr::ByteSlice as _;
use git_ref_format::{Qualified, RefStr};
use link_crypto::PeerId;
use link_git::protocol::{oid, Ref};

use crate::{
    error,
    internal::{self, Layout, UpdateTips},
    refs,
    sigrefs,
    FetchState,
    FilteredRef,
    Identities,
    Negotiation,
    Policy,
    RefPrefix,
    Refdb,
    Update,
    WantsHaves,
};

#[derive(Debug)]
pub struct Fetch<Oid> {
    /// The local id.
    pub local_id: PeerId,
    /// The peer being fetched from.
    pub remote_id: PeerId,
    /// The stack of signed refs describing which refs we'll ask for.
    pub signed_refs: sigrefs::Combined<Oid>,
    /// Maximum number of bytes the fetched packfile can have.
    pub limit: u64,
}

impl<T> Fetch<T> {
    fn ref_prefix(&self, id: &PeerId, prefix: refs::Prefix) -> RefPrefix {
        RefPrefix::from_prefix((&self.remote_id != id).then(|| id), prefix)
    }

    fn signed<N: AsRef<RefStr>>(&self, id: &PeerId, refname: N) -> Option<&T> {
        self.signed_refs
            .refs
            .get(id)
            .and_then(|refs| refs.refs.get(refname.as_ref()))
    }

    fn is_signed<N: AsRef<RefStr>>(&self, id: &PeerId, refname: N) -> bool {
        self.signed(id, refname).is_some()
    }

    fn is_tracked(&self, id: &PeerId) -> bool {
        self.signed_refs.remotes.contains(id)
    }
}

impl<T: AsRef<oid>> Negotiation for Fetch<T> {
    fn ref_prefixes(&self) -> Vec<RefPrefix> {
        let remotes = self
            .signed_refs
            .remotes
            .iter()
            .filter(move |id| *id != &self.local_id)
            .flat_map(move |id| {
                vec![
                    self.ref_prefix(id, refs::Prefix::Heads),
                    self.ref_prefix(id, refs::Prefix::Notes),
                    self.ref_prefix(id, refs::Prefix::Tags),
                    self.ref_prefix(id, refs::Prefix::Cobs),
                ]
            });
        let signed = self
            .signed_refs
            .refs
            .iter()
            .filter(move |(id, _)| *id != &self.local_id)
            .flat_map(move |(id, refs)| {
                refs.refs.keys().filter_map(move |name| {
                    // TODO: sigrefs should be `Owned` already
                    let name = Qualified::from_refstr(name).and_then(refs::owned)?;
                    Some(RefPrefix::from(refs::scoped(id, &self.remote_id, name)))
                })
            });

        remotes.chain(signed).collect()
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use either::Either::*;
        use refs::parsed::Identity;

        let (refname, tip) = refs::into_unpacked(r);
        let parsed = refs::parse::<Identity>(refname.as_bstr()).ok()?;
        match &parsed.inner {
            // Ignore rad/ refs, as we got them already during the peek phase.
            Left(_) => None,
            // TODO: evaluate fetch specs, as per rfc0699
            Right(refname_no_remote) => {
                let remote_id = *parsed.remote.as_ref().unwrap_or(&self.remote_id);
                if self.is_tracked(&remote_id) || self.is_signed(&remote_id, &refname_no_remote) {
                    Some(FilteredRef::new(tip, &remote_id, parsed))
                } else {
                    warn!(
                        %refname_no_remote,
                        "skipping {} as it is neither signed nor tracked", refname
                    );
                    None
                }
            },
        }
    }

    fn wants_haves<'a, R: Refdb>(
        &self,
        db: &R,
        refs: impl IntoIterator<Item = FilteredRef<Self>>,
    ) -> Result<WantsHaves<Self>, R::FindError> {
        let mut wanted = HashSet::new();
        let mut wants = BTreeSet::new();
        let mut haves = BTreeSet::new();

        for r in refs {
            let refname = r.to_remote_tracking();
            let refname_no_remote = r.to_owned();

            let have = db.refname_to_id(refname)?;
            if let Some(oid) = have.as_ref() {
                haves.insert(oid.as_ref().to_owned());
            }

            // If we have a signed ref, we `want` the signed oid. Otherwise, and
            // if the remote id is in the tracking graph, we `want` what we got
            // offered.
            let want: Option<&oid> = self
                .signed(r.remote_id(), refname_no_remote)
                .map(|s| s.as_ref())
                .or_else(|| self.is_tracked(r.remote_id()).then(|| r.tip.as_ref()));

            match (want, have) {
                (Some(want), Some(have)) if want == have.as_ref() => {
                    // No need to want what we already have
                },
                (None, _) => {
                    // Unsolicited
                },
                (Some(_want), _) => {
                    wants.insert(r.tip);
                    wanted.insert(r);
                },
            }
        }

        Ok(WantsHaves {
            wanted,
            wants,
            haves,
        })
    }

    fn fetch_limit(&self) -> u64 {
        self.limit
    }
}

impl<T: AsRef<oid>> UpdateTips for Fetch<T> {
    fn prepare<'a, U, C>(
        &self,
        _: &FetchState<U>,
        _: &C,
        refs: &'a [FilteredRef<Self>],
    ) -> Result<internal::Updates<'a, U>, error::Prepare<C::VerificationError, C::FindError>>
    where
        C: Identities + Refdb,
    {
        let mut tips = Vec::new();
        for r in refs {
            debug_assert!(r.remote_id() != &self.local_id, "never touch our own");
            let name = refs::Qualified::from(r.to_remote_tracking());
            tips.push(Update::Direct {
                name,
                target: r.tip,
                no_ff: Policy::Allow,
            });
        }

        Ok(internal::Updates {
            tips,
            track: vec![],
        })
    }
}

impl<T> Layout for Fetch<T> {
    // [`Fetch`] may request only a part of the refs tree, so no layout error
    // can be determined from the advertised refs alone.
    //
    // XXX: We could reject if only a subset of the signed refs are present. This
    // would interact with fetchspecs, so requires runtime configuration.
    fn pre_validate(&self, _: &[FilteredRef<Self>]) -> Result<(), error::Layout> {
        Ok(())
    }
}
