// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::{TryFrom, TryInto};

use librad::{
    self,
    git::{
        local::url::LocalUrl,
        types::{remote, Flat, Force, GenericRef, Namespace, Reference, Refspec, Remote},
    },
    git_ext as ext,
    reflike,
    refspec_pattern,
};
use librad_test::{
    git::create_commit,
    logging,
    rad::{identities::TestProject, testnet},
};

const NUM_PEERS: usize = 3;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trois() {
    logging::init();

    let peers = testnet::setup(NUM_PEERS).await.unwrap();
    testnet::run_on_testnet(peers, NUM_PEERS, |mut peers| async move {
        let peer1 = peers.pop().unwrap();
        let peer2 = peers.pop().unwrap();
        let peer3 = peers.pop().unwrap();

        let proj = peer1
            .using_storage(move |storage| TestProject::create(&storage))
            .await
            .unwrap()
            .unwrap();
        let default_branch: ext::RefLike = proj
            .project
            .doc
            .payload
            .subject
            .default_branch
            .as_ref()
            .map(|cstring| cstring.to_string())
            .unwrap_or_else(|| "mistress".to_owned())
            .try_into()
            .unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let commit_id = {
            // Perform commit and push to working copy on peer1
            let repo = git2::Repository::init(tmp.path().join("peer1")).unwrap();
            let url = LocalUrl::from(proj.project.urn());
            let heads =
                Reference::heads(Namespace::from(proj.project.urn()), Some(peer1.peer_id()));
            let remotes = GenericRef::heads(
                Flat,
                ext::RefLike::try_from(format!(
                    "{}@{}",
                    proj.owner.subject().name,
                    peer1.peer_id()
                ))
                .unwrap(),
            );
            let mastor = reflike!("refs/heads").join(&default_branch);
            let mut remote = Remote::rad_remote(
                url,
                Refspec {
                    src: &remotes,
                    dst: &heads,
                    force: Force::True,
                },
            );
            let oid = create_commit(&repo, mastor).unwrap();
            let updated = remote
                .push(
                    peer1.clone(),
                    &repo,
                    remote::LocalPushspec::Matching {
                        pattern: refspec_pattern!("refs/heads/*"),
                        force: Force::True,
                    },
                )
                .unwrap()
                .collect::<Vec<_>>();
            tracing::debug!("push updated refs: {:?}", updated);

            oid
        };

        let expected_urn = proj.project.urn().with_path(
            reflike!("refs/remotes")
                .join(peer1.peer_id())
                .join(reflike!("heads"))
                .join(&default_branch),
        );

        struct ExpectedReferences {
            has_commit: bool,
            has_rad_id: bool,
            has_rad_self: bool,
        }

        proj.pull(&peer1, &peer2).await.ok().unwrap();
        proj.pull(&peer2, &peer3).await.ok().unwrap();

        let peer2_expected = peer2
            .using_storage({
                let urn = expected_urn.clone();
                let rad_self = Reference::rad_self(Namespace::from(urn.clone()), peer1.peer_id());
                let rad_id =
                    Reference::rad_id(Namespace::from(urn.clone())).with_remote(peer1.peer_id());
                move |storage| -> Result<ExpectedReferences, anyhow::Error> {
                    Ok(ExpectedReferences {
                        has_commit: storage.has_commit(&urn, Box::new(commit_id))?,
                        has_rad_id: storage.has_ref(&rad_self)?,
                        has_rad_self: storage.has_ref(&rad_id)?,
                    })
                }
            })
            .await
            .unwrap()
            .unwrap();
        let peer3_expected = peer3
            .using_storage({
                let urn = expected_urn.clone();
                let rad_self = Reference::rad_self(Namespace::from(urn.clone()), peer1.peer_id());
                let rad_id =
                    Reference::rad_id(Namespace::from(urn.clone())).with_remote(peer1.peer_id());
                move |storage| -> Result<ExpectedReferences, anyhow::Error> {
                    Ok(ExpectedReferences {
                        has_commit: storage.has_commit(&urn, Box::new(commit_id))?,
                        has_rad_id: storage.has_ref(&rad_self)?,
                        has_rad_self: storage.has_ref(&rad_id)?,
                    })
                }
            })
            .await
            .unwrap()
            .unwrap();

        assert!(
            peer2_expected.has_commit,
            format!("peer 2 missing commit `{}@{}`", expected_urn, commit_id)
        );
        assert!(peer2_expected.has_rad_id, "peer 2 missing `rad/id`");
        assert!(peer2_expected.has_rad_self, "peer 2 missing `rad/self``");

        assert!(
            peer3_expected.has_commit,
            format!("peer 3 missing commit `{}@{}`", expected_urn, commit_id)
        );
        assert!(peer3_expected.has_rad_id, "peer 3 missing `rad/id`");
        assert!(peer3_expected.has_rad_self, "peer 3 missing `rad/self``");
    })
    .await;
}
