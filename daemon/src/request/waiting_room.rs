// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! The black box tracker of [`Request`]s and their lifecycles.

// I reserve the right to not match all the arms when picking out a single case, thank you very
// much.
#![allow(clippy::wildcard_enum_match_arm)]

use std::{
    cmp::PartialOrd,
    collections::HashMap,
    convert::TryFrom,
    ops::{Add, Mul},
};

use either::Either;
use serde::{Deserialize, Serialize};

use librad::{
    git::{identities::Revision, Urn},
    peer::PeerId,
};

use super::event::Event;
use crate::request::{Clones, Queries, Request, RequestState, SomeRequest, Status};

/// The maximum number of query attempts that can be made for a single request.
const MAX_QUERIES: Queries = Queries::Infinite;

/// The maximum number of clone attempts that can be made for a single request.
const MAX_CLONES: Clones = Clones::Infinite;

/// An error that can occur when interacting with the [`WaitingRoom`] API.
#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum Error {
    /// When looking up a `Urn` in the [`WaitingRoom`] it was missing.
    #[error("the URN '{0}' was not found in the waiting room")]
    MissingUrn(Urn),

    /// When performing an operation on the a [`Request`] in the [`WaitingRoom`]
    /// it was found to be in the wrong state for the desired operation.
    ///
    /// For example, if we tried to call `cloning` on a request that has only
    /// been created then this would be an invalid transition.
    #[error("the state fetched '{0}' from the waiting room was not one of the expected states")]
    StateMismatch(RequestState),
}

/// Holds either the newly created request or the request already present for
/// the requested urn.
pub type Created<T> = Either<SomeRequest<T>, SomeRequest<T>>;

/// A `WaitingRoom` knows about a set of `Request`s that have been made, and can
/// look them up via their `Urn`.
///
/// It keeps track of these states as the user tells the waiting room what is
/// happening to the request on the outside.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WaitingRoom<T, D> {
    /// The set of requests keyed by their `Urn`. This helps us keep only unique
    /// requests in the waiting room.
    #[serde(bound = "T: serde_millis::Milliseconds")]
    requests: HashMap<Revision, SomeRequest<T>>,

    /// The configuration of the waiting room.
    config: Config<D>,
}

/// The `Config` for the waiting room tells it what are the maximum number of
/// query and clone attempts that can be made for a single request.
///
/// The recommended approach to initialising the `Config` is to use its
/// `Default` implementation, i.e. `Config::default()`, followed by setting the
/// `delta`, since the usual default values for number values are `0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config<D> {
    /// The maximum number of query attempts that can be made.
    pub max_queries: Queries,
    /// The maximum number of clone attempts that can be made.
    pub max_clones: Clones,
    /// The minimum elapsed time between some provided time and a request's
    /// timestamp. For example, if we had the following setup:
    ///   * `delta = 1`
    ///   * `now = 3`
    ///   * `request.timestamp = 2`
    /// then the `delta` would be compared against `now - request.timestamp`.
    pub delta: D,
}

impl<D> Default for Config<D>
where
    D: Default,
{
    fn default() -> Self {
        Self {
            max_queries: MAX_QUERIES,
            max_clones: MAX_CLONES,
            delta: D::default(),
        }
    }
}

/// A transition in the waiting room and any result the waiting room produced
#[derive(Debug, PartialEq)]
pub struct TransitionWithResult<T, R> {
    /// The event that caused the state change
    event: Event,
    /// The time  the change occurred
    timestamp: T,
    /// The state before the change
    state_before: HashMap<Revision, SomeRequest<T>>,
    /// The state after the change
    state_after: HashMap<Revision, SomeRequest<T>>,
    /// The result the waiting room produced
    pub result: R,
}

impl<T, R> TransitionWithResult<T, R> {
    /// A description of the transition that happened without the attached
    /// result
    #[allow(clippy::missing_const_for_fn)]
    pub fn transition(self) -> Transition<T> {
        Transition {
            state_before: self.state_before,
            state_after: self.state_after,
            event: self.event,
            timestamp: self.timestamp,
        }
    }
}

/// A state change in the waiting room
#[derive(Debug, PartialEq, Clone)]
pub struct Transition<T> {
    /// The event that caused the state change
    pub event: Event,
    /// The time  the change occurred
    pub timestamp: T,
    /// The state before the change
    pub state_before: HashMap<Revision, SomeRequest<T>>,
    /// The state after the change
    pub state_after: HashMap<Revision, SomeRequest<T>>,
}

impl<T, D> WaitingRoom<T, D> {
    /// Initialise a new `WaitingRoom` with the supplied `config`.
    #[must_use]
    pub fn new(config: Config<D>) -> Self {
        Self {
            requests: HashMap::new(),
            config,
        }
    }

    /// Check that the `WaitingRoom` has the given `urn`.
    pub fn has(&self, urn: &Urn) -> bool {
        self.requests.contains_key(&urn.id)
    }

    /// Get the underlying [`SomeRequest`] for the given `urn`.
    ///
    /// Returns `None` if there is no such request.
    #[must_use]
    pub fn get(&self, urn: &Urn) -> Option<&SomeRequest<T>> {
        self.requests.get(&urn.id)
    }

    /// Permanently remove a request from the `WaitingRoom`. If the `urn` did
    /// exist in the `WaitingRoom` then the request will be returned.
    ///
    /// Otherwise, it will return `None` if no such request existed.
    pub fn remove(
        &mut self,
        urn: &Urn,
        timestamp: T,
    ) -> TransitionWithResult<T, Option<SomeRequest<T>>>
    where
        T: Clone,
    {
        let req_before = self.requests.clone();
        let result = self.requests.remove(&urn.id);
        let req_after = self.requests.clone();
        TransitionWithResult {
            state_before: req_before,
            state_after: req_after,
            timestamp,
            result,
            event: Event::Removed { urn: urn.clone() },
        }
    }

    /// This will return the request for the given `urn` if one exists in the
    /// `WaitingRoom`.
    ///
    /// If there is no such `urn` then it create a fresh `Request` using the
    /// `urn` and `timestamp` and it will return `None`.
    pub fn request(
        &mut self,
        urn: &Urn,
        timestamp: T,
    ) -> Either<TransitionWithResult<T, SomeRequest<T>>, SomeRequest<T>>
    where
        T: Clone,
    {
        match self.get(urn) {
            None => {
                let state_before = self.requests.clone();
                let request = SomeRequest::Created(Request::new(urn.clone(), timestamp.clone()));
                self.requests.insert(urn.id, request.clone());
                Either::Left(TransitionWithResult {
                    timestamp,
                    state_before,
                    state_after: self.requests.clone(),
                    result: request,
                    event: Event::Created { urn: urn.clone() },
                })
            },
            Some(request) => Either::Right(request.clone()),
        }
    }

    /// Transition the `Request` found at the provided `urn` and call the
    /// transition function to move it into its `Next` state.
    ///
    /// # Errors
    ///
    ///   * If the `urn` was not in the `WaitingRoom`.
    ///   * If the underlying `Request` was not in the expected state.
    ///   * If the transition function supplied returns an error.
    fn transition<Prev, Next>(
        &mut self,
        matcher: impl FnOnce(SomeRequest<T>) -> Option<Prev>,
        transition: impl FnOnce(Prev) -> (Next, Event),
        urn: &Urn,
    ) -> Result<TransitionWithResult<T, ()>, Error>
    where
        T: Clone,
        Prev: Clone,
        Next: Into<SomeRequest<T>> + Clone,
    {
        match self.get(urn) {
            None => Err(Error::MissingUrn(urn.clone())),
            Some(request) => {
                let state_before = self.requests.clone();
                match request.clone().transition(matcher, transition) {
                    Either::Right((next, event)) => {
                        let req = next.into();
                        let t = req.timestamp().clone();
                        self.requests.insert(urn.id, req);
                        Ok(TransitionWithResult {
                            state_before,
                            state_after: self.requests.clone(),
                            event,
                            timestamp: t,
                            result: (),
                        })
                    },
                    Either::Left(mismatch) => Err(Error::StateMismatch((&mismatch).into())),
                }
            },
        }
    }

    /// Create a transition where the before and after state are the same
    pub fn tick(&self, timestamp: T) -> Transition<T>
    where
        T: Clone,
    {
        Transition {
            state_before: self.requests.clone(),
            state_after: self.requests.clone(),
            event: Event::Tick,
            timestamp,
        }
    }

    /// Tell the `WaitingRoom` that a query was made for the given `urn`.
    ///
    /// If the underlying `Request` was in the `Created` state then it will
    /// transition to the `IsRequested` state.
    ///
    /// If the underlying `Request` was in the `IsRequested` state then it
    /// increments the query attempt.
    ///
    /// # Errors
    ///
    ///   * If the `urn` was not in the `WaitingRoom`.
    ///   * If the underlying `Request` was not in the expected state.
    ///   * If the underlying `Request` timed out.
    pub fn queried(&mut self, urn: &Urn, timestamp: T) -> Result<TransitionWithResult<T, ()>, Error>
    where
        T: Clone,
    {
        let max_queries = self.config.max_queries;
        let max_clones = self.config.max_clones;
        self.transition(
            |request| match request {
                SomeRequest::Created(request) => Some(Either::Right(request.request(timestamp))),
                SomeRequest::Requested(request) => {
                    Some(request.queried(max_queries, max_clones, timestamp))
                },
                _ => None,
            },
            |previous| match &previous {
                Either::Left(r) => (
                    previous.clone(),
                    Event::TimedOut {
                        urn: urn.clone(),
                        attempts: r.attempts,
                    },
                ),
                Either::Right(_) => (previous.clone(), Event::Queried { urn: urn.clone() }),
            },
            urn,
        )
    }

    /// Tell the `WaitingRoom` that a `peer` was found for the given `urn`.
    ///
    /// If the underlying `Request` was in the `IsRequested` state then it will
    /// transition to the `Found` state.
    ///
    /// If the underlying `Request` was in the `Found` or `Cloning` state then
    /// it add this `peer` to the set of found peers.
    ///
    /// # Errors
    ///
    ///   * If the `urn` was not in the `WaitingRoom`.
    ///   * If the underlying `Request` was not in the expected state.
    pub fn found(
        &mut self,
        urn: &Urn,
        remote_peer: PeerId,
        timestamp: T,
    ) -> Result<TransitionWithResult<T, ()>, Error>
    where
        T: Clone,
    {
        self.transition(
            |request| match request {
                SomeRequest::Requested(request) => {
                    Some(request.into_found(remote_peer, timestamp).into())
                },
                SomeRequest::Found(request) => {
                    let some_request: SomeRequest<T> = request.found(remote_peer, timestamp).into();
                    Some(some_request)
                },
                SomeRequest::Cloning(request) => {
                    let some_request: SomeRequest<T> = request.found(remote_peer, timestamp).into();
                    Some(some_request)
                },
                _ => None,
            },
            |prev| {
                (
                    prev,
                    Event::Found {
                        urn: urn.clone(),
                        peer: remote_peer,
                    },
                )
            },
            urn,
        )
    }

    /// Tell the `WaitingRoom` that we are attempting a clone from the `peer`
    /// for the given `urn`.
    ///
    /// If the underlying `Request` was in the `Found` state then it will
    /// transition to the `Cloning` state.
    ///
    /// # Errors
    ///
    ///   * If the `urn` was not in the `WaitingRoom`.
    ///   * If the underlying `Request` was not in the expected state.
    ///   * If the underlying `Request` timed out.
    pub fn cloning(
        &mut self,
        urn: &Urn,
        remote_peer: PeerId,
        timestamp: T,
    ) -> Result<TransitionWithResult<T, ()>, Error>
    where
        T: Clone,
    {
        let max_queries = self.config.max_queries;
        let max_clones = self.config.max_clones;
        self.transition(
            |request| match request {
                SomeRequest::Found(request) => Some(request),
                _ => None,
            },
            |previous| {
                let next = previous.cloning(max_queries, max_clones, remote_peer, timestamp);
                match next {
                    Either::Left(ref r) => (
                        next.clone(),
                        Event::TimedOut {
                            urn: urn.clone(),
                            attempts: r.attempts,
                        },
                    ),
                    Either::Right(_) => (
                        next,
                        Event::Cloning {
                            urn: urn.clone(),
                            peer: remote_peer,
                        },
                    ),
                }
            },
            urn,
        )
    }

    /// Tell the `WaitingRoom` that we failed the attempt to clone from the
    /// `peer` for the given `urn`.
    ///
    /// If the underlying `Request` was in the `Cloning` state then it will
    /// transition to the `Found` state.
    ///
    /// # Errors
    ///
    ///   * If the `urn` was not in the `WaitingRoom`.
    ///   * If the underlying `Request` was not in the expected state.
    pub fn cloning_failed(
        &mut self,
        urn: &Urn,
        remote_peer: PeerId,
        timestamp: T,
        reason: String,
    ) -> Result<TransitionWithResult<T, ()>, Error>
    where
        T: Clone,
    {
        self.transition(
            |request| match request {
                SomeRequest::Cloning(request) => Some(request),
                _ => None,
            },
            |previous| {
                (
                    previous.failed(remote_peer, timestamp),
                    Event::CloningFailed {
                        urn: urn.clone(),
                        peer: remote_peer,
                        reason,
                    },
                )
            },
            urn,
        )
    }

    /// Tell the `WaitingRoom` that we successfully cloned the given `urn`.
    ///
    /// If the underlying `Request` was in the `Cloning` state then it will
    /// transition to the `Cloned` state.
    ///
    /// # Errors
    ///
    ///   * If the `urn` was not in the `WaitingRoom`.
    ///   * If the underlying `Request` was not in the expected state.
    pub fn cloned(
        &mut self,
        urn: &Urn,
        remote_peer: PeerId,
        timestamp: T,
    ) -> Result<TransitionWithResult<T, ()>, Error>
    where
        T: Clone,
    {
        self.transition(
            |request| match request {
                SomeRequest::Cloning(request) => Some(request),
                _ => None,
            },
            |previous| {
                (
                    previous.cloned(remote_peer, timestamp),
                    Event::Cloned {
                        urn: urn.clone(),
                        peer: remote_peer,
                    },
                )
            },
            urn,
        )
    }

    /// Tell the `WaitingRoom` that we are cancelling the request for the given
    /// `urn`.
    ///
    /// If the underlying `Request` was in the `{Created, IsRequested, Found,
    /// Cloning, Cancelled}` state then it will transition to the
    /// `Cancelled` state.
    ///
    /// # Errors
    ///
    ///   * If the `urn` was not in the `WaitingRoom`.
    ///   * If the underlying `Request` was not in the expected state.
    pub fn canceled(
        &mut self,
        urn: &Urn,
        timestamp: T,
    ) -> Result<TransitionWithResult<T, ()>, Error>
    where
        T: Clone,
    {
        self.transition(
            |request| request.cancel(timestamp).right(),
            |prev| (prev, Event::Canceled { urn: urn.clone() }),
            urn,
        )
    }

    /// Return the list of all `Urn`/`SomeRequest` pairs in the `WaitingRoom`.
    pub fn iter(&self) -> impl Iterator<Item = (Urn, &SomeRequest<T>)> {
        self.requests
            .iter()
            .map(|(id, request)| (Urn::new(*id), request))
    }

    /// Filter the `WaitingRoom` by:
    ///   * Choosing which [`RequestState`] you are looking for
    pub fn filter_by_state(
        &self,
        request_state: RequestState,
    ) -> impl Iterator<Item = (Urn, &SomeRequest<T>)> {
        self.iter()
            .filter(move |(_, request)| RequestState::from(*request) == request_state.clone())
    }

    /// Find the first occurring request based on the call to
    /// [`WaitingRoom::filter_by_state`].
    pub fn find_by_state(&self, request_state: RequestState) -> Option<(Urn, &SomeRequest<T>)> {
        self.filter_by_state(request_state).next()
    }

    /// Get the next `Request` that is in a query state, i.e. `Created` or
    /// `Requested`.
    ///
    /// In the case of the `Requested` state we check if:
    ///   * The request is a fresh request that hasn't had an attempt to clone
    ///     yet
    ///   * Or the elapsed time between the `timestamp` and the `Request`'s
    ///     timestamp is greater than the `delta` provided in the [`Config`].
    pub fn next_query(&self, timestamp: T) -> Option<Urn>
    where
        T: Add<D, Output = T> + PartialOrd + Clone,
        D: Mul<u32, Output = D> + Ord + Clone,
    {
        let backoff = |tries: Queries| match tries {
            Queries::Max(i) => self.config.delta.clone() * u32::try_from(i).unwrap_or(u32::MAX),
            Queries::Infinite => self.config.delta.clone(),
        };
        let created = self.find_by_state(RequestState::Created);
        let requested = self
            .filter_by_state(RequestState::Requested)
            .find(move |(_, request)| {
                request.timestamp().clone() + backoff(request.attempts().queries) <= timestamp
            });

        created.or(requested).map(|(urn, _request)| urn)
    }

    /// Get the next `Request` that is in the the `Found` state and the status
    /// of the peer is `Available`.
    pub fn next_clone(&self) -> Option<(Urn, PeerId)> {
        self.find_by_state(RequestState::Found)
            .and_then(|(urn, request)| match request {
                SomeRequest::Found(request) => {
                    request.iter().find_map(|(peer_id, status)| match status {
                        Status::Available => Some((urn.clone(), *peer_id)),
                        _ => None,
                    })
                },
                _ => None,
            })
    }

    #[cfg(test)]
    pub fn insert<R>(&mut self, urn: &Urn, request: R)
    where
        R: Into<SomeRequest<T>>,
    {
        self.requests.insert(urn.id, request.into());
    }
}

#[cfg(test)]
mod test {
    use std::{error, str::FromStr};

    use assert_matches::assert_matches;
    use librad::{git::Urn, git_ext::Oid, keys::SecretKey, peer::PeerId};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::request::Attempts;

    #[test]
    fn happy_path_of_full_request() -> Result<(), Box<dyn error::Error + 'static>> {
        let mut waiting_room: WaitingRoom<usize, usize> = WaitingRoom::new(Config::default());
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);
        let remote_peer = PeerId::from(SecretKey::new());
        let have = waiting_room
            .request(&urn, 0)
            .left()
            .expect("should be a transition")
            .result;
        let want = waiting_room.get(&urn).unwrap();

        assert_eq!(have, want.clone());

        let created = waiting_room.find_by_state(RequestState::Created);
        assert_eq!(
            created,
            Some((
                urn.clone(),
                &SomeRequest::Created(Request::new(urn.clone(), 0))
            )),
        );

        waiting_room.queried(&urn, 0)?;
        let expected = SomeRequest::Requested(Request::new(urn.clone(), 0).request(0));
        assert_eq!(waiting_room.get(&urn), Some(&expected));

        waiting_room.found(&urn, remote_peer, 0)?;
        let expected = SomeRequest::Found(
            Request::new(urn.clone(), 0)
                .request(0)
                .into_found(remote_peer, 0),
        );
        assert_eq!(waiting_room.get(&urn), Some(&expected));

        waiting_room.cloning(&urn, remote_peer, 0)?;
        let expected = SomeRequest::Cloning(
            Request::new(urn.clone(), 0)
                .request(0)
                .into_found(remote_peer, 0)
                .cloning(MAX_QUERIES, MAX_CLONES, remote_peer, 0)
                .unwrap_right(),
        );
        assert_eq!(waiting_room.get(&urn), Some(&expected));

        waiting_room.cloned(&urn, remote_peer, 0)?;
        let expected = SomeRequest::Cloned(
            Request::new(urn.clone(), 0)
                .request(0)
                .into_found(remote_peer, 0)
                .cloning(MAX_QUERIES, MAX_CLONES, remote_peer, 0)
                .unwrap_right()
                .cloned(remote_peer, 0),
        );
        assert_eq!(waiting_room.get(&urn), Some(&expected));

        Ok(())
    }

    #[test]
    fn cannot_create_twice() -> Result<(), Box<dyn error::Error>> {
        let mut waiting_room: WaitingRoom<(), ()> = WaitingRoom::new(Config::default());
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);
        waiting_room.request(&urn, ());
        let request = waiting_room
            .request(&urn, ())
            .right()
            .expect("should not be a transition");

        assert_eq!(request, SomeRequest::Created(Request::new(urn.clone(), ())));

        waiting_room.queried(&urn, ())?;
        let request = waiting_room
            .request(&urn, ())
            .right()
            .expect("should not be a transition");

        assert_eq!(
            request,
            SomeRequest::Requested(Request::new(urn, ()).request(()))
        );

        Ok(())
    }

    #[test]
    fn timeout_on_delta() -> Result<(), Box<dyn std::error::Error>> {
        let mut waiting_room: WaitingRoom<u32, u32> = WaitingRoom::new(Config {
            delta: 5,
            ..Config::default()
        });
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);
        let _req = waiting_room.request(&urn, 0);

        // Initial schedule to be querying after it has been requested.
        let request = waiting_room.next_query(1);
        assert_eq!(request, Some(urn.clone()));

        waiting_room.queried(&urn, 2)?;

        // Should not return the urn again before delta has elapsed.
        let request = waiting_room.next_query(3);
        assert_eq!(request, None);

        // Should return the urn again after delta has elapsed.
        let request = waiting_room.next_query(7);
        assert_eq!(request, Some(urn));

        Ok(())
    }

    #[test]
    fn timeout_on_requests() -> Result<(), Box<dyn error::Error + 'static>> {
        const NUM_QUERIES: usize = 16;
        let mut waiting_room: WaitingRoom<(), ()> = WaitingRoom::new(Config {
            max_queries: Queries::new(NUM_QUERIES),
            max_clones: Clones::new(0),
            delta: (),
        });
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);

        let _req = waiting_room.request(&urn, ());
        for _ in 0..NUM_QUERIES {
            waiting_room.queried(&urn, ())?;
        }

        let mut expected_attempts = Attempts::new();
        expected_attempts.queries = Queries::Max(17);
        assert_eq!(
            waiting_room.queried(&urn, ())?.event,
            Event::TimedOut {
                urn: urn.clone(),
                attempts: expected_attempts
            }
        );

        assert_matches!(waiting_room.get(&urn), Some(SomeRequest::TimedOut(_)));

        Ok(())
    }

    #[allow(clippy::indexing_slicing)]
    #[test]
    fn timeout_on_clones() -> Result<(), Box<dyn error::Error + 'static>> {
        const NUM_CLONES: usize = 16;
        let mut waiting_room: WaitingRoom<(), ()> = WaitingRoom::new(Config {
            max_queries: Queries::new(1),
            max_clones: Clones::new(NUM_CLONES),
            delta: (),
        });
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);

        let mut peers = vec![];
        for _ in 0..=NUM_CLONES {
            peers.push(PeerId::from(SecretKey::new()));
        }

        let _req = waiting_room.request(&urn, ());
        waiting_room.queried(&urn, ())?;

        for remote_peer in &peers {
            waiting_room.found(&urn, *remote_peer, ())?;
        }

        for remote_peer in &peers[0..NUM_CLONES] {
            waiting_room.cloning(&urn, *remote_peer, ())?;
            waiting_room.cloning_failed(&urn, *remote_peer, (), "no reason".to_string())?;
        }

        let mut expected_attempts = Attempts::new();
        expected_attempts.clones = Clones::Max(17);
        expected_attempts.queries = Queries::Max(1);
        assert_eq!(
            waiting_room
                .cloning(
                    &urn,
                    *peers.last().expect(
                        "unless you changed NUM_CLONES to < -1 we should be fine here. qed."
                    ),
                    ()
                )?
                .event,
            Event::TimedOut {
                urn: urn.clone(),
                attempts: expected_attempts
            }
        );

        assert_matches!(waiting_room.get(&urn), Some(SomeRequest::TimedOut(_)));

        Ok(())
    }

    #[test]
    fn cloning_fails_back_to_requested() -> Result<(), Box<dyn error::Error + 'static>> {
        const NUM_CLONES: usize = 5;
        let mut waiting_room: WaitingRoom<u32, u32> = WaitingRoom::new(Config {
            max_queries: Queries::new(1),
            max_clones: Clones::new(NUM_CLONES),
            delta: 5,
        });
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);

        let mut peers = vec![];
        for _ in 0..NUM_CLONES {
            peers.push(PeerId::from(SecretKey::new()));
        }

        let _req = waiting_room.request(&urn, 0);
        waiting_room.queried(&urn, 1)?;

        for remote_peer in peers {
            waiting_room.found(&urn, remote_peer, 2)?;
            waiting_room.cloning(&urn, remote_peer, 2)?;
            waiting_room.cloning_failed(&urn, remote_peer, 2, "no reason".to_string())?;
        }

        assert_matches!(waiting_room.get(&urn), Some(SomeRequest::Requested(_)));

        let request = waiting_room.next_query(3);
        assert_eq!(request, None);

        let request = waiting_room.next_query(7);
        assert_eq!(request, Some(urn));

        Ok(())
    }

    #[test]
    fn cancel_transitions() -> Result<(), Box<dyn error::Error + 'static>> {
        let config = Config::default();
        let mut waiting_room: WaitingRoom<(), ()> = WaitingRoom::new(config);
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);
        let peer = PeerId::from(SecretKey::new());

        // created
        let _req = waiting_room.request(&urn, ());
        waiting_room.canceled(&urn, ())?;
        assert_eq!(
            waiting_room.get(&urn),
            Some(&SomeRequest::Cancelled(
                Request::new(urn.clone(), ()).cancel(())
            ))
        );

        // requested
        let is_requested = Request::new(urn.clone(), ()).request(());
        waiting_room.insert(&urn, is_requested.clone());
        waiting_room.canceled(&urn, ())?;
        assert_eq!(
            waiting_room.get(&urn),
            Some(&SomeRequest::Cancelled(is_requested.clone().cancel(())))
        );

        // found
        let found = is_requested.into_found(peer, ());
        waiting_room.insert(&urn, found.clone());
        waiting_room.canceled(&urn, ())?;
        assert_eq!(
            waiting_room.get(&urn),
            Some(&SomeRequest::Cancelled(found.clone().cancel(())))
        );

        // cloning
        let cloning = found
            .cloning(config.max_queries, config.max_clones, peer, ())
            .unwrap_right();
        waiting_room.insert(&urn, cloning.clone());
        waiting_room.canceled(&urn, ())?;
        assert_eq!(
            waiting_room.get(&urn),
            Some(&SomeRequest::Cancelled(cloning.clone().cancel(())))
        );

        // cloned
        let cloned = cloning.cloned(peer, ());
        waiting_room.insert(&urn, cloned);
        assert_eq!(
            waiting_room.canceled(&urn, ()),
            Err(Error::StateMismatch(RequestState::Cloned))
        );

        // cancel
        let cancelled = Request::new(urn.clone(), ()).cancel(());
        waiting_room.insert(&urn, cancelled.clone());
        waiting_room.canceled(&urn, ())?;
        assert_eq!(
            waiting_room.get(&urn),
            Some(&SomeRequest::Cancelled(cancelled))
        );

        Ok(())
    }

    #[test]
    fn can_get_request_that_is_ready() -> Result<(), Box<dyn error::Error + 'static>> {
        let config = Config::default();
        let mut waiting_room: WaitingRoom<usize, usize> = WaitingRoom::new(config);

        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);
        let remote_peer = PeerId::from(SecretKey::new());

        let ready = waiting_room.find_by_state(RequestState::Cloned);
        assert_eq!(ready, None);

        let _req = waiting_room.request(&urn, 0);
        waiting_room.queried(&urn, 0)?;
        waiting_room.found(&urn, remote_peer, 0)?;
        waiting_room.cloning(&urn, remote_peer, 0)?;
        waiting_room.cloned(&urn, remote_peer, 0)?;

        let ready = waiting_room.find_by_state(RequestState::Cloned);
        let expected = SomeRequest::Cloned(
            Request::new(urn.clone(), 0)
                .request(0)
                .into_found(remote_peer, 0)
                .cloning(config.max_queries, config.max_clones, remote_peer, 0)
                .unwrap_right()
                .cloned(remote_peer, 0),
        );
        assert_eq!(ready, Some((urn, &expected)));

        Ok(())
    }

    #[test]
    fn can_remove_requests() -> Result<(), Box<dyn error::Error + 'static>> {
        let mut waiting_room: WaitingRoom<usize, usize> = WaitingRoom::new(Config::default());
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);
        assert_eq!(waiting_room.remove(&urn, 0).result, None);

        let expected = {
            waiting_room.request(&urn, 0);
            waiting_room.get(&urn).cloned()
        };
        let removed = waiting_room.remove(&urn, 0).result;
        assert_eq!(removed, expected);
        Ok(())
    }

    #[test]
    fn can_backoff_requests() -> Result<(), Box<dyn std::error::Error>> {
        let mut waiting_room: WaitingRoom<u32, u32> = WaitingRoom::new(Config {
            delta: 5,
            ..Config::default()
        });
        let urn: Urn = Urn::new(Oid::from_str("7ab8629dd6da14dcacde7f65b3d58cd291d7e235")?);
        let _req = waiting_room.request(&urn, 0);

        // Initial schedule to be querying after it has been requested.
        let request = waiting_room.next_query(1);
        assert_eq!(request, Some(urn.clone()));

        waiting_room.queried(&urn, 5)?;

        // Should not return the urn again before delta + backoff has elapsed, i.e. 5 +
        // (5 * 1) = 10.
        let request = waiting_room.next_query(8);
        assert_eq!(request, None);

        // The delta + backoff has elapsed.
        let request = waiting_room.next_query(10);
        assert_eq!(request, Some(urn));

        Ok(())
    }
}
