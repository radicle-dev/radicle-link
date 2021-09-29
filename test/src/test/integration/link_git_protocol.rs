// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

use bstr::ByteSlice as _;
use futures::{AsyncReadExt as _, TryFutureExt as _};
use git_repository::{
    self as git,
    prelude::*,
    refs::transaction::{Change, PreviousValue, RefEdit},
};
use link_git_protocol::{fetch, ls, packwriter, upload_pack, ObjectId, PackWriter, Ref};
use tempfile::{tempdir, TempDir};

fn upstream() -> TempDir {
    let tmp = tempdir().unwrap();

    let repo = git::init_bare(&tmp).unwrap().into_easy();
    let auth = git::actor::Signature::now_local_or_utc("apollo", "apollo@cree.de");

    let empty_tree_id = repo
        .write_object(&git::objs::Tree::empty())
        .unwrap()
        .detach();
    let base_id = {
        repo.commit(
            "refs/namespaces/foo/refs/heads/main",
            &auth.to_ref(),
            &auth.to_ref(),
            "initial",
            empty_tree_id,
            git::commit::NO_PARENT_IDS,
        )
        .unwrap()
    };
    let next_commit_id = repo
        .commit(
            "refs/namespaces/foo/refs/heads/next",
            &auth.to_ref(),
            &auth.to_ref(),
            "ng",
            empty_tree_id,
            [base_id],
        )
        .unwrap()
        .detach();
    repo.reference(
        "refs/namespaces/foo/refs/pulls/1/head",
        next_commit_id,
        PreviousValue::Any,
        "pee arrr",
    )
    .unwrap();

    tmp
}

fn collect_refs(repo: &impl git::easy::Access) -> anyhow::Result<Vec<git::refs::Reference>> {
    repo.references()?
        .all()?
        .map(|x| x.map(|r| r.detach()).map_err(|err| anyhow::anyhow!(err)))
        .collect()
}

fn update_tips<'a, T>(repo: &impl git::easy::Access, tips: T) -> Result<(), anyhow::Error>
where
    T: IntoIterator<Item = &'a Ref>,
{
    use std::convert::TryInto;
    let edits = tips
        .into_iter()
        .map(|r| match r {
            Ref::Direct { path, object } => Ok(RefEdit {
                change: Change::Update {
                    log: Default::default(),
                    expected: PreviousValue::Any,
                    new: object.to_owned().into(),
                },
                name: path.as_bstr().try_into()?,
                deref: false,
            }),
            x => anyhow::bail!("unexpected ref variant: {:?}", x),
        })
        .collect::<Result<Vec<_>, _>>()?;
    repo.edit_references(edits, git::lock::acquire::Fail::Immediately, None)?;
    Ok(())
}

fn collect_history(
    repo: &impl git::easy::Access,
    tip: &str,
) -> anyhow::Result<Vec<git::hash::ObjectId>> {
    repo.find_reference(tip)?
        .into_fully_peeled_id()?
        .ancestors()?
        .all()
        .map(|res| res.map(|oid| oid.detach()).map_err(Into::into))
        .collect()
}

fn run_ls_refs<R: AsRef<Path>>(remote: R, opt: ls::Options) -> io::Result<Vec<Ref>> {
    let (client, server) = futures_ringbuf::Endpoint::pair(256, 256);
    let client = async move {
        let (recv, send) = client.split();
        ls::ls_refs(opt, recv, send).await
    };
    let server = {
        let (recv, send) = server.split();
        upload_pack::upload_pack(&remote, recv, send).and_then(|(_hdr, run)| run)
    };

    let (client_out, server_out) =
        futures::executor::block_on(futures::future::try_join(client, server))?;
    assert!(server_out.success());
    Ok(client_out)
}

fn run_fetch<R, B, P>(
    remote: R,
    opt: fetch::Options,
    build_pack_writer: B,
) -> io::Result<fetch::Outputs<P::Output>>
where
    R: AsRef<Path>,
    B: FnOnce(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
{
    let (client, server) = futures_ringbuf::Endpoint::pair(256, 256);
    let client = async move {
        let (recv, send) = client.split();
        fetch::fetch(opt, build_pack_writer, recv, send).await
    };
    let server = {
        let (recv, send) = server.split();
        upload_pack::upload_pack(&remote, recv, send).and_then(|(_hdr, run)| run)
    };

    let (client_out, server_out) =
        futures::executor::block_on(futures::future::try_join(client, server))?;
    assert!(server_out.success());
    Ok(client_out)
}

#[test]
fn smoke() {
    let remote = upstream();
    let refs = run_ls_refs(
        &remote,
        ls::Options {
            repo: "foo".into(),
            extra_params: vec![],
            ref_prefixes: vec!["refs/heads/".into(), "refs/pulls/".into()],
        },
    )
    .unwrap();

    assert_eq!(
        refs.iter().map(|r| r.unpack().0).collect::<BTreeSet<_>>(),
        [
            "refs/heads/main".into(),
            "refs/heads/next".into(),
            "refs/pulls/1/head".into()
        ]
        .iter()
        .collect::<BTreeSet<_>>()
    );

    let out = run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            haves: vec![],
            wants: vec![],
            want_refs: refs.iter().map(|r| r.unpack().0.clone()).collect(),
        },
        |_| packwriter::Discard,
    )
    .unwrap();

    assert!(out.pack.is_some());
}

#[test]
fn want_ref() {
    let remote = upstream();
    let out = run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            haves: vec![],
            wants: vec![],
            want_refs: vec!["refs/heads/main".into(), "refs/pulls/1/head".into()],
        },
        |_| packwriter::Discard,
    )
    .unwrap();

    assert!(out.pack.is_some());
    assert_eq!(
        out.wanted_refs
            .iter()
            .map(|r| r.unpack().0)
            .collect::<BTreeSet<_>>(),
        ["refs/heads/main".into(), "refs/pulls/1/head".into(),]
            .iter()
            .collect::<BTreeSet<_>>()
    )
}

#[test]
#[should_panic(expected = "`fetch` is empty")]
fn empty_fetch() {
    let remote = upstream();
    run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            haves: vec![],
            wants: vec![],
            want_refs: vec![],
        },
        |_| packwriter::Discard,
    )
    .unwrap();
}

fn clone_with<R, L, B, P>(remote: R, local: L, build_pack_writer: B)
where
    R: Into<PathBuf>,
    L: Into<PathBuf>,
    B: FnOnce(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
{
    let (remote, local) = (remote.into(), local.into());
    let refs = run_ls_refs(
        &remote,
        ls::Options {
            repo: "foo".into(),
            extra_params: vec![],
            ref_prefixes: vec!["refs/heads/".into(), "refs/pulls/".into()],
        },
    )
    .unwrap();
    let out = run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            haves: vec![],
            wants: vec![],
            want_refs: refs.iter().map(|r| r.unpack().0.clone()).collect(),
        },
        build_pack_writer,
    )
    .unwrap();

    assert!(out.pack.is_some());

    let mut remote_repo = git::open(remote).unwrap().into_easy_arc_exclusive();
    remote_repo.set_namespace("foo").unwrap();
    let local_repo = git::open(local).unwrap().into_easy();

    update_tips(&local_repo, &out.wanted_refs).unwrap();

    let mut remote_refs = collect_refs(&remote_repo).unwrap();
    let mut local_refs = collect_refs(&local_repo).unwrap();

    remote_refs.sort();
    local_refs.sort();

    assert_eq!(remote_refs, local_refs);
}

#[test]
fn clone_libgit() {
    let remote = upstream();
    let local = tempdir().unwrap();
    let local_repo = git2::Repository::init(&local).unwrap();

    clone_with(remote.path(), local.path(), move |stop| {
        packwriter::Libgit::new(packwriter::Options::default(), local_repo, stop)
    })
}

#[test]
fn clone_gitoxide() {
    let remote = upstream();
    let local = tempdir().unwrap();
    let local_repo = git::init(&local).unwrap();

    clone_with(remote.path(), &local.path(), move |stop| {
        let git_dir = local_repo.path();
        packwriter::Standard::new(
            git_dir,
            packwriter::Options::default(),
            packwriter::StandardThickener::new(git_dir),
            stop,
        )
    })
}

fn thin_pack_with<R, L, B, P>(remote: R, local: L, build_pack_writer: B)
where
    R: Into<PathBuf>,
    L: Into<PathBuf>,
    B: Fn(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
{
    let (remote, local) = (remote.into(), local.into());
    // Clone main only
    {
        let out = run_fetch(
            &remote,
            fetch::Options {
                repo: "foo".into(),
                extra_params: vec![],
                haves: vec![],
                wants: vec![],
                want_refs: vec!["refs/heads/main".into()],
            },
            &build_pack_writer,
        )
        .unwrap();
        assert!(out.pack.is_some());
    }

    let mut remote_repo = git::open(remote.clone()).unwrap().into_easy_arc_exclusive(); // TODO: use `into_easy()` once GATs have landed
    remote_repo.set_namespace("foo").unwrap();
    let local_repo = git::open(local).unwrap().into_easy_arc_exclusive();

    // Fetch next, which is ahead of main
    {
        let head = remote_repo
            .find_reference("main")
            .unwrap()
            .into_fully_peeled_id()
            .unwrap();
        let out = run_fetch(
            &remote,
            fetch::Options {
                repo: "foo".into(),
                extra_params: vec![],
                haves: vec![ObjectId::from_20_bytes(head.as_bytes())],
                wants: vec![],
                want_refs: vec!["refs/heads/next".into()],
            },
            build_pack_writer,
        )
        .unwrap();
        assert!(out.pack.is_some());

        update_tips(&local_repo, &out.wanted_refs).unwrap();
    }

    // Need to refresh it as it didn't notice the new pack
    local_repo.refresh_object_database().unwrap();
    let remote_history = collect_history(&remote_repo, "refs/heads/next").unwrap();
    let local_history = collect_history(&local_repo, "refs/heads/next").unwrap();

    assert!(!remote_history.is_empty());
    assert_eq!(remote_history, local_history)
}

#[test]
fn thin_pack_libgit() {
    let remote = upstream();
    let local = tempdir().unwrap();

    thin_pack_with(remote.path(), local.path(), |stop| {
        let local_repo = git2::Repository::init(&local).unwrap();
        packwriter::Libgit::new(packwriter::Options::default(), local_repo, stop)
    })
}

#[test]
fn thin_pack_gitoxide() {
    let remote = upstream();
    let local = tempdir().unwrap();
    let local_repo = git::init(&local).unwrap();

    thin_pack_with(remote.path(), local.path(), move |stop| {
        let git_dir = local_repo.path();
        packwriter::Standard::new(
            git_dir,
            packwriter::Options::default(),
            packwriter::StandardThickener::new(git_dir),
            stop,
        )
    });
}
