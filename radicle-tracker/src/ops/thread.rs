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
    absurd,
    id::{Gen, UniqueTimestamp},
    sequence::{self, OrdSequence},
    visibility::Hide,
    Apply,
};
use std::ops::{Deref, DerefMut};

#[cfg(test)]
use nonempty::NonEmpty;

mod item;
pub use item::{Item, Modifier};

mod error;
pub use error::Error;

/// A `SubThread` is an [`Appendage`] of `NonEmpty` [`Item`]s.
/// It represents where we replied to the main thread and now has the
/// opportunity to become a thread of items itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubThread<M, A>(OrdSequence<Modifier<M>, UniqueTimestamp, Item<A>>);

impl<M, T> Deref for SubThread<M, T> {
    type Target = OrdSequence<Modifier<M>, UniqueTimestamp, Item<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M, T> DerefMut for SubThread<M, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

type SubThreadOp<M, A> = sequence::Op<Modifier<M>, UniqueTimestamp, Item<A>>;

impl<M, A> Apply for SubThread<M, A>
where
    A: Apply<Op = M>,
{
    type Op = SubThreadOp<M, A>;
    type Error = sequence::Error<A::Error>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        self.0.apply(op)
    }
}

/// `Replies` is the structure that represents the replies to the main thread.
/// Each one of these can is potentially a sub-thread itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replies<M, A>(OrdSequence<SubThreadOp<M, A>, UniqueTimestamp, SubThread<M, A>>);

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
        let mut thread = SubThread(OrdSequence::new());
        thread.append(Item::new(a));
        self.0.append(thread)
    }

    fn append<E>(&mut self, ix: usize, new: A) -> Result<Op<M, A>, Error<E>>
    where
        A: Clone,
    {
        let thread = &mut self
            .0
            .get_mut(ix)
            .ok_or(Error::Thread(sequence::Error::IndexOutOfBounds(ix)))?
            .1;

        Ok(Op::Thread {
            main: ix,
            op: thread.append(Item::new(new)),
        })
    }
}

type MainOp<M, A> = sequence::Op<
    sequence::Op<Modifier<M>, UniqueTimestamp, Item<A>>,
    UniqueTimestamp,
    SubThread<M, A>,
>;

/// Operations on a [`Thread`] can be performed on any of the items in thread.
/// This structure allows us to focus in on what part of the structure we're
/// operating on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op<M, A> {
    /// `Root` allows us to modify the root item, i.e. the first element of
    /// the thread.
    Root(Modifier<M>),
    /// `Main` allows us to append to the main thread or modify one of its
    /// items.
    Main(MainOp<M, A>),
    /// `Thread` allows us to append to a sub-thread (of the main thread) or
    /// modify one the sub-thread's items.
    Thread {
        main: usize,
        op: sequence::Op<Modifier<M>, UniqueTimestamp, Item<A>>,
    },
}

impl<M, A> Op<M, A> {
    fn root_modifier(m: Modifier<M>) -> Self {
        Op::Root(m)
    }

    fn root_edit(e: M) -> Self {
        Self::root_modifier(Modifier::Edit(e))
    }

    fn root_delete() -> Self {
        Self::root_modifier(Modifier::Delete(Hide {}))
    }
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
    // TODO: there should be ops that tell us how the structure was initialised as
    // well.
    pub fn new(a: A) -> Self {
        Thread {
            root: Item::new(a),
            replies: Replies::new(),
        }
    }

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

    pub fn edit(&mut self, finger: Finger, op: M) -> Result<Op<M, A>, Error<A::Error>>
    where
        A: Apply<Op = M> + Clone + Ord,
        M: Clone,
    {
        match finger {
            Finger::Root => {
                self.root.val.apply(op.clone())?;
                Ok(Op::root_edit(op))
            },
            Finger::Reply(reply) => match reply {
                ReplyFinger::Main(ix) => self
                    .replies
                    .0
                    .modify(
                        ix,
                        sequence::Op::Modify {
                            id: UniqueTimestamp::gen(),
                            ix: 0,
                            op: Modifier::Edit(op),
                        },
                    )
                    .map(Op::Main)
                    .map_err(Error::flatten_main),
                ReplyFinger::Thread { main, reply } => {
                    let thread = &mut self.replies.0[main].1;
                    thread
                        .modify(reply, Modifier::Edit(op))
                        .map(|op| Op::Thread { main, op })
                        .map_err(Error::Thread)
                },
            },
        }
    }

    pub fn delete(&mut self, finger: Finger) -> Result<Op<M, A>, Error<A::Error>>
    where
        A: Apply<Op = M> + Clone + Ord,
        M: Clone + Ord,
    {
        match finger {
            Finger::Root => {
                self.root
                    .visibility
                    .apply(Hide {})
                    .map_err(absurd::<Error<A::Error>>)?;
                Ok(Op::root_delete())
            },
            Finger::Reply(reply) => match reply {
                ReplyFinger::Main(ix) => self
                    .replies
                    .0
                    .modify(
                        ix,
                        sequence::Op::Modify {
                            id: UniqueTimestamp::gen(),
                            ix: 0,
                            op: Modifier::Delete(Hide {}),
                        },
                    )
                    .map(Op::Main)
                    .map_err(Error::flatten_main),
                ReplyFinger::Thread { main, reply } => {
                    let thread = &mut self.replies.0[main].1;
                    thread
                        .modify(reply, Modifier::Delete(Hide {}))
                        .map(|op| Op::Thread { main, op })
                        .map_err(Error::Thread)
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
                .map(|(_, v)| v.iter().cloned().map(|(_, v)| v).collect::<Vec<_>>())
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
                    .map_err(Error::Main)?
                    .1;
                thread.apply(op).map_err(Error::Thread)
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
