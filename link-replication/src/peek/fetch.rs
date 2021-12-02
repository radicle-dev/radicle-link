// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashSet},
};

use bstr::{BString, ByteSlice as _};
use link_crypto::PeerId;
use link_git::protocol::{ObjectId, Ref};

use super::{guard_required, mk_ref_update, ref_prefixes, required_refs};
use crate::{
    error,
    ids,
    internal::{Layout, UpdateTips},
    refdb,
    refs,
    FetchState,
    FilteredRef,
    Identities,
    Negotiation,
    Refdb,
    Update,
    WantsHaves,
};

#[derive(Debug)]
pub struct ForFetch {
    /// The local peer, so we don't fetch our own data.
    pub local_id: PeerId,
    /// The remote peer being fetched from.
    pub remote_id: PeerId,
    /// The set of keys the latest known identity revision delegates to.
    /// Indirect delegations are resolved.
    pub delegates: BTreeSet<PeerId>,
    /// Additional peers being tracked (ie. excluding `delegates`).
    pub tracked: BTreeSet<PeerId>,
    /// Maximum number of bytes the fetched packfile is allowed to have.
    pub limit: u64,
}

impl ForFetch {
    pub fn peers(&self) -> impl Iterator<Item = &PeerId> {
        self.delegates
            .iter()
            .chain(self.tracked.iter())
            .filter(move |id| *id != &self.local_id)
    }

    pub fn required_refs(&self) -> impl Iterator<Item = refs::Scoped<'_, 'static>> {
        self.delegates
            .iter()
            .filter(move |id| *id != &self.local_id)
            .flat_map(move |id| required_refs(id, &self.remote_id))
    }
}

impl Negotiation for ForFetch {
    fn ref_prefixes(&self) -> Vec<refs::Scoped<'_, 'static>> {
        self.peers()
            .flat_map(move |id| ref_prefixes(id, &self.remote_id))
            .collect()
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use refs::parsed::Identity;

        let (name, tip) = refs::into_unpacked(r);
        let parsed = refs::parse::<Identity>(name.as_bstr())?;

        match parsed.remote {
            Some(remote_id) if remote_id == self.local_id => None,
            Some(remote_id) => Some(FilteredRef::new(name, tip, &remote_id, parsed)),
            None => Some(FilteredRef::new(name, tip, &self.remote_id, parsed)),
        }
    }

    fn wants_haves<R: Refdb>(
        &self,
        db: &R,
        refs: impl IntoIterator<Item = FilteredRef<Self>>,
    ) -> Result<WantsHaves<Self>, R::FindError> {
        let mut wanted = HashSet::new();
        let mut wants = BTreeSet::new();
        let mut haves = BTreeSet::new();

        for r in refs {
            let refname = refs::remote_tracking(&r.remote_id, r.name.as_bstr());
            match db.refname_to_id(&refname)? {
                Some(oid) => {
                    if oid.as_ref() != r.tip {
                        wants.insert(r.tip);
                    }
                    haves.insert(oid.into());
                },
                None => {
                    wants.insert(r.tip);
                },
            }
            wanted.insert(r);
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

impl UpdateTips for ForFetch {
    fn prepare<'a, U, C>(
        &self,
        s: &FetchState<U>,
        cx: &C,
        refs: &'a [FilteredRef<Self>],
    ) -> Result<Vec<Update<'a>>, error::Prepare<C::VerificationError, C::FindError>>
    where
        U: ids::Urn + Ord,
        C: Identities<Urn = U> + Refdb,
    {
        use ids::VerifiedIdentity as _;
        use refdb::{Policy, SymrefTarget};

        let mut updates = Vec::new();
        for r in refs {
            debug_assert!(r.remote_id != self.local_id, "never touch our own");
            let is_delegate = self.delegates.contains(&r.remote_id);
            // symref `rad/self` if we already have the top-level identity
            if r.name.ends_with(b"rad/self") {
                match Identities::verify(cx, r.tip, |_| None::<ObjectId>) {
                    Err(e) if is_delegate => return Err(error::Prepare::Verification(e)),
                    Err(e) => warn!("invalid `rad/self`: {}: {}", r.name, e),
                    Ok(id) => {
                        let top = refs::Namespaced {
                            namespace: Some(BString::from(id.urn().encode_id()).into()),
                            refname: refs::RadId.into(),
                        };
                        let oid = Refdb::refname_to_id(cx, top.qualified()).map_err(|source| {
                            error::Prepare::FindRef {
                                name: top.qualified(),
                                source,
                            }
                        })?;
                        let track_as =
                            Cow::from(refs::remote_tracking(&r.remote_id, r.name.as_bstr()));
                        let up = match oid {
                            Some(oid) => Update::Symbolic {
                                name: track_as,
                                target: SymrefTarget {
                                    name: top,
                                    target: oid.as_ref().to_owned(),
                                },
                                type_change: Policy::Allow,
                            },
                            None => Update::Direct {
                                name: track_as,
                                target: r.tip,
                                no_ff: Policy::Abort,
                            },
                        };

                        updates.push(up);
                    },
                }
            } else {
                // XXX: we should verify all ids at some point, but non-delegates
                // would be a warning only
                if is_delegate && r.name.ends_with(b"rad/id") {
                    Identities::verify(cx, r.tip, s.lookup_delegations(&r.remote_id))
                        .map_err(error::Prepare::Verification)?;
                }
                if let Some(u) = mk_ref_update::<_, C::Urn>(r) {
                    updates.push(u)
                }
            }
        }

        Ok(updates)
    }
}

impl Layout for ForFetch {
    fn pre_validate(&self, refs: &[FilteredRef<Self>]) -> Result<(), error::Layout> {
        guard_required(
            self.required_refs().collect(),
            refs.iter()
                .map(|x| refs::scoped(&x.remote_id, &self.remote_id, x.name.as_bstr()))
                .collect(),
        )
    }
}
