// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{BTreeSet, HashSet};

use link_crypto::PeerId;
use link_git::protocol::Ref;

use super::{guard_required, mk_ref_update, ref_prefixes, required_refs};
use crate::{
    error,
    ids,
    internal::{self, Layout, UpdateTips},
    refs,
    FetchState,
    FilteredRef,
    Identities,
    Negotiation,
    RefPrefix,
    Refdb,
    WantsHaves,
};

#[derive(Debug)]
pub struct ForClone {
    pub remote_id: PeerId,
    pub limit: u64,
}

impl ForClone {
    pub fn required_refs(&self) -> impl Iterator<Item = refs::Scoped<'_, 'static>> {
        required_refs(&self.remote_id, &self.remote_id)
    }
}

impl Negotiation for ForClone {
    fn ref_prefixes(&self) -> Vec<RefPrefix> {
        ref_prefixes(&self.remote_id, &self.remote_id).collect()
    }

    fn ref_filter(&self, r: Ref) -> Option<FilteredRef<Self>> {
        use either::Either::Left;
        use refs::parsed::Identity;

        let (name, tip) = refs::into_unpacked(r);
        match refs::parse::<Identity>(name.as_ref()).ok()? {
            parsed @ refs::Parsed {
                remote: None,
                inner: Left(_),
                ..
            } => Some(FilteredRef::new(tip, &self.remote_id, parsed)),
            _ => None,
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
            if r.remote_id() != &self.remote_id {
                continue;
            }
            let refname = refs::Qualified::from(r.to_remote_tracking());
            match db.refname_to_id(refname)? {
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

impl UpdateTips for ForClone {
    fn prepare<'a, U, C>(
        &self,
        s: &FetchState<U>,
        cx: &C,
        refs: &'a [FilteredRef<Self>],
    ) -> Result<internal::Updates<'a, U>, error::Prepare<C::VerificationError, C::FindError>>
    where
        U: ids::Urn + Ord,
        C: Identities<Urn = U> + Refdb,
    {
        use ids::VerifiedIdentity as _;

        let verified = Identities::verify(
            cx,
            s.id_tip(&self.remote_id)
                .expect("BUG: `pre_validate` must ensure we got a rad/id ref"),
            s.lookup_delegations(&self.remote_id),
        )
        .map_err(error::Prepare::Verification)?;

        let tips = if verified.delegate_ids().contains(&self.remote_id) {
            refs.iter().filter_map(mk_ref_update::<_, C::Urn>).collect()
        } else {
            vec![]
        };

        Ok(internal::Updates {
            tips,
            track: vec![],
        })
    }
}

impl Layout for ForClone {
    fn pre_validate(&self, refs: &[FilteredRef<Self>]) -> Result<(), error::Layout> {
        guard_required(
            self.required_refs().collect(),
            refs.iter()
                .map(|x| refs::scoped(x.remote_id(), &self.remote_id, x.to_owned()))
                .collect(),
        )
    }
}
