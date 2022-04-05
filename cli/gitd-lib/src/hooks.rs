// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, path::PathBuf, sync::Arc};

use tracing::instrument;

use librad::git::{
    refs::{self, Refs},
    storage,
    Urn,
};
use link_async::Spawner;
use linkd_lib::api::client::Reply;

#[derive(Clone)]
pub(crate) struct Hooks {
    spawner: Arc<Spawner>,
    pool: Arc<storage::Pool<storage::Storage>>,
    post_receive: PostReceive,
}

impl fmt::Debug for Hooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hooks")
            .field("post_receive", &self.post_receive)
            .finish()
    }
}

const LINKD_CLIENT_NAME: &str = "lnk-gitd";

pub(crate) struct Progress(String);

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for Progress {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for Progress {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error<E: std::error::Error + Send + 'static> {
    #[error("error notifying client of progress: {0}")]
    Progress(E),
    #[error("could not open storage: {0}")]
    OpenStorage(#[from] storage::pool::PoolError),
    #[error("error updating refs: {0}")]
    UpdateRefs(#[from] librad::git::refs::stored::Error),
    #[error("failed to connect to linkd node: {0}")]
    LinkdConnect(#[source] std::io::Error),
    #[error("linkd rpc transport failed: {0}")]
    LinkdTransport(
        #[source] linkd_lib::api::client::ReplyError<linkd_lib::api::io::SocketTransportError>,
    ),
    #[error("the linkd node reported an error: {0}")]
    Linkd(String),
}

pub(crate) trait ProgressReporter {
    type Error;
    fn report(&mut self, progress: Progress)
        -> futures::future::BoxFuture<Result<(), Self::Error>>;
}

impl Hooks {
    /// Create the default hooks, which do nothing except for update the sigrefs
    /// after a push
    pub(crate) fn new(spawner: Arc<Spawner>, pool: Arc<storage::Pool<storage::Storage>>) -> Hooks {
        Self {
            spawner,
            pool,
            post_receive: PostReceive { announce: None },
        }
    }

    /// Create a `Hooks` which announces any new changes to the linkd node
    /// running at `rpc_socket_path`
    pub(crate) fn announce(
        spawner: Arc<Spawner>,
        pool: Arc<storage::Pool<storage::Storage>>,
        rpc_socket_path: PathBuf,
    ) -> Hooks {
        Self {
            spawner,
            pool,
            post_receive: PostReceive {
                announce: Some(Announce { rpc_socket_path }),
            },
        }
    }

    #[instrument(skip(self, reporter), err)]
    pub(crate) async fn post_receive<
        E: std::error::Error + Send + 'static,
        P: ProgressReporter<Error = E>,
    >(
        self,
        mut reporter: P,
        urn: Urn,
    ) -> Result<(), Error<E>> {
        // Update `rad/signed_refs`
        report(&mut reporter, "updating signed refs".into()).await?;
        let update_result = {
            let storage = self.pool.get().await?;
            let urn = urn.clone();
            self.spawner
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
                    &mut reporter,
                    "sigrefs race whilst updating signed refs, you may need to retry".into(),
                )
                .await?;
                return Ok(());
            },
        };
        reporter
            .report(format!("signed refs {}", msg).into())
            .await
            .map_err(Error::Progress)?;

        if let Some(Announce { rpc_socket_path }) = self.post_receive.announce {
            tracing::info!("running post receive announcement hook");
            report(&mut reporter, "announcing new refs".into()).await?;
            tracing::trace!(?rpc_socket_path, "attempting to send announcement");
            let conn =
                linkd_lib::api::client::Connection::connect(LINKD_CLIENT_NAME, rpc_socket_path)
                    .await
                    .map_err(|e| Error::LinkdConnect(e))?;
            let cmd = linkd_lib::api::client::Command::announce(urn.clone(), at.into());
            let mut replies = cmd
                .execute_with_reply(conn)
                .await
                .map_err(|e| Error::LinkdTransport(e))?;
            loop {
                match replies.next().await {
                    Ok(Reply::Progress {
                        replies: next_replies,
                        msg,
                    }) => {
                        tracing::trace!(?msg, "got progress message from linkd node");
                        report(&mut reporter, msg.into()).await?;
                        replies = next_replies;
                    },
                    Ok(Reply::Success { .. }) => {
                        tracing::trace!("got success from linkd node");
                        report(&mut reporter, "successful_announcement".into()).await?;
                        break;
                    },
                    Ok(Reply::Error { msg, .. }) => {
                        tracing::error!(?msg, "got error from linkd node");
                        return Err(Error::Linkd(msg));
                    },
                    Err((_, e)) => {
                        tracing::error!(err=?e, "error communicating with linkd node");
                        return Err(Error::LinkdTransport(e));
                    },
                }
            }
        }
        Ok(())
    }
}

async fn report<E: std::error::Error + Send + 'static, P: ProgressReporter<Error = E>>(
    reporter: &mut P,
    msg: Progress,
) -> Result<(), Error<E>> {
    reporter.report(msg).await.map_err(Error::Progress)
}

#[derive(Debug, Clone)]
struct Announce {
    rpc_socket_path: PathBuf,
}

#[derive(Debug, Clone)]
struct PostReceive {
    announce: Option<Announce>,
}
