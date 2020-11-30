// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::peer::PeerId;

/// Constraint for [sealed traits] under the `git` module hierarchy.
///
/// [sealed traits]: https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed
pub trait Sealed {}

impl Sealed for PeerId {}
impl Sealed for &PeerId {}
