// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use bstr::{BString, ByteVec as _};
use futures_lite::io::{AsyncBufRead, AsyncRead, AsyncWrite};
use git_features::progress::{self, Progress};
use git_protocol::{
    fetch::{Action, Arguments, Delegate, DelegateBlocking, LsRefsAction, Response},
    transport::client,
};
use once_cell::sync::Lazy;
use versions::Version;

pub use git_protocol::fetch::Ref;

use super::{remote_git_version, transport};

// Work around `git-upload-pack` not handling namespaces properly
//
// cf. https://lore.kernel.org/git/pMV5dJabxOBTD8kJBaPuWK0aS6OJhRQ7YFGwfhPCeSJEbPDrIFBza36nXBCgUCeUJWGmpjPI1rlOGvZJEh71Ruz4SqljndUwOCoBUDRHRDU=@eagain.st/
//
// Based on testing with git 2.25.1 in Ubuntu 20.04, this workaround is
// not needed. Hence the checked version is lowered to 2.25.0.
fn must_namespace(caps: &client::Capabilities) -> bool {
    static MIN_GIT_VERSION_NAMESPACES: Lazy<Version> =
        Lazy::new(|| Version::new("2.25.0").unwrap());

    remote_git_version(caps)
        .map(|version| version < *MIN_GIT_VERSION_NAMESPACES)
        .unwrap_or(false)
}

#[derive(Debug)]
pub struct Options {
    /// The remote (logical) repository to fetch from.
    ///
    /// Normally, this is the path to a repo on the remote side (eg.
    /// `/git.git`). `radicle-link` serves only a single namespaced repo, so
    /// this value should be the name of a namespace.
    pub repo: BString,

    /// [Extra Parameters][extra] to send with the initial transport header.
    ///
    /// [extra]: https://git.kernel.org/pub/scm/git/git.git/tree/Documentation/technical/pack-protocol.txt#n52
    pub extra_params: Vec<(String, Option<String>)>,

    /// Prefixes of refs to ask the server to advertise via `ls-refs`.
    ///
    /// If the [`Vec`] is empty, the server is asked to return all refs it knows
    /// about. Otherwise, the server is asked to only return refs matching
    /// the given prefixes.
    pub ref_prefixes: Vec<BString>,
}

/// [`Delegate`] for running a stateless `ls-refs` command.
pub struct LsRefs {
    opt: Options,
    out: Vec<Ref>,
}

impl LsRefs {
    pub fn new(opt: Options) -> Self {
        Self {
            opt,
            out: Vec::new(),
        }
    }
}

impl DelegateBlocking for LsRefs {
    fn handshake_extra_parameters(&self) -> Vec<(String, Option<String>)> {
        self.opt.extra_params.clone()
    }

    fn prepare_ls_refs(
        &mut self,
        caps: &client::Capabilities,
        args: &mut Vec<BString>,
        _: &mut Vec<(&str, Option<&str>)>,
    ) -> io::Result<LsRefsAction> {
        let must_namespace = must_namespace(caps);
        for prefix in &self.opt.ref_prefixes {
            let mut arg = BString::from("ref-prefix ");
            if must_namespace {
                arg.push_str("refs/namespaces/");
                arg.push_str(&self.opt.repo);
                arg.push_char('/');
            }
            arg.push_str(prefix);
            args.push(arg)
        }
        Ok(LsRefsAction::Continue)
    }

    fn prepare_fetch(
        &mut self,
        _: git_protocol::transport::Protocol,
        _: &client::Capabilities,
        _: &mut Vec<(&str, Option<&str>)>,
        refs: &[Ref],
    ) -> io::Result<Action> {
        self.out.extend_from_slice(refs);
        Ok(Action::Cancel)
    }

    fn negotiate(
        &mut self,
        _: &[Ref],
        _: &mut Arguments,
        _: Option<&Response>,
    ) -> io::Result<Action> {
        unreachable!("`negotiate` called even though no `fetch` command was sent")
    }
}

#[async_trait(?Send)]
impl Delegate for LsRefs {
    async fn receive_pack(
        &mut self,
        _: impl AsyncBufRead + Unpin + 'async_trait,
        _: impl Progress,
        _: &[Ref],
        _: &Response,
    ) -> io::Result<()> {
        unreachable!("`receive_pack` called even though no `fetch` command was sent")
    }
}

pub async fn ls_refs<R, W>(opt: Options, recv: R, send: W) -> io::Result<Vec<Ref>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut conn = transport::Stateless::new(opt.repo.clone(), recv, send);
    let mut delegate = LsRefs::new(opt);
    git_protocol::fetch(
        &mut conn,
        &mut delegate,
        |_| unreachable!("credentials helper requested"),
        progress::Discard,
        git_protocol::FetchConnection::AllowReuse,
    )
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(delegate.out)
}
