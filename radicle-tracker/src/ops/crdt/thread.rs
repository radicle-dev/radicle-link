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

#![allow(missing_docs, warnings)]

use crdts::{
    self,
    lseq::{self, LSeq},
    vclock::Actor,
};

use crate::ops::thread::ThreadOp;

use crate::thread::{DataState, Finger, ReplyTo};

type SubThread<A: Clone, User: Actor> = LSeq<DataState<A>, User>;

struct MainThread<A: Clone, User: Actor>(LSeq<SubThread<A, User>, User>);

impl<A: Clone, User: Actor> MainThread<A, User> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn get_main(&self, main: usize) -> Result<&DataState<A>, ()> {
        self.get_thread(main, 0)
    }

    fn get_thread(&self, main_ix: usize, thread_ix: usize) -> Result<&DataState<A>, ()> {
        let thread = &self.0.raw_entries().get(main_ix).ok_or(())?.val;
        let val = &thread.raw_entries().get(thread_ix).ok_or(())?.val;
        Ok(val)
    }

    fn reply_main(&mut self, user: User, a: A) -> Op<A, User> {
        let ix = self.0.len();
        let mut new_thread = LSeq::new(user);
        let op = new_thread.insert_index(0, DataState::Live(a));

        Op::ReplyMain {
            create: op,
            insert: self.0.insert_index(ix, new_thread),
        }
    }

    // TODO: This is a hack because there's no "edit" on an LSeq, so we need to
    // delete the thread and insert the new thread.
    fn reply_thread(&mut self, main_ix: usize, user: User, a: A) -> Op<A, User> {
        let mut sub_thread = self.0.raw_entries().get(main_ix).unwrap().val.clone();
        let delete = self.0.delete_index(main_ix).unwrap();
        let ix = self.0.len();
        let op = sub_thread.insert_index(ix, DataState::Live(a));
        let insert = self.0.insert_index(main_ix, sub_thread);

        Op::ReplyThread {
            edit_delete: delete,
            edit_insert: insert,
            insert: op,
        }
    }
}

pub struct Thread<A: Clone, User: Actor> {
    user: User,
    root: DataState<A>,
    main_thread: MainThread<A, User>,
}

pub enum Op<A, User: Actor> {
    /// Since this is a hack for editing, the deletion should happen before
    /// the insertion.
    ReplyThread {
        edit_delete: lseq::Op<LSeq<DataState<A>, User>, User>,
        edit_insert: lseq::Op<LSeq<DataState<A>, User>, User>,
        insert: lseq::Op<DataState<A>, User>,
    },
    ReplyMain {
        create: lseq::Op<DataState<A>, User>,
        insert: lseq::Op<LSeq<DataState<A>, User>, User>,
    },
}

impl<A: Clone, User: Actor> ThreadOp<A, Op<A, User>> for Thread<A, User> {
    type Error = ();

    fn reply(
        &mut self,
        finger: Finger,
        a: A,
        reply_to: ReplyTo,
    ) -> Result<Op<A, User>, Self::Error> {
        match finger {
            Finger::Root => Ok(self.main_thread.reply_main(self.user.clone(), a)),
            Finger::Main(main) => Ok(self.main_thread.reply_main(self.user.clone(), a)),
            Finger::Thread { main, reply } => unimplemented!(),
        }
    }

    fn delete(&mut self, finger: Finger) -> Result<Op<A, User>, Self::Error> {
        unimplemented!()
    }

    fn edit<F: FnOnce(&mut A)>(
        &mut self,
        finger: Finger,
        f: F,
    ) -> Result<Op<A, User>, Self::Error> {
        unimplemented!()
    }

    fn view(&mut self, finger: Finger) -> Result<&DataState<A>, Self::Error> {
        match finger {
            Finger::Root => Ok(&self.root),
            Finger::Main(main) => self.main_thread.get_main(main),
            Finger::Thread { main, reply } => self.main_thread.get_thread(main, reply),
        }
    }
}
