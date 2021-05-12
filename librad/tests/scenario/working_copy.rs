// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt::Debug, ops::Index as _, path::Path, time::Duration};

use futures::StreamExt as _;
use tempfile::tempdir;

use librad::{
    git::{
        identities::{self, Person, Project},
        include,
        local::url::LocalUrl,
        tracking,
        types::{
            remote::{LocalFetchspec, LocalPushspec},
            Flat,
            Force,
            GenericRef,
            Namespace,
            Reference,
            Refspec,
            Remote,
        },
        Urn,
    },
    git_ext as ext,
    net::{
        peer::Peer,
        protocol::{
            event::{self, upstream::predicate::gossip_from},
            gossip::{self, Rev},
        },
    },
    peer::PeerId,
    reflike,
    refspec_pattern,
    signer::Signer,
};

use librad_test::{
    git::create_commit,
    logging,
    rad::{identities::TestProject, testnet},
};

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

/// This integration test is to ensure that we can setup a working copy that can
/// fetch changes. The breakdown of the test into substeps is:
///
/// 1. Two peers are setup: peer1 and peer2.
/// 2. peer1 creates a project in their monorepo
/// 3. peer2 clones it
/// 4. peer1 creates a working copy and commits changes to it
/// 5. peer2 receives the changes via an announcement
/// 6. peer2 decides to create a working copy
/// 7. peer2 creates an include file, based of the tracked users of the project
/// i.e. peer1 8. peer2 includes this file in their working copy's config
/// 9. peer2 fetches in the working copy and sees the commit
#[test]
fn can_fetch() {
    logging::init();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let peer2_events = peer2.subscribe();

        let proj = peer1
            .using_storage(move |store| TestProject::create(&store))
            .await
            .unwrap()
            .unwrap();
        proj.pull(peer1, peer2).await.ok().unwrap();

        let tracked_persons = {
            let urn = proj.project.urn();
            peer2
                .using_storage(move |store| {
                    tracking::tracked(&store, &urn)
                        .unwrap()
                        .map(|peer| {
                            let self_ref = Reference::rad_self(Namespace::from(&urn), peer);
                            let person = identities::person::get(
                                &store,
                                &Urn::try_from(self_ref).expect("namespace is set"),
                            )
                            .unwrap()
                            .expect("tracked person should exist");
                            (person, peer)
                        })
                        .collect::<Vec<(Person, PeerId)>>()
                })
                .await
                .unwrap()
        };
        assert!(!tracked_persons.is_empty());

        let tmp = tempdir().unwrap();
        {
            let commit_id =
                commit_and_push(tmp.path().join("peer1"), &peer1, &proj.owner, &proj.project)
                    .await
                    .unwrap();
            event::upstream::expect(
                peer2_events.boxed(),
                gossip_from(peer1.peer_id()),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
            let peer2_repo = create_working_copy(
                tmp.path().join("peer2"),
                tmp.path().to_path_buf(),
                &peer2,
                &proj.project,
                tracked_persons,
            )
            .unwrap();
            assert!(peer2_repo.find_commit(commit_id).is_ok());
        }
    })
}

// Perform commit and push to working copy on peer1
#[tracing::instrument(skip(peer), err)]
async fn commit_and_push<P, S>(
    repo_path: P,
    peer: &Peer<S>,
    owner: &Person,
    project: &Project,
) -> Result<git2::Oid, anyhow::Error>
where
    P: AsRef<Path> + Debug,
    S: Signer + Clone,
{
    let repo = git2::Repository::init(repo_path)?;
    let url = LocalUrl::from(project.urn());

    let fetchspec = Refspec {
        src: Reference::heads(Namespace::from(project.urn()), peer.peer_id()),
        dst: GenericRef::heads(
            Flat,
            ext::RefLike::try_from(format!("{}@{}", owner.subject().name, peer.peer_id())).unwrap(),
        ),
        force: Force::True,
    }
    .into_fetchspec();

    let master = reflike!("refs/heads/master");

    let oid = create_commit(&repo, master.clone())?;
    let mut remote = Remote::rad_remote(url, fetchspec);
    remote
        .push(
            peer.clone(),
            &repo,
            LocalPushspec::Matching {
                pattern: refspec_pattern!("refs/heads/*"),
                force: Force::True,
            },
        )?
        .for_each(drop);

    peer.announce(gossip::Payload {
        origin: None,
        urn: project.urn().with_path(master),
        rev: Some(Rev::Git(oid)),
    })
    .unwrap();

    Ok(oid)
}

// Create working copy of project
#[tracing::instrument(skip(peer), err)]
fn create_working_copy<P, S, I>(
    repo_path: P,
    inc_path: P,
    peer: &Peer<S>,
    project: &Project,
    tracked_persons: I,
) -> Result<git2::Repository, anyhow::Error>
where
    P: AsRef<Path> + Debug,
    S: Signer + Clone,
    I: IntoIterator<Item = (Person, PeerId)> + Debug,
{
    let repo = git2::Repository::init(repo_path)?;

    let inc = include::Include::from_tracked_persons(
        inc_path,
        LocalUrl::from(project.urn()),
        tracked_persons.into_iter().map(|(person, peer_id)| {
            (
                ext::RefLike::try_from(person.subject().name.as_str()).unwrap(),
                peer_id,
            )
        }),
    );
    let inc_path = inc.file_path();
    inc.save()?;

    // Add the include above to include.path of the repo config
    include::set_include_path(&repo, inc_path)?;

    // Fetch from the working copy and check we have the commit in the working copy
    for remote in repo.remotes()?.iter().flatten() {
        let mut remote = Remote::find(&repo, ext::RefLike::try_from(remote).unwrap())?
            .expect("should exist, because libgit told us about it");
        remote
            .fetch(peer.clone(), &repo, LocalFetchspec::Configured)?
            .for_each(drop);
    }

    Ok(repo)
}
