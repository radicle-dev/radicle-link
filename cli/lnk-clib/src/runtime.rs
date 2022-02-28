// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::future::Future;

use once_cell::sync::Lazy;

static RUNTIME: Lazy<tokio::runtime::Handle> = Lazy::new(|| {
    tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
        let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
        let handle = rt.handle().clone();
        std::thread::Builder::new()
            .name("async-global-executor/tokio".to_string())
            .spawn(move || {
                rt.block_on(futures_lite::future::pending::<()>());
            })
            .expect("failed to spawn tokio driver thread");
        handle
    })
});

fn enter() -> tokio::runtime::EnterGuard<'static> {
    RUNTIME.enter()
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let _guard = enter();
    RUNTIME.block_on(future)
}
