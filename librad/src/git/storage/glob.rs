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

use std::path::Path;

use git_ext as ext;

pub trait Pattern {
    fn matches<P: AsRef<Path>>(&self, path: P) -> bool;
}

impl Pattern for globset::GlobMatcher {
    fn matches<P: AsRef<Path>>(&self, path: P) -> bool {
        self.is_match(path)
    }
}

impl Pattern for globset::GlobSet {
    fn matches<P: AsRef<Path>>(&self, path: P) -> bool {
        self.is_match(path)
    }
}

#[derive(Clone, Debug)]
pub struct RefspecMatcher(globset::GlobMatcher);

impl From<ext::RefspecPattern> for RefspecMatcher {
    fn from(pat: ext::RefspecPattern) -> Self {
        Self(globset::Glob::new(pat.as_str()).unwrap().compile_matcher())
    }
}

impl Pattern for RefspecMatcher {
    fn matches<P: AsRef<Path>>(&self, path: P) -> bool {
        self.0.is_match(path)
    }
}
