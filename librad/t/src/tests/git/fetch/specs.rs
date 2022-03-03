// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{BTreeMap, BTreeSet};

use pretty_assertions::assert_eq;

use librad::git::fetch::Fetchspecs;
use link_identities::{urn::test::FakeId, Urn};
use radicle_git_ext as ext;

lazy_static! {
    // "PeerId"s
    static ref LOLEK: ext::RefLike = reflike!("lolek");
    static ref BOLEK: ext::RefLike = reflike!("bolek");
    static ref TOLA: ext::RefLike = reflike!("tola");

    // "URN"s
    static ref PROJECT_URN: Urn<FakeId> = Urn::new(FakeId(32));
    static ref LOLEK_URN: Urn<FakeId> = Urn::new(FakeId(1));
    static ref BOLEK_URN: Urn<FakeId> = Urn::new(FakeId(2));

    // namespaces
    static ref PROJECT_NAMESPACE: ext::RefLike = reflike!("refs/namespaces").join(&*PROJECT_URN);
    static ref LOLEK_NAMESPACE: ext::RefLike = reflike!("refs/namespaces").join(&*LOLEK_URN);
    static ref BOLEK_NAMESPACE: ext::RefLike = reflike!("refs/namespaces").join(&*BOLEK_URN);
}

#[test]
fn peek_looks_legit() {
    let specs = Fetchspecs::Peek {
        remotes: Some(TOLA.clone()).into_iter().collect(),
        limit: Default::default(),
    }
    .refspecs(&*PROJECT_URN, TOLA.clone(), &Default::default());
    assert_eq!(
        specs
            .iter()
            .map(|spec| spec.to_string())
            .collect::<Vec<_>>(),
        [
            (
                refspec_pattern!("refs/rad/id"),
                refspec_pattern!("refs/remotes/tola/rad/id")
            ),
            (
                refspec_pattern!("refs/rad/self"),
                refspec_pattern!("refs/remotes/tola/rad/self")
            ),
            (
                refspec_pattern!("refs/rad/signed_refs"),
                refspec_pattern!("refs/remotes/tola/rad/signed_refs")
            ),
            (
                refspec_pattern!("refs/rad/ids/*"),
                refspec_pattern!("refs/remotes/tola/rad/ids/*")
            )
        ]
        .iter()
        .cloned()
        .map(|(remote, local)| format!(
            "{}:{}",
            PROJECT_NAMESPACE.with_pattern_suffix(remote),
            PROJECT_NAMESPACE.with_pattern_suffix(local),
        ))
        .collect::<Vec<_>>()
    )
}

#[test]
fn replicate_looks_legit() {
    use crate::make_refs;
    use librad::git::refs::{Refs, Remotes};

    lazy_static! {
        static ref ZERO: ext::Oid = ext::Oid::from(git2::Oid::zero());
    }

    let delegates = [LOLEK_URN.clone(), BOLEK_URN.clone()]
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    // Obviously, we have lolek and bolek's sigrefs
    let tracked_sigrefs = [
        (
            LOLEK.clone(),
            Refs {
                categorised_refs: make_refs! {
                    "heads" => {"mister" => *ZERO,},
                },
                remotes: Remotes::new(),
            },
        ),
        (
            BOLEK.clone(),
            Refs {
                categorised_refs: make_refs! {
                    "heads" => {
                        "mister" => *ZERO,
                        "next" => *ZERO,
                    },
                },
                remotes: Remotes::new(),
            },
        ),
    ]
    .iter()
    .cloned()
    .collect::<BTreeMap<_, _>>();

    // Tola is tracking PROJECT_URN, therefore she also has lolek and bolek
    let remote_heads = [
        (
            reflike!("refs/namespaces")
                .join(&*PROJECT_URN)
                .join(reflike!("refs/heads/mister")),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*PROJECT_URN)
                .join(reflike!("refs/rad/id")),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*PROJECT_URN)
                .join(reflike!("refs/rad/ids"))
                .join(&*LOLEK_URN),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*PROJECT_URN)
                .join(reflike!("refs/rad/ids"))
                .join(&*BOLEK_URN),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*PROJECT_URN)
                .join(reflike!("refs/remotes/lolek/heads/mister")),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*PROJECT_URN)
                .join(reflike!("refs/remotes/bolek/heads/mister")),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*PROJECT_URN)
                .join(reflike!("refs/remotes/bolek/heads/next")),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*LOLEK_URN)
                .join(reflike!("refs/rad/id")),
            *ZERO,
        ),
        (
            reflike!("refs/namespaces")
                .join(&*BOLEK_URN)
                .join(reflike!("refs/rad/id")),
            *ZERO,
        ),
    ]
    .iter()
    .cloned()
    .collect::<BTreeMap<_, _>>()
    .into();

    let specs = Fetchspecs::Replicate {
        tracked_sigrefs,
        delegates,
        limit: Default::default(),
    }
    .refspecs(&*PROJECT_URN, TOLA.clone(), &remote_heads);

    assert_eq!(
        specs
            .into_iter()
            .map(|spec| spec.to_string())
            .collect::<BTreeSet<String>>(),
        [
            // First, lolek + bolek's heads (forced)
            format!(
                "{}:{}",
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/mister")),
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/mister"))
            ),
            format!(
                "{}:{}",
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/next")),
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/next"))
            ),
            format!(
                "{}:{}",
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/lolek/heads/mister")),
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/lolek/heads/mister"))
            ),
            // Tola's rad/*
            format!(
                "{}:{}",
                PROJECT_NAMESPACE.join(reflike!("refs/rad/id")),
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/tola/rad/id"))
            ),
            format!(
                "{}:{}",
                PROJECT_NAMESPACE.join(reflike!("refs/rad/self")),
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/tola/rad/self"))
            ),
            format!(
                "{}:{}",
                PROJECT_NAMESPACE.with_pattern_suffix(refspec_pattern!("refs/rad/ids/*")),
                PROJECT_NAMESPACE
                    .with_pattern_suffix(refspec_pattern!("refs/remotes/tola/rad/ids/*"))
            ),
            format!(
                "{}:{}",
                PROJECT_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                PROJECT_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs")),
            ),
            // Tola's view of rad/* of lolek + bolek's top-level namespaces
            format!(
                "{}:{}",
                BOLEK_NAMESPACE.join(reflike!("refs/rad/id")),
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/id"))
            ),
            format!(
                "{}:{}",
                BOLEK_NAMESPACE.join(reflike!("refs/rad/self")),
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/self"))
            ),
            format!(
                "{}:{}",
                BOLEK_NAMESPACE.with_pattern_suffix(refspec_pattern!("refs/rad/ids/*")),
                BOLEK_NAMESPACE
                    .with_pattern_suffix(refspec_pattern!("refs/remotes/tola/rad/ids/*"))
            ),
            format!(
                "{}:{}",
                BOLEK_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs")),
            ),
            format!(
                "{}:{}",
                LOLEK_NAMESPACE.join(reflike!("refs/rad/id")),
                LOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/id"))
            ),
            format!(
                "{}:{}",
                LOLEK_NAMESPACE.join(reflike!("refs/rad/self")),
                LOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/self"))
            ),
            format!(
                "{}:{}",
                LOLEK_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                LOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs"))
            ),
            format!(
                "{}:{}",
                LOLEK_NAMESPACE.with_pattern_suffix(refspec_pattern!("refs/rad/ids/*")),
                LOLEK_NAMESPACE
                    .with_pattern_suffix(refspec_pattern!("refs/remotes/tola/rad/ids/*"))
            ),
            // Bolek's signed_refs for BOLEK_URN
            format!(
                "{}:{}",
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs")),
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs"))
            ),
            // Lolek's signed_refs for BOLEK_URN (because we're tracking him)
            format!(
                "{}:{}",
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs")),
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs"))
            ),
            format!(
                "{}:{}",
                BOLEK_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs"))
            ),
            // Lolek's signed_refs for LOLEK_URN
            format!(
                "{}:{}",
                LOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs")),
                LOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs"))
            ),
            // Bolek's signed_refs for LOLEK_URN (because we're tracking him)
            format!(
                "{}:{}",
                LOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs")),
                LOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs"))
            ),
        ]
        .iter()
        .map(std::borrow::ToOwned::to_owned)
        .collect::<BTreeSet<String>>()
    )
}
