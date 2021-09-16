// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use radicle_daemon::{project::checkout, state, RunConfig};

use assert_matches::assert_matches;
use pretty_assertions::assert_eq;

use crate::{
    daemon::common::{blocking, shia_le_pathbuf, Harness},
    logging,
};

#[test]
fn upstream_for_default() -> Result<(), Box<dyn std::error::Error>> {
    logging::init();

    let mut harness = Harness::new();
    let alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    harness.enter(async move {
        let create = shia_le_pathbuf(alice.path.join("radicle"));
        let working_copy_path = create.repo.full_path();
        let _ = state::init_project(&alice.peer, &alice.owner, create).await?;

        blocking(move || {
            let repo = git2::Repository::open(working_copy_path).unwrap();
            let remote = repo.branch_upstream_remote("refs/heads/it").unwrap();
            assert_eq!(remote.as_str().unwrap(), "rad");

            let branch = repo.find_branch("rad/it", git2::BranchType::Remote);
            assert!(branch.is_ok(), "could not find `rad/it`");
        })
        .await;

        Ok(())
    })
}

#[test]
fn checkout_twice_fails() -> Result<(), Box<dyn std::error::Error>> {
    logging::init();

    let mut harness = Harness::new();
    let alice = harness.add_peer("alice", RunConfig::default(), &[])?;
    harness.enter(async move {
        let project = state::init_project(
            &alice.peer,
            &alice.owner,
            shia_le_pathbuf(alice.path.clone()),
        )
        .await?;

        let _ = state::checkout(
            &alice.peer,
            project.urn(),
            None,
            alice.path.join("checkout"),
        )
        .await?;

        assert_matches!(
            state::checkout(
                &alice.peer,
                project.urn(),
                None,
                alice.path.join("checkout"),
            )
            .await
            .err(),
            Some(state::Error::Checkout(checkout::Error::AlreadExists(_)))
        );

        Ok(())
    })
}
