// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(clippy::type_complexity)]

use std::{
    fmt::{Debug, Display},
    marker::PhantomData,
    ops::Deref,
};

use serde::ser::SerializeStruct;

use super::{delegation::Delegations, payload::Payload, sealed, sign::Signatures, urn::Urn};

pub mod error;

/// The identity document, carrying metadata `T` and trust delegations `D`.
///
/// In `git`, this is represented as a `blob`, where the previous revision
/// `replaces` is a `tree` oid.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct Doc<T, D, Revision> {
    /// Protocol version. Always serialised as `0` (zero).
    pub version: u8,
    pub replaces: Option<Revision>,
    pub payload: T,
    pub delegations: D,
}

impl<T, D, Revision> serde::Serialize for Doc<T, D, Revision>
where
    T: serde::Serialize,
    D: serde::Serialize,
    Revision: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut doc = serializer.serialize_struct("Doc", 4)?;
        doc.serialize_field("version", &0)?;
        doc.serialize_field("replaces", &self.replaces)?;
        doc.serialize_field("payload", &self.payload)?;
        doc.serialize_field("delegations", &self.delegations)?;
        doc.end()
    }
}

impl<T, D, R> Doc<T, D, R> {
    /// Bifunctorial map.
    ///
    /// Map over the payload `T` and the delegations `D` at the same time.
    pub fn bimap<F, U, G, E>(self, f: F, g: G) -> Doc<U, E, R>
    where
        F: FnOnce(T) -> U,
        G: FnOnce(D) -> E,
    {
        Doc {
            version: self.version,
            replaces: self.replaces,
            payload: f(self.payload),
            delegations: g(self.delegations),
        }
    }

    /// Map covariantly over `T`.
    pub fn first<F, U>(self, f: F) -> Doc<U, D, R>
    where
        F: FnOnce(T) -> U,
    {
        self.bimap(f, |x| x)
    }

    /// Map covariantly over `D`.
    pub fn second<G, E>(self, g: G) -> Doc<T, E, R>
    where
        G: FnOnce(D) -> E,
    {
        self.bimap(|x| x, g)
    }

    /// Map a fallible function over `T`.
    ///
    /// Like `bitraverse id pure . first` in Haskell.
    pub fn try_first<F, U, Error>(self, f: F) -> Result<Doc<U, D, R>, Error>
    where
        F: FnOnce(T) -> Result<U, Error>,
    {
        let doc = self.first(f);
        Ok(Doc {
            version: doc.version,
            replaces: doc.replaces,
            payload: doc.payload?,
            delegations: doc.delegations,
        })
    }

    /// Map a fallible function of `D`.
    ///
    /// Like `bitraverse pure id . second` in Haskell.
    pub fn try_second<G, E, Error>(self, g: G) -> Result<Doc<T, E, R>, Error>
    where
        G: FnOnce(D) -> Result<E, Error>,
    {
        let doc = self.second(g);
        Ok(Doc {
            version: doc.version,
            replaces: doc.replaces,
            payload: doc.payload,
            delegations: doc.delegations?,
        })
    }
}

impl<T, D, R> sealed::Sealed for Doc<T, D, R> {}

/// An identity attestation.
///
/// An [`Identity`] is content-addressable by `ContentId`, and signed by at
/// least one [`super::sign::Signature`] over the `revision` (this invariant is
/// maintained by [`Verifying::signed`]). It carries the root (or initial)
/// `Revision` of the identity document `T` (usually a [`Doc`]), which is also
/// the stable identifier which forms the identity's [`Urn`].
///
/// In `git`, an [`Identity`] is represented by a `commit`, where the
/// `content_id` is the commit `oid`, the `root` is the `blob` hash of the
/// initial version of the `doc`, and the [`Signatures`] are over the commit's
/// `tree` hash. The signatures are encoded in the commit message as [trailers].
///
/// [trailers]: https://git-scm.com/docs/git-interpret-trailers
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Identity<T, Revision, ContentId> {
    pub content_id: ContentId,
    pub root: Revision,
    pub revision: Revision,
    pub doc: T,
    pub signatures: Signatures,
}

impl<T, R, C> Identity<T, R, C> {
    /// The stable identifier of this identity.
    pub fn urn(&self) -> Urn<R>
    where
        R: Clone,
    {
        Urn::new(self.root.clone())
    }

    /// Non-allocating variant of [`Self::urn`].
    pub fn urn_ref(&self) -> Urn<&R> {
        Urn::new(&self.root)
    }

    /// Functorial map.
    ///
    /// Map a function over the identity document `T`.
    pub fn map<F, U>(self, f: F) -> Identity<U, R, C>
    where
        F: FnOnce(T) -> U,
    {
        Identity {
            content_id: self.content_id,
            root: self.root,
            revision: self.revision,
            doc: f(self.doc),
            signatures: self.signatures,
        }
    }
}

impl<T, R, C, Error> Identity<Result<T, Error>, R, C> {
    /// Transposes an `Identity<Result<T, E>, _, _>` into a `Result<Identity<T,
    /// _, _>, E>`.
    ///
    /// Allows to pass a fallible function to [`Self::map`], and "extract" the
    /// error.
    pub fn transpose(self) -> Result<Identity<T, R, C>, Error> {
        Ok(Identity {
            content_id: self.content_id,
            root: self.root,
            revision: self.revision,
            doc: self.doc?,
            signatures: self.signatures,
        })
    }
}

impl<T, R, C> AsRef<T> for Identity<T, R, C> {
    fn as_ref(&self) -> &T {
        &self.doc
    }
}

impl<T, R, C> sealed::Sealed for Identity<T, R, C> {}

impl<T, D, R, C> Identity<Doc<Payload<T>, D, R>, R, C> {
    /// Shortcut to get at `self.doc.payload.subject`.
    pub fn subject(&self) -> &T {
        &self.doc.payload.subject
    }

    /// Shortcut to get at `self.doc.payload`.
    pub fn payload(&self) -> &Payload<T> {
        &self.doc.payload
    }
}

impl<T, D, R, C> Identity<Doc<T, D, R>, R, C> {
    /// Shortcut to get at `self.doc.delegations`.
    pub fn delegations(&self) -> &D {
        &self.doc.delegations
    }
}

/// Ad-hoc trait which allows us to keep the `T` parameter of [`Identity`]
/// polymorphic for verification.
pub trait Replaces: sealed::Sealed {
    type Revision;

    fn replaces(&self) -> Option<&Self::Revision>;
}

impl<T, D, R> Replaces for Doc<T, D, R> {
    type Revision = R;

    fn replaces(&self) -> Option<&Self::Revision> {
        self.replaces.as_ref()
    }
}

/// Untrusted, well-formed input.
#[derive(Clone, Copy, Debug)]
pub struct Untrusted;

/// Well-formed and signed by at least one key delegation.
#[derive(Clone, Copy, Debug)]
pub struct Signed;

/// Signed by a quorum of the **current** key delegations.
#[derive(Clone, Copy, Debug)]
pub struct Quorum;

/// Signed by a quorum of the **current** key delegations **AND** a quorum
/// of the **parent**'s key delegations.
#[derive(Clone, Copy, Debug)]
pub struct Verified;

/// An identity `T` under verification.
///
/// The verification status (ie. which predicates where successfully applied to
/// `T`) is tracked on the type level, as intermediate states may have meaning
/// elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Verifying<T, S> {
    inner: T,
    state: PhantomData<S>,
}

impl<T, S> Deref for Verifying<T, S> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T, S> Verifying<T, S> {
    /// Create a [`Verifying`] from arbitrary input `T`.
    ///
    /// Type inference is usually better when using `Verifying::from`.
    pub fn from_untrusted(t: T) -> Verifying<T, Untrusted> {
        Verifying {
            inner: t,
            state: PhantomData,
        }
    }

    /// Strip the [`Verifying`] wrapper from `T`.
    pub fn into_inner(self) -> T {
        self.inner
    }

    fn coerce<U>(self) -> Verifying<T, U> {
        Verifying {
            inner: self.inner,
            state: PhantomData,
        }
    }
}

impl<T> From<T> for Verifying<T, Untrusted> {
    fn from(t: T) -> Self {
        Self::from_untrusted(t)
    }
}

impl<T, R, C> Verifying<Identity<T, R, C>, Untrusted> {
    /// Attempt to transition an [`Untrusted`] [`Identity`] to the [`Signed`]
    /// state.
    ///
    /// # Errors
    ///
    /// If the set of signatures is empty, or one or more signatures are invlid.
    pub fn signed(self) -> Result<Verifying<Identity<T, R, C>, Signed>, error::Verify<R, C>>
    where
        T: Delegations,
        T::Error: std::error::Error + Send + Sync + 'static,

        R: Debug + Display + AsRef<[u8]>,
        C: Debug + Display,
    {
        if self.signatures.is_empty() {
            Err(error::Verify::NoSignatures)
        } else if !self
            .signatures
            .iter()
            .all(|(pk, sig)| sig.verify(self.revision.as_ref(), pk))
        {
            Err(error::Verify::SignatureVerification)
        } else {
            Ok(self.coerce())
        }
    }

    /// Attempt to transition from [`Untrusted`] to [`Quorum`]
    ///
    /// Convenience for when [`Signed`] is not interesting.
    pub fn quorum(self) -> Result<Verifying<Identity<T, R, C>, Quorum>, error::Verify<R, C>>
    where
        T: Delegations,
        T::Error: std::error::Error + Send + Sync + 'static,

        R: Debug + Display + AsRef<[u8]>,
        C: Debug + Display,
    {
        self.signed()?.quorum()
    }

    /// Attempt to transition from [`Untrusted`] straight to [`Verified`].
    ///
    /// Convenience for when the intermediate states are not interesting.
    pub fn verified(
        self,
        parent: Option<&Verifying<Identity<T, R, C>, Verified>>,
    ) -> Result<Verifying<Identity<T, R, C>, Verified>, error::Verify<R, C>>
    where
        T: Delegations + Replaces<Revision = R>,
        T::Error: std::error::Error + Send + Sync + 'static,

        R: Clone + Debug + Display + PartialEq + AsRef<[u8]>,
        C: Clone + Debug + Display,
    {
        self.signed()?.quorum()?.verified(parent)
    }
}

impl<T, R, C> Verifying<Identity<T, R, C>, Signed> {
    /// Attempt to transition a [`Signed`] [`Identity`] to the [`Quorum`] state.
    ///
    /// # Errors
    ///
    /// If the number of signatures does not reach the
    /// [`Delegations::quorum_threshold`].
    pub fn quorum(self) -> Result<Verifying<Identity<T, R, C>, Quorum>, error::Verify<R, C>>
    where
        T: Delegations,
        T::Error: std::error::Error + Send + Sync + 'static,

        R: Debug + Display,
        C: Debug + Display,
    {
        let eligible = self
            .doc
            .eligible(self.signatures.keys().collect())
            .map_err(error::Verify::eligibility)?
            .len();

        if eligible > 0 && eligible > self.doc.quorum_threshold() {
            Ok(self.coerce())
        } else {
            Err(error::Verify::Quorum)
        }
    }
}

impl<T, R, C> Verifying<Identity<T, R, C>, Quorum> {
    /// Attempt to transition a [`Quorum`] [`Identity`] to the [`Verified`]
    /// state.
    ///
    /// This requires to supply the parent identity, ie. an [`Identity`] with
    /// the same `root` and a `revision` matching the `replaces` attribute
    /// of the identity [`Doc`]. If `self` is the initial revision (ie.
    /// `replaces` is `None`), the parent MUST be `None`.
    ///
    /// # Errors
    ///
    /// * `self` and `parent` don't point to the same `root`
    /// * `parent` is `Some`, but `self` does not have a previous revision
    /// * `parent` is `None`, but `self` **does** have a previous revision
    /// * the `parent` revision doesn't match `replaces`
    /// * `self`'s signatures do not reach a quorum of the `parent`'s
    ///   delegations. In other words,
    ///   `parent.eligible(self.signatures.keys()).len() >
    ///   parent.doc.quorum_threshold()`
    /// * `parent.eligible(self.signatures.keys())` returns an error
    pub fn verified(
        self,
        parent: Option<&Verifying<Identity<T, R, C>, Verified>>,
    ) -> Result<Verifying<Identity<T, R, C>, Verified>, error::Verify<R, C>>
    where
        T: Delegations + Replaces<Revision = R>,
        T::Error: std::error::Error + Send + Sync + 'static,

        R: Clone + Debug + Display + PartialEq + AsRef<[u8]>,
        C: Clone + Debug + Display,
    {
        match (self.doc.replaces(), parent) {
            (_, Some(parent)) if parent.root != self.root => Err(error::Verify::RootMismatch {
                expected: self.inner.root,
                actual: parent.root.clone(),
            }),

            (None, Some(parent)) => Err(error::Verify::DanglingParent(
                self.content_id.to_owned(),
                parent.content_id.to_owned(),
            )),
            (Some(replaces), None) => Err(error::Verify::MissingParent(replaces.to_owned())),

            (None, None) => Ok(self.coerce()),

            (Some(replaces), Some(parent)) => {
                if replaces != &parent.revision {
                    Err(error::Verify::ParentMismatch {
                        expected: replaces.to_owned(),
                        actual: parent.revision.to_owned(),
                    })
                } else {
                    let votes = parent
                        .doc
                        .eligible(self.signatures.keys().collect())
                        .map_err(error::Verify::eligibility)?
                        .len();

                    if votes > 0 && votes > parent.doc.quorum_threshold() {
                        Ok(self.coerce())
                    } else {
                        Err(error::Verify::ParentQuorum)
                    }
                }
            },
        }
    }
}

/// The result of running [`Verifying::verify`].
///
/// In addition to the most recent verified [`Identity`], the parent used to
/// call [`Verifying::verified`] is retained.
#[derive(Clone, Debug)]
pub struct Folded<T, R, C> {
    pub head: Verifying<Identity<T, R, C>, Verified>,
    pub parent: Option<Verifying<Identity<T, R, C>, Verified>>,
}

impl<T, R, C> Verifying<Identity<T, R, C>, Verified> {
    /// Starting from a [`Verified`] base [`Identity`], and its progeny, attempt
    /// to verify each identity in the progeny until either verification
    /// fails, or we find no more identities, in which case the most recent one
    /// is returned.
    ///
    /// Conceptually, this is a right-fold over the hash-linked history of
    /// identity attestations. In order to simplify implementations, we do
    /// not, however, constrain the iterator to be a
    /// [`DoubleEndedIterator`]. This means that it is up to the caller to
    /// ensure that the [`Iterator`] yields elements in reverse order.
    ///
    /// [`Signed`] identities in the progeny, which do not pass [`Quorum`] are
    /// skipped. This is to allow proposals to be made over the same protocol.
    pub fn verify<E>(
        self,
        mut progeny: impl Iterator<Item = Result<Verifying<Identity<T, R, C>, Untrusted>, E>>,
    ) -> Result<Folded<T, R, C>, error::Verify<R, C>>
    where
        T: Delegations + Replaces<Revision = R>,
        T::Error: std::error::Error + Send + Sync + 'static,

        R: Clone + Debug + Display + PartialEq + AsRef<[u8]>,
        C: Clone + Debug + Display,

        E: std::error::Error + Send + Sync + 'static,
    {
        progeny.try_fold(
            Folded {
                head: self,
                parent: None,
            },
            |acc, cur| {
                // Not signed is an error
                let signed = cur.map_err(error::Verify::history)?.signed()?;
                match signed.quorum() {
                    // Not reaching quorum is ok, skip
                    Err(_) => Ok(acc),
                    Ok(quorum) => {
                        // A confirmation of `self` is ok, but `parent` stays
                        // the same then. We need to be careful to not let a
                        // current quorum invalidate our already-confirmed state
                        // -- so skip if this doesn't pass `verified`, instead
                        // of returning an error (which would render this
                        // history invalid).
                        if quorum.revision == acc.head.revision
                            && quorum.doc.replaces() == acc.head.doc.replaces()
                        {
                            match quorum.verified(acc.parent.as_ref()) {
                                Err(_) => Ok(acc),
                                Ok(verified) => Ok(Folded {
                                    head: verified,
                                    parent: acc.parent,
                                }),
                            }
                        } else {
                            quorum.verified(Some(&acc.head)).map(|verified| Folded {
                                head: verified,
                                parent: Some(acc.head),
                            })
                        }
                    },
                }
            },
        )
    }
}
