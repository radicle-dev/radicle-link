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

use librad::{
    git::{
        identities::{self, Project, User},
        storage::Storage,
    },
    identities::{delegation, payload},
};

pub struct TestProject {
    pub owner: User,
    pub project: Project,
}

pub fn create_test_project(storage: &Storage) -> Result<TestProject, anyhow::Error> {
    let peer_id = storage.peer_id();
    let alice = identities::user::create(
        storage,
        payload::User {
            name: "alice".into(),
        },
        Some(*peer_id.as_public_key()).into_iter().collect(),
    )?;
    let local_id = identities::local::load(storage, alice.urn())?
        .expect("local id must exist as we just created it");
    let proj = identities::project::create(
        storage,
        local_id,
        payload::Project {
            name: "radicle-link".into(),
            description: Some("pea two pea".into()),
            default_branch: Some("next".into()),
        },
        delegation::Indirect::from(alice.clone()),
    )?;

    Ok(TestProject {
        owner: alice,
        project: proj,
    })
}
