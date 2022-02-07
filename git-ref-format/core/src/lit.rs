// Copyright © 2022 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::{name, Qualified, RefStr};

/// A literal [`RefStr`].
///
/// Types implementing [`Lit`] must be [`name::Component`]s, and provide a
/// conversion from a component _iff_ the component's [`RefStr`] representation
/// is equal to [`Lit::NAME`]. Because these morphisms can only be guaranteed
/// axiomatically, the trait can not currently be implemented by types outside
/// of this crate.
///
/// [`Lit`] types are useful for efficiently creating known-valid [`Qualified`]
/// refs, and sometimes for pattern matching.
pub trait Lit: Sized + sealed::Sealed {
    const SELF: Self;
    const NAME: &'static RefStr;

    #[inline]
    fn from_component(c: &name::Component) -> Option<Self> {
        (c.as_ref() == Self::NAME).then(|| Self::SELF)
    }
}

impl<T: Lit> From<T> for &'static RefStr {
    #[inline]
    fn from(_: T) -> Self {
        T::NAME
    }
}

mod sealed {
    pub trait Sealed {}
}

/// All known literal [`RefStr`]s.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum KnownLit {
    Refs,
    Heads,
    Namespaces,
    Remotes,
    Tags,
    Notes,

    #[cfg(feature = "link-literals")]
    Rad,
    #[cfg(feature = "link-literals")]
    Id,
    #[cfg(feature = "link-literals")]
    Ids,
    #[cfg(feature = "link-literals")]
    Selv,
    #[cfg(feature = "link-literals")]
    SignedRefs,
    #[cfg(feature = "link-literals")]
    Cobs,
}

impl KnownLit {
    #[inline]
    pub fn from_component(c: &name::Component) -> Option<Self> {
        let c: &RefStr = c.as_ref();
        if c == Refs::NAME {
            Some(Self::Refs)
        } else if c == Heads::NAME {
            Some(Self::Heads)
        } else if c == Namespaces::NAME {
            Some(Self::Namespaces)
        } else if c == Remotes::NAME {
            Some(Self::Remotes)
        } else if c == Tags::NAME {
            Some(Self::Tags)
        } else if c == Notes::NAME {
            Some(Self::Notes)
        } else {
            #[cfg(feature = "link-literals")]
            {
                if c == Rad::NAME {
                    Some(Self::Rad)
                } else if c == Id::NAME {
                    Some(Self::Id)
                } else if c == Ids::NAME {
                    Some(Self::Ids)
                } else if c == Selv::NAME {
                    Some(Self::Selv)
                } else if c == SignedRefs::NAME {
                    Some(Self::SignedRefs)
                } else if c == Cobs::NAME {
                    Some(Self::Cobs)
                } else {
                    None
                }
            }
            #[cfg(not(feature = "link-literals"))]
            None
        }
    }
}

impl From<KnownLit> for name::Component<'_> {
    #[inline]
    fn from(k: KnownLit) -> Self {
        match k {
            KnownLit::Refs => Refs.into(),
            KnownLit::Heads => Heads.into(),
            KnownLit::Namespaces => Namespaces.into(),
            KnownLit::Remotes => Remotes.into(),
            KnownLit::Tags => Tags.into(),
            KnownLit::Notes => Notes.into(),
            #[cfg(feature = "link-literals")]
            KnownLit::Rad => Rad.into(),
            #[cfg(feature = "link-literals")]
            KnownLit::Id => Id.into(),
            #[cfg(feature = "link-literals")]
            KnownLit::Ids => Ids.into(),
            #[cfg(feature = "link-literals")]
            KnownLit::Selv => Selv.into(),
            #[cfg(feature = "link-literals")]
            KnownLit::SignedRefs => SignedRefs.into(),
            #[cfg(feature = "link-literals")]
            KnownLit::Cobs => Cobs.into(),
        }
    }
}

/// Either a [`KnownLit`] or a [`name::Component`]
pub enum SomeLit<'a> {
    Known(KnownLit),
    Any(name::Component<'a>),
}

impl SomeLit<'_> {
    pub fn known(self) -> Option<KnownLit> {
        match self {
            Self::Known(k) => Some(k),
            _ => None,
        }
    }
}

impl<'a> From<name::Component<'a>> for SomeLit<'a> {
    #[inline]
    fn from(c: name::Component<'a>) -> Self {
        match KnownLit::from_component(&c) {
            Some(k) => Self::Known(k),
            None => Self::Any(c),
        }
    }
}

pub type RefsHeads<T> = (Refs, Heads, T);
pub type RefsTags<T> = (Refs, Tags, T);
pub type RefsNotes<T> = (Refs, Notes, T);
pub type RefsRemotes<T> = (Refs, Remotes, T);
pub type RefsNamespaces<'a, T> = (Refs, Namespaces, T, Qualified<'a>);

#[inline]
pub fn refs_heads<T: AsRef<RefStr>>(name: T) -> RefsHeads<T> {
    (Refs, Heads, name)
}

#[inline]
pub fn refs_tags<T: AsRef<RefStr>>(name: T) -> RefsTags<T> {
    (Refs, Tags, name)
}

#[inline]
pub fn refs_notes<T: AsRef<RefStr>>(name: T) -> RefsNotes<T> {
    (Refs, Notes, name)
}

#[inline]
pub fn refs_remotes<T: AsRef<RefStr>>(name: T) -> RefsRemotes<T> {
    (Refs, Remotes, name)
}

#[inline]
pub fn refs_namespaces<'a, 'b, T>(namespace: T, name: Qualified<'b>) -> RefsNamespaces<'b, T>
where
    T: Into<name::Component<'a>>,
{
    (Refs, Namespaces, namespace, name)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Refs;

impl Lit for Refs {
    const SELF: Self = Self;
    const NAME: &'static RefStr = name::REFS;
}
impl sealed::Sealed for Refs {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Heads;

impl Lit for Heads {
    const SELF: Self = Self;
    const NAME: &'static RefStr = name::HEADS;
}
impl sealed::Sealed for Heads {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Namespaces;

impl Lit for Namespaces {
    const SELF: Self = Self;
    const NAME: &'static RefStr = name::NAMESPACES;
}
impl sealed::Sealed for Namespaces {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Remotes;

impl Lit for Remotes {
    const SELF: Self = Self;
    const NAME: &'static RefStr = name::REMOTES;
}
impl sealed::Sealed for Remotes {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Tags;

impl Lit for Tags {
    const SELF: Self = Self;
    const NAME: &'static RefStr = name::TAGS;
}
impl sealed::Sealed for Tags {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Notes;

impl Lit for Notes {
    const SELF: Self = Self;
    const NAME: &'static RefStr = name::NOTES;
}
impl sealed::Sealed for Notes {}

#[cfg(feature = "link-literals")]
mod link {
    use super::*;

    pub type RefsRadId = (Refs, Rad, Id);
    pub type RefsRadSelf = (Refs, Rad, Selv);
    pub type RefsRadSignedRefs = (Refs, Rad, SignedRefs);
    pub type RefsRadIds<T> = (Refs, Rad, Ids, T);
    pub type RefsCobs<T, I> = (Refs, Cobs, T, I);

    pub const REFS_RAD_ID: (Refs, Rad, Id) = (Refs, Rad, Id);
    pub const REFS_RAD_SELF: (Refs, Rad, Selv) = (Refs, Rad, Selv);
    pub const REFS_RAD_SIGNED_REFS: (Refs, Rad, SignedRefs) = (Refs, Rad, SignedRefs);

    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    pub struct Rad;

    impl Lit for Rad {
        const SELF: Self = Self;
        const NAME: &'static RefStr = RefStr::from_str("rad");
    }
    impl sealed::Sealed for Rad {}

    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    pub struct Id;

    impl Lit for Id {
        const SELF: Self = Self;
        const NAME: &'static RefStr = RefStr::from_str("id");
    }
    impl sealed::Sealed for Id {}

    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    pub struct Ids;

    impl Lit for Ids {
        const SELF: Self = Self;
        const NAME: &'static RefStr = RefStr::from_str("ids");
    }
    impl sealed::Sealed for Ids {}

    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    pub struct Selv;

    impl Lit for Selv {
        const SELF: Self = Self;
        const NAME: &'static RefStr = RefStr::from_str("self");
    }
    impl sealed::Sealed for Selv {}

    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    pub struct SignedRefs;

    impl Lit for SignedRefs {
        const SELF: Self = Self;
        const NAME: &'static RefStr = RefStr::from_str("signed_refs");
    }
    impl sealed::Sealed for SignedRefs {}

    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
    pub struct Cobs;

    impl Lit for Cobs {
        const SELF: Self = Self;
        const NAME: &'static RefStr = RefStr::from_str("cobs");
    }
    impl sealed::Sealed for Cobs {}
}

#[cfg(feature = "link-literals")]
pub use link::*;
