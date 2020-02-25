use std::{fmt, fmt::Display, io, path::Path, str::FromStr};

use git2;
use olpc_cjson::CanonicalFormatter;
use radicle_surf::vcs::git as surf;
use serde::Serialize;
use serde_json;

use crate::{
    keys,
    keys::{device, pgp},
    meta,
    paths::Paths,
    peer::PeerId,
};

pub mod server;

const PROJECT_METADATA_BRANCH: &str = "rad/project";
const PROJECT_METADATA_FILE: &str = "project.json";

const CONTRIBUTOR_METADATA_BRANCH: &str = "rad/contributor";
const CONTRIBUTOR_METADATA_FILE: &str = "contributor.json";

const RAD_REMOTE_NAME: &str = "rad";

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Invalid PGP key: missing UserID packet")]
    MissingPgpUserId,

    #[fail(display = "Invalid PGP key: missing address")]
    MissingPgpAddr,

    #[fail(display = "Project already exists")]
    ProjectExists,

    #[fail(display = "Project not found")]
    NoSuchProject,

    #[fail(
        display = "Branch {} specified as default_branch does not exist in the source repo",
        0
    )]
    MissingDefaultBranch(String, #[fail(cause)] git2::Error),

    #[fail(display = "{}", 0)]
    Libgit(git2::Error),

    #[fail(display = "{}", 0)]
    Io(io::Error),

    #[fail(display = "{}", 0)]
    Serde(serde_json::error::Error),

    #[fail(display = "{}", 0)]
    Pgp(keys::pgp::Error),

    #[fail(display = "{:?}", 0)]
    Surf(surf::error::Error),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<git2::Error> for Error {
    fn from(err: git2::Error) -> Self {
        Error::Libgit(err)
    }
}

impl From<serde_json::error::Error> for Error {
    fn from(err: serde_json::error::Error) -> Self {
        Error::Serde(err)
    }
}

impl From<keys::pgp::Error> for Error {
    fn from(err: keys::pgp::Error) -> Self {
        Error::Pgp(err)
    }
}

impl From<surf::error::Error> for Error {
    fn from(err: surf::error::Error) -> Self {
        Error::Surf(err)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectId(git2::Oid);

impl ProjectId {
    pub fn new(oid: git2::Oid) -> Self {
        Self(oid)
    }
}

pub mod projectid {
    #[derive(Debug, Fail)]
    pub enum ParseError {
        #[fail(display = "Invalid backend: `{}`, expected `git`", 0)]
        InvalidBackend(String),

        #[fail(display = "Invalid oid: `{}` ({})", 0, 1)]
        InvalidOid(String, git2::Error),

        #[fail(
            display = "Invalid ProjectId format, expected `<identifier> '.' <backend>`: {}",
            0
        )]
        InvalidFormat(String),
    }
}

impl FromStr for ProjectId {
    type Err = projectid::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '.');
        let may_oid = parts.next();
        let may_typ = parts.next();
        match (may_oid, may_typ) {
            (Some(oid), Some("git")) => git2::Oid::from_str(oid)
                .map(ProjectId)
                .map_err(|e| Self::Err::InvalidOid(oid.to_string(), e)),

            (_, Some(typ)) => Err(Self::Err::InvalidBackend(typ.to_string())),

            _ => Err(Self::Err::InvalidFormat(s.to_string())),
        }
    }
}

impl Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.git", self.0)
    }
}

pub struct GitProject(git2::Repository);

impl GitProject {
    pub fn open(path: &Path) -> Result<GitProject, Error> {
        git2::Repository::open_bare(path)
            .map(GitProject)
            .map_err(|e| e.into())
    }

    pub fn init(
        paths: &Paths,
        key: &device::Key,
        sources: &git2::Repository,
        metadata: meta::Project,
        founder: meta::Contributor,
    ) -> Result<ProjectId, Error> {
        // TODO: resolve URL ref iff rad://
        let (nickname, fullname) = match founder.profile {
            Some(meta::ProfileRef::UserProfile(meta::UserProfile {
                ref nick, ref name, ..
            })) => (nick.to_owned(), name.to_owned()),
            _ => ("anonymous".into(), None),
        };
        let mut pgp_key = key.clone().into_pgp(&nickname, fullname)?;

        // Link all metadata to the tip of the default branch
        let default_branch = metadata.default_branch.clone();
        let parent = sources
            .find_branch(&default_branch, git2::BranchType::Local)
            .map_err(|e| Error::MissingDefaultBranch(default_branch.clone(), e))?
            .into_reference()
            .peel_to_commit()?;

        // Ensure the signing key is a maintainer
        let mut metadata = metadata;
        metadata.add_maintainer(&PeerId::from(key.clone()));

        // Create the metadata in the sources repo
        let pid = commit_project_meta(
            sources,
            &parent,
            &mut pgp_key,
            "Radicle: intial project metadata",
            metadata,
        )?;
        let mut proj_branch =
            sources.branch(PROJECT_METADATA_BRANCH, &sources.find_commit(pid)?, true)?;

        // The ProjectId is the commit SHA1
        let pid = ProjectId(pid);

        // Add initial contributor metadata from the profile
        let cid = commit_contributor_meta(
            sources,
            &parent,
            &mut pgp_key,
            "Radicle: initial contributor metadata",
            founder,
        )?;
        let mut contrib_branch = sources.branch(
            CONTRIBUTOR_METADATA_BRANCH,
            &sources.find_commit(cid)?,
            true,
        )?;

        // Create a remote in our state dir
        let res = register_project(paths, &pid, &default_branch, sources);

        // Clean up local stuff
        let _ = proj_branch.delete();
        let _ = contrib_branch.delete();

        res.map(|_| pid)
    }

    pub fn metadata(&self) -> Result<meta::Project, Error> {
        let blob = {
            self.0
                .find_branch(PROJECT_METADATA_BRANCH, git2::BranchType::Local)?
                .get()
                .peel_to_tree()?
                .get_name(PROJECT_METADATA_FILE)
                .expect("Missing project.json")
                .to_object(&self.0)?
                .peel_to_blob()
        }?;
        let meta = serde_json::from_slice(blob.content())?;
        Ok(meta)
    }

    pub fn builder(
        project_name: &str,
        founder_key: &device::Key,
        founder_meta: meta::Contributor,
    ) -> project::Builder {
        project::Builder::new(project_name, founder_key, founder_meta)
    }

    pub fn browser(self) -> Result<surf::Browser, Error> {
        surf::Browser::new(self.into()).map_err(|e| e.into())
    }
}

impl From<GitProject> for surf::Repository {
    fn from(proj: GitProject) -> Self {
        proj.0.into()
    }
}

pub mod project {
    use super::*;

    pub struct Builder {
        key: device::Key,
        founder: meta::Contributor,
        name: String,
        description: Option<String>,
        default_branch: String,
        rel: Vec<meta::Relation>,
    }

    impl Builder {
        pub fn new(name: &str, key: &device::Key, founder: meta::Contributor) -> Self {
            Self {
                key: key.clone(),
                founder,
                name: name.to_owned(),
                description: None,
                default_branch: meta::default_branch(),
                rel: vec![],
            }
        }

        pub fn set_description(&mut self, descr: String) -> &mut Self {
            self.description = Some(descr);
            self
        }

        pub fn set_default_branch(&mut self, branch: String) -> &mut Self {
            self.default_branch = branch;
            self
        }

        pub fn add_rel(&mut self, rel: meta::Relation) -> &mut Self {
            self.rel.push(rel);
            self
        }

        pub fn add_rels(&mut self, rels: &mut Vec<meta::Relation>) -> &mut Self {
            self.rel.append(rels);
            self
        }

        pub fn set_rels(&mut self, rels: Vec<meta::Relation>) -> &mut Self {
            self.rel = rels;
            self
        }

        pub fn init_project(
            self,
            paths: &Paths,
            sources: &git2::Repository,
        ) -> Result<ProjectId, Error> {
            let mut meta = meta::Project::new(&self.name, &PeerId::from(self.key.clone()));
            meta.default_branch = self.default_branch;
            meta.description = self.description;
            meta.rel = self.rel;

            GitProject::init(paths, &self.key, sources, meta, self.founder.clone())
        }
    }
}

fn commit_project_meta(
    repo: &git2::Repository,
    parent: &git2::Commit,
    pgp_key: &mut pgp::Key,
    msg: &str,
    meta: meta::Project,
) -> Result<git2::Oid, Error> {
    commit_meta(repo, parent, pgp_key, msg, meta, PROJECT_METADATA_FILE)
}

fn commit_contributor_meta(
    repo: &git2::Repository,
    parent: &git2::Commit,
    pgp_key: &mut pgp::Key,
    msg: &str,
    meta: meta::Contributor,
) -> Result<git2::Oid, Error> {
    commit_meta(repo, parent, pgp_key, msg, meta, CONTRIBUTOR_METADATA_FILE)
}

fn commit_meta<M>(
    repo: &git2::Repository,
    parent: &git2::Commit,
    pgp_key: &mut pgp::Key,
    msg: &str,
    meta: M,
    filename: &str,
) -> Result<git2::Oid, Error>
where
    M: Serialize,
{
    let blob_oid = {
        let mut blob = repo.blob_writer(None)?;
        let mut ser = serde_json::Serializer::with_formatter(&mut blob, CanonicalFormatter::new());
        meta.serialize(&mut ser)?;
        blob.commit()?
    };

    let tree = {
        let mut builder = repo.treebuilder(None)?;
        builder.insert(filename, blob_oid, 0o100_644)?;
        let oid = builder.write()?;
        repo.find_tree(oid)?
    };

    let author = {
        let uid = pgp_key
            .userids()
            .next()
            .ok_or(Error::MissingPgpUserId)
            .map(|binding| binding.userid())?;

        // FIXME: use `Option::flatten` once out of nightly
        let addr = if let Ok(Some(addr)) = uid.email() {
            Ok(addr)
        } else {
            Err(Error::MissingPgpAddr)
        }?;

        let name = if let Ok(Some(name)) = uid.name() {
            name
        } else {
            "Radicle".into()
        };

        git2::Signature::now(&name, &addr)
    }?;

    let commit = repo.commit_create_buffer(&author, &author, msg, &tree, &[parent])?;
    let sig = pgp_key.sign(&commit)?;

    Ok(repo.commit_signed(
        std::str::from_utf8(&commit).unwrap(),
        &sig.to_string(),
        None,
    )?)
}

fn register_project(
    paths: &Paths,
    pid: &ProjectId,
    default_branch: &str,
    sources: &git2::Repository,
) -> Result<(), Error> {
    // FIXME: It's unfortunate this is duplicated in `project::ProjectId::into_path`
    let dest = paths.projects_dir().join(pid.to_string());
    if dest.is_dir() {
        Err(Error::ProjectExists)
    } else {
        let _ = git2::Repository::init_opts(
            &dest,
            git2::RepositoryInitOptions::new()
                .bare(true)
                .no_reinit(true)
                .external_template(false)
                .initial_head(default_branch),
        )?;
        let mut remote = sources.remote(RAD_REMOTE_NAME, &dest.to_string_lossy())?;

        // Push the metadata
        remote.push(
            &[
                &to_refname(PROJECT_METADATA_BRANCH),
                &to_refname(CONTRIBUTOR_METADATA_BRANCH),
                &to_refname(default_branch),
            ],
            None,
        )?;

        // Set up fetchspecs to hide rad/* branches
        // FIXME: libgit2's `git_remote_create_with_fetchspec` is not available in
        // `git2-rs`, so we need to remove the default:
        sources.config()?.remove("remote.rad.fetch")?;
        sources.remote_add_fetch("rad", "+refs/heads/src/*:refs/remotes/rad/*")?;
        sources.remote_add_push("rad", "+refs/heads/*:refs/heads/src/*")?;

        Ok(())
    }
}

fn to_refname(branch_name: &str) -> String {
    format!("refs/heads/{}", branch_name)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use proptest::prelude::*;

    fn gen_oid() -> impl Strategy<Value = git2::Oid> {
        proptest::collection::vec(any::<u8>(), 1..32)
            .prop_map(|bytes| git2::Oid::hash_object(git2::ObjectType::Blob, &bytes).unwrap())
    }

    fn gen_projectid() -> impl Strategy<Value = ProjectId> {
        gen_oid().prop_map(ProjectId)
    }

    #[allow(clippy::unnecessary_operation)]
    proptest! {
        #[test]
        fn prop_projectid_roundtrip(pid in gen_projectid()) {
            let pid2 = ProjectId::from_str(&pid.to_string()).expect("Error parsing ProjectId");
            assert_eq!(pid, pid2)
        }
    }
}
