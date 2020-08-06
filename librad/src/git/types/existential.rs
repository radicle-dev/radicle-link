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

use super::reference::{Multiple, Namespace, Reference, Single};

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

impl<Namespaced: Clone> From<Reference<Namespaced, Single>> for SomeReference
where
    Namespaced: Into<SomeNamespace>,
{
    fn from(other: Reference<Namespaced, Single>) -> Self {
        let namespace = other._namespace.clone().into();
        Self::Single(other.with_namespace(namespace))
    }
}

impl<Namespaced: Clone> From<Reference<Namespaced, Multiple>> for SomeReference
where
    Namespaced: Into<SomeNamespace>,
{
    fn from(other: Reference<Namespaced, Multiple>) -> Self {
        let namespace = other._namespace.clone().into();
        Self::Multiple(other.with_namespace(namespace))
    }
}

impl<N: Clone> Reference<SomeNamespace, N> {
    fn sequence(&self) -> Either<Reference<PhantomData<!>, N>, Reference<Namespace, N>> {
        match &self._namespace.0 {
            Either::Left(_) => Either::Left(self.clone().with_namespace(PhantomData)),

            Either::Right(namespace) => {
                Either::Right(self.clone().with_namespace(namespace.clone()))
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SomeReference {
    Single(Reference<SomeNamespace, Single>),
    Multiple(Reference<SomeNamespace, Multiple>),
}

impl<N: Clone> Display for Reference<SomeNamespace, N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.sequence() {
            Either::Left(reference) => write!(f, "{}", reference),
            Either::Right(reference) => write!(f, "{}", reference),
        }
    }
}

impl Display for SomeReference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Single(reference) => write!(f, "{}", reference),
            Self::Multiple(reference) => write!(f, "{}", reference),
        }
    }
}
