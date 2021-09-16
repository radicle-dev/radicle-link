// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures_lite::io::{AsyncBufRead, AsyncRead};

/// Like [`futures_lite::io::Take`], but returns an error if and when the
/// `limit` is exceeded.
///
/// Note that, unlike [`futures_lite::io::Take`], if a single poll reads past
/// the limit, the excess bytes are _not_ discarded. Instead, an error is
/// returned on the next poll.
pub struct TryTake<R> {
    limit: u64,
    inner: R,
}

impl<R> TryTake<R> {
    pub fn new(inner: R, limit: u64) -> Self {
        Self { limit, inner }
    }
}

impl<R> AsyncRead for TryTake<R>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        if self.limit == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Other,
                "max input size exceeded",
            )));
        }

        let this = self.get_mut();
        Pin::new(&mut this.inner).poll_read(cx, buf).map(|ready| {
            if let Ok(siz) = ready {
                this.limit = this.limit.saturating_sub(siz as u64);
            }

            ready
        })
    }
}

impl<R> AsyncBufRead for TryTake<R>
where
    R: AsyncBufRead + Unpin,
{
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<&[u8], io::Error>> {
        if self.limit == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Other,
                "max input size exceeded",
            )));
        }

        Pin::new(&mut self.get_mut().inner).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        Pin::new(&mut self.get_mut().inner).consume(amt)
    }
}
