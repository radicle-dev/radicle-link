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

use std::{
    fmt::{self, Display},
    io::{self, Read, Write},
    path::PathBuf,
    process::{ChildStdin, ChildStdout, Command, Stdio},
    str::FromStr,
    sync::{Arc, Mutex, Once, RwLock},
    thread,
};

use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use thiserror::Error;

use crate::{
    git::{
        ext::into_git_err,
        storage::{self, Storage, WithSigner},
    },
    hash::{self, Hash},
    keys::SecretKey,
    paths::Paths,
    uri::{self, RadUrn},
};

const URL_SCHEME: &str = "radl";

const UPLOAD_PACK_HEADER: &[u8] = b"001e# service=git-upload-pack\n0000";
const RECEIVE_PACK_HEADER: &[u8] = b"001f# service=git-receive-pack\n0000";

lazy_static! {
    static ref SETTINGS: Arc<RwLock<Option<Settings>>> = Arc::new(RwLock::new(None));
}

#[derive(Clone)]
pub struct Settings {
    pub paths: Paths,
    pub signer: SecretKey,
}

pub fn register(settings: Settings) {
    static INIT: Once = Once::new();

    LocalTransportFactory::new().configure(settings);
    unsafe {
        INIT.call_once(move || {
            git2::transport::register(URL_SCHEME, move |remote| {
                Transport::smart(&remote, true, LocalTransportFactory::new())
            })
            .unwrap()
        });
    }
}

#[derive(Clone)]
struct LocalTransportFactory {
    settings: Arc<RwLock<Option<Settings>>>,
}

impl LocalTransportFactory {
    fn new() -> Self {
        Self {
            settings: SETTINGS.clone(),
        }
    }

    fn configure(&self, settings: Settings) {
        *self.settings.write().unwrap() = Some(settings)
    }
}

impl SmartSubtransport for LocalTransportFactory {
    fn action(
        &self,
        url: &str,
        service: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        let settings = self.settings.read().unwrap();
        match *settings {
            None => Err(git2::Error::from_str("local transport unconfigured")),
            Some(ref settings) => {
                let url = url.parse::<LocalUrl>().map_err(into_git_err)?;
                let transport = LocalTransport::new(settings.clone()).map_err(into_git_err)?;
                let stream = transport.connect(url, service, Mode::Stateless)?;
                Ok(Box::new(stream))
            },
        }
    }

    fn close(&self) -> Result<(), git2::Error> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Stateless,
    Stateful,
}

#[derive(Clone)]
pub struct LocalTransport {
    storage: Arc<Mutex<Storage<WithSigner>>>,
}

impl LocalTransport {
    pub fn new(settings: Settings) -> Result<Self, storage::Error> {
        let storage = Storage::open(&settings.paths)?.with_signer(settings.signer)?;
        Ok(LocalTransport {
            storage: Arc::new(Mutex::new(storage)),
        })
    }

    pub fn connect(
        &self,
        url: LocalUrl,
        service: Service,
        mode: Mode,
    ) -> Result<LocalStream, git2::Error> {
        let urn = url.into();
        self.guard_has_urn(&urn)?;

        let mut git = Command::new("git");
        git.envs(::std::env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
            .current_dir(self.repo_path())
            .args(&[
                &format!("--namespace={}", urn.id),
                "-c",
                "transfer.hiderefs=refs/",
                "-c",
                "transfer.hiderefs=!refs/heads",
                "-c",
                "transfer.hiderefs=!refs/tags",
            ]);

        match service {
            Service::UploadPack | Service::UploadPackLs => {
                // Fetching remotes is ok, pushing is not
                self.visible_remotes(&urn)?.for_each(|remote_ref| {
                    git.arg("-c")
                        .arg(format!("uploadpack.hiderefs=!^{}", remote_ref));
                });
                git.args(&["upload-pack", "--strict", "--timeout=5"]);
            },

            Service::ReceivePack | Service::ReceivePackLs => {
                git.arg("receive-pack");
            },
        }

        if let Mode::Stateless = mode {
            git.arg("--stateless-rpc");
        }

        if matches!(service, Service::UploadPackLs | Service::ReceivePackLs) {
            git.arg("--advertise-refs");
        }

        let mut child = git
            .arg(".")
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(into_git_err)?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        // Spawn a thread to `wait(2)` on the child process, to ensure it gets
        // reaped by the OS. Also update `rad/refs` while we're there.
        let this = self.clone();
        let was_push = matches!(service, Service::ReceivePack);
        thread::spawn(move || match child.wait() {
            Err(e) => eprintln!("error waiting for child: {}", e),
            Ok(status) => {
                if status.success() {
                    if was_push {
                        this.update_refs(&urn)
                    }
                } else {
                    eprintln!("child exited non-zero: {:?}", status)
                }
            },
        });

        let header = match mode {
            Mode::Stateless => match service {
                Service::UploadPackLs => Some(UPLOAD_PACK_HEADER.to_vec()),
                Service::ReceivePackLs => Some(RECEIVE_PACK_HEADER.to_vec()),
                _ => None,
            },

            Mode::Stateful => None,
        };

        Ok(LocalStream {
            read: LocalRead {
                header,
                inner: stdout,
            },
            write: LocalWrite { inner: stdin },
        })
    }

    fn guard_has_urn(&self, urn: &RadUrn) -> Result<(), git2::Error> {
        self.storage
            .lock()
            .unwrap()
            .has_urn(urn)
            .map_err(into_git_err)
            .and_then(|have| {
                have.then_some(())
                    .ok_or_else(|| git2::Error::from_str(&format!("`{}` not found", urn)))
            })
    }

    fn visible_remotes(&self, urn: &RadUrn) -> Result<impl Iterator<Item = String>, git2::Error> {
        const GLOBS: &[&str] = &["remotes/**/heads/*", "remotes/**/tags/*"];

        self.storage
            .lock()
            .unwrap()
            .references_glob(urn, GLOBS)
            .map(|iter| iter.map(|(name, _)| name).collect::<Vec<_>>())
            .map_err(into_git_err)
            .map(|v| v.into_iter())
    }

    fn repo_path(&self) -> PathBuf {
        self.storage.lock().unwrap().path().to_path_buf()
    }

    fn update_refs(&self, urn: &RadUrn) {
        self.storage
            .lock()
            .unwrap()
            .update_refs(urn)
            .unwrap_or_else(|e| eprintln!("Failed to sign updated refs!\n{}", e))
    }
}

pub struct LocalStream {
    read: LocalRead,
    write: LocalWrite,
}

impl LocalStream {
    pub fn split(self) -> (LocalRead, LocalWrite) {
        let LocalStream { read, write } = self;
        (read, write)
    }
}

impl Read for LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read.read(buf)
    }
}

impl Write for LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write.flush()
    }
}

pub struct LocalRead {
    header: Option<Vec<u8>>,
    inner: ChildStdout,
}

impl Read for LocalRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.header.take() {
            None => self.inner.read(buf),
            Some(hdr) => {
                buf[..hdr.len()].copy_from_slice(&hdr);
                buf[hdr.len()] = b'\n';
                Ok(hdr.len())
            },
        }
    }
}

pub struct LocalWrite {
    inner: ChildStdin,
}

impl Write for LocalWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Clone)]
pub struct LocalUrl {
    pub repo: Hash,
}

impl Display for LocalUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}://{}.git", URL_SCHEME, self.repo)
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    #[error("Invalid scheme: {0}")]
    InvalidScheme(String),

    #[error("Cannot-be-a-base URL")]
    CannotBeABase,

    #[error("Malformed URL")]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Hash(#[from] hash::ParseError),
}

impl FromStr for LocalUrl {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = url::Url::parse(s)?;
        if url.scheme() != URL_SCHEME {
            return Err(Self::Err::InvalidScheme(url.scheme().to_owned()));
        }
        if url.cannot_be_a_base() {
            return Err(Self::Err::CannotBeABase);
        }

        let repo = url
            .host_str()
            .expect("we checked for cannot-be-a-base. qed")
            .trim_end_matches(".git")
            .parse()?;

        Ok(Self { repo })
    }
}

impl Into<RadUrn> for LocalUrl {
    fn into(self) -> RadUrn {
        RadUrn {
            id: self.repo,
            proto: uri::Protocol::Git,
            path: uri::Path::empty(),
        }
    }
}
