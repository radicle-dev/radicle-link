use std::convert::TryFrom;

use librad::git::{
    local::url::LocalUrl,
    types::{remote::LocalPushspec, Fetchspec, Force, Remote},
};
use radicle_git_ext::RefspecPattern;

use radicle_daemon::{identities::payload::Person, state, RunConfig};

#[macro_use]
mod common;
use common::{build_peer, init_logging, shia_le_pathbuf, started};

/// Given two peers Alice and Bob
/// Alice creates a project
/// Bob clones the project from alice and checks it out
/// Alice tracks Bob
/// Bob creates a new commit and patch tag in the working copy
/// Bob pushes the patch tag to the `rad` remote
/// Alice fetches from Bob
/// Alice sees the patch in `radicle_daemon::patch::list`
#[tokio::test]
async fn patch_replication() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let alice_tmp_dir = tempfile::tempdir()?;
    let alice_repo_path = alice_tmp_dir.path().join("radicle");
    let alice_peer = build_peer(&alice_tmp_dir, RunConfig::default()).await?;
    let (alice_peer, alice_addrs) = {
        let peer = alice_peer.peer.clone();
        let events = alice_peer.subscribe();
        let mut peer_control = alice_peer.control();
        tokio::task::spawn(alice_peer.run());
        started(events).await?;
        let alice_addrs = peer_control.listen_addrs().await;
        (peer, alice_addrs)
    };
    let alice = state::init_owner(
        &alice_peer,
        Person {
            name: "alice".into(),
        },
    )
    .await?;

    let bob_tmp_dir = tempfile::tempdir()?;
    let bob_peer = build_peer(&bob_tmp_dir, RunConfig::default()).await?;
    let (bob_peer, bob_addrs) = {
        let peer = bob_peer.peer.clone();
        let events = bob_peer.subscribe();
        let mut peer_control = bob_peer.control();
        tokio::task::spawn(bob_peer.run());
        started(events).await?;
        let bob_addrs = peer_control.listen_addrs().await;
        (peer, bob_addrs)
    };
    let _bob = state::init_owner(&bob_peer, Person { name: "bob".into() }).await?;

    let project = state::init_project(
        &alice_peer,
        &alice,
        shia_le_pathbuf(alice_repo_path.clone()),
    )
    .await?;

    state::clone_project(
        &bob_peer,
        project.urn(),
        alice_peer.peer_id(),
        alice_addrs.clone(),
        None,
    )
    .await?;

    let bob_repo_path = bob_tmp_dir.path().join("radicle");
    state::checkout(
        &bob_peer,
        project.urn(),
        alice_peer.peer_id(),
        bob_repo_path.clone(),
    )
    .await
    .unwrap();

    state::track(&alice_peer, project.urn(), bob_peer.peer_id())
        .await
        .unwrap();

    let bob_signature = git2::Signature::now("bob", "bob@example.com")?;
    let repo = git2::Repository::open(bob_repo_path.join(project.subject().name.to_string()))?;
    let default_branch = project.subject().default_branch.clone().unwrap();

    let default_branch_head = repo
        .find_reference(&format!("refs/heads/{}", default_branch))?
        .peel_to_commit()
        .unwrap();

    let _tag_id = repo
        .tag(
            "radicle-patch/BOBS-PATCH",
            &default_branch_head.as_object(),
            &bob_signature,
            "MESSAGE",
            false,
        )
        .unwrap();

    let mut rad =
        Remote::<LocalUrl>::rad_remote::<_, Fetchspec>(LocalUrl::from(project.urn()), None);
    let _ = rad.push(
        state::settings(&bob_peer),
        &repo,
        LocalPushspec::Matching {
            pattern: RefspecPattern::try_from("refs/tags/*").unwrap(),
            force: Force::False,
        },
    )?;

    state::fetch(
        &alice_peer,
        project.urn(),
        bob_peer.peer_id(),
        bob_addrs,
        None,
    )
    .await
    .unwrap();
    let alice_patches = radicle_daemon::patch::list(&alice_peer, project.urn())
        .await
        .unwrap();
    assert_eq!(alice_patches.len(), 1);
    assert_eq!(alice_patches[0].id, "BOBS-PATCH",);

    state::fetch(
        &bob_peer,
        project.urn(),
        alice_peer.peer_id(),
        alice_addrs,
        None,
    )
    .await
    .unwrap();
    let bob_patches = radicle_daemon::patch::list(&bob_peer, project.urn())
        .await
        .unwrap();
    assert_eq!(bob_patches.len(), 1);
    assert_eq!(bob_patches[0].id, "BOBS-PATCH",);

    Ok(())
}

/// Alice creates a patch using Git. We check that all fields in `Patch` have
/// the correct values.
#[tokio::test]
async fn patch_struct() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let alice_tmp_dir = tempfile::tempdir()?;
    let alice_repo_path = alice_tmp_dir.path().join("radicle");
    let alice_peer = build_peer(&alice_tmp_dir, RunConfig::default()).await?;
    let alice_peer = {
        let peer = alice_peer.peer.clone();
        let events = alice_peer.subscribe();
        tokio::task::spawn(alice_peer.run());
        started(events).await?;
        peer
    };
    let alice = state::init_owner(
        &alice_peer,
        Person {
            name: "alice".into(),
        },
    )
    .await?;
    let alice_signature =
        git2::Signature::now(&alice.subject().name.to_string(), "alice@example.com")?;

    let project = state::init_project(
        &alice_peer,
        &alice,
        shia_le_pathbuf(alice_repo_path.clone()),
    )
    .await?;

    let repo = git2::Repository::open(alice_repo_path.join(project.subject().name.to_string()))?;
    let default_branch = project.subject().default_branch.clone().unwrap();
    let default_branch_ref = format!("refs/heads/{}", default_branch);

    let original_default_branch_head = repo
        .find_reference(&default_branch_ref)?
        .peel_to_commit()
        .unwrap();

    // Create a new commit and update the default branch
    repo.commit(
        Some(&default_branch_ref),
        &alice_signature,
        &alice_signature,
        "",
        &original_default_branch_head.tree().unwrap(),
        &[&original_default_branch_head],
    )
    .unwrap();

    let patch_head_id = repo
        .commit(
            None,
            &alice_signature,
            &alice_signature,
            "",
            &original_default_branch_head.tree().unwrap(),
            &[&original_default_branch_head],
        )
        .unwrap();
    let patch_head = repo.find_commit(patch_head_id).unwrap();

    let _tag_id = repo
        .tag(
            "radicle-patch/ALICES-PATCH",
            &patch_head.as_object(),
            &alice_signature,
            "MESSAGE",
            false,
        )
        .unwrap();

    let mut rad =
        Remote::<LocalUrl>::rad_remote::<_, Fetchspec>(LocalUrl::from(project.urn()), None);
    let _ = rad.push(
        state::settings(&alice_peer),
        &repo,
        LocalPushspec::Matching {
            pattern: RefspecPattern::try_from("refs/tags/*").unwrap(),
            force: Force::False,
        },
    )?;

    let patches = radicle_daemon::patch::list(&alice_peer, project.urn())
        .await
        .unwrap();
    let patch = &patches[0];
    assert_eq!(patch.id, "ALICES-PATCH",);
    assert_eq!(patch.merge_base, Some(original_default_branch_head.id()));
    assert_eq!(patch.commit, patch_head_id);
    assert_eq!(patch.message, Some("MESSAGE".to_string()));
    assert_eq!(patch.merged(), false);

    Ok(())
}
