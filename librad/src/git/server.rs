use std::{io, path::PathBuf, process::Stdio};

use futures::{
    self,
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
};
use log::error;
use tokio::process::Command;
use tokio_util::compat::{Tokio02AsyncReadCompatExt, Tokio02AsyncWriteCompatExt};

#[derive(Clone)]
pub struct GitServer {
    /// Base directory under which all git repositories are "exported", i.e.
    /// available for pull. The `git-daemon-export-ok` file is not checked.
    pub export: PathBuf,
}

impl GitServer {
    pub async fn invoke_service<'a, R, W>(&self, (recv, mut send): (R, W)) -> Result<(), io::Error>
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

        let (service, repo) = {
            let mut parts = header.split(|c| c == ' ' || c == '\0');

            let service = parts.next();
            let repo = parts.next();
            (service, repo)
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

        let cmd = Command::new("git")
            .args(&["upload-pack", "--strict", "--timeout=5", "."])
            .current_dir(repo_path)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn();

        match cmd {
            Err(e) => {
                error!("Error forking upload-pack: {}", e);
                send_err(&mut send, "internal server error").await
            },

            Ok(child) => {
                let mut stdin = child.stdin.unwrap().compat_write();
                let mut stdout = child.stdout.unwrap().compat();

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
