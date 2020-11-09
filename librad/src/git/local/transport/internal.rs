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
    panic,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
        Condvar,
        Mutex,
        Once,
    },
    thread,
    time::Duration,
};

use git_ext::{into_git_err, RECEIVE_PACK_HEADER, UPLOAD_PACK_HEADER};
use rustc_hash::FxHashMap;
use thiserror::Error;

use super::super::{super::storage::Storage, url::LocalUrl};
use crate::signer::BoxedSigner;

pub(super) fn activate<F>(open_storage: F, url: LocalUrl) -> (LocalUrl, Arc<Results>)
where
    F: Fn() -> Result<Storage<BoxedSigner>, Box<dyn std::error::Error + Send + Sync + 'static>>
        + Send
        + Sync
        + 'static,
{
    let act = Active::new(open_storage);
    let res = Arc::clone(&act.results);
    let idx = Factory::new().add(act);
    let url = LocalUrl {
        active_index: Some(idx),
        ..url
    };

    (url, res)
}

struct Active {
    storage: Box<
        dyn Fn() -> Result<Storage<BoxedSigner>, Box<dyn std::error::Error + Send + Sync + 'static>>
            + Send
            + Sync
            + 'static,
    >,
    results: Arc<Results>,
}

impl Active {
    pub(super) fn new<F>(open_storage: F) -> Self
    where
        F: Fn() -> Result<Storage<BoxedSigner>, Box<dyn std::error::Error + Send + Sync + 'static>>
            + Send
            + Sync
            + 'static,
    {
        Self {
            storage: Box::new(open_storage),
            results: Arc::new(Results::new()),
        }
    }
}

pub(super) struct Results {
    expected: AtomicUsize,
    results: Mutex<Vec<thread::Result<Result<(), super::Error>>>>,
    cvar: Condvar,
}

impl Results {
    fn new() -> Self {
        Self {
            expected: AtomicUsize::new(0),
            results: Mutex::new(Vec::new()),
            cvar: Condvar::new(),
        }
    }

    pub(super) fn wait(
        &self,
        timeout: Duration,
    ) -> Option<Vec<thread::Result<Result<(), super::Error>>>> {
        let mut guard = self.results.lock().unwrap();
        loop {
            if guard.len() > 0 {
                if self.expected.load(Ordering::SeqCst) > guard.len() {
                    continue;
                } else {
                    return Some(guard.drain(0..).collect());
                }
            } else {
                let res = self.cvar.wait_timeout(guard, timeout).unwrap();
                if res.1.timed_out() {
                    return None;
                } else {
                    guard = res.0;
                }
            }
        }
    }

    fn expect(&self) {
        self.expected.fetch_add(1, Ordering::SeqCst);
    }

    fn done(&self, res: thread::Result<Result<(), super::Error>>) {
        self.results.lock().unwrap().push(res);
        self.cvar.notify_all();
    }
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
        ensure_registered();
        let idx = NEXT.fetch_add(1, Ordering::SeqCst);
        let mut actives = self.active.lock().unwrap();
        if actives.contains_key(&idx) {
            panic!("too many active transports")
        } else {
            actives.insert(idx, active);
        }

        idx
    }

    fn get(&self, idx: usize) -> Result<(Storage<BoxedSigner>, Arc<Results>), FactoryError> {
        let actives = self.active.lock().unwrap();
        match actives.get(&idx) {
            None => Err(FactoryError::NoSuchActive),
            Some(active) => {
                let storage = (active.storage)()?;
                Ok((storage, active.results.clone()))
            },
        }
    }
}

fn ensure_registered() {
    static INIT: Once = Once::new();
    unsafe {
        INIT.call_once(move || {
            git2::transport::register(super::super::URL_SCHEME, move |remote| {
                git2::transport::Transport::smart(&remote, true, Factory::new())
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
                "invalid URL: active index is missing",
            )
        })?;

        let (storage, results) = self.get(idx)?;
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

        results.expect();
        thread::spawn(move || {
            let res = panic::catch_unwind(move || child.wait());
            results.done(res)
        });

        let header = match service {
            git2::transport::Service::UploadPackLs => Some(UPLOAD_PACK_HEADER.to_vec()),
            git2::transport::Service::ReceivePackLs => Some(RECEIVE_PACK_HEADER.to_vec()),
            _ => None,
        };

        Ok(Box::new(super::LocalStream {
            read: super::LocalRead {
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
