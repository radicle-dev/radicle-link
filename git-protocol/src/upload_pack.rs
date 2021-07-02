// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{future::Future, io, path::Path, process::ExitStatus, str::FromStr};

use async_process::{Command, Stdio};
use futures_lite::io::{copy, AsyncRead, AsyncWrite};
use futures_util::try_join;
use git_packetline::PacketLine;

#[derive(Debug, PartialEq)]
pub struct Header {
    pub path: String,
    pub host: Option<(String, Option<u16>)>,
    pub extra: Vec<(String, Option<String>)>,
}

impl FromStr for Header {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s
            .strip_prefix("git-upload-pack ")
            .ok_or("unsupported service")?
            .split_terminator('\0');

        let path = parts.next().ok_or("missing path").and_then(|path| {
            if path.is_empty() {
                Err("empty path")
            } else {
                Ok(path.to_owned())
            }
        })?;
        let host = match parts.next() {
            None | Some("") => None,
            Some(host) => match host.strip_prefix("host=") {
                None => return Err("invalid host"),
                Some(host) => match host.split_once(':') {
                    None => Some((host.to_owned(), None)),
                    Some((host, port)) => {
                        let port = port.parse::<u16>().or(Err("invalid port"))?;
                        Some((host.to_owned(), Some(port)))
                    },
                },
            },
        };
        let extra = parts
            .skip_while(|part| part.is_empty())
            .map(|part| match part.split_once('=') {
                None => (part.to_owned(), None),
                Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            })
            .collect();

        Ok(Self { path, host, extra })
    }
}

pub async fn upload_pack<R, W>(
    git_dir: impl AsRef<Path>,
    recv: R,
    mut send: W,
) -> io::Result<(Header, impl Future<Output = io::Result<ExitStatus>>)>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut recv = git_packetline::StreamingPeekableIter::new(recv, &[]);
    let header: Header = {
        let pkt = recv
            .read_line()
            .await
            .ok_or_else(|| invalid_data("missing header"))?
            .map_err(invalid_data)?
            .map_err(invalid_data)?;
        match pkt {
            PacketLine::Data(data) => std::str::from_utf8(data)
                .map_err(invalid_data)?
                .parse()
                .map_err(invalid_data),
            _ => Err(invalid_data("not a header packet")),
        }?
    };
    let namespace = header.path.clone();
    let mut recv = recv.into_inner();

    let fut = async move {
        let mut child = Command::new("git")
            .current_dir(git_dir)
            .env_clear()
            .envs(std::env::vars().filter(|(key, _)| key == "PATH" || key.starts_with("GIT_TRACE")))
            .env("GIT_PROTOCOL", "version=2")
            .env("GIT_NAMESPACE", namespace)
            .args(&[
                "-c",
                "uploadpack.allowanysha1inwant=true",
                "-c",
                "uploadpack.allowrefinwant=true",
                "-c",
                "lsrefs.unborn=ignore",
                "upload-pack",
                "--strict",
                ".",
            ])
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .reap_on_drop(true)
            .spawn()?;

        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();

        try_join!(
            copy(&mut recv, &mut stdin),
            copy(&mut stdout, &mut send),
            child.status(),
        )
        .map(|(_, _, status)| status)
    };

    Ok((header, fut))
}

fn invalid_data<E>(inner: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Sync + Send>>,
{
    io::Error::new(io::ErrorKind::InvalidData, inner)
}
