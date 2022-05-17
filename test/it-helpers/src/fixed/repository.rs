use std::io;

use git_ref_format::{lit, refspec, RefString};
use librad::{
    git::{
        local::url::LocalUrl,
        types::{remote, Flat, Force, GenericRef, Namespace, Reference, Refspec, Remote},
    },
    git_ext as ext,
    identities::{Person, Project},
    net::peer::{Peer, RequestPullGuard},
    PeerId,
    Signer,
};
use test_helpers::tempdir::WithTmpDir;

use crate::git::create_commit;

pub type TmpRepository = WithTmpDir<git2::Repository>;

pub fn repository(peer: PeerId) -> TmpRepository {
    WithTmpDir::new(|path| {
        git2::Repository::init(path.join(peer.to_string()))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    })
    .unwrap()
}

pub fn commit<S, G>(
    peer: Peer<S, G>,
    repo: TmpRepository,
    project: &Project,
    owner: &Person,
    default_branch: RefString,
) -> impl FnOnce() -> ext::Oid
where
    S: Signer + Clone,
    G: RequestPullGuard + Clone,
{
    let urn = project.urn();
    let owner_subject = owner.subject().clone();
    let id = peer.peer_id();
    move || {
        // Perform commit and push to working copy on peer1
        let url = LocalUrl::from(urn.clone());
        let heads = Reference::heads(Namespace::from(urn), Some(id));
        let remotes = GenericRef::heads(
            Flat,
            ext::RefLike::try_from(format!("{}@{}", id, owner_subject.name)).unwrap(),
        );
        let mastor = lit::refs_heads(default_branch).into();
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
                peer,
                &repo,
                remote::LocalPushspec::Matching {
                    pattern: refspec::pattern!("refs/heads/*").into(),
                    force: Force::True,
                },
            )
            .unwrap()
            .collect::<Vec<_>>();
        debug!("push updated refs: {:?}", updated);

        ext::Oid::from(oid)
    }
}
