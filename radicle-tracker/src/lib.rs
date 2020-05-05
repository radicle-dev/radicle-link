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
//! An [`Issue`] is a conversation that forms around code collaboration. When a
//! user creates a new issue to interact with they kick off a [`Thread`]. The
//! thread is made up of a main thread that other users may reply to. On each
//! comment on the main thread a single sub-thread can also happen.
//!
//! Issues are more than just conversations, they also carry [`Metadata`]
//! alongside them so that we can enrich the experience of our conversations,
//! allowing us to label for organisation, react for emotions, and assign to
//! users to help responsibility.
//!
//! ```
//! # use std::error::Error;
//! #
//! # fn main() -> Result<(), Box<dyn Error>> {
//! use radicle_tracker::{Comment, Issue, Metadata, ReplyTo, Reaction, Title, Thread};
//! use std::str::FromStr;
//!
//! // Setting up some way of giving out "global" identifiers for
//! // issues and comments.
//! let mut global_issue_id = 0;
//! let mut new_issue_id = || {
//!     let new_id = global_issue_id.clone();
//!     global_issue_id += 1;
//!     new_id
//! };
//!
//! let mut global_comment_id = 0;
//! let mut new_comment_id = || {
//!     let new_id = global_comment_id.clone();
//!     global_comment_id += 1;
//!     new_id
//! };
//!
//! // Create a new issue for our buggy beouf
//! let mut issue = Issue::new(
//!     new_issue_id(),
//!     new_comment_id(),
//!     String::from("Monadic"),
//!     Title::from("Buggy Boeuf"),
//!     String::from("We have bugs in our boeuf"),
//! );
//!
//! // We can grab the Thread of our issue to view and manipulate.
//! let mut thread = issue.thread_mut().clone();
//! let initial_comment = thread.view()?.get().clone();
//!
//! assert_eq!(issue.author(), &String::from("Monadic"));
//! assert_eq!(issue.title(), &Title::from("Buggy Boeuf"));
//! assert_eq!(initial_comment.content(), &String::from("We have bugs in our boeuf"));
//! assert!(initial_comment.author() == issue.author());
//! assert_eq!(issue.meta(), &Metadata::new());
//!
//! // Let's reply to the main thread
//! let finto_comment = Comment::new(
//!     new_comment_id(),
//!     String::from("finto"),
//!     String::from("How do we find the bugs in our beouf")
//! );
//! thread.reply(finto_comment, ReplyTo::Main);
//!
//! // And then we reply to that first comment
//! let kim_comment = Comment::new(
//!     new_comment_id(),
//!     String::from("kim"),
//!     String::from("There are a few techniques to beouf bug finding...")
//! );
//! thread.reply(kim_comment, ReplyTo::Thread);
//!
//! // And we react to this comment with surprise!
//! let current_comment = thread.view_mut()?.get_mut();
//! current_comment.react(Reaction::new(String::from("massi"), String::from("surprise")));
//! #
//! #     Ok(())
//! # }
//! ```

#![deny(missing_docs, unused_import_braces, unused_qualifications, warnings)]
#![allow(clippy::new_without_default)]

mod thread;
pub use thread::{DataState, Error as ThreadError, Finger, Replies, ReplyTo, Thread};

mod metadata;
pub use metadata::*;

/// TODO: Comment
pub mod comment;
/// TODO: Issue
pub mod issue;
/// TODO: Ops
pub mod ops;

#[cfg(test)]
pub mod test;
