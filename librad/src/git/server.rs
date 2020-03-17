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
use log::{error, trace};
use tokio::process::{self, Command};
use tokio_util::compat::{Tokio02AsyncReadCompatExt, Tokio02AsyncWriteCompatExt};

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
        let mut recv = BufReader::new(recv);
        let mut header = String::with_capacity(512);
        if let Err(e) = recv.read_line(&mut header).await {
            error!("Error reading git service header: {}", e);
            return send_err(&mut send, "garbage header").await;
        }

        let (service, repo, mode) = {
            let mut parts = header.split(|c| c == ' ' || c == '\0');

            let service = parts.next();
            let repo = parts.next();
            let mode = parts.next().unwrap_or("");
            (service, repo, mode)
        };

        if service != Some("git-upload-pack") {
            error!("Invalid git service: {:?}", service);
            return send_err(&mut send, "service not enabled").await;
        }

        let repo_path = {
            let repo_path = repo
                .ok_or_else(|| git2::Error::from_str("No repo specified by client"))
                .and_then(|path| {
                    git2::Repository::open_bare(self.export.join(path.trim_start_matches('/')))
                })
                .map(|repo| repo.path().to_path_buf());

            match repo_path {
                Ok(repo_path) => repo_path,
                Err(e) => {
                    error!("Error opening repo {:?}: {}", repo, e);
                    return send_err(&mut send, "repo not found or access denied").await;
                },
            }
        };

        trace!(
            "git service: {:?}, repo: {}, mode: {}",
            service,
            repo_path.display(),
            mode
        );

        if mode.is_empty() {
            UploadPack::upload_pack(&repo_path)?.run(recv, send).await
        } else if mode == "advertise" {
            UploadPack::advertise(&repo_path)?.run(recv, send).await
        } else {
            error!("Invalid mode: expected `advertise`, got `{}`", mode);
            send_err(&mut send, "invalid mode").await
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
                    error!("upload-pack exited non-zero: {:?}", status)
                }
            })
            .map_err(|e| error!("upload-pack error: {}", e));
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
