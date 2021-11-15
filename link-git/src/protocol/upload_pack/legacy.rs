// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021      The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, path::Path, process::ExitStatus};

use async_process::{Command, Stdio};
use futures_lite::io::{copy, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use futures_util::try_join;
use git_ref::{
    file::{Store as Refdb, WriteReflog},
    FullName,
    Reference,
};

pub(super) async fn advertise_refs<R, W>(
    git_dir: impl AsRef<Path>,
    namespace: &str,
    mut recv: R,
    mut send: W,
) -> io::Result<ExitStatus>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let unhide = blocking::unblock({
        let git_dir = git_dir.as_ref().to_path_buf();
        let prefix = Path::new("refs")
            .join("namespaces")
            .join(namespace)
            .join("refs");
        move || -> io::Result<Vec<FullName>> {
            let refdb = Refdb::at(git_dir, WriteReflog::Disable);
            let packed = refdb
                .packed_buffer()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            let refs = refdb
                .iter_prefixed(packed.as_ref(), prefix)?
                .filter_map(|r| r.ok().map(|Reference { name, .. }| name))
                .filter(|name| {
                    const PATTERN: &[u8] = b"rad/ids/any";
                    const SEPARAT: u8 = b'/';
                    name.as_bstr()
                        .rsplit(|b| b == &SEPARAT)
                        .zip(PATTERN.rsplit(|b| b == &SEPARAT))
                        .skip(1)
                        .all(|(a, b)| a == b)
                })
                .collect::<Vec<_>>();

            Ok(refs)
        }
    })
    .await?;

    let mut child = {
        let mut cmd = Command::new("git");
        cmd.current_dir(git_dir)
            .env_clear()
            .envs(std::env::vars().filter(|(key, _)| key == "PATH" || key.starts_with("GIT_TRACE")))
            .arg("-c")
            .arg("uploadpack.hiderefs=refs/")
            .arg("-c")
            .arg(format!(
                "uploadpack.hiderefs=!refs/namespaces/{}",
                namespace
            ));

        for r in unhide {
            cmd.arg("-c")
                .arg(format!("uploadpack.hiderefs=!{}", r.as_bstr()));
        }

        cmd.args(&[
            "upload-pack",
            "--strict",
            "--timeout=5",
            "--stateless-rpc",
            "--advertise-refs",
            ".",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .reap_on_drop(true)
        .spawn()?
    };
    let mut stdout = child.stdout.take().unwrap();

    const HEADER: &[u8] = b"001e# service=git-upload-pack\n0000";
    send.write_all(HEADER).await?;
    let status = try_join!(copy(&mut stdout, &mut send), child.status()).map(|x| x.1);

    // Read one byte off the read stream to ensure it is driven to completion
    // (we expect EOF immediately). Failure to do so may cause resource leaks.
    //
    // Cf. 900b6cf6 (replication: Ensure git stream is closed, 2021-04-26)
    let mut buf = [0; 1];
    recv.read(&mut buf).await?;

    status
}
