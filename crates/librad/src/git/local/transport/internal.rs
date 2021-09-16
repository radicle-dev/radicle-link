// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io::{self, Cursor, Read, Write},
    panic,
    process::{ChildStdin, ChildStdout},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
        Arc,
        Mutex,
        Once,
    },
    thread,
};

use git_ext::{into_git_err, RECEIVE_PACK_HEADER, UPLOAD_PACK_HEADER};
use rustc_hash::FxHashMap;
use thiserror::Error;

use super::{super::url::LocalUrl, CanOpenStorage};

pub(super) fn with<F, G, A>(open_storage: F, url: LocalUrl, g: G) -> Result<A, super::Error>
where
    F: CanOpenStorage + 'static,
    G: FnOnce(LocalUrl) -> A,
{
    let (tx, rx) = mpsc::channel();
    let act = Active {
        storage: Arc::new(Box::new(open_storage)),
        results: tx,
    };
    let fct = Factory::new();
    let idx = fct.add(act);
    let url = LocalUrl {
        active_index: Some(idx),
        ..url
    };

    // while `g` is running, references to `tx` maybe be obtained
    let ret = g(url);
    // after we're done, remove the last reference to `tx`
    fct.remove(idx);
    // now we can drain `rx`, as there can't be a sender anymore
    match rx.iter().filter_map(|res| res.err()).next() {
        Some(e) => Err(e),
        None => Ok(ret),
    }
}

#[derive(Clone)]
struct Active {
    storage: Arc<Box<dyn CanOpenStorage>>,
    results: mpsc::Sender<Result<(), super::Error>>,
}

#[derive(Debug, Error)]
enum FactoryError {
    #[error("no active transport found for request")]
    NoSuchActive,

    #[error("failed to obtain storage")]
    MkStorage(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl From<FactoryError> for git2::Error {
    fn from(e: FactoryError) -> Self {
        use FactoryError::*;

        match e {
            NoSuchActive => git2::Error::new(
                git2::ErrorCode::Invalid,
                git2::ErrorClass::Invalid,
                &e.to_string(),
            ),
            MkStorage(e) => git2::Error::new(
                git2::ErrorCode::GenericError,
                git2::ErrorClass::Repository,
                &e.to_string(),
            ),
        }
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Factory {
    active: Arc<Mutex<FxHashMap<usize, Active>>>,
}

impl Factory {
    fn new() -> Self {
        lazy_static! {
            static ref ACTIVE: Arc<Mutex<FxHashMap<usize, Active>>> =
                Arc::new(Mutex::new(FxHashMap::default()));
        }

        Self {
            active: ACTIVE.clone(),
        }
    }

    fn add(&self, active: Active) -> usize {
        use std::collections::hash_map::Entry::Vacant;

        ensure_registered();
        let idx = NEXT.fetch_add(1, Ordering::SeqCst);
        let mut actives = self.active.lock().unwrap();
        if let Vacant(entry) = actives.entry(idx) {
            entry.insert(active);
        } else {
            panic!("too many active transports")
        }

        idx
    }

    fn remove(&self, idx: usize) -> Option<Active> {
        self.active.lock().unwrap().remove(&idx)
    }

    fn get(&self, idx: usize) -> Result<Active, FactoryError> {
        let actives = self.active.lock().unwrap();
        match actives.get(&idx) {
            None => Err(FactoryError::NoSuchActive),
            Some(act) => Ok(act.clone()),
        }
    }
}

fn ensure_registered() {
    static INIT: Once = Once::new();
    unsafe {
        INIT.call_once(move || {
            git2::transport::register(super::super::URL_SCHEME, move |remote| {
                git2::transport::Transport::smart(remote, true, Factory::new())
            })
            .unwrap()
        });
    }
}

impl git2::transport::SmartSubtransport for Factory {
    fn action(
        &self,
        url: &str,
        service: git2::transport::Service,
    ) -> Result<Box<dyn git2::transport::SmartSubtransportStream>, git2::Error> {
        let url = url.parse::<LocalUrl>().map_err(|e| {
            git2::Error::new(
                git2::ErrorCode::Invalid,
                git2::ErrorClass::Invalid,
                &e.to_string(),
            )
        })?;
        let idx = url.active_index.ok_or_else(|| {
            git2::Error::new(
                git2::ErrorCode::Invalid,
                git2::ErrorClass::Invalid,
                "invalid URL: active index is missing. Did you use git::types::Remote?",
            )
        })?;

        let Active { storage, results } = self.get(idx)?;
        let mut transport = super::LocalTransport::from(storage);
        let mut child = transport
            .connect(
                url,
                service,
                super::Mode::Stateless,
                super::Localio::piped(),
            )
            .map_err(into_git_err)?;

        let stdin = child.process.stdin.take().unwrap();
        let stdout = child.process.stdout.take().unwrap();

        thread::spawn(move || results.send(child.wait()).ok());

        let header = match service {
            git2::transport::Service::UploadPackLs => Some(UPLOAD_PACK_HEADER.to_vec()),
            git2::transport::Service::ReceivePackLs => Some(RECEIVE_PACK_HEADER.to_vec()),
            _ => None,
        };

        Ok(Box::new(LocalStream {
            read: LocalRead {
                header,
                inner: stdout,
            },
            write: stdin,
        }))
    }

    fn close(&self) -> Result<(), git2::Error> {
        Ok(())
    }
}

struct LocalStream {
    read: LocalRead,
    write: ChildStdin,
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

struct LocalRead {
    header: Option<Vec<u8>>,
    inner: ChildStdout,
}

impl Read for LocalRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.header.take() {
            None => self.inner.read(buf),
            Some(hdr) => Cursor::new(hdr).read(buf),
        }
    }
}
