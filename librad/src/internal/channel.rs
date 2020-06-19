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

//! A simple async single-producer multi-consumer channel

use std::sync::Arc;

use futures::{channel::mpsc, lock::Mutex, sink::SinkExt};

#[derive(Clone, Default)]
pub struct Fanout<A> {
    subscribers: Arc<Mutex<Vec<mpsc::UnboundedSender<A>>>>,
}

impl<A: Clone> Fanout<A> {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<A> {
        let (tx, rx) = mpsc::unbounded();
        self.subscribers.lock().await.push(tx);
        rx
    }

    pub async fn emit(&self, event: A) {
        let mut subscribers = self.subscribers.lock().await;

        // Copy&Pasta of `std::vec::Vec::retain` to support an async predicate.
        //
        // We simply move all sender channels which don't have a receiving end
        // (ie. `send` fails) to the end of the vector, and then truncate it to
        // the number of alive channels.
        let len = subscribers.len();
        let mut del = 0;
        {
            let v = &mut **subscribers;
            for i in 0..len {
                let mut ch = &v[i];
                let keep = ch.send(event.clone()).await.is_ok();

                if !keep {
                    del += 1;
                } else if del > 0 {
                    v.swap(i - del, i)
                }
            }
        }
        if del > 0 {
            subscribers.truncate(len - del)
        }
    }
}
