// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! A git server
//!
//! This implements the same protocol as [`git-daemon`], with a minor adjustment
//! due to a (possible) bug in `libgit2` or `git2-rs`, which prevents us from
//! registering a stateful transport: when parsing the header line, we look for
//! a null-terminated string "advertise" to decide whether we should wait for
//! data to be fed into `stdin` of `git upload-pack` or not.
//!
//! [`git-daemon`]: https://git-scm.com/docs/git-daemon

use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
};

use futures::{
    self,
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
};
use git2::transport::Service;
use tokio::process::{self, Command};
use tokio_util::compat::{Tokio02AsyncReadCompatExt, Tokio02AsyncWriteCompatExt};

use crate::git::header::{self, Header};

#[derive(Clone)]
pub struct GitServer {
    /// Base directory under which all git repositories are "exported", i.e.
    /// available for pull. The `git-daemon-export-ok` file is not checked.
    pub export: PathBuf,
}

impl GitServer {
    pub async fn invoke_service<'a, R, W>(&self, (recv, mut send): (R, W)) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let span = tracing::trace_span!("GitServer::invoke_service", git.server.path = %self.export.display());
        let _guard = span.enter();

        let mut recv = BufReader::new(recv);
        let mut hdr_buf = String::with_capacity(256);
        if let Err(e) = recv.read_line(&mut hdr_buf).await {
            tracing::error!("Error reading git service header: {}", e);
            return send_err(&mut send, "garbage header").await;
        }

        let header = match hdr_buf.parse::<Header>() {
            Ok(hdr) => hdr,
            Err(e) => {
                tracing::error!("Error parsing git service header: {}", e);
                return send_err(&mut send, "invalid header").await;
            },
        };

        let repo_path = {
            let repo = git2::Repository::open_bare(
                self.export
                    .join(format!("{}.git", header.repo.id.to_string())),
            );
            match repo {
                Ok(repo) => repo.path().to_path_buf(),
                Err(e) => {
                    tracing::error!("Error opening repo {:?}: {}", header.repo, e);
                    return send_err(&mut send, "repo not found or access denied").await;
                },
            }
        };

        tracing::trace!(
            git.service = ?header.service,
            git.repo.path = %repo_path.display(),
        );

        match *header.service {
            Service::UploadPack => UploadPack::upload_pack(&repo_path)?.run(recv, send).await,
            Service::UploadPackLs => UploadPack::advertise(&repo_path)?.run(recv, send).await,
            service => {
                tracing::error!("Invalid git service: {:?}", header::Service(service));
                send_err(&mut send, "service not enabled").await
            },
        }
    }
}

const ADVERTISE_REFS_HEADER: &[u8] = b"001e# service=git-upload-pack\n0000";

enum UploadPack {
    AdvertiseRefs(process::Child),
    UploadPack(process::Child),
}

impl UploadPack {
    fn advertise(repo_path: &Path) -> io::Result<Self> {
        Command::new("git")
            .current_dir(repo_path)
            .args(&[
                "upload-pack",
                "--strict",
                "--timeout=5",
                "--stateless-rpc",
                "--advertise-refs",
                ".",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .map(Self::AdvertiseRefs)
    }

    fn upload_pack(repo_path: &Path) -> io::Result<Self> {
        Command::new("git")
            .current_dir(repo_path)
            .args(&[
                "upload-pack",
                "--strict",
                "--timeout=5",
                "--stateless-rpc",
                ".",
            ])
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .map(Self::UploadPack)
    }

    async fn run<R, W>(self, mut recv: R, mut send: W) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        match self {
            Self::AdvertiseRefs(mut child) => {
                let mut stdout = child.stdout.take().unwrap().compat();

                spawn_child(child);

                send.write_all(ADVERTISE_REFS_HEADER).await?;
                futures::io::copy(&mut stdout, &mut send).await.map(|_| ())
            },

            Self::UploadPack(mut child) => {
                let mut stdin = child.stdin.take().unwrap().compat_write();
                let mut stdout = child.stdout.take().unwrap().compat();

                spawn_child(child);

                futures::try_join!(
                    futures::io::copy(&mut recv, &mut stdin),
                    futures::io::copy(&mut stdout, &mut send)
                )
                .map(|_| ())
            },
        }
    }
}

async fn send_err<W>(writer: &mut W, msg: &str) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(pkt_line(&format!("ERR {}", msg)).as_bytes())
        .await
}

fn spawn_child(child: process::Child) {
    tokio::spawn(async {
        let _ = child
            .await
            .map(|status| {
                if !status.success() {
                    tracing::error!("upload-pack exited non-zero: {:?}", status)
                }
            })
            .map_err(|e| tracing::error!("upload-pack error: {}", e));
    });
}

fn pkt_line(msg: &str) -> String {
    assert!(
        msg.len() <= 65516,
        "pkt-line data must not exceed 65516 bytes"
    );

    format!("{:04x}{}", 4 + msg.len(), msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkt_line() {
        assert_eq!("0006a\n", pkt_line("a\n"));
        assert_eq!("0005a", pkt_line("a"));
        assert_eq!("000bfoobar\n", pkt_line("foobar\n"));
        assert_eq!("0004", pkt_line(""));
    }
}
