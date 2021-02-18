// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! 🩺 Patience is a virtue, so please take a ticket for the waiting room and be
//! patient.
//!
//! An API for keeping track of requests and their state transitions.
//!
//! See [`Request`] and [`WaitingRoom`] for a high-level view of
//! the API.

pub mod request;
pub mod types;
pub mod waiting_room;

// Re-export types
pub use request::{Request, SomeRequest};
pub use waiting_room::WaitingRoom;

/// Private trait for sealing the traits we use here.
mod sealed;
