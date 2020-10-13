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

use std::{
    fmt::{self, Display},
    marker::PhantomData,
};

use either::Either;

use super::reference::{Multiple, Namespace, Namespace2, Reference, Single};

#[derive(Debug, Clone, PartialEq)]
pub struct SomeNamespace(Either<PhantomData<!>, Namespace>);

impl From<PhantomData<!>> for SomeNamespace {
    fn from(_: PhantomData<!>) -> Self {
        Self(Either::Left(PhantomData))
    }
}

impl From<Namespace> for SomeNamespace {
    fn from(other: Namespace) -> Self {
        Self(Either::Right(other))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SomeNamespace2(Either<PhantomData<!>, Namespace2>);

impl From<PhantomData<!>> for SomeNamespace2 {
    fn from(_: PhantomData<!>) -> Self {
        Self(Either::Left(PhantomData))
    }
}

impl From<Namespace2> for SomeNamespace2 {
    fn from(other: Namespace2) -> Self {
        Self(Either::Right(other))
    }
}

impl<N: Clone, R: Clone> From<Reference<N, R, Single>> for SomeReference<R>
where
    N: Into<SomeNamespace>,
{
    fn from(other: Reference<N, R, Single>) -> Self {
        Self::Single(other.some_namespace())
    }
}

impl<N: Clone, R: Clone> From<Reference<N, R, Multiple>> for SomeReference<R>
where
    N: Into<SomeNamespace>,
{
    fn from(other: Reference<N, R, Multiple>) -> Self {
        Self::Multiple(other.some_namespace())
    }
}

impl<N: Clone, R: Clone> Reference<SomeNamespace, R, N> {
    fn sequence(&self) -> Either<Reference<PhantomData<!>, R, N>, Reference<Namespace, R, N>> {
        match &self._namespace.0 {
            Either::Left(_) => Either::Left(self.clone().with_namespace(PhantomData)),

            Either::Right(namespace) => {
                Either::Right(self.clone().with_namespace(namespace.clone()))
            },
        }
    }
}

impl<N: Clone, R: Clone> Reference<SomeNamespace2, R, N> {
    fn sequence(&self) -> Either<Reference<PhantomData<!>, R, N>, Reference<Namespace2, R, N>> {
        match &self._namespace.0 {
            Either::Left(_) => Either::Left(self.clone().with_namespace(PhantomData)),

            Either::Right(namespace) => {
                Either::Right(self.clone().with_namespace(namespace.clone()))
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SomeReference<R> {
    Single(Reference<SomeNamespace, R, Single>),
    Multiple(Reference<SomeNamespace, R, Multiple>),
}

impl<N: Clone, R: Clone + Display> Display for Reference<SomeNamespace, R, N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.sequence() {
            Either::Left(reference) => write!(f, "{}", reference),
            Either::Right(reference) => write!(f, "{}", reference),
        }
    }
}

impl<N: Clone, R: Clone + Display> Display for Reference<SomeNamespace2, R, N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.sequence() {
            Either::Left(reference) => write!(f, "{}", reference),
            Either::Right(reference) => write!(f, "{}", reference),
        }
    }
}

impl<R: Clone + Display> Display for SomeReference<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Single(reference) => write!(f, "{}", reference),
            Self::Multiple(reference) => write!(f, "{}", reference),
        }
    }
}
