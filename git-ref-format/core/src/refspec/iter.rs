// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::{self, Display};

use super::PatternStr;
use crate::RefStr;

pub type Iter<'a> = std::str::Split<'a, char>;

pub enum Component<'a> {
    Glob(Option<&'a PatternStr>),
    Normal(&'a RefStr),
}

impl Component<'_> {
    #[inline]
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl AsRef<str> for Component<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        match self {
            Self::Glob(None) => "*",
            Self::Glob(Some(x)) => x.as_str(),
            Self::Normal(x) => x.as_str(),
        }
    }
}

impl Display for Component<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone)]
pub struct Components<'a> {
    inner: Iter<'a>,
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|next| match next {
            "*" => Component::Glob(None),
            x if x.contains('*') => Component::Glob(Some(PatternStr::from_str(x))),
            x => Component::Normal(RefStr::from_str(x)),
        })
    }
}

impl<'a> From<&'a PatternStr> for Components<'a> {
    #[inline]
    fn from(p: &'a PatternStr) -> Self {
        Self {
            inner: p.as_str().split('/'),
        }
    }
}
