// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use futures::{executor::block_on, io::Cursor, AsyncReadExt as _};
use radicle_link_git_protocol::take::TryTake;
use std::io;

#[test]
fn when_within_limit() {
    let input = b"the world is everything that is the case";
    let output = block_on(async move {
        let mut buf = Vec::with_capacity(input.len());
        TryTake::new(Cursor::new(input), input.len() as u64 + 1)
            .read_to_end(&mut buf)
            .await?;
        Ok::<_, io::Error>(buf)
    })
    .unwrap();

    assert_eq!(input, output.as_slice())
}

#[test]
fn when_limit_exceeded() {
    let input = b"what is the case, the fact, is the existence of atomic facts";
    let output =
        block_on(TryTake::new(Cursor::new(input), 10).read_to_end(&mut Vec::new())).unwrap_err();

    assert_eq!(output.to_string(), "max input size exceeded")
}

#[test]
fn excess_bytes_remain() {
    let input = b"whereof one cannot speak, thereof one must be silent";
    let output = block_on(async move {
        let mut buf = Vec::with_capacity(input.len());
        let res = TryTake::new(Cursor::new(input), input.len() as u64)
            .read_to_end(&mut buf)
            .await;
        assert!(res.is_err());
        buf
    });

    assert_eq!(input, output.as_slice())
}
