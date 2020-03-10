use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
};

use futures::{
    self,
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
};
use log::{error, trace, warn};
use tokio::process::Command;
use tokio_util::compat::{Tokio02AsyncReadCompatExt, Tokio02AsyncWriteCompatExt};

const ADVERTISE_REFS_HEADER: &[u8] = b"001e# service=git-upload-pack\n0000";

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
                .map(|path| self.export.join(path.trim_start_matches('/')))
                .filter(|path| path.exists());
            match repo_path {
                None => {
                    error!("Invalid repo path: {:?}", repo_path);
                    return send_err(&mut send, "repo not found or access denied").await;
                },

                Some(path) => path,
            }
        };

        trace!(
            "git service: {:?}, repo: {}, mode: {}",
            service,
            repo_path.display(),
            mode
        );

        if mode.is_empty() {
            self.handle_upload_pack(&repo_path, recv, send).await
        } else if mode == "advertise" {
            self.handle_upload_pack_advertise(&repo_path, send).await
        } else {
            error!("Invalid mode: expected `advertise`, got `{}`", mode);
            send_err(&mut send, "invalid mode").await
        }
    }

    async fn handle_upload_pack_advertise<W>(&self, repo_path: &Path, mut send: W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        let cmd = Command::new("git")
            .args(&[
                "upload-pack",
                "--strict",
                "--timeout=5",
                "--stateless-rpc",
                "--advertise-refs",
                ".",
            ])
            .current_dir(repo_path)
            .stdout(Stdio::piped())
            .spawn();

        match cmd {
            Err(e) => {
                error!("Error forking upload-pack: {}", e);
                send_err(&mut send, "internal server error").await
            },

            Ok(mut child) => {
                let mut stdout = child.stdout.take().unwrap().compat();
                tokio::spawn(async {
                    let _ = child
                        .await
                        .map(|status| trace!("upload-pack exited with {:?}", status))
                        .map_err(|e| warn!("upload-pack error: {}", e));
                });
                send.write_all(ADVERTISE_REFS_HEADER).await?;
                futures::io::copy(&mut stdout, &mut send).await.map(|_| ())
            },
        }
    }

    async fn handle_upload_pack<R, W>(
        &self,
        repo_path: &Path,
        mut recv: R,
        mut send: W,
    ) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let cmd = Command::new("git")
            .args(&[
                "upload-pack",
                "--strict",
                "--timeout=5",
                "--stateless-rpc",
                ".",
            ])
            .current_dir(repo_path)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn();

        match cmd {
            Err(e) => {
                error!("Error forking upload-pack: {}", e);
                send_err(&mut send, "internal server error").await
            },

            Ok(mut child) => {
                let mut stdin = child.stdin.take().unwrap().compat_write();
                let mut stdout = child.stdout.take().unwrap().compat();

                tokio::spawn(async {
                    let _ = child
                        .await
                        .map(|status| {
                            if !status.success() {
                                warn!("upload-pack exited non-zero exit status: {:?}", status)
                            }
                        })
                        .map_err(|e| warn!("upload-pack error: {}", e));
                });

                futures::try_join!(
                    futures::io::copy(&mut recv, &mut stdin),
                    futures::io::copy(&mut stdout, &mut send)
                )
                .map(|_| ())
            },
        }
    }
}

async fn send_err<W>(writer: &mut W, msg: &str) -> Result<(), io::Error>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(pkt_line(&format!("ERR {}", msg)).as_bytes())
        .await
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
