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

#![allow(missing_docs, unused_import_braces, unused_qualifications, warnings)]

use crate::thread;

pub trait Editor<A> {
    fn make_edit(self, a: &mut A);
}

pub enum Thread<F: Editor<A>, A> {
    New(A),
    Reply(thread::Finger, thread::ReplyTo, A, Box<Thread<F, A>>),
    Delete(thread::Finger, Box<Thread<F, A>>),
    Edit(thread::Finger, F, Box<Thread<F, A>>),
}

// TODO(fintan) thread::Thread should output _this_ Thread so that they can be
// interpreted by the original functions, and other functions such for
// outputting operations.
impl<F, A> Thread<F, A>
where
    F: Editor<A>,
{
    pub fn eval(self) -> Result<thread::Thread<A>, thread::Error> {
        match self {
            Thread::New(a) => Ok(thread::Thread::new(a)),
            Thread::Reply(finger, reply_to, a, thread) => {
                let mut thread = thread.eval()?;
                thread.navigate_to(finger)?;
                thread.reply(a, reply_to);
                Ok(thread)
            },
            Thread::Delete(finger, thread) => {
                let mut thread = thread.eval()?;
                thread.navigate_to(finger)?;
                thread.delete()?;
                Ok(thread)
            },
            Thread::Edit(finger, editor, thread) => {
                let mut thread = thread.eval()?;
                thread.navigate_to(finger)?;
                thread.edit(|a| editor.make_edit(a));
                Ok(thread)
            },
        }
    }
}
