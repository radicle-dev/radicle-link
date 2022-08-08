// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, path::PathBuf, sync::Arc};

use futures::StreamExt as _;
use lnk_clib::seed::Seeds;
use tracing::instrument;

use librad::{
    git::{
        refs::{self, Refs},
        storage,
        Urn,
    },
    git_ext as ext,
    net::{peer::Client, protocol::request_pull, quic},
};
use link_async::Spawner;
use linkd_lib::api::client::Reply;

pub mod error;
mod progress;
pub(crate) use progress::{report, Progress, ProgressReporter};

#[derive(Clone)]
pub(crate) struct Hooks<Signer> {
    spawner: Arc<Spawner>,
    client: Client<Signer, quic::SendOnly>,
    seeds: Seeds,
    pool: Arc<storage::Pool<storage::Storage>>,
    post_receive: PostReceive,
    pre_upload: PreUpload,
}

impl<S> fmt::Debug for Hooks<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hooks")
            .field("post_receive", &self.post_receive)
            .finish()
    }
}

const LINKD_CLIENT_NAME: &str = "lnk-gitd";

impl<S> Hooks<S>
where
    S: librad::Signer + Clone,
{
    pub(crate) fn new(
        spawner: Arc<Spawner>,
        client: Client<S, quic::SendOnly>,
        seeds: Seeds,
        pool: Arc<storage::Pool<storage::Storage>>,
        post_receive: PostReceive,
        pre_upload: PreUpload,
    ) -> Self {
        Self {
            spawner,
            client,
            seeds,
            pool,
            post_receive,
            pre_upload,
        }
    }

    #[instrument(skip(self, reporter), err)]
    pub(crate) async fn post_receive<P, E>(
        &self,
        reporter: &mut P,
        urn: Urn,
    ) -> Result<(), error::PostReceive<E>>
    where
        E: std::error::Error + Send + 'static,
        P: ProgressReporter<Error = E>,
    {
        // update the signed refs before a possible request_pull,
        // so that the peer can receive the latest refs.
        let at = update_signed_refs(
            reporter,
            self.spawner.clone(),
            self.pool.clone(),
            urn.clone(),
        )
        .await?;

        if self.post_receive.request_pull {
            tracing::info!("executing request-pull");
            request_pull(reporter, &self.client, &self.seeds, urn.clone()).await?;
        } else {
            report(
                reporter,
                "skipping request-pull, use `--push-seeds` if you wish to execute this step",
            )
            .await?;
        }
        let at = match at {
            Some(at) => at,
            None => return Ok(()),
        };
        if let Some(ann) = &self.post_receive.announce {
            announce(reporter, ann, urn, at).await?;
        } else {
            report(
                reporter,
                "skipping announce, use `--announce-on-push` if you wish to execute this step",
            )
            .await?;
        }
        Ok(())
    }

    #[instrument(skip(self, reporter))]
    pub(crate) async fn pre_upload<
        E: std::error::Error + Send + 'static,
        P: ProgressReporter<Error = E>,
    >(
        &self,
        reporter: &mut P,
        urn: Urn,
    ) -> Result<(), error::Progress<E>> {
        if self.pre_upload.replicate {
            replicate(reporter, &self.client, &self.seeds, urn).await?;
        } else {
            report(
                reporter,
                "skipping replication, use `--fetch-seeds` if you wish to execute this step",
            )
            .await?;
        }
        Ok(())
    }
}

async fn replicate<S, P, E>(
    reporter: &mut P,
    client: &Client<S, quic::SendOnly>,
    seeds: &Seeds,
    urn: Urn,
) -> Result<(), error::Progress<E>>
where
    S: librad::Signer + Clone,
    P: ProgressReporter<Error = E>,
    E: std::error::Error + Send + 'static,
{
    for seed in seeds {
        let from = (seed.peer, seed.addrs.clone());
        if let Some(label) = &seed.label {
            report(
                reporter,
                format!("replicating from `{}` label: {}", seed.peer, label),
            )
            .await?
        } else {
            report(reporter, format!("replicating from `{}`", seed.peer)).await?
        }
        match client.replicate(from, urn.clone(), None).await {
            Ok(result) => report(reporter, progress::Namespaced::new(&urn, &result)).await?,
            Err(err) => {
                report(
                    reporter,
                    format!("failed to replicate from `{}`: {}", seed.peer, err),
                )
                .await?
            },
        }
    }
    Ok(())
}

async fn announce<P, E>(
    reporter: &mut P,
    Announce { rpc_socket_path }: &Announce,
    urn: Urn,
    at: ext::Oid,
) -> Result<(), error::Announce<E>>
where
    P: ProgressReporter<Error = E>,
    E: std::error::Error + Send + 'static,
{
    tracing::info!("running post receive announcement hook");
    report(reporter, "announcing new refs").await?;
    tracing::trace!(?rpc_socket_path, "attempting to send announcement");
    let conn = linkd_lib::api::client::Connection::connect(LINKD_CLIENT_NAME, rpc_socket_path)
        .await
        .map_err(error::Announce::LinkdConnect)?;
    let cmd = linkd_lib::api::client::Command::announce(urn.clone(), at);
    let mut replies = cmd
        .execute_with_reply(conn)
        .await
        .map_err(error::Announce::LinkdTransport)?;
    loop {
        match replies.next().await {
            Ok(Reply::Progress {
                replies: next_replies,
                msg,
            }) => {
                tracing::trace!(?msg, "got progress message from linkd node");
                report(reporter, msg).await?;
                replies = next_replies;
            },
            Ok(Reply::Success { .. }) => {
                tracing::trace!("got success from linkd node");
                report(reporter, "successfully announced refs").await?;
                return Ok(());
            },
            Ok(Reply::Error { msg, .. }) => {
                tracing::error!(?msg, "got error from linkd node");
                return Err(error::Announce::Linkd(msg));
            },
            Err((_, e)) => {
                tracing::error!(err=?e, "error communicating with linkd node");
                return Err(error::Announce::LinkdTransport(e));
            },
        }
    }
}

async fn update_signed_refs<P, E>(
    reporter: &mut P,
    spawner: Arc<Spawner>,
    pool: Arc<storage::Pool<storage::Storage>>,
    urn: Urn,
) -> Result<Option<ext::Oid>, error::UpdateSignedRefs<E>>
where
    P: ProgressReporter<Error = E>,
    E: std::error::Error + Send + 'static,
{
    // Update `rad/signed_refs`
    report(reporter, "updating signed refs").await?;
    let update_result = {
        let storage = pool.get().await?;
        spawner
            .blocking::<_, Result<_, refs::stored::Error>>(move || {
                Refs::update(storage.as_ref(), &urn)
            })
            .await
    }?;
    let (at, msg) = match update_result {
        refs::Updated::Updated { at, .. } => (at, "updated"),
        refs::Updated::Unchanged { at, .. } => (at, "not changed"),
        refs::Updated::ConcurrentlyModified => {
            tracing::warn!("attempted concurrent updates of signed refs");
            report(
                reporter,
                "sigrefs race whilst updating signed refs, you may need to retry",
            )
            .await?;
            return Ok(None);
        },
    };
    report(reporter, format!("signed refs state was {}", msg)).await?;
    Ok(Some(at.into()))
}

#[instrument(skip(client, reporter))]
async fn request_pull<S, E, P>(
    reporter: &mut P,
    client: &Client<S, quic::SendOnly>,
    seeds: &Seeds,
    urn: Urn,
) -> Result<(), error::RequestPull<E>>
where
    S: librad::Signer + Clone,
    E: std::error::Error + Send + 'static,
    P: ProgressReporter<Error = E>,
{
    tracing::info!(urn=%urn, seeds=?seeds, "request-pull to seeds");
    for seed in seeds {
        let to = (seed.peer, seed.addrs.clone());
        if let Some(label) = &seed.label {
            report(
                reporter,
                format!("request-pull to `{}` label: {}", seed.peer, label),
            )
            .await?
        } else {
            report(reporter, format!("request-pull to `{}`", seed.peer)).await?
        }
        match client.request_pull(to, urn.clone()).await {
            Ok(mut request) => {
                while let Some(resp) = request.next().await {
                    match resp {
                        Ok(request_pull::Response::Success(s)) => {
                            report(reporter, progress::Namespaced::new(&urn, &s)).await?;
                            break;
                        },
                        Ok(request_pull::Response::Error(e)) => {
                            tracing::error!(peer=%seed.peer, err=%e.message, "request-pull failed");
                            report(reporter, e.message).await?;
                            break;
                        },
                        Ok(request_pull::Response::Progress(p)) => {
                            report(reporter, p.message).await?
                        },
                        Err(err) => {
                            tracing::error!(peer=%seed.peer, err=%err, "request-pull transport failed");

                            report(reporter, err.to_string()).await?;
                            break;
                        },
                    }
                }
            },
            Err(err) => {
                report(
                    reporter,
                    format!("failed to request-pull to `{}`: {}", seed.peer, err),
                )
                .await?
            },
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Announce {
    pub rpc_socket_path: PathBuf,
}

/// Actions to be taken after a `git receive-pack`.
#[derive(Debug, Clone)]
pub struct PostReceive {
    /// Announce new changes via the RPC socket.
    pub announce: Option<Announce>,
    /// Make a request-pull to configured seeds.
    pub request_pull: bool,
}

/// Actions to be taken after a `git receive-pack`.
#[derive(Debug, Clone)]
pub struct PreUpload {
    /// Replicate from configured seeds.
    pub replicate: bool,
}
