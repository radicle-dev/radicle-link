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

//! `radicle-tracker` is where we get to the meat of an issue.
//!
//! An [`issue::Issue`] is a conversation that forms around code collaboration.
//! When a user creates a new issue to interact with they kick off a
//! [`thread::Thread`]. The thread is made up of a main thread that other users
//! may reply to. On each comment on the main thread a single sub-thread can
//! also happen.
//!
//! Issues are more than just conversations, they also carry [`Metadata`]
//! alongside them so that we can enrich the experience of our conversations,
//! allowing us to label for organisation, react for emotions, and assign to
//! users to help responsibility.

#![deny(missing_docs, unused_import_braces, unused_qualifications, warnings)]
#![allow(clippy::new_without_default)]

mod metadata;
pub use metadata::*;

/// A comment is a user's reply to an issue.
pub mod comment;
/// An issue is a conversation of multiple comments and lot's of emojis.
pub mod issue;
/// Operations are the backbone of building up code collaboration data. They all
/// implement [`ops::Apply`]. What this means is that they are a data structure
/// paired with operations to modify this structure. The expectation is that a
/// well formed timeline of operations will successfully mutate the structure so
/// that it can be viewed.
pub mod ops;
/// A [`thread::Thread`] is the composition of two
/// [`ops::sequence::OrdSequence`]s. It represents a thread of elements that can
/// be of infinite depth — we can continuously append to a sequence. It only,
/// however, has a breadth of one — appending to a single sub-thread.
pub mod thread;

#[cfg(test)]
pub mod test;
