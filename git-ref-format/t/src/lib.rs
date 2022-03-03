// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(test)]
#[macro_use]
extern crate assert_matches;

#[cfg(any(test, feature = "test"))]
pub mod gen;
#[cfg(test)]
mod properties;
#[cfg(test)]
mod tests;
