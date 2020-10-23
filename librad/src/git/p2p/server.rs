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
use radicle_git_ext::{into_io_err, RefLike, References, UPLOAD_PACK_HEADER};
use tokio::process::{self, Command};
use tokio_util::compat::{Tokio02AsyncReadCompatExt, Tokio02AsyncWriteCompatExt};

use crate::{
    git::{
        header::{self, Header},
        types::namespace::AsNamespace,
    },
    paths::Paths,
};

#[derive(Clone)]
pub struct GitServer {
    monorepo: PathBuf,
}

impl GitServer {
    pub fn new(paths: &Paths) -> Self {
        Self {
            monorepo: paths.git_dir().to_path_buf(),
        }
    }
}

impl GitServer {
    pub async fn invoke_service<R, W>(&self, (recv, mut send): (R, W)) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let span = tracing::trace_span!("GitServer::invoke_service", git.server.path = %self.monorepo.display());
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

        tracing::trace!(
            git.service = ?header.service,
            git.urn = %header.repo,
        );

        match *header.service {
            Service::UploadPack => {
                UploadPack::upload_pack(&self.monorepo)?
                    .run(recv, send)
                    .await
            },
            Service::UploadPackLs => {
                UploadPack::advertise(&self.monorepo, header.repo.id)?
                    .run(recv, send)
                    .await
            },
            service => {
                tracing::error!("Invalid git service: {:?}", header::Service(service));
                send_err(&mut send, "service not enabled").await
            },
        }
    }
}

enum UploadPack {
    AdvertiseRefs(process::Child),
    UploadPack(process::Child),
}

impl UploadPack {
    fn advertise<N>(repo_path: &Path, namespace: N) -> io::Result<Self>
    where
        N: AsNamespace + Clone,
    {
        let namespace: RefLike = namespace.into();

        let mut git = Command::new("git");
        git.args(&["-c", "uploadpack.hiderefs=refs/"])
            .arg("-c")
            .arg(format!(
                "uploadpack.hiderefs=!{}",
                reflike!("refs/namespaces").join(&namespace).as_str()
            ));

        // FIXME: we should probably keep one git2::Repository around, but
        // `GitServer` needs to be `Sync`
        let repo = git2::Repository::open_bare(repo_path).map_err(into_io_err)?;
        let mut refs = References::from_globs(
            &repo,
            &[
                format!("refs/namespaces/{}/refs/rad/ids/*", namespace.as_str()),
                format!(
                    "refs/namespaces/{}/refs/remotes/**/rad/ids/*",
                    namespace.as_str()
                ),
            ],
        )
        .map_err(into_io_err)?;

        for id_ref in refs.names() {
            if let Some(id) = id_ref.ok().and_then(|name| name.split('/').next_back()) {
                git.arg("-c")
                    .arg(format!("uploadpack.hiderefs=!refs/namespaces/{}", id));
            }
        }

        git_tracing(&mut git);
        git.args(&[
            "upload-pack",
            "--strict",
            "--timeout=5",
            "--stateless-rpc",
            "--advertise-refs",
            ".",
        ])
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map(Self::AdvertiseRefs)
    }

    fn upload_pack(repo_path: &Path) -> io::Result<Self> {
        let mut git = Command::new("git");
        git_tracing(&mut git);
        git.args(&[
            "upload-pack",
            "--strict",
            "--timeout=5",
            "--stateless-rpc",
            ".",
        ])
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::inherit())
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

                send.write_all(UPLOAD_PACK_HEADER).await?;
                futures::io::copy(&mut stdout, &mut send).await.map(|_| ())
            },

            Self::UploadPack(mut child) => {
                let mut stdin = child.stdin.take().unwrap().compat_write();
                let mut stdout = child.stdout.take().unwrap().compat();

                spawn_child(child);

                futures::try_join!(
                    futures::io::copy(&mut recv, &mut stdin),
                    futures::io::copy(&mut stdout, &mut send),
                )
                .map(|_| ())
            },
        }
    }
}

fn git_tracing(git: &mut Command) {
    git.envs(::std::env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")));
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
