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

use crate::ops::{
    sequence::{self, OrdSequence},
    Apply,
};

#[cfg(test)]
use nonempty::NonEmpty;
#[cfg(test)]
use std::iter;

/// We use [`Item`]s as the values of the `Thread`. This allows us to modify the
/// elements and perform soft deletes.
pub mod item;
pub use item::Item;

mod error;
pub use error::Error;

/// A `SubThread` is an [`OrdSequence`] of [`Item`]s.
/// It represents where we replied to the main thread and now has the
/// opportunity to become a thread of items itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubThread<M, A> {
    root: Item<A>,
    replies: OrdSequence<item::Op<M>, Item<A>>,
}

impl<M, A> SubThread<M, A> {
    fn edit_root<F>(&mut self, f: F) -> Result<SubThreadOp<M, A>, Error<A::Error>>
    where
        A: Apply<Op = M>,
        F: FnOnce(&mut A) -> Result<M, A::Error>,
    {
        self.root
            .edit(f)
            .map(SubThreadOp::Root)
            .map_err(Error::MainRoot)
    }

    fn delete_root(&mut self) -> SubThreadOp<M, A> {
        SubThreadOp::Root(self.root.delete())
    }

    fn edit_reply<F>(&mut self, ix: usize, f: F) -> Result<SubThreadOp<M, A>, Error<A::Error>>
    where
        A: Apply<Op = M> + Clone,
        M: Clone,
        F: FnOnce(&mut A) -> Result<M, A::Error>,
    {
        self.replies
            .modify(ix, |item| item.edit(f))
            .map(SubThreadOp::Reply)
            .map_err(Error::MainReply)
    }

    fn delete_reply(&mut self, ix: usize) -> Result<SubThreadOp<M, A>, Error<A::Error>>
    where
        A: Apply<Op = M> + Clone,
        M: Clone,
    {
        self.replies
            .modify(ix, |item| Ok(item.delete()))
            .map(SubThreadOp::Reply)
            .map_err(Error::MainReply)
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &Item<A>> {
        iter::once(&self.root).chain(self.replies.iter().map(|(_, a)| a))
    }
}

impl<M, A> Apply for SubThread<M, A>
where
    A: Apply<Op = M>,
{
    type Op = SubThreadOp<M, A>;
    type Error = Error<A::Error>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            SubThreadOp::Root(op) => self.root.apply(op).map_err(Error::MainRoot),
            SubThreadOp::Reply(op) => self.replies.apply(op).map_err(Error::MainReply),
        }
    }
}

/// `Replies` is the structure that represents the replies to the main thread.
/// Each one of these can is potentially a sub-thread itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replies<M, A>(OrdSequence<SubThreadOp<M, A>, SubThread<M, A>>);

impl<M, A> Default for Replies<M, A> {
    fn default() -> Self {
        Replies::new()
    }
}

impl<M, A> Replies<M, A> {
    fn new() -> Self {
        Replies(OrdSequence::new())
    }

    fn push(&mut self, a: A) -> MainOp<M, A>
    where
        A: Clone,
        M: Clone,
    {
        let thread = SubThread {
            root: Item::new(a),
            replies: OrdSequence::new(),
        };
        self.0.append(thread)
    }

    fn append<E>(&mut self, ix: usize, new: A) -> Result<Op<M, A>, Error<E>>
    where
        A: Clone,
    {
        let thread = &mut self
            .0
            .get_mut(ix)
            .ok_or(sequence::Error::IndexOutOfBounds(ix))
            .map_err(Error::MainReply)?
            .1;

        Ok(Op::Thread {
            main: ix,
            op: SubThreadOp::Reply(thread.replies.append(Item::new(new))),
        })
    }
}

type MainOp<M, A> = sequence::Op<SubThreadOp<M, A>, SubThread<M, A>>;

/// An operation that affects a sub-thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubThreadOp<M, A> {
    /// An operation that affects the root item of a sub-thread.
    Root(item::Op<M>),
    /// An operation that affects one of the replies to a sub-thread.
    Reply(sequence::Op<item::Op<M>, Item<A>>),
}

/// Operations on a [`Thread`] can be performed on any of the items in thread.
/// This structure allows us to focus in on what part of the structure we're
/// operating on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op<M, A> {
    /// `Root` allows us to modify the root item, i.e. the first element of
    /// the thread.
    Root(item::Op<M>),
    /// `Main` allows us to append to the main thread or modify one of its
    /// items.
    Main(MainOp<M, A>),
    /// `Thread` allows us to append to a sub-thread (of the main thread) or
    /// modify one the sub-thread's items.
    Thread {
        /// What main thread did this operation occur on.
        main: usize,
        /// The operation that was applied to the [`SubThread`].
        op: SubThreadOp<M, A>, // sequence::Op<item::Op<M>, Item<A>>,
    },
}

/// A structure for pointing into a [`Thread`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Finger {
    /// This finger points to the root value in a `Thread`.
    Root,
    /// This finger points to a reply within the `Thread`.
    Reply(ReplyFinger),
}

/// A structure for pointing into [`Replies`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReplyFinger {
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

/// A `Thread` consists of a root item, and a series of replies to that item.
/// This consists of the main thread. For each item in the main thread there can
/// be a sub-thread full of items.
///
/// The main operations to interact with the `Thread` are:
/// * [`Thread::append`]
/// * [`Thread::edit`]
/// * [`Thread::delete`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread<M, A> {
    root: Item<A>,
    replies: Replies<M, A>,
}

/// This tells us where we want to append a new item to the [`Thread`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendTo {
    /// Append to the main thread.
    Main,
    /// Append to a given sub-thread. The `usize` is to index into the main
    /// thread.
    Thread(usize),
}

impl<M, A: Apply> Thread<M, A> {
    /// Create a new thread where the supplied element acts as the root of the
    /// `Thread`.
    pub fn new(a: A) -> Self {
        Thread {
            root: Item::new(a),
            replies: Replies::new(),
        }
    }

    /// Append the element to the `Thread`.
    ///
    ///     * If the `AppendTo` value is `Main`, then the element will be
    ///       appended to the main thread.
    ///
    ///     * If the `AppendTo` value is `Thread`, then we find the main thread
    ///       element, and append the element to its replies.
    pub fn append(&mut self, ix: AppendTo, new: A) -> Result<Op<M, A>, Error<A::Error>>
    where
        A: Clone,
        M: Clone,
    {
        match ix {
            AppendTo::Main => Ok(Op::Main(self.replies.push(new))),
            AppendTo::Thread(ix) => self.replies.append(ix, new),
        }
    }

    /// Edit the element of the `Thread` found at the given [`Finger`].
    ///
    /// The [`Op`] returned will be the composition of a modification and the
    /// operation returned by the function.
    pub fn edit<F>(&mut self, finger: Finger, f: F) -> Result<Op<M, A>, Error<A::Error>>
    where
        A: Apply<Op = M> + Clone,
        M: Clone,
        F: FnOnce(&mut A) -> Result<M, A::Error>,
    {
        match finger {
            Finger::Root => {
                let op = self.root.edit(f)?;
                Ok(Op::Root(op))
            },
            Finger::Reply(reply) => match reply {
                ReplyFinger::Main(ix) => self
                    .replies
                    .0
                    .modify(ix, |thread| thread.edit_root(f))
                    .map(Op::Main)
                    .map_err(Error::flatten_main),
                ReplyFinger::Thread { main, reply } => {
                    let thread = &mut self.replies.0[main].1; // TODO: Error handling
                    thread
                        .edit_reply(reply, f)
                        .map(|op| Op::Thread { main, op })
                },
            },
        }
    }

    /// Delete the element of the `Thread` found at the given [`Finger`].
    pub fn delete(&mut self, finger: Finger) -> Result<Op<M, A>, Error<A::Error>>
    where
        A: Apply<Op = M> + Clone,
        M: Clone + Ord,
    {
        match finger {
            Finger::Root => {
                let op = self.root.delete();
                Ok(Op::Root(op))
            },
            Finger::Reply(reply) => match reply {
                ReplyFinger::Main(ix) => self
                    .replies
                    .0
                    .modify(ix, |thread| Ok(thread.delete_root()))
                    .map(Op::Main)
                    .map_err(Error::flatten_main),
                ReplyFinger::Thread { main, reply } => {
                    let thread = &mut self.replies.0[main].1;
                    thread.delete_reply(reply).map(|op| Op::Thread { main, op })
                },
            },
        }
    }

    #[cfg(test)]
    fn to_vec(&self) -> Vec<NonEmpty<Item<A>>>
    where
        A: Clone,
        M: Clone,
    {
        let mut result = vec![NonEmpty::new(self.root.clone())];
        result.append(
            &mut self
                .replies
                .0
                .val
                .iter()
                .cloned()
                .map(|(_, v)| v.iter().cloned().collect::<Vec<_>>())
                .filter_map(|t| NonEmpty::from_slice(&t))
                .collect(),
        );
        result
    }
}

impl<M, A: Apply<Op = M>> Apply for Thread<M, A> {
    type Op = Op<M, A>;
    type Error = Error<A::Error>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Root(modifier) => Ok(self.root.apply(modifier)?),
            Op::Main(op) => {
                let main_thread = &mut self.replies.0;
                main_thread.apply(op).map_err(Error::flatten_main)
            },
            Op::Thread { main, op } => {
                let thread = &mut self
                    .replies
                    .0
                    .get_mut(main)
                    .ok_or(sequence::Error::IndexOutOfBounds(main))
                    .map_err(Error::MainReply)?
                    .1;
                thread.apply(op)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{convert::Infallible, error};

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Int(u32);

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct IntOp(u32);

    impl Apply for Int {
        type Op = IntOp;
        type Error = Infallible;

        fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
            self.0 += op.0;
            Ok(())
        }
    }

    type TestResult = Result<(), Box<dyn error::Error + 'static>>;

    #[test]
    fn sync_appends() -> TestResult {
        let expected = vec![
            NonEmpty::new(Item::new(Int(1))),
            NonEmpty::new(Item::new(Int(2))),
            NonEmpty::new(Item::new(Int(3))),
        ];

        let mut left = Thread::new(Int(1));
        let append1 = left.append(AppendTo::Main, Int(2))?;
        let append2 = left.append(AppendTo::Main, Int(3))?;

        let mut right = Thread::new(Int(1));
        right.apply(append1)?;
        right.apply(append2)?;

        assert_eq!(left.to_vec(), expected);
        assert_eq!(right.to_vec(), expected);
        assert_eq!(left, right);

        Ok(())
    }

    /*
    #[test]
    fn commutative_ints_with_edit() {
        let append1 = Op::main_append(0, Int(2));
        let append2 = Op::main_append(1, Int(3));
        let edit = Op::main_edit(1, IntOp(42));

        let expected = vec![
            NonEmpty::new(Item::new(Int(1))),
            NonEmpty::new(Item::new(Int(2))),
            NonEmpty::new(Item::new(Int(45))),
        ];

        let mut left = Thread::new(Int(1));
        left.apply(append1.clone());
        left.apply(append2.clone());
        left.apply(edit.clone());

        let mut right = Thread::new(Int(1));
        right.apply(edit);
        right.apply(append2);
        right.apply(append1);

        assert_eq!(left.to_vec(), expected);
        assert_eq!(right.to_vec(), expected);
        assert_eq!(left, right);
    }
    */
}
