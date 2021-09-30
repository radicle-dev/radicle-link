// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{future::Future, io, path::Path, process::ExitStatus, str::FromStr};

use async_process::{Command, Stdio};
use futures_lite::io::{copy, AsyncBufReadExt as _, AsyncRead, AsyncWrite, BufReader};
use futures_util::try_join;
use git_packetline::PacketLineRef;
use once_cell::sync::Lazy;
use versions::Version;

mod legacy;

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
    let mut recv = BufReader::new(recv);
    let header: Header = match recv.fill_buf().await?.get(0) {
        // legacy clients don't send a proper pktline header :(
        Some(b'g') => {
            let mut buf = String::with_capacity(256);
            recv.read_line(&mut buf).await?;
            buf.parse().map_err(invalid_data)?
        },
        Some(_) => {
            let mut pktline = git_packetline::StreamingPeekableIter::new(recv, &[]);
            let pkt = pktline
                .read_line()
                .await
                .ok_or_else(|| invalid_data("missing header"))?
                .map_err(invalid_data)?
                .map_err(invalid_data)?;
            let hdr = match pkt {
                PacketLineRef::Data(data) => std::str::from_utf8(data)
                    .map_err(invalid_data)?
                    .parse()
                    .map_err(invalid_data),
                _ => Err(invalid_data("not a header packet")),
            }?;
            recv = pktline.into_inner();

            hdr
        },
        None => {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "expected header",
            ))
        },
    };

    let namespace = header
        .path
        // legacy clients redundantly send a full URN
        .strip_prefix("rad:git:")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| header.path.clone());
    let protocol_version = header
        .extra
        .iter()
        .find_map(|kv| match kv {
            (ref k, Some(v)) if k == "version" => {
                let version = match v.as_str() {
                    "2" => 2,
                    "1" => 1,
                    _ => 0,
                };
                Some(version)
            },
            _ => None,
        })
        .unwrap_or(0);
    // legacy
    let stateless_ls = header.extra.iter().any(|(k, _)| k == "ls");

    let fut = async move {
        if protocol_version < 2 {
            if stateless_ls {
                return legacy::advertise_refs(git_dir, &namespace, recv, send).await;
            }
        } else {
            advertise_capabilities(&mut send).await?;
        }

        let mut child = {
            let mut cmd = Command::new("git");
            cmd.current_dir(git_dir)
                .env_clear()
                .envs(
                    std::env::vars()
                        .filter(|(key, _)| key == "PATH" || key.starts_with("GIT_TRACE")),
                )
                .env("GIT_PROTOCOL", format!("version={}", protocol_version))
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
                    "--stateless-rpc",
                    ".",
                ])
                .stdout(Stdio::piped())
                .stdin(Stdio::piped())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .reap_on_drop(true)
                .spawn()?
        };

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

async fn advertise_capabilities<W>(mut send: W) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    use git_packetline::encode;

    // Thou shallt not upgrade your `git` installation while a link instance is
    // running!
    static GIT_VERSION: Lazy<Version> = Lazy::new(|| git_version().unwrap());
    static AGENT: Lazy<Vec<u8>> = Lazy::new(|| format!("agent=git/{}", *GIT_VERSION).into_bytes());
    static CAPABILITIES: Lazy<[&[u8]; 4]> = Lazy::new(|| {
        [
            b"version 2",
            AGENT.as_slice(),
            b"object-format=sha1",
            b"fetch=ref-in-want",
        ]
    });

    for cap in *CAPABILITIES {
        encode::text_to_write(cap, &mut send).await?;
    }
    encode::flush_to_write(&mut send).await?;

    Ok(())
}

fn git_version() -> io::Result<Version> {
    let out = std::process::Command::new("git")
        .arg("--version")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "failed to read `git` version",
        ));
    }
    out.stdout
        .rsplit(|x| x == &b' ')
        .next()
        .and_then(|s| {
            let s = std::str::from_utf8(s).ok()?;
            Version::new(s.trim())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to parse `git` version"))
}

fn invalid_data<E>(inner: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Sync + Send>>,
{
    io::Error::new(io::ErrorKind::InvalidData, inner)
}
