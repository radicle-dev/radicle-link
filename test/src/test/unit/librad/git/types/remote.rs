// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, io};

use pretty_assertions::assert_eq;

use librad::{
    git::{
        local::url::LocalUrl,
        types::{remote::Remote, AsNamespace, Force, Namespace, Reference, Refspec},
        Urn,
    },
    git_ext as ext,
    keys::SecretKey,
    peer::PeerId,
    reflike,
    refspec_pattern,
};

use crate::tempdir::WithTmpDir;

lazy_static! {
    static ref PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
        167, 44, 200, 200, 213, 81, 154, 10, 55, 187, 241, 156, 54, 52, 39, 112, 217, 179, 101, 43,
        167, 22, 230, 111, 42, 226, 79, 33, 126, 97, 51, 208
    ]));
    static ref URN: Urn = Urn::new(ext::Oid::from(
        git2::Oid::hash_object(git2::ObjectType::Commit, b"meow-meow").unwrap()
    ));
}

#[test]
fn can_create_remote() {
    WithTmpDir::new::<_, io::Error>(|path| {
        let repo = git2::Repository::init(path).expect("failed to init repo");

        let fetch = Refspec {
            src: refspec_pattern!("refs/heads/*"),
            dst: Reference::heads(Namespace::from(&*URN), None),
            force: Force::True,
        };
        let push = Refspec {
            src: reflike!("refs/heads/next"),
            dst: Reference::head(Namespace::from(&*URN), None, reflike!("next")),
            force: Force::False,
        };

        {
            let url = LocalUrl::from(URN.clone());
            let mut remote = Remote::rad_remote(url, fetch).with_pushspecs(Some(push));
            remote.save(&repo).expect("failed to persist the remote");
        }

        let remote = Remote::<LocalUrl>::find(&repo, reflike!("rad"))
            .unwrap()
            .expect("should exist");

        assert_eq!(
            remote
                .fetchspecs
                .iter()
                .map(|spec| spec.to_string())
                .collect::<Vec<_>>(),
            vec![format!(
                "+refs/heads/*:refs/namespaces/{}/refs/heads/*",
                Namespace::from(&*URN).into_namespace()
            )],
        );

        assert_eq!(
            remote
                .pushspecs
                .iter()
                .map(|spec| spec.to_string())
                .collect::<Vec<_>>(),
            vec![format!(
                "refs/heads/next:refs/namespaces/{}/refs/heads/next",
                Namespace::from(&*URN).into_namespace()
            )],
        );

        Ok(())
    })
    .unwrap();
}

#[test]
fn check_remote_fetch_spec() -> Result<(), git2::Error> {
    let url = LocalUrl::from(URN.clone());
    let name = ext::RefLike::try_from(format!("lyla@{}", *PEER_ID)).unwrap();

    let heads = Reference::heads(None, *PEER_ID);
    let remotes = reflike!("refs/remotes")
        .join(&name)
        .with_pattern_suffix(refspec_pattern!("*"));
    let mut remote = Remote {
        url,
        name: name.clone(),
        fetchspecs: vec![Refspec {
            src: heads,
            dst: remotes,
            force: Force::True,
        }
        .into()],
        pushspecs: vec![],
    };

    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path())?;
    remote.save(&repo)?;
    let git_remote = repo.find_remote(name.as_str())?;
    let fetch_refspecs = git_remote.fetch_refspecs()?;

    assert_eq!(
        fetch_refspecs.iter().flatten().collect::<Vec<_>>(),
        vec![format!("+refs/remotes/{}/heads/*:refs/remotes/{}/*", *PEER_ID, name).as_str()]
    );

    Ok(())
}
