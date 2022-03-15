// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021      The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, process::ExitStatus};

use futures::io::{AsyncRead, AsyncWrite};
use link_git::protocol::upload_pack::{upload_pack, Header};
use thiserror::Error;
use tracing::{error, info};

use crate::net::{
    connection::Duplex,
    protocol::State,
    upgrade::{self, Upgraded},
};

#[derive(Debug, Error)]
enum Error {
    #[error("upload-pack exited with {0}")]
    UploadPack(ExitStatus),

    #[error(transparent)]
    Io(#[from] io::Error),
}

pub(in crate::net::protocol) async fn git<S, G, T>(
    state: &State<S, G>,
    stream: Upgraded<upgrade::Git, T>,
) where
    T: Duplex,
    T::Read: AsyncRead + Unpin,
    T::Write: AsyncWrite + Unpin,
{
    if let Err(e) = serve(state, stream).await {
        error!(err = ?e, "upload-pack error");
    }
}

async fn serve<S, G, T>(state: &State<S, G>, stream: Upgraded<upgrade::Git, T>) -> Result<(), Error>
where
    T: Duplex,
    T::Read: AsyncRead + Unpin,
    T::Write: AsyncWrite + Unpin,
{
    let (recv, send) = stream.into_stream().split();
    let git_dir = state.config.paths.git_dir();

    let (Header { path, host, extra }, run) = upload_pack(git_dir, recv, send).await?;
    info!(%path, ?host, ?extra, "upload-pack");

    let status = run.await?;
    // XXX: #![feature(exit_status_error)] ?
    // https://github.com/rust-lang/rust/issues/84908
    if !status.success() {
        return Err(Error::UploadPack(status));
    }

    Ok(())
}
