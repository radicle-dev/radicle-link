// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    future::Future,
    io,
    mem,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use bstr::{BString, ByteSlice as _};
use futures_lite::{
    future,
    io::{AsyncBufRead, AsyncRead, AsyncWrite},
};
use git_features::progress::{self, Progress};
use git_protocol::fetch::{
    response,
    Action,
    Arguments,
    Delegate,
    DelegateBlocking,
    LsRefsAction,
    Response,
};
use git_transport::client;
use pin_project::{pin_project, pinned_drop};

pub use git_hash::ObjectId;
pub use git_protocol::fetch::Ref;

use crate::{packwriter::PackWriter, transport};

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

    /// [`ObjectId`]s to send as `want` lines.
    pub wants: Vec<ObjectId>,

    /// [`ObjectId`]s to send as `have` lines.
    pub haves: Vec<ObjectId>,

    /// Known refs to ask the server to include in the packfile.
    pub want_refs: Vec<BString>,
}

/// Result of a succesful [`fetch`].
#[derive(Debug)]
pub struct Outputs<T> {
    /// The `wanted-refs` as acknowledged by the server.
    pub wanted_refs: Vec<Ref>,
    /// If a packfile was received successfully, some info about it.
    pub pack: Option<T>,
}

impl<T> Default for Outputs<T> {
    fn default() -> Self {
        Self {
            wanted_refs: Vec::new(),
            pack: None,
        }
    }
}

/// [`Delegate`] driving the fetch end of the [pack protocol].
///
/// [pack protocol]: https://git.kernel.org/pub/scm/git/git.git/tree/Documentation/technical/pack-protocol.txt
pub struct Fetch<P, O> {
    opt: Options,
    pack_writer: P,
    out: Outputs<O>,
}

impl<P, O> Fetch<P, O> {
    pub fn new(opt: Options, pack_writer: P) -> Self {
        Self {
            opt,
            pack_writer,
            out: Outputs::default(),
        }
    }

    pub fn outputs(&self) -> &Outputs<O> {
        &self.out
    }

    pub fn take_outputs(&mut self) -> Outputs<O> {
        mem::take(&mut self.out)
    }
}

impl<P: PackWriter> DelegateBlocking for Fetch<P, P::Output> {
    fn handshake_extra_parameters(&self) -> Vec<(String, Option<String>)> {
        self.opt.extra_params.clone()
    }

    fn prepare_ls_refs(
        &mut self,
        _: &client::Capabilities,
        _: &mut Vec<BString>,
        _: &mut Vec<(&str, Option<&str>)>,
    ) -> io::Result<LsRefsAction> {
        Ok(LsRefsAction::Skip)
    }

    fn prepare_fetch(
        &mut self,
        _: git_transport::Protocol,
        caps: &client::Capabilities,
        _: &mut Vec<(&str, Option<&str>)>,
        _: &[Ref],
    ) -> io::Result<Action> {
        if !self.opt.want_refs.is_empty() && !remote_supports_ref_in_want(caps) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "`want-ref`s given, but server does not support `ref-in-want`",
            ));
        }

        if self.opt.wants.is_empty() && self.opt.want_refs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "`fetch` is empty",
            ));
        }

        Ok(Action::Continue)
    }

    fn negotiate(
        &mut self,
        _: &[Ref],
        args: &mut Arguments,
        _: Option<&Response>,
    ) -> io::Result<Action> {
        for oid in &self.opt.wants {
            args.want(oid);
        }

        for oid in &self.opt.haves {
            args.have(oid)
        }

        for name in &self.opt.want_refs {
            // Work around `git-upload-pack` not handling namespaces properly,
            // cf. https://lore.kernel.org/git/CD2XNXHACAXS.13J6JTWZPO1JA@schmidt/
            let want_ref = format!("refs/namespaces/{}/{}", self.opt.repo, name);
            args.want_ref(BString::from(want_ref).as_bstr());
        }

        // send done, as we don't bother with further negotiation
        Ok(Action::Cancel)
    }

    fn indicate_client_done_when_fetch_completes(&self) -> bool {
        false
    }
}

#[async_trait(?Send)]
impl<P: PackWriter> Delegate for Fetch<P, P::Output> {
    async fn receive_pack(
        &mut self,
        pack: impl AsyncBufRead + Unpin + 'async_trait,
        prog: impl Progress,
        _: &[Ref],
        resp: &Response,
    ) -> io::Result<()> {
        // Strip any namespaces leaked by the other end due to workarounds
        let namespace = format!("refs/namespaces/{}/", self.opt.repo);
        self.out.wanted_refs.extend(resp.wanted_refs().iter().map(
            |response::WantedRef { id, path }| {
                Ref::Direct {
                    path: path
                        .strip_prefix(namespace.as_bytes())
                        .map(BString::from)
                        .unwrap_or_else(|| path.clone()),
                    object: *id,
                }
            },
        ));
        let out = self.pack_writer.write_pack(pack, prog)?;
        self.out.pack = Some(out);

        Ok(())
    }
}

/// Future created by the [`fetch`] function.
///
/// Ensures that a running inner [`PackWriter`] is cancelled when the
/// [`Fetching`] future is dropped without also dropping the [`AsyncRead`] data
/// source.
#[pin_project(PinnedDrop)]
struct Fetching<T> {
    stop: Arc<AtomicBool>,
    #[pin]
    task: T,
}

#[pinned_drop]
impl<T> PinnedDrop for Fetching<T> {
    fn drop(self: Pin<&mut Self>) {
        self.stop.store(true, Ordering::Release)
    }
}

impl<T> Future for Fetching<T>
where
    T: Future,
{
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.project().task.poll(cx)
    }
}

pub fn fetch<B, P, R, W>(
    opt: Options,
    build_pack_writer: B,
    recv: R,
    send: W,
) -> impl Future<Output = io::Result<Outputs<P::Output>>>
where
    B: FnOnce(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let task = blocking::unblock({
        let mut conn = transport::Stateless::new(opt.repo.clone(), recv, send);
        let pack_writer = build_pack_writer(Arc::clone(&stop));

        move || {
            let mut delegate = Fetch::new(opt, pack_writer);
            future::block_on(git_protocol::fetch(
                &mut conn,
                &mut delegate,
                |_| unreachable!("credentials helper requested"),
                progress::Discard,
            ))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            Ok(delegate.out)
        }
    });

    Fetching { stop, task }
}

fn remote_supports_ref_in_want(caps: &client::Capabilities) -> bool {
    caps.capability("fetch")
        .and_then(|cap| cap.supports("ref-in-want"))
        .unwrap_or(false)
}
