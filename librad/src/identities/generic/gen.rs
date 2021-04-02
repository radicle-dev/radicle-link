// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display},
    iter,
};

use either::Either::{self, *};
use nonempty::NonEmpty;
use proptest::prelude::*;

use super::*;
use crate::{
    identities::delegation,
    keys::{gen::gen_secret_key, PublicKey, SecretKey, Signature},
};

/// A completely irrelevant value.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Boring;

impl Arbitrary for Boring {
    type Parameters = ();
    type Strategy = fn() -> Self;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        || Boring
    }
}

impl Display for Boring {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Boring")
    }
}

impl AsRef<[u8]> for Boring {
    fn as_ref(&self) -> &[u8] {
        b"oring"
    }
}

/// [`Vec`] with at least 2 elements.
#[derive(Clone, Debug, PartialEq)]
pub struct VecOf2<T>(Vec<T>);

impl<T> From<VecOf2<T>> for Vec<T> {
    fn from(vo2: VecOf2<T>) -> Self {
        vo2.0
    }
}

impl<T> Deref for VecOf2<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn gen_vecof2<T>(element: T, max: usize) -> impl Strategy<Value = VecOf2<T::Value>>
where
    T: Strategy,
{
    prop::collection::vec(element, 2..max).prop_map(VecOf2)
}

/// A revision that looks a bit like a git SHA1, but is faster to generate.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(String);

impl Arbitrary for Revision {
    type Parameters = ();
    type Strategy = prop::strategy::Map<&'static str, fn(String) -> Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        "[a-z0-9]{40}".prop_map(Self)
    }
}

impl AsRef<[u8]> for Revision {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// "Existentialised" delegations.
#[derive(Clone, Debug, PartialEq)]
pub enum SomeDelegations<T, R: Ord, C: Ord> {
    Direct(delegation::Direct),
    Indirect(delegation::Indirect<T, R, C>),
}

impl<T, R: Ord, C: Ord> Delegations for SomeDelegations<T, R, C> {
    type Error = Either<
        <delegation::Direct as Delegations>::Error,
        <delegation::Indirect<T, R, C> as Delegations>::Error,
    >;

    fn eligible(&self, votes: BTreeSet<&PublicKey>) -> Result<BTreeSet<&PublicKey>, Self::Error> {
        match self {
            SomeDelegations::Direct(direct) => Ok(direct.eligible(votes)),
            SomeDelegations::Indirect(indirect) => indirect.eligible(votes).map_err(Right),
        }
    }

    fn quorum_threshold(&self) -> usize {
        match self {
            SomeDelegations::Direct(direct) => direct.quorum_threshold(),
            SomeDelegations::Indirect(indirect) => indirect.quorum_threshold(),
        }
    }
}

impl<T, R: Ord, C: Ord> sealed::Sealed for SomeDelegations<T, R, C> {}

/// Official radicle presence of [The Most Interesting Man In The World].
///
/// [The Most Interesting Man In The World]: https://imgflip.com/i/4dlpj1
pub fn boring<D>(
    delegations: D,
    signatures: Signatures,
) -> Identity<Doc<Boring, D, Boring>, Boring, Boring>
where
    D: Delegations,
{
    Identity {
        content_id: Boring,
        root: Boring,
        revision: Boring,
        doc: Doc {
            version: 0,
            replaces: None,
            payload: Boring,
            delegations,
        },
        signatures,
    }
}

pub type ArbitraryIdentity<R> =
    Identity<Doc<Boring, SomeDelegations<Boring, R, Boring>, R>, R, Boring>;

/// Very random [`Identity`].
pub fn gen_identity<R>() -> impl Strategy<Value = ArbitraryIdentity<R>>
where
    R: Arbitrary + Clone + Debug + Display + Ord + AsRef<[u8]>,
{
    (
        gen_signing_keys(),
        any::<R>(),
        any::<R>(),
        any::<Option<R>>(),
    )
        .prop_flat_map(|(signing_keys, root, revision, replaces)| {
            gen_identity_with(signing_keys, root, revision, replaces)
        })
}

/// [`Identity`] with some fixed values.
pub fn gen_identity_with<R>(
    signing_keys: VecOf2<SecretKey>,
    root: R,
    revision: R,
    replaces: Option<R>,
) -> impl Strategy<Value = ArbitraryIdentity<R>>
where
    R: Arbitrary + Clone + Debug + Display + Ord + AsRef<[u8]>,
{
    (
        Just((root, revision.clone(), replaces)),
        gen_delegations_with(signing_keys, revision),
    )
        .prop_map(
            |((root, revision, replaces), (signatures, delegations))| Identity {
                content_id: Boring,
                root,
                revision,
                doc: Doc {
                    version: 0,
                    replaces,
                    payload: Boring,
                    delegations,
                },
                signatures,
            },
        )
}

/// [`Identity`] which replaces nothing.
pub fn gen_root_identity<R>() -> impl Strategy<Value = ArbitraryIdentity<R>>
where
    R: Arbitrary + Clone + Debug + Display + Ord + AsRef<[u8]>,
{
    gen_signing_keys().prop_flat_map(gen_root_identity_with)
}

/// Like [`gen_root_identity`], but with a fixed set of keys.
pub fn gen_root_identity_with<R>(
    signing_keys: VecOf2<SecretKey>,
) -> impl Strategy<Value = ArbitraryIdentity<R>>
where
    R: Arbitrary + Clone + Debug + Display + Ord + AsRef<[u8]>,
{
    (Just(signing_keys), any::<R>(), any::<R>()).prop_flat_map(|(signing_keys, root, revision)| {
        gen_identity_with(signing_keys, root, revision, None)
    })
}

/// An identity history of length `len` (plus the root revision), which should
/// pass verification.
///
/// Note that the `content_id` is still `Boring`, only the `Doc` hash-links are
/// relevant for verification.
///
/// To reach quorum at each revision, we just sign with the same set of keys,
/// meaning that the delegations don't actually change -- an exercise for the
/// future maintainer to randomise this as well.
///
/// The history is in reverse order, ie. starts with the root revision.
pub fn gen_history(
    len: impl Into<prop::collection::SizeRange>,
) -> impl Strategy<Value = NonEmpty<ArbitraryIdentity<Revision>>> {
    (Just(len.into()), gen_signing_keys()).prop_ind_flat_map(move |(len, keys)| {
        (
            Just(keys.clone()),
            gen_root_identity_with(keys),
            prop::collection::vec(any::<Revision>(), len),
        )
            .prop_map(|(keys, root, revisions)| {
                let keys = keys
                    .iter()
                    .map(|sk| (sk.public(), sk))
                    .collect::<BTreeMap<_, _>>();

                let tail = revisions
                    .into_iter()
                    .fold((Vec::new(), root.clone()), |(mut acc, parent), revision| {
                        let signatures = parent
                            .signatures
                            .iter()
                            .map(|(pk, _)| {
                                let sk = keys.get(pk).unwrap();
                                (*pk, sk.sign(revision.as_ref()))
                            })
                            .collect::<BTreeMap<_, _>>()
                            .into();

                        let next = Identity {
                            revision,
                            signatures,
                            ..parent.clone()
                        }
                        .map(|doc| Doc {
                            replaces: Some(parent.revision.clone()),
                            ..doc
                        });

                        acc.push(next.clone());
                        (acc, next)
                    })
                    .0;

                NonEmpty { head: root, tail }
            })
    })
}

fn mk_direct(
    signing_keys: &[SecretKey],
    data_to_sign: impl AsRef<[u8]>,
) -> (Signatures, delegation::Direct) {
    let signatures: Signatures = signing_keys
        .iter()
        .map(|key| (key.public(), key.sign(data_to_sign.as_ref())))
        .collect::<BTreeMap<_, _>>()
        .into();

    let delegations: delegation::Direct = signatures
        .iter()
        .map(|(pk, _)| *pk)
        .collect::<BTreeSet<_>>()
        .into();

    (signatures, delegations)
}

fn mk_indirect_with<R>(
    signing_keys: VecOf2<SecretKey>,
    revision_to_sign: R,
    inner_root: R,
    inner_revision: R,
    inner_replaces: Option<R>,
    num_keys_indirect: usize,
) -> (Signatures, delegation::Indirect<Boring, R, Boring>)
where
    R: Clone + Debug + Display + Ord + AsRef<[u8]>,
{
    // First chunk shall be indirect
    let (inner_keys, direct_keys) = signing_keys.split_at(num_keys_indirect);

    let (indirect_signatures, indirect_delegations): (
        (PublicKey, Signature),
        delegation::indirect::IndirectlyDelegating<Boring, R, Boring>,
    ) = {
        let (signatures, delegations) = mk_direct(inner_keys, revision_to_sign.clone());

        // Pick the first signature to be used for the `Identity` containing our
        // delegations -- we can use only one to not cause a double-vote
        let sig = signatures
            .iter()
            .next()
            .map(|(k, s)| (*k, s.clone()))
            .unwrap();
        let inner = Identity {
            content_id: Boring,
            root: inner_root,
            revision: inner_revision,
            doc: Doc {
                version: 0,
                replaces: inner_replaces,
                payload: Boring,
                delegations,
            },
            signatures,
        };

        (sig, inner)
    };

    // Rest shall be direct
    let (mut signatures, direct_delegations) = mk_direct(direct_keys, revision_to_sign);
    signatures.insert(indirect_signatures.0, indirect_signatures.1);

    let delegations: delegation::Indirect<Boring, _, _> = delegation::Indirect::try_from_iter(
        iter::once(Right(indirect_delegations)).chain(direct_delegations.into_iter().map(Left)),
    )
    .unwrap();

    (signatures, delegations)
}

/// [`delegation::Indirect`] from a set of signing keys and some data to sign.
///
/// Returns the [`Signatures`] made, maintaining the invariant that only one of
/// them is owned by the [`delegation::Indirect`] (ie. no
/// [`delegation::indirect::error::DoubleVote`] can occur).
pub fn gen_indirect<R>(
    signing_keys: VecOf2<SecretKey>,
    revision_to_sign: R,
) -> impl Strategy<Value = (Signatures, delegation::Indirect<Boring, R, Boring>)>
where
    R: Arbitrary + Clone + Debug + Display + Ord + AsRef<[u8]>,
{
    let num_keys = signing_keys.len();
    (
        Just(signing_keys),
        Just(revision_to_sign),
        any::<R>(),
        any::<R>(),
        any::<Option<R>>(),
        1..num_keys,
    )
        .prop_map(
            |(
                signing_keys,
                revision_to_sign,
                inner_root,
                inner_revision,
                inner_replaces,
                num_keys_indirect,
            )| {
                mk_indirect_with(
                    signing_keys,
                    revision_to_sign,
                    inner_root,
                    inner_revision,
                    inner_replaces,
                    num_keys_indirect,
                )
            },
        )
}

/// Delegations of some type, with fixed parameters.
pub fn gen_delegations_with<R>(
    signing_keys: VecOf2<SecretKey>,
    revision: R,
) -> impl Strategy<Value = (Signatures, SomeDelegations<Boring, R, Boring>)>
where
    R: Arbitrary + Clone + Debug + Display + Ord + AsRef<[u8]>,
{
    prop_oneof![
        Just({
            let (signatures, delegations) = mk_direct(&signing_keys, &revision);
            (signatures, SomeDelegations::Direct(delegations))
        }),
        gen_indirect(signing_keys, revision).prop_map(|(s, d)| (s, SomeDelegations::Indirect(d)))
    ]
}

pub fn gen_signing_keys() -> impl Strategy<Value = VecOf2<SecretKey>> {
    gen_vecof2(gen_secret_key(), 8)
}
