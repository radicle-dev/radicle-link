#![feature(str_strip)]

use std::{
    net::ToSocketAddrs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use bytes::Bytes;
use futures::{AsyncReadExt, StreamExt};
use git2;
use http::{Method, Response, StatusCode};
use log::{debug, error, info};
use quinn_h3::server::{IncomingRequest, RecvRequest};
use tempfile::tempdir;
use tokio::{self, task};

use librad::{
    git::{h3::register_h3_transport, server::GitServer},
    keys::device,
    net::h3,
};

// FIXME(kim): convince quinn folks to return the actual listen addresses after
// binding a server. We should be able to pass port 0 and later determine which
// one the OS chose. Also to latch we can wait on before issuing requests
// against the server.
async fn run_h3_git_server<S: ToSocketAddrs>(
    key: device::Key,
    export: PathBuf,
    listen_addr: S,
) -> Result<()> {
    let git = GitServer { export };

    let mut incoming = h3::make_server(&key, listen_addr)?;

    info!("server listening");
    while let Some(connecting) = incoming.next().await {
        debug!("new connection");
        match connecting.await {
            Err(e) => error!("accept failed: {:?}", e),
            Ok(conn) => {
                let git = git.clone();
                let _ = task::spawn(async move {
                    if let Err(e) = handle_connection(git, conn).await {
                        error!("handling connection failed: {:?}", e)
                    }
                });
            },
        }
    }

    Ok(())
}

async fn handle_connection(srv: GitServer, mut incoming: IncomingRequest) -> Result<()> {
    while let Some(request) = incoming.next().await {
        let srv = srv.clone();
        task::spawn(async move {
            if let Err(e) = handle_request(srv, request).await {
                error!("request error: {}", e)
            }
        });
    }

    Ok(())
}

async fn handle_request(srv: GitServer, recv: RecvRequest) -> Result<()> {
    let (req, mut recv_body, sender) = recv.await?;

    let resp = match req.method() {
        &Method::GET => {
            let repo = req.uri().path_and_query().and_then(|pnq| {
                pnq.query()
                    .filter(|q| *q == "service=git-upload-pack")
                    .and_then(|_| {
                        pnq.path()
                            .strip_suffix("/info/refs")
                            .and_then(|path| path.strip_prefix("/"))
                    })
            });

            if let Some(repo) = repo {
                srv.handle_advertise_refs(repo).await
            } else {
                not_found()
            }
        },

        &Method::POST => {
            if let Some(repo) = req
                .uri()
                .path()
                .strip_suffix("/git-upload-pack")
                .and_then(|path| path.strip_prefix("/"))
            {
                let mut body = Vec::with_capacity(1024);
                recv_body.read_to_end(&mut body).await?;

                srv.handle_upload_pack(repo, Bytes::from(body)).await
            } else {
                not_found()
            }
        },

        _ => method_not_allowed(),
    };

    sender.send_response(resp).await?;

    Ok(())
}

fn method_not_allowed<T: Default>() -> Response<T> {
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Default::default())
        .unwrap()
}

fn not_found<T: Default>() -> Response<T> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Default::default())
        .unwrap()
}

fn make_git_repo<P: AsRef<Path>>(path: P) -> Result<()> {
    let repo = git2::Repository::init_bare(path)?;
    let sig = git2::Signature::now("knacknake", "knacknake@leboeuf.xyz")?;
    let tree = {
        let mut index = repo.index()?;
        let tree_id = index.write_tree()?;
        repo.find_tree(tree_id)?
    };

    repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

    Ok(())
}

#[test]
fn test_clone() {
    let server_key = device::Key::new();

    let server_repos_dir = tempdir().unwrap();
    let server_repo_dir = server_repos_dir.path().join("knacknake");

    make_git_repo(server_repo_dir.clone()).unwrap();

    let mut rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        task::spawn(run_h3_git_server(
            server_key.clone(),
            server_repos_dir.path().to_path_buf(),
            "localhost:4433",
        ));

        task::spawn_blocking(move || {
            let client_checkout_dir = tempdir().unwrap();

            let client = h3::make_client(&device::Key::new()).expect("failed to create h3 client");
            unsafe { register_h3_transport(client) };

            git2::Repository::clone(
                &format!(
                    "rad://{}.radicle@localhost:4433/{}",
                    server_key,
                    server_repo_dir.file_name().unwrap().to_str().unwrap()
                ),
                client_checkout_dir.path(),
            )
            .expect("clone failed")
        })
        .await
    })
    .unwrap();
}
