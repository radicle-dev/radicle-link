use std::path::Path;

use git_ref_format::{lit, name, refspec, Qualified, RefStr, RefString};

use librad::{
    git::{
        local::url::LocalUrl,
        types::{
            remote::{LocalFetchspec, LocalPushspec},
            Fetchspec,
            Force,
            Refspec,
            Remote,
        },
    },
    git_ext as ext,
    net::{peer::Peer, protocol::RequestPullGuard},
    refspec_pattern,
    PeerId,
    Signer,
};

use crate::fixed::TestProject;

/// A remote in the working copy
pub enum WorkingRemote {
    /// A remote representing a remote peer, named `PeerId::encode_id`
    Peer(PeerId),
    /// A remote representing the local peer, named "rad"
    Rad,
}

impl From<PeerId> for WorkingRemote {
    fn from(p: PeerId) -> Self {
        WorkingRemote::Peer(p)
    }
}

impl WorkingRemote {
    fn fetchspec(&self) -> Fetchspec {
        match self {
            Self::Peer(peer_id) => {
                let name = RefString::try_from(format!("{}", peer_id)).expect("peer is refstring");
                let dst = RefString::from(Qualified::from(lit::refs_remotes(name.clone())))
                    .with_pattern(refspec::STAR);
                let src = RefString::from(Qualified::from(lit::refs_remotes(name)))
                    .and(name::HEADS)
                    .with_pattern(refspec::STAR);
                let refspec = Refspec {
                    src,
                    dst,
                    force: Force::True,
                };
                refspec.into_fetchspec()
            },
            Self::Rad => {
                let name = RefString::try_from("rad").unwrap();
                let src =
                    RefString::from_iter([name::REFS, name::HEADS]).with_pattern(refspec::STAR);
                Refspec {
                    src,
                    dst: RefString::from(Qualified::from(lit::refs_remotes(name)))
                        .with_pattern(refspec::STAR),
                    force: Force::True,
                }
                .into_fetchspec()
            },
        }
    }

    fn remote_ref(&self, branch: &RefStr) -> RefString {
        let name = match self {
            Self::Rad => name::RAD.to_owned(),
            Self::Peer(peer_id) => {
                RefString::try_from(peer_id.to_string()).expect("peer id is refstring")
            },
        };
        RefString::from(Qualified::from(lit::refs_remotes(name))).join(branch)
    }
}

/// A `WorkingCopy` for test driving interactions with the monorepo where one
/// needs to update the tree of a project.
///
/// Remotes are named after the peer ID, except in the case of the remote
/// representing the local Peer ID - which is called "rad".
pub struct WorkingCopy<'a, S, G> {
    repo: git2::Repository,
    peer: &'a Peer<S, G>,
    project: &'a TestProject,
}

impl<'a, S, G> WorkingCopy<'a, S, G>
where
    S: Signer + Clone,
    G: RequestPullGuard,
{
    /// Create a new working copy. This initializes a git repository and then
    /// fetches the state of the local peer into `refs/remotes/rad/*`.
    pub fn new<P: AsRef<Path>>(
        project: &'a TestProject,
        repo_path: P,
        peer: &'a Peer<S, G>,
    ) -> Result<WorkingCopy<'a, S, G>, anyhow::Error> {
        let repo = git2::Repository::init(repo_path.as_ref())?;

        let mut copy = WorkingCopy {
            peer,
            project,
            repo,
        };
        copy.fetch(WorkingRemote::Rad)?;
        Ok(copy)
    }

    /// Fetch changes from the monorepo into the working copy. The fetchspec
    /// used depends on the peer ID.
    ///
    /// * If `from` is `WorkingRemote::Peer` then `refs/remotes/<peer
    ///   ID>/refs/*:refs/remotes/<peer ID>/heads/*`
    /// * If `from` is `WorkingRemote::Rad` then
    ///   `refs/heads/*:refs/remotes/rad/*`
    ///
    /// I.e. changes from remote peers end up in a remote called
    /// `PeerId::encode_id` whilst changes from the local peer end up in a
    /// remote called "rad".
    pub fn fetch(&mut self, from: WorkingRemote) -> Result<(), anyhow::Error> {
        let fetchspec = from.fetchspec();
        let url = LocalUrl::from(self.project.project.urn());
        let mut remote = Remote::rad_remote(url, fetchspec);
        let _ = remote.fetch(self.peer.clone(), &self.repo, LocalFetchspec::Configured)?;
        Ok(())
    }

    /// Push changes from `refs/heads/*` to the local peer
    pub fn push(&mut self) -> Result<(), anyhow::Error> {
        let url = LocalUrl::from(self.project.project.urn());
        let name = RefString::try_from("rad").unwrap();
        let fetchspec = Refspec {
            src: RefString::from_iter([name::REFS, name::HEADS]).with_pattern(refspec::STAR),
            dst: RefString::from(Qualified::from(lit::refs_remotes(name)))
                .with_pattern(refspec::STAR),
            force: Force::True,
        }
        .into_fetchspec();
        let mut remote = Remote::rad_remote(url, fetchspec);
        let _ = remote.push(
            self.peer.clone(),
            &self.repo,
            LocalPushspec::Matching {
                pattern: refspec_pattern!("refs/heads/*"),
                force: Force::True,
            },
        )?;
        Ok(())
    }

    /// Create a new commit on top of whichever commit is the head of
    /// `on_branch`. If the branch does not exist this will create it.
    pub fn commit(
        &mut self,
        message: &str,
        on_branch: Qualified,
    ) -> Result<git2::Oid, anyhow::Error> {
        let branch_name = on_branch.non_empty_components().2;
        let parent = match self.repo.find_branch(&branch_name, git2::BranchType::Local) {
            Ok(b) => b.get().target().and_then(|o| self.repo.find_commit(o).ok()),
            Err(e) if ext::error::is_not_found_err(&e) => None,
            Err(e) => return Err(anyhow::Error::from(e)),
        };
        let empty_tree = {
            let mut index = self.repo.index()?;
            let oid = index.write_tree()?;
            self.repo.find_tree(oid).unwrap()
        };
        let author = git2::Signature::now("The Animal", "animal@muppets.com").unwrap();
        let parents = match &parent {
            Some(p) => vec![p],
            None => Vec::new(),
        };
        self.repo
            .commit(
                Some(&on_branch),
                &author,
                &author,
                message,
                &empty_tree,
                &parents,
            )
            .map_err(anyhow::Error::from)
    }

    /// Create a branch at `refs/heads/<branch>` which tracks the given remote.
    /// The remote branch name depends on `from`.
    ///
    /// * If `from` is `WorkingCopy::Rad` then `refs/remotes/rad/<branch>`
    /// * If `from` is `WorkingCopy::Peer(peer_id)` then `refs/remotes/<peer
    ///   id>/<branch>`
    pub fn create_remote_tracking_branch(
        &self,
        from: WorkingRemote,
        branch: &RefStr,
    ) -> Result<(), anyhow::Error> {
        let target = self
            .repo
            .find_reference(from.remote_ref(branch).as_str())?
            .target()
            .ok_or_else(|| anyhow::anyhow!("remote ref is not a direct reference"))?;
        let commit = self.repo.find_commit(target)?;
        self.repo.branch(branch.as_str(), &commit, false)?;
        Ok(())
    }

    /// Fast forward the local branch `refs/heads/<branch>` to whatever is
    /// pointed to by `refs/remotes/<remote>/<branch>`
    ///
    /// * If `from` is `WorkingRemote::Peer(peer_id)` then `remote` is
    ///   `peer_id.encode_id()`
    /// * If `from` is `WorkingRemote::Rad` then `remote` is `"rad"`
    ///
    /// # Errors
    ///
    /// * If the local branch does not exist
    /// * If the remote branch does not exist
    /// * If either of the branches does not point at a commit
    /// * If the remote branch is not a descendant of the local branch
    pub fn fast_forward_to(&self, from: WorkingRemote, branch: &RefStr) -> anyhow::Result<()> {
        let remote_ref = from.remote_ref(branch);
        let remote_target = self
            .repo
            .find_reference(&remote_ref)?
            .target()
            .ok_or_else(|| anyhow::anyhow!("remote ref had no target"))?;
        let local_ref = RefString::from(Qualified::from(lit::refs_heads(branch)));
        let local_target = self
            .repo
            .find_reference(&local_ref)?
            .target()
            .ok_or_else(|| anyhow::anyhow!("local ref had no target"))?;
        if !self.repo.graph_descendant_of(remote_target, local_target)? {
            anyhow::bail!("remote ref was not a descendant of local ref");
        } else {
            self.repo
                .reference(&local_ref, remote_target, true, "fast forward")?;
        }
        Ok(())
    }

    /// Create a new commit which merges `refs/heads/<branch>` and
    /// `refs/remotes/<remote>/<branch>`
    ///
    /// this will create a new commit with two parents, one for the remote
    /// branch and one for the local branch
    ///
    /// # Errors
    ///
    /// * If the remote branch does not exist
    /// * If the local branch does not exist
    /// * If either of the references does not point to a commit
    pub fn merge_remote(&self, remote: PeerId, branch: &RefStr) -> anyhow::Result<git2::Oid> {
        let peer_branch = WorkingRemote::Peer(remote).remote_ref(branch);
        let peer_commit = self
            .repo
            .find_reference(&peer_branch.to_string())?
            .peel_to_commit()?;
        let local_branch = Qualified::from(lit::refs_heads(branch));
        let local_commit = self
            .repo
            .find_reference(&local_branch.to_string())?
            .peel_to_commit()?;

        let message = format!("merge {} into {}", peer_branch, local_branch);
        let empty_tree = {
            let mut index = self.repo.index()?;
            let oid = index.write_tree()?;
            self.repo.find_tree(oid).unwrap()
        };
        let author = git2::Signature::now("The Animal", "animal@muppets.com").unwrap();
        let parents = vec![&peer_commit, &local_commit];
        self.repo
            .commit(
                Some(&local_branch),
                &author,
                &author,
                &message,
                &empty_tree,
                &parents,
            )
            .map_err(anyhow::Error::from)
    }
}
