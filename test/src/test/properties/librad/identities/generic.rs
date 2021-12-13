// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeMap;

use librad::identities::{
    delegation::Delegations,
    generic::{error, Doc, Identity},
    sign::Signatures,
    Verifying,
};
use nonempty::NonEmpty;
use proptest::prelude::*;
use std_ext::Void;

use crate::librad::identities::generic::*;

proptest! {
    #[test]
    fn signed(id in gen_identity::<Boring>()) {
        assert_eq!(
            Verifying::from(id.clone())
                .signed()
                .unwrap()
                .into_inner(),
            id
        )
    }

    #[test]
    fn quorum_below_threshold(
        (id, num_sigs) in
            gen_identity::<Boring>().prop_flat_map(|id| {
                let threshold = id.quorum_threshold();
                (Just(id), 1..=threshold)
            })
    ) {
        let signatures: Signatures = BTreeMap::from(id.signatures)
            .into_iter()
            .take(num_sigs)
            .collect::<BTreeMap<_, _>>()
            .into();

        let id = Identity {
            signatures,
            ..id
        };

        assert_matches!(
            Verifying::from(id).quorum(),
            Err(error::Verify::Quorum)
        )
    }

    #[test]
    fn quorum(id in gen_identity::<Boring>()) {
        assert_eq!(
            Verifying::from(id.clone())
                .quorum()
                .unwrap()
                .into_inner(),
            id
        )
    }

    #[test]
    fn verified_root(id in gen_root_identity::<Revision>()) {
        assert_eq!(
            Verifying::from(id.clone())
                .verified(None)
                .unwrap()
                .into_inner(),
            id
        )
    }

    #[test]
    fn verified(NonEmpty { head, tail } in gen_history(1)) {
        match tail.as_slice() {
            [next] => {
                let parent = Verifying::from(head).verified(None).unwrap();
                let child = Verifying::from(next.clone())
                    .verified(Some(&parent))
                    .unwrap()
                    .into_inner();

                assert_eq!(&child, next)
            },

            _ => unreachable!(),
        }
    }

    #[test]
    fn verified_dangling_parent(parent in gen_root_identity::<Revision>()) {
        let parent = Verifying::from(parent).verified(None).unwrap();
        let child = Verifying::from(parent.clone().into_inner().map(|doc| Doc {
            replaces: None,
            ..doc
        }))
        .verified(Some(&parent));

        assert_matches!(child, Err(error::Verify::DanglingParent { .. }))
    }

    #[test]
    fn verified_root_mismatch(
        id in gen_root_identity::<Revision>(),
        parent_root in any::<Revision>(),
    ) {
        let parent = Verifying::from(Identity {
            root: parent_root,
            ..id.clone()
        })
        .verified(None)
        .unwrap();
        let child = Verifying::from(id).verified(Some(&parent));

        assert_matches!(child, Err(error::Verify::RootMismatch { .. }))
    }

    #[test]
    fn verified_parent_mismatch(
        parent in gen_root_identity::<Revision>(),
        bogus_replaces in any::<Revision>()
    ) {
        let parent = Verifying::from(parent).verified(None).unwrap();
        let child = Verifying::from(parent.clone().into_inner().map(|doc| Doc {
            replaces: Some(bogus_replaces),
            ..doc
        })).verified(Some(&parent));

        assert_matches!(child, Err(error::Verify::ParentMismatch { .. }))
    }

    #[test]
    fn verified_parent_quorum_below_threshold(
        (NonEmpty { head, tail }, num_sigs) in
            gen_history(1).prop_flat_map(|hist| {
                let threshold = hist.tail[0].quorum_threshold();
                (Just(hist), 1..=threshold)
            })
    ) {
        match tail.as_slice() {
            [next] => {
                let parent = Verifying::from(head).verified(None).unwrap();
                let next = Identity {
                    signatures: BTreeMap::from(next.signatures.clone())
                        .into_iter()
                        .take(num_sigs)
                        .collect::<BTreeMap<_, _>>()
                        .into(),
                    ..next.clone()
                };

                assert_matches!(
                    Verifying::from(next).verified(Some(&parent)),
                    Err(error::Verify::Quorum)
                )
            },

            _ => unreachable!(),
        }
    }

    #[test]
    fn verify(history in gen_history(0..10)) {
        let NonEmpty { head, tail } = history;
        let root = Verifying::from(head).verified(None).unwrap();
        let expected = if tail.is_empty() {
            root.clone().into_inner()
        } else {
            tail[tail.len() - 1].clone()
        };
        let folded = root
            .verify(tail.into_iter().map(|x| Ok::<_, Void>(Verifying::from(x))))
            .unwrap();

        assert_eq!(folded.head.into_inner(), expected)
    }
}
