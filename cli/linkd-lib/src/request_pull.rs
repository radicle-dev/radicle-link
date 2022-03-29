// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

use thiserror::Error;

use librad::{
    git::{storage, tracking, Urn},
    net::protocol::request_pull::Guard,
    PeerId,
};

use crate::tracking::Tracker;

#[derive(Clone)]
pub struct State {
    storage: storage::Pool<storage::Storage>,
    tracker: Option<Tracker>,
}

impl State {
    pub fn new(
        storage: storage::Pool<storage::Storage>,
        tracker: impl Into<Option<Tracker>>,
    ) -> Self {
        State {
            storage,
            tracker: tracker.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to access storage for tracking")]
    Init(#[from] storage::PoolError),
    #[error(transparent)]
    IsTracked(#[from] tracking::error::IsTracked),
    #[error(transparent)]
    Track(#[from] tracking::error::Track),
    #[error("`{0}` was rejected")]
    Rejected(Urn),
}

pub struct Tracked {
    tracked: Option<Result<tracking::Ref, tracking::PreviousError>>,
    urn: Urn,
}

impl fmt::Display for Tracked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.tracked {
            None => write!(f, "Already tracking `{}`", self.urn),
            Some(Ok(reference)) => write!(f, "Tracked `{}`", reference.name),
            Some(Err(previous)) => write!(f, "Attempted to track `{}`: {}", self.urn, previous),
        }
    }
}

impl Guard for State {
    type Error = Error;

    type Output = Tracked;

    fn guard(&self, peer: &PeerId, urn: &Urn) -> Result<Self::Output, Self::Error> {
        match &self.tracker {
            Some(tracker) => {
                if tracker.guard(peer, urn).unwrap() {
                    let storage = futures::executor::block_on(self.storage.get())?;
                    if !tracking::is_tracked(storage.as_ref(), urn, Some(*peer))? {
                        let tracked = tracking::track(
                            storage.as_ref(),
                            urn,
                            Some(*peer),
                            tracking::Config::default(),
                            tracking::policy::Track::MustNotExist,
                        )?;
                        Ok(Tracked {
                            tracked: Some(tracked),
                            urn: urn.clone(),
                        })
                    } else {
                        Ok(Tracked {
                            tracked: None,
                            urn: urn.clone(),
                        })
                    }
                } else {
                    Err(Error::Rejected(urn.clone()))
                }
            },
            None => Err(Error::Rejected(urn.clone())),
        }
    }
}
