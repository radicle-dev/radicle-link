// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_crypto::PeerId;
use link_git::protocol::Ref;
use radicle_data::NonEmptyVec;

use super::{guard_required, mk_ref_update, ref_prefixes, required_refs};
use crate::{
    error,
    ids,
    internal::{self, Layout, UpdateTips},
    refs,
    transmit::{self, ExpectLs, LsRefs},
    FetchState,
    FilteredRef,
    Identities,
    Negotiation,
    Odb,
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
    fn ls_refs(&self) -> Option<LsRefs> {
        let prefixes = ref_prefixes(&self.remote_id, &self.remote_id);
        NonEmptyVec::from_vec(prefixes.collect()).map(|prefixes| LsRefs::Prefix {
            prefixes,
            response: ExpectLs::NonEmpty,
        })
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

    fn wants_haves<R>(
        &self,
        db: &R,
        refs: impl IntoIterator<Item = FilteredRef<Self>>,
    ) -> Result<WantsHaves<Self>, transmit::error::WantsHaves<R::FindError>>
    where
        R: Refdb + Odb,
    {
        Ok(WantsHaves::default().expect_all(
            db,
            refs.into_iter()
                .filter(|r| r.remote_id() != &self.remote_id),
        )?)
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
    ) -> Result<internal::Updates<'a, U>, error::Prepare>
    where
        U: ids::Urn + Ord,
        C: Identities<Urn = U>,
    {
        use ids::VerifiedIdentity as _;

        let verified = Identities::verify(
            cx,
            s.id_tips()
                .get(&self.remote_id)
                .expect("BUG: `pre_validate` must ensure we got a rad/id ref"),
            s.lookup_delegations(&self.remote_id),
        )
        .map_err(|e| error::Prepare::Verification(e.into()))?;

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
