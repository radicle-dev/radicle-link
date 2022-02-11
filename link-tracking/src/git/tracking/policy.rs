// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use either::Either::{self, Left, Right};
use radicle_git_ext::Oid;

use super::refdb::PreviousValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// Will attempt to create the tracking entry even if it already exists.
    Any,
    /// Will only attempt to create the tracking entry iff it did not already
    /// exist.
    MustNotExist,
    /// Will only attempt to create the tracking entry iff if already existed.
    /// This can be used to safely set the existing tracking entry to a new
    /// configuration.
    MustExist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Untrack {
    /// Will attempt to remove the tracking entry even if it did not already
    /// exist.
    Any,
    /// Will only attempt to remove the tracking entry iff it already existed.
    MustExist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UntrackAll {
    /// Will forcefully attempt to remove the tracking entries.
    Any,
    /// Will only attempt to remove the tracking entry iff they already existed
    /// *and* the targets read matched.
    MustExistAndMatch,
}

impl From<Track> for PreviousValue<Oid> {
    fn from(policy: Track) -> Self {
        match policy {
            Track::Any => Self::Any,
            Track::MustNotExist => Self::MustNotExist,
            Track::MustExist => Self::MustExist,
        }
    }
}

impl From<Untrack> for PreviousValue<Oid> {
    fn from(policy: Untrack) -> Self {
        match policy {
            Untrack::Any => Self::Any,
            Untrack::MustExist => Self::MustExist,
        }
    }
}

impl UntrackAll {
    pub(super) fn into_previous_value(self, oid: Oid) -> PreviousValue<Oid> {
        match self {
            Self::Any => PreviousValue::Any,
            Self::MustExistAndMatch => PreviousValue::MustExistAndMatch(oid),
        }
    }
}

/// For an explanation of the composition rules see:
/// `./docs/rfc/0699-tracking-storage.adoc#_batch_tracking`
///
/// The rules are inlined here for convenience:
///
/// * `track Any c' ∘ x === track Any c'`
/// * `track MustExist c' ∘ track Any c === track Any c'`
/// * `track MustExist c' ∘ track MustExist c === track MustExist c'`
/// * `track MustExist c' ∘ track MustNotExist c === track Any c'`
/// * `track MustNotExist c' ∘ track Any c === track Any c`
/// * `track MustNotExist c' ∘ track MustExist c === track MustNotExist c' ∘
///   track MustExist c`
/// * `track MustNotExist c' ∘ track MustNotExist c === track MustNotExist c`
/// * `untrack Any ∘ x === untrack Any`
/// * `untrack MustExist ∘ untrack Any === untrack Any`
/// * `untrack MustExist ∘ untrack MustExist === untrack MustExist`
/// * `track Any ∘ x === track Any`
/// * `track MustExist ∘ untrack p === untrack p`
/// * `track MustNotExist ∘ x === track Any`
/// * `untrack Any ∘ x === untrack Any`
/// * `untrack MustExist ∘ x === untrack Any`
pub mod compose {
    use super::*;

    /// When composing policies, they can either reduce to a single form, or
    /// they get stuck and can't compose. This does not mean they're stuck
    /// forever though. Another term can reduce both in one fell swoop.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Reduction<T> {
        Simple(T),
        Stuck { first: T, second: T },
    }

    impl<T> From<T> for Reduction<T> {
        fn from(s: T) -> Self {
            Reduction::Simple(s)
        }
    }

    impl<T> IntoIterator for Reduction<T> {
        type Item = T;

        type IntoIter =
            Either<std::iter::Once<T>, std::iter::Chain<std::iter::Once<T>, std::iter::Once<T>>>;

        fn into_iter(self) -> Self::IntoIter {
            match self {
                Self::Simple(one) => Left(std::iter::once(one)),
                Self::Stuck { first, second } => {
                    Right(std::iter::once(first).chain(std::iter::once(second)))
                },
            }
        }
    }

    /// When composing the [`Track`] policy we also want to consider what
    /// configuration we are going to end up writing to the location.
    /// `WithConfig` carries the `policy` along with the `config`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct WithConfig<'a, C> {
        pub policy: Track,
        pub config: &'a C,
    }

    /// Composition over `Self` composed with `Other` which results in
    /// `Codomain`. This allows us to be malleable in how we compose our
    /// policies.
    ///
    /// While composition is traditionally considered as `second . first`, this
    /// trait captures composition as `first.compose(second)` -- read as "first
    /// then second".
    pub trait Compose<Other> {
        type Codomain;

        fn compose(&self, other: &Other) -> Self::Codomain;
    }

    impl<T: Compose<T, Codomain = Reduction<T>>> Compose<T> for Reduction<T> {
        type Codomain = Reduction<T>;

        fn compose(&self, other: &T) -> Self::Codomain {
            match self {
                Reduction::Simple(s) => s.compose(other),
                Reduction::Stuck { first, second } => match second.compose(other) {
                    Reduction::Simple(new) => first.compose(&new),
                    // SAFETY: `second.compose(other)` should *always* reduce in our semantics since
                    // we only have one case. See radicle-link/docs/rfc/
                    // 0699-tracking-storage.adoc#_track_track for a list of the rules.
                    //
                    // The above `Stuck` can only be of the form `first: MustExist` and `second:
                    // MustNotExist`. According to all our rules, `MustNotExist` will always reduce
                    // to a `Simple` case.
                    Reduction::Stuck { .. } => unreachable!(),
                },
            }
        }
    }

    impl<'a, C> Compose<WithConfig<'a, C>> for WithConfig<'a, C> {
        type Codomain = Reduction<WithConfig<'a, C>>;

        /// Equivalent to `track p' c' . track p c`
        fn compose(&self, other: &WithConfig<'a, C>) -> Self::Codomain {
            match (other.policy, self.policy) {
                // track Any c' ∘ x === track Any c'
                (Track::Any, _) => WithConfig {
                    policy: Track::Any,
                    config: other.config,
                }
                .into(),
                // track MustNotExist c' ∘ track Any c === track Any c
                (Track::MustNotExist, Track::Any) => WithConfig {
                    policy: Track::Any,
                    config: self.config,
                }
                .into(),
                // track MustNotExist c' ∘ track MustNotExist c === track MustNotExist c
                (Track::MustNotExist, Track::MustNotExist) => WithConfig {
                    policy: Track::MustNotExist,
                    config: self.config,
                }
                .into(),
                // track MustNotExist c' ∘ track MustExist c === track MustNotExist c' ∘ track
                // MustExist c
                (Track::MustNotExist, Track::MustExist) => Reduction::Stuck {
                    first: WithConfig {
                        policy: Track::MustExist,
                        config: self.config,
                    },
                    second: WithConfig {
                        policy: Track::MustNotExist,
                        config: other.config,
                    },
                },
                // track MustExist c' ∘ track Any c === track Any c'
                (Track::MustExist, Track::Any) => WithConfig {
                    policy: Track::Any,
                    config: self.config,
                }
                .into(),
                // track MustExist c' ∘ track MustNotExist c === track Any c'
                (Track::MustExist, Track::MustNotExist) => WithConfig {
                    policy: Track::Any,
                    config: self.config,
                }
                .into(),
                // track MustExist c' ∘ track MustExist c === track MustExist c'
                (Track::MustExist, Track::MustExist) => WithConfig {
                    policy: Track::MustExist,
                    config: other.config,
                }
                .into(),
            }
        }
    }

    impl Compose<Untrack> for Track {
        type Codomain = Untrack;

        /// Equivalent to `untrack . track`
        fn compose(&self, _: &Untrack) -> Self::Codomain {
            Untrack::Any
        }
    }

    impl Compose<Untrack> for Untrack {
        type Codomain = Self;

        /// Equivalent to `untrack . untrack`        
        fn compose(&self, other: &Self) -> Self::Codomain {
            match (other, self) {
                // untrack Any ∘ x === untrack Any
                (Untrack::Any, _) => Untrack::Any,
                // untrack MustExist ∘ untrack Any === untrack Any
                (Untrack::MustExist, Untrack::Any) => Untrack::Any,
                // untrack MustExist ∘ untrack MustExist === untrack MustExist
                (Untrack::MustExist, Untrack::MustExist) => Untrack::MustExist,
            }
        }
    }

    impl Compose<Track> for Untrack {
        type Codomain = Either<Track, Untrack>;

        /// Equivalent to `track . untrack`
        fn compose(&self, track: &Track) -> Self::Codomain {
            match (track, self) {
                // track Any . x === track Any
                (Track::Any, _) => Left(Track::Any),
                // track MustNotExist . x === track Any
                (Track::MustNotExist, _) => Left(Track::Any),
                // track MustExist . x === x
                (Track::MustExist, u) => Right(*u),
            }
        }
    }

    impl<'a, C: std::fmt::Debug + Clone> Compose<Either<WithConfig<'a, C>, Untrack>>
        for Either<WithConfig<'a, C>, Untrack>
    {
        type Codomain = Reduction<Self>;

        /// Equivalent to being able to compose a mixture of `track` or
        /// `untrack` policies:
        /// * `track p' c' . track p c`
        /// * `track . untrack`
        /// * `untrack . track`
        /// * `untrack . untrack`
        fn compose(&self, other: &Either<WithConfig<'a, C>, Untrack>) -> Self::Codomain {
            match (other, self) {
                // track p' c' . track p c
                (Left(second), Left(first)) => match first.compose(second) {
                    Reduction::Simple(WithConfig { policy, config }) => {
                        Reduction::Simple(Left(WithConfig { config, policy }))
                    },
                    Reduction::Stuck { first, second } => Reduction::Stuck {
                        first: Left(first),
                        second: Left(second),
                    },
                },
                // track . untrack
                (
                    Left(WithConfig {
                        policy: track,
                        config,
                    }),
                    Right(untrack),
                ) => Reduction::Simple(match untrack.compose(track) {
                    Left(track) => Left(WithConfig {
                        config,
                        policy: track,
                    }),
                    Right(untrack) => Right(untrack),
                }),
                // untrack . track
                (Right(untrack), Left(WithConfig { policy: track, .. })) => {
                    Reduction::Simple(Right(track.compose(untrack)))
                },
                // untrack . untrack
                (Right(untrackl), Right(untrackr)) => {
                    Reduction::Simple(Right(untrackr.compose(untrackl)))
                },
            }
        }
    }
}
