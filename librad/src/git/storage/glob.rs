// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
