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

use librad::identities::payload;

lazy_static! {
    static ref ALICE: payload::User = payload::User {
        name: "alice".into()
    };
    static ref BOB: payload::User = payload::User { name: "bob".into() };
    static ref RADICLE: payload::Project = payload::Project {
        name: "radicle".into(),
        description: Some("pea two pea".into()),
        default_branch: Some("next".into())
    };
}
