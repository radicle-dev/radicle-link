// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::borrow::Cow;

use bstr::BStr;

pub mod component {
    pub const REFS: &[u8] = b"refs";
    pub const REMOTES: &[u8] = b"remotes";
    pub const NAMESPACES: &[u8] = b"namespaces";

    // standard
    pub const HEADS: &[u8] = b"heads";
    pub const NOTES: &[u8] = b"notes";
    pub const TAGS: &[u8] = b"tags";

    // rad
    pub const RAD: &[u8] = b"rad";
    pub const ID: &[u8] = b"id";
    pub const IDS: &[u8] = b"ids";
    pub const SELF: &[u8] = b"self";
    pub const SIGNED_REFS: &[u8] = b"signed_refs";
    pub const COBS: &[u8] = b"cobs";
}

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
pub struct RadId;

impl RadId {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.as_ref()
    }
}

impl AsRef<str> for RadId {
    fn as_ref(&self) -> &str {
        "refs/rad/id"
    }
}

impl AsRef<[u8]> for RadId {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl From<RadId> for Cow<'static, BStr> {
    fn from(x: RadId) -> Self {
        Cow::from(&x).into_owned().into()
    }
}

impl<'a> From<&'a RadId> for Cow<'a, BStr> {
    fn from(x: &'a RadId) -> Self {
        Cow::from(AsRef::<BStr>::as_ref(x.as_str()))
    }
}

pub struct RadSelf;

impl RadSelf {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.as_ref()
    }
}

impl AsRef<str> for RadSelf {
    fn as_ref(&self) -> &str {
        "refs/rad/self"
    }
}

impl AsRef<[u8]> for RadSelf {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}

impl From<RadSelf> for Cow<'static, BStr> {
    fn from(x: RadSelf) -> Self {
        Cow::from(&x).into_owned().into()
    }
}

impl<'a> From<&'a RadSelf> for Cow<'a, BStr> {
    fn from(x: &'a RadSelf) -> Self {
        Cow::from(AsRef::<BStr>::as_ref(x.as_str()))
    }
}

pub struct Signed;

impl Signed {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.as_ref()
    }
}

impl AsRef<str> for Signed {
    fn as_ref(&self) -> &str {
        "refs/rad/signed_refs"
    }
}

impl AsRef<[u8]> for Signed {
    fn as_ref(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}
impl From<Signed> for Cow<'static, BStr> {
    fn from(x: Signed) -> Self {
        Cow::from(&x).into_owned().into()
    }
}

impl<'a> From<&'a Signed> for Cow<'a, BStr> {
    fn from(x: &'a Signed) -> Self {
        Cow::from(AsRef::<BStr>::as_ref(x.as_str()))
    }
}
