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

pub trait ResultExt<T, E>: Sized {
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
