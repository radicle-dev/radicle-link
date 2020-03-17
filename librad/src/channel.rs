//! A simple async single-producer multi-consumer channel

use std::{mem, sync::Arc};

use futures::{channel::mpsc, lock::Mutex, sink::SinkExt, stream::StreamExt};

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

        // Why is there no `retain` on streams?
        let subscribers1: Vec<_> = futures::stream::iter(subscribers.iter_mut())
            .filter_map(|ch| {
                let event = event.clone();
                async move {
                    if ch.send(event).await.is_err() {
                        Some(ch.clone())
                    } else {
                        None
                    }
                }
            })
            .collect()
            .await;

        mem::replace(&mut *subscribers, subscribers1);
    }
}
