// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
};

use futures::{future::FusedFuture, lock::Mutex};

/// A simple monitor future.
///
/// The value of type `A` can be `put` exactly one. The future will resolve once
/// the value has been set.
#[derive(Clone, Default)]
pub struct Monitor<A> {
    val: Arc<Mutex<Option<A>>>,
    is_set: Arc<RwLock<bool>>,
}

impl<A> Monitor<A> {
    pub fn new() -> Self {
        Self {
            val: Arc::new(Mutex::new(None)),
            is_set: Arc::new(RwLock::new(false)),
        }
    }

    /// Write a value of type `A` to the monitor variable.
    ///
    /// If `put` was called before, this resolves to `false`, otherwise `true`.
    pub async fn put(&self, val: A) -> bool {
        let mut var = self.val.lock().await;
        let was_none = var.is_none();
        if was_none {
            *var = Some(val);
            *self.is_set.write().unwrap() = true;
        }

        was_none
    }
}

impl<A: Clone> Future for Monitor<A> {
    type Output = A;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match Pin::into_inner(self).val.try_lock() {
            None => Poll::Pending,
            Some(var) => var
                .as_ref()
                .map(|val| Poll::Ready(val.clone()))
                .unwrap_or(Poll::Pending),
        }
    }
}

impl<A: Clone> FusedFuture for Monitor<A> {
    fn is_terminated(&self) -> bool {
        *self.is_set.read().unwrap()
    }
}
