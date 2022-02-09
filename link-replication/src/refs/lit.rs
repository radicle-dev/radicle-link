// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::borrow::Cow;

use bstr::BStr;

pub use git_ref_format::lit::*;

#[derive(Clone, Copy)]
pub enum Prefix {
    Heads,
    Notes,
    Rad,
    RadIds,
    Remotes,
    Tags,
    Cobs,
}

impl Prefix {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.as_ref()
    }
}

impl AsRef<str> for Prefix {
    fn as_ref(&self) -> &str {
        match self {
            Self::Heads => "refs/heads/",
            Self::Notes => "refs/notes/",
            Self::Rad => "refs/rad/",
            Self::RadIds => "refs/rad/ids/",
            Self::Remotes => "refs/remotes/",
            Self::Tags => "refs/tags/",
            Self::Cobs => "refs/cobs/",
        }
    }
}

impl AsRef<[u8]> for Prefix {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl AsRef<BStr> for Prefix {
    fn as_ref(&self) -> &BStr {
        self.as_str().into()
    }
}

impl From<Prefix> for Cow<'static, BStr> {
    fn from(p: Prefix) -> Self {
        Cow::from(&p).into_owned().into()
    }
}

impl<'a> From<&'a Prefix> for Cow<'a, BStr> {
    fn from(p: &'a Prefix) -> Self {
        Cow::from(AsRef::<BStr>::as_ref(p))
    }
}
