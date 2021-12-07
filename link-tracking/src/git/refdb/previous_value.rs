// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#[derive(Clone, Copy, Debug)]
pub enum PreviousValue<Oid> {
    /// Will always succeed.
    Any,
    /// The reference must exist.
    MustExist,
    /// The reference must not exist.
    MustNotExist,
    /// The reference must exist and point to the given `Oid`.
    MustExistAndMatch(Oid),
    /// If the reference exists it must point to the given `Oid`, however, it is
    /// allowed to not exist.
    IfExistsMustMatch(Oid),
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum PreviousError<Oid> {
    #[error("the reference was expected to exist but it did not")]
    DidNotExist,
    #[error("the reference was expected to not exist but it did")]
    DidExist,
    #[error("the reference target was expected to be `{expected}`, but found `{actual}`")]
    DidNotMatch { expected: Oid, actual: Oid },
}

impl<Oid> PreviousValue<Oid> {
    /// Guard on the policy of `self` and the provided `previous` value.
    /// When the conditions are successful, the `on_success` callback is
    /// executed, and `None` is returned. When the conditions are
    /// unsuccessful the corresponding [`PreviousError`] is returned based on
    /// the variant of `self`.
    pub fn guard<Err>(
        &self,
        previous: Option<&Oid>,
        on_success: impl FnOnce() -> Result<(), Err>,
    ) -> Result<Option<PreviousError<Oid>>, Err>
    where
        Oid: Clone + PartialEq,
    {
        Ok(match self {
            Self::Any => {
                on_success()?;
                None
            },
            Self::MustExist => {
                if previous.is_some() {
                    on_success()?;
                    None
                } else {
                    Some(PreviousError::DidNotExist)
                }
            },
            Self::MustNotExist => {
                if previous.is_none() {
                    on_success()?;
                    None
                } else {
                    Some(PreviousError::DidExist)
                }
            },
            Self::MustExistAndMatch(expected) => match previous {
                None => Some(PreviousError::DidNotExist),
                Some(actual) => {
                    if expected == actual {
                        on_success()?;
                        None
                    } else {
                        Some(PreviousError::DidNotMatch {
                            expected: expected.clone(),
                            actual: actual.clone(),
                        })
                    }
                },
            },
            Self::IfExistsMustMatch(expected) => match previous {
                None => {
                    on_success()?;
                    None
                },
                Some(actual) => {
                    if expected == actual {
                        on_success()?;
                        None
                    } else {
                        Some(PreviousError::DidNotMatch {
                            expected: expected.clone(),
                            actual: actual.clone(),
                        })
                    }
                },
            },
        })
    }
}
