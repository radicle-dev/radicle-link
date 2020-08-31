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

use nonempty::NonEmpty;
use thiserror::Error;

/// The "liveness" status of some data.
///
/// TODO: we may want to consider `Modified`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataState<A> {
    /// The data has been created.
    Live(A),
    /// The data has been created and also deleted.
    Dead(A),
}

impl<A> DataState<A> {
    /// Mark the status as `Dead`, no matter what the original status was.
    fn kill(&mut self)
    where
        A: Clone,
    {
        *self = Self::Dead(self.get().clone())
    }

    /// Get the reference to the value inside the status.
    pub fn get(&self) -> &A {
        match self {
            Self::Live(a) => a,
            Self::Dead(a) => a,
        }
    }

    /// Get the mutable reference to the value inside the status.
    pub fn get_mut(&mut self) -> &mut A {
        match self {
            Self::Live(a) => a,
            Self::Dead(a) => a,
        }
    }

    /// If the status is `Live` then return a reference to it.
    pub fn live(&self) -> Option<&A> {
        match self {
            Self::Live(a) => Some(a),
            _ => None,
        }
    }

    /// If the status is `Dead` then return a reference to it.
    pub fn dead(&self) -> Option<&A> {
        match self {
            Self::Dead(a) => Some(a),
            _ => None,
        }
    }
}

/// Errors can occur when navigating around a thread or when attempting to
/// delete the root item of a thread.
#[derive(Error, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Error {
    /// An attempt was made to delete the root of a thread, but the root is
    /// immutable.
    #[error("cannot delete the main item of the thread")]
    DeleteRoot,
    /// An attempt was made to move to the previous item in the main thread, but
    /// the pointer is already at the root item.
    #[error("tried to move to previous item in the main thread, but we are at the first")]
    PreviousOnRoot,
    ///
    #[error("an attempt was made to move to {attempt}, but this is out of bounds where the bounds are {main:?}, {reply:?}.")]
    OutOfBounds {
        ///
        attempt: Finger,
        ///
        main: Option<usize>,
        ///
        reply: Option<usize>,
    },
}

/// A collection of replies where a reply is any item that has a [`DataState`].
///
/// `Replies` are deliberately opaque as they should mostly be interacted with
/// via [`Thread`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Replies<A>(NonEmpty<DataState<A>>);

impl<A> Replies<A> {
    fn new(a: A) -> Self {
        Replies(NonEmpty::new(DataState::Live(a)))
    }

    fn reply(&mut self, a: A) {
        self.0.push(DataState::Live(a))
    }

    fn first(&self) -> &DataState<A> {
        self.0.first()
    }

    fn first_mut(&mut self) -> &mut DataState<A> {
        self.0.first_mut()
    }

    /// Check is the `Replies` are empty. It is always `false`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Check the length of the `Replies`.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&DataState<A>> {
        self.0.get(index)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut DataState<A>> {
        self.0.get_mut(index)
    }

    /// Get the [`Iterator`] for the `Replies`.
    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &DataState<A>> + 'a {
        self.0.iter()
    }
}

/// A structure for pointing into a [`Thread`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Finger {
    /// This finger points to the root value in a `Thread`.
    Root,
    /// This finger points to a single item in the "main thread", i.e. the
    /// replies to the root item. The index `0` points to the first item in
    /// the main thread.
    Main(usize),
    /// This finger points to a single item in a "reply thread".
    Thread {
        /// This index refers to which sub-thread we are referring to on the
        /// "main thread". The index `0` points to the first item in the main
        /// thread.
        main: usize,
        /// This index refers to which reply in the sub-thread we are referring
        /// to on the "reply thread". The index `1` points to the first
        /// _reply_ in the reply thread. This is because the first item
        /// is the main thread item.
        reply: usize,
    },
}

impl std::fmt::Display for Finger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root => write!(f, "ROOT"),
            Self::Main(main) => write!(f, "main_thread@{{ {} }}", main),
            Self::Thread { main, reply } => write!(f, "reply_thread@{{ {}, {} }}", main, reply),
        }
    }
}

// This point to the main thread, and the first item in that thread.
const ROOT_FINGER: Finger = Finger::Root;

/// A `Thread` is the root item followed by a series of non-empty replies to the
/// root item. For each item in reply to the root item there may be 0 or more
/// replies.
#[derive(Debug, Clone)]
pub struct Thread<A> {
    // A finger points into the `main_thread` structure. It allows us to efficiently look at the
    // current item, and gives us a way to move around the data structure as if reading a thread.
    finger: Finger,

    // root and main_thread make up the actual data of the data structure.
    root: DataState<A>,
    main_thread: Vec<Replies<A>>,
}

impl<A: PartialEq> PartialEq for Thread<A> {
    fn eq(&self, other: &Self) -> bool {
        self.main_thread == other.main_thread
    }
}

/// `ReplyTo` tells the navigation and reply functions whether they should take
/// action on the "main thread" or on a "reply thread".
///
/// See [`Thread::reply`] for an example of how it is used.
pub enum ReplyTo {
    /// Reply to the main thread.
    Main,
    /// Reply to the reply thread.
    Thread,
}

impl<A> Thread<A> {
    /// Create a new `Thread` with `a` as the root of the `Thread`.
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::{DataState, Thread};
    ///
    /// let (thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("Discussing rose trees"))));
    /// ```
    pub fn new(a: A) -> Self {
        Thread {
            finger: ROOT_FINGER,
            root: DataState::Live(a),
            main_thread: vec![],
        }
    }

    /// Look at the previous reply of the thread. If it's the case that we are
    /// looking at the first reply to an item on the main thread, then we
    /// will point to the main thread item.
    ///
    /// The [`ReplyTo`] value will be ignored when we are pointer is
    /// `Finger::Main`, and we will attempt to move to the previous reply on
    /// the main thread regardless.
    ///
    /// # Errors
    ///
    /// * [`Error::OutOfBounds`] - If the navigation to the next item in the
    ///   thread is out of bounds. This usual means we are at the first item in
    ///   the thread.
    ///
    /// * [`Error::PreviousOnRoot`] - If we are pointing to the root of the
    ///   thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// thread.previous_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("Discussing rose trees"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment
    /// thread.reply(String::from("Is this about flowers?"), ReplyTo::Thread);
    ///
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("Is this about flowers?"))));
    ///
    /// thread.previous_reply(ReplyTo::Thread)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("I love rose trees!"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn previous_reply(&mut self, reply_to: ReplyTo) -> Result<(), Error> {
        match self.finger {
            Finger::Root => Err(Error::PreviousOnRoot),
            Finger::Main(main) if main == 0 => {
                self.finger = Finger::Root;
                Ok(())
            },
            Finger::Main(main) => self.navigate_to(Finger::Main(main - 1)),
            Finger::Thread { main, reply } => match reply_to {
                ReplyTo::Main => self.navigate_to(Finger::Main(main - 1)),
                ReplyTo::Thread => {
                    // If we're at the first reply, then we move to the main thread.
                    if reply == 0 {
                        self.navigate_to(Finger::Main(main))
                    } else {
                        self.navigate_to(Finger::Thread {
                            main,
                            reply: reply - 1,
                        })
                    }
                },
            },
        }
    }

    /// Look at the next reply of the thread.
    ///
    /// # Errors
    ///
    /// * [`Error::OutOfBounds`] - If the navigation to the next item in the
    ///   thread is out of bounds. This usual means we are at the last item in
    ///   the thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to first comment
    /// thread.reply(String::from("I love rose bushes!"), ReplyTo::Thread);
    ///
    /// thread.navigate_to_root();
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("I love rose trees!"))));
    ///
    /// thread.next_reply(ReplyTo::Thread)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("I love rose bushes!"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn next_reply(&mut self, reply_to: ReplyTo) -> Result<(), Error> {
        match self.finger {
            Finger::Root => self.navigate_to(Finger::Main(0)),
            Finger::Main(main) => match reply_to {
                ReplyTo::Main => self.navigate_to(Finger::Main(main + 1)),
                ReplyTo::Thread => self.navigate_to(Finger::Thread { main, reply: 1 }),
            },
            Finger::Thread { main, reply } => match reply_to {
                ReplyTo::Main => self.navigate_to(Finger::Main(main + 1)),
                ReplyTo::Thread => self.navigate_to(Finger::Thread {
                    main,
                    reply: reply + 1,
                }),
            },
        }
    }

    /// Look at the root of the thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// thread.navigate_to_root();
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("Discussing rose trees"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn navigate_to_root(&mut self) {
        self.finger = ROOT_FINGER;
    }

    /// Absolute navigation to a position in the `Thread` using a [`Finger`].
    ///
    /// * [`Error::OutOfBounds`] - If the navigation to the next item in the
    ///   thread is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Finger, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to first comment
    /// thread.reply(String::from("I love rose bushes!"), ReplyTo::Thread);
    ///
    /// thread.navigate_to_root();
    /// thread.navigate_to(Finger::Thread { main: 0, reply: 1 })?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("I love rose bushes!"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn navigate_to(&mut self, finger: Finger) -> Result<(), Error> {
        match finger {
            Finger::Root => {
                self.finger = finger;
                Ok(())
            },
            Finger::Main(main) => {
                if main > self.main_thread.len() - 1 {
                    return Err(Error::OutOfBounds {
                        attempt: finger,
                        main: Some(self.main_thread.len() - 1),
                        reply: None,
                    });
                }

                self.finger = finger;
                Ok(())
            },
            Finger::Thread { main, reply } => {
                if main > self.main_thread.len() - 1 {
                    return Err(Error::OutOfBounds {
                        attempt: finger,
                        main: Some(self.main_thread.len() - 1),
                        reply: None,
                    });
                }

                let replies = self.index_main(main);

                if reply > replies.len() {
                    return Err(Error::OutOfBounds {
                        attempt: finger,
                        main: Some(self.main_thread.len() - 1),
                        reply: Some(replies.len()),
                    });
                }

                self.finger = finger;
                Ok(())
            },
        }
    }

    /// Reply to the thread. Depending on what type of [`ReplyTo`] value we pass
    /// we will either reply to the main thread or we will reply to the
    /// reply thread.
    ///
    /// Once we have replied we will be pointing to the latest reply, whether it
    /// is on the main thread or the reply thread.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment on the main thread
    /// thread.reply(
    ///     String::from("Did you know rose trees are equivalent to Cofree []?"),
    ///     ReplyTo::Thread
    /// );
    ///
    /// thread.reply(String::from("What should we use them for?"), ReplyTo::Main);
    ///
    /// thread.navigate_to_root();
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("Discussing rose trees"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("I love rose trees!"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("What should we use them for?"))));
    ///
    /// thread.previous_reply(ReplyTo::Main)?;
    /// thread.next_reply(ReplyTo::Thread)?;
    /// assert_eq!(
    ///     thread.view(),
    ///     Ok(&DataState::Live(String::from("Did you know rose trees are equivalent to Cofree []?")))
    /// );
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn reply(&mut self, a: A, reply_to: ReplyTo) {
        match self.finger {
            // TODO: Always replies to main if we're at the root.
            // Is this ok?
            Finger::Root => self.reply_main(a),
            Finger::Main(main) => match reply_to {
                ReplyTo::Main => self.reply_main(a),
                ReplyTo::Thread => self.reply_thread(main, a),
            },
            Finger::Thread { main, .. } => match reply_to {
                ReplyTo::Main => self.reply_main(a),
                ReplyTo::Thread => self.reply_thread(main, a),
            },
        }
    }

    /// Delete the item that we are looking at. This does not remove the item
    /// from the thread but rather marks it as [`DataState::Dead`].
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Error
    ///
    /// Fails with [`Error::DeleteRoot`] if we attempt to delete the first
    /// item in the main thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Thread, ThreadError};
    ///
    /// let mut thread = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment on the main thread
    /// thread.reply(
    ///     String::from("Did you know rose trees are equivalent to Cofree []?"),
    ///     ReplyTo::Thread
    /// );
    ///
    /// thread.reply(String::from("What should we use them for?"), ReplyTo::Main);
    ///
    /// // Delete the last comment on the main thread
    /// thread.delete();
    ///
    /// thread.navigate_to_root();
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("Discussing rose trees"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("I love rose trees!"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&DataState::Dead(String::from("What should we use them for?"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn delete(&mut self) -> Result<(), Error>
    where
        A: Clone,
    {
        match self.finger {
            Finger::Root => Err(Error::DeleteRoot),
            Finger::Main(main) => {
                let node = self.index_main_mut(main).first_mut();
                node.kill();
                Ok(())
            },
            Finger::Thread { main, reply } => {
                let replies = self.index_main_mut(main);
                let node = replies
                    .get_mut(reply)
                    .unwrap_or_else(|| panic!("Reply index is out of bounds: {}", reply));

                node.kill();
                Ok(())
            },
        }
    }

    /// Edit the item we are looking at with the function `f`.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, DataState, Thread, ThreadError};
    ///
    /// let mut thread = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment on the main thread
    /// thread.reply(
    ///     String::from("Did you know rose trees are equivalent to Cofree []?"),
    ///     ReplyTo::Thread
    /// );
    ///
    /// thread.reply(String::from("What should we use them for?"), ReplyTo::Main);
    /// thread.edit(|body| *body = String::from("How can we use them?"));
    ///
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("How can we use them?"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn edit<F>(&mut self, f: F)
    where
        F: FnOnce(&mut A),
    {
        match self.finger {
            Finger::Root => f(self.root.get_mut()),
            Finger::Main(main) => {
                let node = self.index_main_mut(main).first_mut();
                f(node.get_mut());
            },
            Finger::Thread { main, reply } => {
                let replies = self.index_main_mut(main);
                let node = replies
                    .get_mut(reply)
                    .unwrap_or_else(|| panic!("Reply index is out of bounds: {}", reply));
                f(node.get_mut())
            },
        }
    }

    /// Expand the current main thread item we are looking at into the full
    /// non-empty view of items.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    pub fn expand(&self) -> NonEmpty<DataState<A>>
    where
        A: Clone,
    {
        let main = match self.finger {
            Finger::Root => {
                return NonEmpty::from((
                    self.root.clone(),
                    self.main_thread
                        .clone()
                        .iter()
                        .map(|thread| thread.first().clone())
                        .collect(),
                ));
            },
            Finger::Main(main) => main,
            Finger::Thread { main, .. } => main,
        };

        self.index_main(main).0.clone()
    }

    /// Look at the current item we are pointing to in the thread.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::{DataState, Thread};
    ///
    /// let (thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// assert_eq!(thread.view(), Ok(&DataState::Live(String::from("Discussing rose trees"))));
    /// ```
    pub fn view(&self) -> Result<&DataState<A>, Error> {
        match self.finger {
            Finger::Root => Ok(&self.root),
            Finger::Main(main) => Ok(self.index_main(main).first()),
            Finger::Thread { main, reply } => Ok(self
                .index_main(main)
                .get(reply)
                .unwrap_or_else(|| panic!("Reply index is out of bounds: {}", reply))),
        }
    }

    /// Look at the current item we are pointing to in the thread via a mutable
    /// reference.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    pub fn view_mut(&mut self) -> Result<&mut DataState<A>, Error> {
        match self.finger {
            Finger::Root => Ok(&mut self.root),
            Finger::Main(main) => Ok(self.index_main_mut(main).first_mut()),
            Finger::Thread { main, reply } => Ok(self
                .index_main_mut(main)
                .get_mut(reply)
                .unwrap_or_else(|| panic!("Reply index is out of bounds: {}", reply))),
        }
    }

    fn index_main(&self, main: usize) -> &Replies<A> {
        self.main_thread
            .get(main)
            .unwrap_or_else(|| panic!("Main index is out of bounds: {}", main))
    }

    fn index_main_mut(&mut self, main: usize) -> &mut Replies<A> {
        self.main_thread
            .get_mut(main)
            .unwrap_or_else(|| panic!("Main index is out of bounds: {}", main))
    }

    fn reply_main(&mut self, a: A) {
        self.main_thread.push(Replies::new(a));
        self.finger = Finger::Main(self.main_thread.len() - 1);
    }

    fn reply_thread(&mut self, main: usize, a: A) {
        let replies = self.index_main_mut(main);
        replies.reply(a);
        let reply = replies.len() - 1;
        self.finger = Finger::Thread { main, reply };
    }

    // Prune the Dead items from the tree so that we can effectively test
    // the view of deletion compared to another tree that contains the same
    // Live items.
    #[cfg(test)]
    fn prune(&mut self)
    where
        A: Clone,
    {
        let mut thread = vec![];
        for replies in self.main_thread.iter() {
            let live_replies = replies
                .iter()
                .cloned()
                .filter(|node| node.live().is_some())
                .collect::<Vec<DataState<_>>>();

            match NonEmpty::from_slice(&live_replies) {
                None => {},
                Some(r) => thread.push(Replies(r)),
            }
        }

        self.main_thread = thread;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// forall a. Thread::new(a).view() === a
    fn prop_view_of_new<A: Eq + Clone>(a: A) -> bool {
        Thread::new(a.clone()).view() == Ok(&DataState::Live(a))
    }

    /// { new_path = thread.reply(path, comment)?
    ///   thread.delete(new_path)
    /// } === thread
    fn prop_deleting_a_replied_comment_is_noop<A>(
        thread: &mut Thread<A>,
        a: A,
    ) -> Result<bool, Error>
    where
        A: std::fmt::Debug + Clone + PartialEq,
    {
        let old_thread = thread.clone();
        thread.reply(a, ReplyTo::Main);
        thread.delete()?;
        thread.prune();

        Ok(*thread == old_thread)
    }

    /// Thread::new(comment).delete(comment) === None
    fn prop_deleting_root_should_not_be_possible<A: Eq>(a: A) -> bool
    where
        A: Clone,
    {
        Thread::new(a).delete() == Err(Error::DeleteRoot)
    }

    /// Thread::new(comment).edit(f, comment) ===
    /// Thread::new(f(comment).unwrap())
    fn prop_new_followed_by_edit_is_same_as_editing_followed_by_new<A, F>(mut a: A, f: &F) -> bool
    where
        A: Eq + Clone,
        F: Fn(&mut A),
    {
        let mut lhs = Thread::new(a.clone());
        lhs.edit(f);

        f(&mut a);
        let rhs = Thread::new(a);

        lhs == rhs
    }

    /// let (thread, path) = Thread::new(a)
    /// => thread.view(path) == a
    fn prop_root_followed_by_view<A>(a: A) -> bool
    where
        A: Eq + Clone,
    {
        let thread = Thread::new(a.clone());
        *thread.view().unwrap() == DataState::Live(a)
    }

    #[test]
    fn check_view_of_new() {
        assert!(prop_view_of_new("New thread"))
    }

    #[test]
    fn check_root_followed_by_view() {
        assert!(prop_root_followed_by_view("New thread"))
    }

    #[test]
    fn check_deleting_a_replied_comment_is_noop() {
        let mut thread = Thread::new("New thread");
        let result =
            prop_deleting_a_replied_comment_is_noop(&mut thread, "New comment").expect("Error");
        assert!(result);
    }

    #[test]
    fn check_deleting_root_should_not_be_possible() {
        assert!(prop_deleting_root_should_not_be_possible("New thread"))
    }

    #[test]
    fn check_new_followed_by_edit_is_same_as_editing_followed_by_new() {
        assert!(
            prop_new_followed_by_edit_is_same_as_editing_followed_by_new(
                String::from("new thread"),
                &|body| {
                    body.insert_str(0, "edit: ");
                }
            )
        )
    }
}
