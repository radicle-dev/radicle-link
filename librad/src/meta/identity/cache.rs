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

use super::{Error, Identity, Revision, Verified};

pub trait VerificationCache {
    fn is_verified(&self, rev: &Revision) -> bool;
    fn register_verified(&mut self, id: &Identity<Verified>) -> Result<(), Error>;
}

#[cfg(test)]
pub mod test {
    use super::*;

    pub struct NullVerificationCache {}

    impl VerificationCache for NullVerificationCache {
        fn is_verified(&self, _rev: &Revision) -> bool {
            false
        }
        fn register_verified(&mut self, _id: &Identity<Verified>) -> Result<(), Error> {
            Ok(())
        }
    }

    pub struct TrueVerificationCache {}

    impl VerificationCache for TrueVerificationCache {
        fn is_verified(&self, _rev: &Revision) -> bool {
            true
        }
        fn register_verified(&mut self, _id: &Identity<Verified>) -> Result<(), Error> {
            Ok(())
        }
    }
}
