// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{
        identities::{self, Person, Project},
        storage::Storage,
    },
    identities::{delegation, payload},
};

pub struct TestProject {
    pub owner: Person,
    pub project: Project,
}

pub fn create_test_project(storage: &Storage) -> Result<TestProject, anyhow::Error> {
    let peer_id = storage.peer_id();
    let alice = identities::person::create(
        storage,
        payload::Person {
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
