// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeSet,
    io,
    path::Path,
    process::ExitStatus,
    sync::{atomic::AtomicBool, Arc},
};

use bstr::ByteSlice as _;
use futures::{AsyncReadExt as _, TryFutureExt as _};
use radicle_link_git_protocol::{
    fetch,
    packwriter,
    upload_pack,
    ObjectId,
    PackWriter,
    Ref,
    WantRef,
};
use tempfile::{tempdir, TempDir};

fn upstream() -> TempDir {
    let tmp = tempdir().unwrap();

    let repo = git2::Repository::init_bare(&tmp).unwrap();
    let auth = git2::Signature::now("apollo", "apollo@cree.de").unwrap();

    let tree = {
        let empty = repo.treebuilder(None).unwrap();
        let oid = empty.write().unwrap();
        repo.find_tree(oid).unwrap()
    };
    let base = {
        let oid = repo
            .commit(
                Some("refs/namespaces/foo/refs/heads/main"),
                &auth,
                &auth,
                "initial",
                &tree,
                &[],
            )
            .unwrap();
        repo.find_commit(oid).unwrap()
    };
    let next = repo
        .commit(
            Some("refs/namespaces/foo/refs/heads/next"),
            &auth,
            &auth,
            "ng",
            &tree,
            &[&base],
        )
        .unwrap();
    repo.reference(
        "refs/namespaces/foo/refs/pulls/1/head",
        next,
        true,
        "pee arrr",
    )
    .unwrap();

    tmp
}

fn collect_refs(repo: &git2::Repository) -> Result<Vec<(String, git2::Oid)>, git2::Error> {
    repo.references()?
        .map(|x| x.map(|r| (r.name().unwrap().to_owned(), r.target().unwrap())))
        .collect()
}

fn update_tips<'a, T>(repo: &git2::Repository, tips: T) -> Result<(), anyhow::Error>
where
    T: IntoIterator<Item = &'a Ref>,
{
    for r in tips {
        match r {
            Ref::Direct { path, object } => {
                repo.reference(
                    path.to_str()?,
                    git2::Oid::from_bytes(object.as_slice())?,
                    true,
                    "",
                )?;
            },
            x => anyhow::bail!("unexpected ref variant: {:?}", x),
        }
    }

    Ok(())
}

fn collect_history(repo: &git2::Repository, tip: &str) -> Result<Vec<git2::Oid>, git2::Error> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_ref(tip)?;
    revwalk.collect()
}

fn want_id(r: &Ref) -> io::Result<Option<fetch::WantHave>> {
    Ok(Some(fetch::WantHave {
        want: *r.unpack().1,
        have: None,
    }))
}

fn run_fetch<R, F, B, P>(
    remote: R,
    opt: fetch::Options,
    filter_refs: F,
    build_pack_writer: B,
) -> io::Result<(fetch::Outputs<P::Output>, ExitStatus)>
where
    R: AsRef<Path>,
    F: Fn(&Ref) -> io::Result<Option<fetch::WantHave>> + Send + 'static,
    B: FnOnce(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
{
    let (client, server) = futures_ringbuf::Endpoint::pair(256, 256);
    let client = async move {
        let (recv, send) = client.split();
        fetch::fetch(opt, filter_refs, build_pack_writer, recv, send).await
    };
    let server = {
        let (recv, send) = server.split();
        upload_pack::upload_pack(&remote, recv, send).and_then(|(_hdr, run)| run)
    };

    futures::executor::block_on(futures::future::try_join(client, server))
}

#[test]
fn smoke() {
    let remote = upstream();
    let (client_out, server_out) = run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            ref_prefixes: Some(vec!["refs/heads/".into(), "refs/pulls/".into()]),
            want_refs: vec![],
            done_after_pack: true,
        },
        want_id,
        |_| packwriter::Discard,
    )
    .unwrap();

    assert!(server_out.success());
    assert!(client_out.pack.is_some());
    assert_eq!(
        client_out
            .refs
            .iter()
            .map(|r| r.unpack().0)
            .collect::<BTreeSet<_>>(),
        [
            "refs/heads/main".into(),
            "refs/heads/next".into(),
            "refs/pulls/1/head".into()
        ]
        .iter()
        .collect::<BTreeSet<_>>()
    )
}

#[test]
fn want_ref() {
    let remote = upstream();
    let (client_out, server_out) = run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            ref_prefixes: None,
            want_refs: vec![
                WantRef {
                    name: "refs/heads/main".into(),
                    have: None,
                },
                WantRef {
                    name: "refs/pulls/1/head".into(),
                    have: None,
                },
            ],
            done_after_pack: true,
        },
        want_id,
        |_| packwriter::Discard,
    )
    .unwrap();

    assert!(server_out.success());
    assert!(client_out.pack.is_some());
    assert_eq!(
        client_out
            .refs
            .iter()
            .map(|r| r.unpack().0)
            .collect::<BTreeSet<_>>(),
        ["refs/heads/main".into(), "refs/pulls/1/head".into(),]
            .iter()
            .collect::<BTreeSet<_>>()
    )
}

#[test]
fn empty_fetch() {
    let remote = upstream();
    let (client_out, server_out) = run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            ref_prefixes: None,
            want_refs: vec![],
            done_after_pack: true,
        },
        want_id,
        |_| packwriter::Discard,
    )
    .unwrap();

    assert!(server_out.success());
    assert!(client_out.pack.is_none());
}

fn clone_with<R, L, B, P>(remote: R, local: L, build_pack_writer: B)
where
    R: AsRef<Path>,
    L: AsRef<Path>,
    B: FnOnce(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
{
    let (client_out, server_out) = run_fetch(
        &remote,
        fetch::Options {
            repo: "foo".into(),
            extra_params: vec![],
            ref_prefixes: Some(vec![]),
            want_refs: vec![],
            done_after_pack: true,
        },
        want_id,
        build_pack_writer,
    )
    .unwrap();

    assert!(server_out.success());
    assert!(client_out.pack.is_some());

    let remote_repo = git2::Repository::open(remote).unwrap();
    remote_repo.set_namespace("foo").unwrap();
    let local_repo = git2::Repository::open(&local).unwrap();

    update_tips(&local_repo, &client_out.refs).unwrap();

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

    clone_with(&remote, &local, move |stop| {
        packwriter::Libgit::new(packwriter::Options::default(), local_repo, stop)
    })
}

#[test]
fn clone_gitoxide() {
    let remote = upstream();
    let local = tempdir().unwrap();
    let local_repo = git2::Repository::init(&local).unwrap();

    clone_with(&remote, &local, move |stop| {
        packwriter::Standard::new(local_repo.path(), packwriter::Options::default(), stop)
    })
}

fn thin_pack_with<R, L, B, P>(remote: R, local: L, build_pack_writer: B)
where
    R: AsRef<Path>,
    L: AsRef<Path>,
    B: Fn(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
{
    // Clone main only
    {
        let (client_out, server_out) = run_fetch(
            &remote,
            fetch::Options {
                repo: "foo".into(),
                extra_params: vec![],
                ref_prefixes: None,
                want_refs: vec![WantRef {
                    name: "refs/heads/main".into(),
                    have: None,
                }],
                done_after_pack: true,
            },
            want_id,
            &build_pack_writer,
        )
        .unwrap();

        assert!(server_out.success());
        assert!(client_out.pack.is_some());
    }

    let remote_repo = git2::Repository::open(&remote).unwrap();
    remote_repo.set_namespace("foo").unwrap();
    let local_repo = git2::Repository::open(&local).unwrap();

    // Fetch next, which is ahead of main
    {
        let head = remote_repo.refname_to_id("refs/heads/main").unwrap();
        let (client_out, server_out) = run_fetch(
            &remote,
            fetch::Options {
                repo: "foo".into(),
                extra_params: vec![],
                ref_prefixes: None,
                want_refs: vec![WantRef {
                    name: "refs/heads/next".into(),
                    have: Some(ObjectId::from_20_bytes(head.as_bytes())),
                }],
                done_after_pack: true,
            },
            want_id,
            build_pack_writer,
        )
        .unwrap();
        assert!(server_out.success());
        assert!(client_out.pack.is_some());

        update_tips(&local_repo, &client_out.refs).unwrap();
    }

    let remote_history = collect_history(&remote_repo, "refs/heads/next").unwrap();
    let local_history = collect_history(&local_repo, "refs/heads/next").unwrap();

    assert!(!remote_history.is_empty());
    assert_eq!(remote_history, local_history)
}

#[test]
fn thin_pack_libgit() {
    let remote = upstream();
    let local = tempdir().unwrap();

    thin_pack_with(&remote, &local, |stop| {
        let local_repo = git2::Repository::init(&local).unwrap();
        packwriter::Libgit::new(packwriter::Options::default(), local_repo, stop)
    })
}

#[test]
fn thin_pack_gitoxide() {
    let remote = upstream();
    let local = tempdir().unwrap();
    let local_repo = git2::Repository::init(&local).unwrap();
    let git_dir = local_repo.path().to_owned();

    thin_pack_with(&remote, &local, move |stop| {
        packwriter::Standard::new(git_dir.clone(), packwriter::Options::default(), stop)
    })
}
