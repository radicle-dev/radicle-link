// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::collections::{BTreeMap, BTreeSet};

use nonempty::NonEmpty;
use proptest::prelude::*;

use super::{gen::*, *};
use crate::{identities::delegation, keys::tests::gen_secret_key};

proptest! {
    #[test]
    fn signed_empty_delegations(
        signing_keys in prop::collection::vec(gen_secret_key(), 1..3).no_shrink(),
    ) {
        let signatures = signing_keys
            .into_iter()
            .map(|key| (key.public(), key.sign(Boring.as_ref())))
            .collect::<BTreeMap<_, _>>()
            .into();

        assert!(matches!(
            Verifying::from(boring(
                delegation::Direct::from(BTreeSet::new()),
                signatures
            ))
            .signed::<!>(),
            Err(error::Verify::NoValidSignatures(_, _))
        ))
    }

    #[test]
    fn signed(id in gen_identity::<Boring>()) {
        assert_eq!(
            Verifying::from(id.clone())
                .signed::<!>()
                .unwrap()
                .into_inner(),
            id
        )
    }

    #[test]
    fn quorum_below_threshold(id in gen_identity::<Boring>()) {
        // Hitting the threshold exactly is one too few
        let threshold = id.quorum_threshold();
        let signatures: Signatures = BTreeMap::from(id.signatures)
            .into_iter()
            .take(threshold)
            .collect::<BTreeMap<_, _>>()
            .into();

        let id = Identity {
            signatures,
            ..id
        };

        assert!(matches!(
            Verifying::from(id).quorum::<!>(),
            Err(error::Verify::Quorum)
        ))
    }

    #[test]
    fn quorum(id in gen_identity::<Boring>()) {
        assert_eq!(
            Verifying::from(id.clone())
                .quorum::<!>()
                .unwrap()
                .into_inner(),
            id
        )
    }

    #[test]
    fn verified_root(id in gen_root_identity::<Revision>()) {
        assert_eq!(
            Verifying::from(id.clone())
                .verified::<!>(None)
                .unwrap()
                .into_inner(),
            id
        )
    }

    #[test]
    fn verified(NonEmpty { head, tail } in gen_history(1)) {
        match tail.as_slice() {
            [next] => {
                let parent = Verifying::from(head).verified::<!>(None).unwrap();
                let child = Verifying::from(next.clone())
                    .verified::<!>(Some(&parent))
                    .unwrap()
                    .into_inner();

                assert_eq!(&child, next)
            },

            _ => unreachable!(),
        }
    }

    #[test]
    fn verified_dangling_parent(parent in gen_root_identity::<Revision>()) {
        let parent = Verifying::from(parent).verified::<!>(None).unwrap();
        let child = Verifying::from(parent.clone().into_inner().map(|doc| Doc {
            replaces: None,
            ..doc
        }))
        .verified::<!>(Some(&parent));

        assert!(matches!(child, Err(error::Verify::DanglingParent { .. })))
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
        .verified::<!>(None)
        .unwrap();
        let child = Verifying::from(id).verified::<!>(Some(&parent));

        assert!(matches!(child, Err(error::Verify::RootMismatch { .. })))
    }

    #[test]
    fn verified_parent_mismatch(
        parent in gen_root_identity::<Revision>(),
        bogus_replaces in any::<Revision>()
    ) {
        let parent = Verifying::from(parent).verified::<!>(None).unwrap();
        let child = Verifying::from(parent.clone().into_inner().map(|doc| Doc {
            replaces: Some(bogus_replaces),
            ..doc
        })).verified::<!>(Some(&parent));

        assert!(matches!(child, Err(error::Verify::ParentMismatch { .. })))
    }

    #[test]
    fn verified_parent_quorum_below_threshold(
        NonEmpty { head, tail } in gen_history(1)
    ) {
        match tail.as_slice() {
            [next] => {
                let parent = Verifying::from(head).verified::<!>(None).unwrap();
                let next = Identity {
                    signatures: BTreeMap::from(next.signatures.clone())
                        .into_iter()
                        .take(next.quorum_threshold())
                        .collect::<BTreeMap<_, _>>()
                        .into(),
                    ..next.clone()
                };

                assert!(matches!(
                    Verifying::from(next).verified::<!>(Some(&parent)),
                    Err(error::Verify::Quorum)
                ))
            },

            _ => unreachable!(),
        }
    }

    #[test]
    fn verify(history in gen_history(0..10)) {
        let NonEmpty { head, tail } = history;
        let root = Verifying::from(head).verified::<!>(None).unwrap();
        let expected = if tail.is_empty() {
            root.clone().into_inner()
        } else {
            tail[tail.len() - 1].clone()
        };
        let folded = root
            .verify(tail.into_iter().map(|x| Ok::<_, !>(Verifying::from(x))))
            .unwrap();

        assert_eq!(folded.head.into_inner(), expected)
    }
}
