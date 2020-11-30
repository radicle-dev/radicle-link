// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub trait ResultExt<T, E> {
    /// Calls `f` if the result is [`Err`], **and** the predicate `pred` on the
    /// error value returns true. Otherwise returns the [`Ok`] value of
    /// `self`. Note that `f` may change the error type, so as long as the
    /// target type can be converted from the original one.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io;
    /// use radicle_std_ext::result::ResultExt as _;
    ///
    /// let res = Err(io::Error::new(io::ErrorKind::Other, "crashbug"))
    ///     .or_matches::<io::Error, _, _>(|e| matches!(e.kind(), io::ErrorKind::Other), || Ok(()))
    ///     .unwrap();
    ///
    /// assert_eq!((), res)
    /// ```
    fn or_matches<E2, P, F>(self, pred: P, f: F) -> Result<T, E2>
    where
        E2: From<E>,
        P: FnOnce(&E) -> bool,
        F: FnOnce() -> Result<T, E2>;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn or_matches<E2, P, F>(self, pred: P, f: F) -> Result<T, E2>
    where
        E2: From<E>,
        P: FnOnce(&E) -> bool,
        F: FnOnce() -> Result<T, E2>,
    {
        self.or_else(|e| if pred(&e) { f() } else { Err(e.into()) })
    }
}
