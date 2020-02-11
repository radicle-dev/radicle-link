use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use bytes::Bytes;
use http::Response;
use log::{debug, error};

/// A very simple git HTTP server.
///
/// It supports the "smart" git HTTP protocol, and just shells out to `git
/// upload-pack`. Push (`git receive-pack`) is not supported).
#[derive(Clone)]
pub struct GitServer {
    /// Base directory under which all git repositories are "exported", i.e.
    /// available for pull. The `git-daemon-export-ok` file is not checked.
    pub export: PathBuf,
}

impl GitServer {
    pub async fn handle_advertise_refs(&self, repo: &str) -> Response<Bytes> {
        self.with_repo(repo, |repo| {
            advertise_refs(repo)
                .map(|out| {
                    Response::builder()
                        .status(200)
                        .header(
                            "Content-Type",
                            "application/x-git-upload-pack-advertisement",
                        )
                        .body(out)
                        .unwrap()
                })
                .unwrap_or_else(|_| resp_500())
        })
    }

    pub async fn handle_upload_pack(&self, repo: &str, body: Bytes) -> Response<Bytes> {
        self.with_repo(repo, |repo| {
            upload_pack(repo, body)
                .map(|out| {
                    Response::builder()
                        .status(200)
                        .header("Content-Type", "application/x-git-upload-pack-result")
                        .body(out)
                        .unwrap()
                })
                .unwrap_or_else(|_| resp_500())
        })
    }

    fn with_repo<F, T>(&self, repo: &str, f: F) -> Response<T>
    where
        F: FnOnce(&Path) -> Response<T>,
        T: Default,
    {
        let repo = self.export.join(repo);
        debug!("repo path: {}", repo.display());
        if repo.exists() {
            f(&repo)
        } else {
            error!("repo {} doesn't exist!", repo.display());
            resp_404()
        }
    }
}

const ADVERTISE_REFS_HEADER: &str = "001e# service=git-upload-pack\n0000";

fn advertise_refs(repo: &Path) -> io::Result<Bytes> {
    let out = Command::new("git")
        .args(&["upload-pack", "--stateless-rpc", "--advertise-refs", "."])
        .current_dir(repo)
        .stdout(Stdio::piped())
        .output()?;

    if out.status.success() {
        let header = ADVERTISE_REFS_HEADER.as_bytes();
        let mut stdout = Vec::with_capacity(header.len() + out.stdout.len());
        stdout.extend(header);
        stdout.extend(out.stdout);
        Ok(Bytes::from(stdout))
    } else {
        error!("advertise_refs error");
        Err(io::Error::new(
            io::ErrorKind::Other,
            "advertise_refs: Error invoking git-upload-pack",
        ))
    }
}

// FIXME(kim): we should stream the result
fn upload_pack(repo: &Path, haves: Bytes) -> io::Result<Bytes> {
    let mut child = Command::new("git")
        .args(&["upload-pack", "--stateless-rpc"])
        .arg(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    {
        let stdin = child.stdin.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Couldn't open stdin of child process",
            )
        })?;
        stdin.write_all(haves.as_ref())?;
    }

    let out = child.wait_with_output()?;
    if out.status.success() {
        debug!("upload_pack ok");
        Ok(Bytes::from(out.stdout))
    } else {
        error!("upload_pack error");
        Err(io::Error::new(
            io::ErrorKind::Other,
            "upload_pack: Error invoking git-upload-pack",
        ))
    }
}

fn resp_404<T: Default>() -> Response<T> {
    Response::builder()
        .status(404)
        .body(Default::default())
        .unwrap()
}

fn resp_500<T: Default>() -> Response<T> {
    Response::builder()
        .status(500)
        .body(Default::default())
        .unwrap()
}
