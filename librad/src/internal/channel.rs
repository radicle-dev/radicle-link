use std::{
    mem,
    sync::{Arc, Mutex},
};

use futures::{channel::mpsc, sink::SinkExt, stream::StreamExt, Stream};

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

    pub fn subscribe(&self) -> impl Stream<Item = A> {
        let (tx, rx) = mpsc::unbounded();
        self.subscribers.lock().unwrap().push(tx);
        rx
    }

    pub async fn emit(&self, event: A) {
        let mut subscribers = self.subscribers.lock().unwrap();

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
