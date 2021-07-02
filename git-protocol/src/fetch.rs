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
use versions::Version;

pub use git_hash::ObjectId;
pub use git_protocol::fetch::Ref;

use crate::packwriter::PackWriter;

/// A known reference to fetch.
///
/// Can be used to bypass `ls-refs` entirely via the `ref-in-want` feature, if
/// all wanted refs are known a priori.
#[derive(Debug)]
pub struct WantRef {
    /// Name of the ref.
    ///
    /// Must be in canonical form, eg. `refs/heads/pu`, not `pu`.
    pub name: BString,
    /// The local tip of the ref, if available.
    ///
    /// If the ref is already present locally, the remote end will be told that
    /// we `have` the `oid` already, so as to allow it to assemble a smaller
    /// packfile.
    pub have: Option<ObjectId>,
}

/// A reference to fetch.
///
/// This is the result of the `filter` function passed to [`Fetch`]: the refs
/// advertised by the remote (if and when `ls-refs` is executed) are passed
/// through the `filter` for packfile negotiation.
#[derive(Debug)]
pub struct WantHave {
    /// Tip of the ref the server said it has, and which we want.
    pub want: ObjectId,
    /// Local tip of the ref, if we have it.
    pub have: Option<ObjectId>,
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
    /// If this is `None`, the `ls-refs` command will not be sent. If it is
    /// `Some`, but contains an empty [`Vec`], the server is asked to return
    /// all refs it knows about. Otherwise, the server is asked to only
    /// return refs matching the given prefixes.
    pub ref_prefixes: Option<Vec<BString>>,

    /// Known refs to ask the server to include in the packfile.
    ///
    /// If both `want_refs` and `ref_prefixes` are empty, it is unlikely that
    /// the server will send a packfile at all.
    pub want_refs: Vec<WantRef>,

    /// Whether to indicate to the server that we're done after having received
    /// a packfile.
    ///
    /// If `true`, send a FLUSH packet after the packfile was received. This
    /// will cause the remote end to close the stream, so it cannot be
    /// reused. `false` does not send a FLUSH packet, allowing to send
    /// further commands over the same stream.
    pub done_after_pack: bool,
}

/// Result of a succesful [`fetch`].
#[derive(Debug)]
pub struct Outputs<T> {
    /// The refs contained in the packfile (if not empty).
    ///
    /// These are the refs advertised in response to `ls-refs`, filtered through
    /// the `filter_refs` function of [`Fetch`], and any `wanted-ref`s in
    /// response to `want-ref`.
    pub refs: Vec<Ref>,
    /// If a packfile was received successfully, some info about it.
    pub pack: Option<T>,
}

impl<T> Default for Outputs<T> {
    fn default() -> Self {
        Self {
            refs: Vec::default(),
            pack: None,
        }
    }
}

/// [`Delegate`] driving the fetch end of the [pack protocol].
///
/// [pack protocol]: https://git.kernel.org/pub/scm/git/git.git/tree/Documentation/technical/pack-protocol.txt
pub struct Fetch<F, P, O> {
    opt: Options,
    filter_refs: F,
    pack_writer: P,
    out: Outputs<O>,
}

impl<F, P, O> Fetch<F, P, O> {
    pub fn new(opt: Options, filter_refs: F, pack_writer: P) -> Self {
        Self {
            opt,
            filter_refs,
            pack_writer,
            out: Outputs::default(),
        }
    }

    pub fn with_filter_refs<G>(self, filter_refs: G) -> Fetch<G, P, O> {
        Fetch {
            opt: self.opt,
            filter_refs,
            pack_writer: self.pack_writer,
            out: self.out,
        }
    }

    pub fn outputs(&self) -> &Outputs<O> {
        &self.out
    }

    pub fn take_outputs(&mut self) -> Outputs<O> {
        mem::take(&mut self.out)
    }
}

impl<F, P> DelegateBlocking for Fetch<F, P, P::Output>
where
    F: Fn(&Ref) -> io::Result<Option<WantHave>>,
    P: PackWriter,
{
    fn handshake_extra_parameters(&self) -> Vec<(String, Option<String>)> {
        self.opt.extra_params.clone()
    }

    fn prepare_ls_refs(
        &mut self,
        caps: &client::Capabilities,
        args: &mut Vec<BString>,
        _: &mut Vec<(&str, Option<&str>)>,
    ) -> io::Result<LsRefsAction> {
        let act = match &self.opt.ref_prefixes {
            None => LsRefsAction::Skip,
            Some(prefixes) => {
                // Work around `git-upload-pack` not handling namespaces properly
                // before git/2.31
                //
                // cf. https://lore.kernel.org/git/pMV5dJabxOBTD8kJBaPuWK0aS6OJhRQ7YFGwfhPCeSJEbPDrIFBza36nXBCgUCeUJWGmpjPI1rlOGvZJEh71Ruz4SqljndUwOCoBUDRHRDU=@eagain.st/
                let must_namespace = remote_git_version(caps)
                    .map(|version| version < Version::new("2.31.0").unwrap())
                    .unwrap_or(false);
                for prefix in prefixes {
                    let prefix = if must_namespace {
                        format!("ref-prefix refs/namespaces/{}/{}", self.opt.repo, prefix)
                    } else {
                        format!("ref-prefix {}", prefix)
                    };
                    args.push(prefix.into())
                }
                LsRefsAction::Continue
            },
        };

        Ok(act)
    }

    fn prepare_fetch(
        &mut self,
        _: git_transport::Protocol,
        caps: &client::Capabilities,
        _: &mut Vec<(&str, Option<&str>)>,
        refs: &[Ref],
    ) -> io::Result<Action> {
        if !self.opt.want_refs.is_empty() && !remote_supports_ref_in_want(caps) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "`want-ref`s given, but server does not support `ref-in-want`",
            ));
        }

        // If we don't send any `have`s, and no `want-ref`s, the server will
        // just ignore the fetch command and wait for the next. This would cause
        // `git_protocol::fetch` to hang indefinitely, so abort here.
        let act = if refs.is_empty() && self.opt.want_refs.is_empty() {
            Action::Cancel
        } else {
            Action::Continue
        };

        Ok(act)
    }

    fn negotiate(
        &mut self,
        refs: &[Ref],
        args: &mut Arguments,
        _: Option<&Response>,
    ) -> io::Result<Action> {
        for r in refs {
            if let Some(WantHave { want, have }) = (self.filter_refs)(r)? {
                self.out.refs.push(r.to_owned());
                args.want(want);
                if let Some(oid) = have {
                    args.have(oid);
                }
            }
        }

        for WantRef { name, have } in &self.opt.want_refs {
            // Work around `git-upload-pack` not handling namespaces properly,
            // cf. https://lore.kernel.org/git/CD2XNXHACAXS.13J6JTWZPO1JA@schmidt/
            let want_ref = format!("refs/namespaces/{}/{}", self.opt.repo, name);
            args.want_ref(BString::from(want_ref).as_bstr());
            if let Some(oid) = have {
                args.have(oid);
            }
        }

        // send done, as we don't bother with further negotiation
        Ok(Action::Cancel)
    }

    fn indicate_client_done_when_fetch_completes(&self) -> bool {
        self.opt.done_after_pack
    }
}

#[async_trait(?Send)]
impl<F, P> Delegate for Fetch<F, P, P::Output>
where
    F: Fn(&Ref) -> io::Result<Option<WantHave>>,
    P: PackWriter,
{
    async fn receive_pack(
        &mut self,
        pack: impl AsyncBufRead + Unpin + 'async_trait,
        prog: impl Progress,
        _: &[Ref],
        resp: &Response,
    ) -> io::Result<()> {
        // Strip any namespaces leaked by the other end due to workarounds
        let namespace = format!("refs/namespaces/{}/", self.opt.repo);
        self.out.refs.extend(
            resp.wanted_refs()
                .iter()
                .map(|response::WantedRef { id, path }| Ref::Direct {
                    path: path
                        .strip_prefix(namespace.as_bytes())
                        .map(BString::from)
                        .unwrap_or_else(|| path.clone()),
                    object: *id,
                }),
        );
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

pub fn fetch<F, B, P, R, W>(
    opt: Options,
    filter_refs: F,
    build_pack_writer: B,
    recv: R,
    send: W,
) -> impl Future<Output = io::Result<Outputs<P::Output>>>
where
    F: Fn(&Ref) -> io::Result<Option<WantHave>> + Send + 'static,
    B: FnOnce(Arc<AtomicBool>) -> P,
    P: PackWriter + Send + 'static,
    P::Output: Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    use git_transport::{
        client::git::{ConnectMode, Connection},
        Protocol,
    };

    let stop = Arc::new(AtomicBool::new(false));
    let task = blocking::unblock({
        let mut conn = Connection::new(
            recv,
            send,
            Protocol::V2,
            opt.repo.clone(),
            None::<(String, Option<u16>)>,
            ConnectMode::Daemon,
        );
        let pack_writer = build_pack_writer(Arc::clone(&stop));

        move || {
            let mut delegate = Fetch::new(opt, filter_refs, pack_writer);
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

fn remote_git_version(caps: &client::Capabilities) -> Option<Version> {
    let agent = caps.capability("agent").and_then(|cap| {
        cap.value()
            .and_then(|bs| bs.to_str().map(|s| s.to_owned()).ok())
    })?;
    Version::new(agent.strip_prefix("git/")?)
}

fn remote_supports_ref_in_want(caps: &client::Capabilities) -> bool {
    caps.capability("fetch")
        .and_then(|cap| cap.supports("ref-in-want"))
        .unwrap_or(false)
}
