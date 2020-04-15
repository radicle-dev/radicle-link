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

use std::{convert::Infallible, hash::Hash, str::FromStr};

pub mod clock;

/// The title of an issue.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Title(String);

impl Title {
    /// Create a new title.
    pub fn new(title: String) -> Self {
        Title(title)
    }
}

impl std::ops::Deref for Title {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&str> for Title {
    fn from(s: &str) -> Self {
        Title(s.to_string())
    }
}

impl FromStr for Title {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Title(s.to_string()))
    }
}

/// A custom label that can be added to an issue.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(String);

impl Label {
    /// Create a new `Label`.
    pub fn new(label: String) -> Self {
        Label(label)
    }
}

impl std::ops::Deref for Label {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromStr for Label {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Label(s.to_string()))
    }
}

/// A reaction is the pair of a user and a free-form reaction.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reaction<User> {
    pub(crate) user: User,
    pub(crate) value: String,
}

impl<User> Reaction<User> {
    /// Create a new reaction.
    pub fn new(user: User, value: String) -> Self {
        Reaction { user, value }
    }

    /// Get the reference to the user of this reaction.
    pub fn user(&self) -> &User {
        &self.user
    }

    /// Get the reference to the value of this reaction.
    pub fn value(&self) -> &String {
        &self.value
    }
}
