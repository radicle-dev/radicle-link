// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Cow,
    fmt::{self, Display},
    ops::Deref,
};

use crate::{lit, name, Component, RefStr, RefString};

/// A fully-qualified refname.
///
/// A refname is qualified _iff_ it starts with "refs/" and has at least three
/// components. This implies that a [`Qualified`] ref has a category, such as
/// "refs/heads/main".
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Qualified<'a>(pub(crate) Cow<'a, RefStr>);

impl<'a> Qualified<'a> {
    /// Infallibly create a [`Qualified`] from components.
    ///
    /// Note that the "refs/" prefix is implicitly added, so `a` is the second
    /// [`Component`]. Mirroring [`Self::non_empty_components`], providing
    /// two [`Component`]s guarantees well-formedness of the [`Qualified`].
    /// `tail` may be empty.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use git_ref_format::{component, Qualified};
    ///
    /// assert_eq!(
    ///     "refs/heads/main",
    ///     Qualified::from_components(component::HEADS, component::MAIN, None).as_str()
    /// )
    /// ```
    pub fn from_components<'b, 'c, 'd, A, B, C>(a: A, b: B, tail: C) -> Self
    where
        A: Into<Component<'b>>,
        B: Into<Component<'c>>,
        C: IntoIterator<Item = Component<'d>>,
    {
        let mut inner = name::REFS.join(a.into()).and(b.into());
        inner.extend(tail);

        Self(inner.into())
    }

    pub fn from_refstr(r: impl Into<Cow<'a, RefStr>>) -> Option<Self> {
        Self::_from_refstr(r.into())
    }

    fn _from_refstr(r: Cow<'a, RefStr>) -> Option<Self> {
        let mut iter = r.iter();
        match (iter.next()?, iter.next()?, iter.next()?) {
            ("refs", _, _) => Some(Qualified(r)),
            _ => None,
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    #[inline]
    pub fn join<'b, R>(&self, other: R) -> Qualified<'b>
    where
        R: AsRef<RefStr>,
    {
        Qualified(self.0.join(other).into())
    }

    #[inline]
    pub fn namespaced(&self) -> Option<Namespaced> {
        self.0.as_ref().into()
    }

    /// Add a namespace.
    ///
    /// Creates a new [`Namespaced`] by prefxing `self` with
    /// "refs/namespaces/<ns>".
    pub fn add_namespace<'b>(&self, ns: Component<'b>) -> Namespaced<'a> {
        Namespaced(Cow::Owned(
            IntoIterator::into_iter([lit::Refs.into(), lit::Namespaces.into(), ns])
                .chain(self.0.components())
                .collect(),
        ))
    }

    /// Like [`Self::non_empty_components`], but with string slices.
    pub fn non_empty_iter(&self) -> (&str, &str, &str, name::Iter) {
        let mut iter = self.iter();
        (
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter,
        )
    }

    /// Return the first three [`Component`]s, and a possibly empty iterator
    /// over the remaining ones.
    ///
    /// A qualified ref is guaranteed to have at least three components, which
    /// this method provides a witness of. This is useful eg. for pattern
    /// matching on the prefix.
    pub fn non_empty_components(&self) -> (Component, Component, Component, name::Components) {
        let mut cs = self.components();
        (
            cs.next().unwrap(),
            cs.next().unwrap(),
            cs.next().unwrap(),
            cs,
        )
    }

    #[inline]
    pub fn to_owned<'b>(&self) -> Qualified<'b> {
        Qualified(Cow::Owned(self.0.clone().into_owned()))
    }

    #[inline]
    pub fn into_owned<'b>(self) -> Qualified<'b> {
        Qualified(Cow::Owned(self.0.into_owned()))
    }

    #[inline]
    pub fn into_refstring(self) -> RefString {
        self.into()
    }
}

impl Deref for Qualified<'_> {
    type Target = RefStr;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<RefStr> for Qualified<'_> {
    #[inline]
    fn as_ref(&self) -> &RefStr {
        self
    }
}

impl AsRef<str> for Qualified<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl AsRef<Self> for Qualified<'_> {
    #[inline]
    fn as_ref(&self) -> &Self {
        self
    }
}

impl<'a> From<Qualified<'a>> for Cow<'a, RefStr> {
    #[inline]
    fn from(q: Qualified<'a>) -> Self {
        q.0
    }
}

impl From<Qualified<'_>> for RefString {
    #[inline]
    fn from(q: Qualified) -> Self {
        q.0.into_owned()
    }
}

impl<T, U> From<(lit::Refs, T, U)> for Qualified<'_>
where
    T: AsRef<RefStr>,
    U: AsRef<RefStr>,
{
    #[inline]
    fn from((refs, cat, name): (lit::Refs, T, U)) -> Self {
        let refs: &RefStr = refs.into();
        Self(Cow::Owned(refs.join(cat).and(name)))
    }
}

impl<T> From<lit::RefsHeads<T>> for Qualified<'_>
where
    T: AsRef<RefStr>,
{
    #[inline]
    fn from((refs, heads, name): lit::RefsHeads<T>) -> Self {
        Self(Cow::Owned(
            IntoIterator::into_iter([Component::from(refs), heads.into()])
                .collect::<RefString>()
                .and(name),
        ))
    }
}

impl<T> From<lit::RefsTags<T>> for Qualified<'_>
where
    T: AsRef<RefStr>,
{
    #[inline]
    fn from((refs, tags, name): lit::RefsTags<T>) -> Self {
        Self(Cow::Owned(
            IntoIterator::into_iter([Component::from(refs), tags.into()])
                .collect::<RefString>()
                .and(name),
        ))
    }
}

impl<T> From<lit::RefsNotes<T>> for Qualified<'_>
where
    T: AsRef<RefStr>,
{
    #[inline]
    fn from((refs, notes, name): lit::RefsNotes<T>) -> Self {
        Self(Cow::Owned(
            IntoIterator::into_iter([Component::from(refs), notes.into()])
                .collect::<RefString>()
                .and(name),
        ))
    }
}

impl<T> From<lit::RefsRemotes<T>> for Qualified<'_>
where
    T: AsRef<RefStr>,
{
    #[inline]
    fn from((refs, remotes, name): lit::RefsRemotes<T>) -> Self {
        Self(Cow::Owned(
            IntoIterator::into_iter([Component::from(refs), remotes.into()])
                .collect::<RefString>()
                .and(name),
        ))
    }
}

impl Display for Qualified<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "link-literals")]
mod link {
    use super::*;

    impl From<lit::RefsRadId> for Qualified<'_> {
        #[inline]
        fn from((refs, rad, id): lit::RefsRadId) -> Self {
            Self(Cow::Owned(
                IntoIterator::into_iter([Component::from(refs), rad.into(), id.into()]).collect(),
            ))
        }
    }

    impl From<lit::RefsRadSelf> for Qualified<'_> {
        #[inline]
        fn from((refs, rad, selv): lit::RefsRadSelf) -> Self {
            Self(Cow::Owned(
                IntoIterator::into_iter([Component::from(refs), rad.into(), selv.into()]).collect(),
            ))
        }
    }

    impl From<lit::RefsRadSignedRefs> for Qualified<'_> {
        #[inline]
        fn from((refs, rad, sig): lit::RefsRadSignedRefs) -> Self {
            Self(Cow::Owned(
                IntoIterator::into_iter([Component::from(refs), rad.into(), sig.into()]).collect(),
            ))
        }
    }

    impl<'a, T: Into<Component<'a>>> From<lit::RefsRadIds<T>> for Qualified<'_> {
        #[inline]
        fn from((refs, rad, ids, id): lit::RefsRadIds<T>) -> Self {
            Self(Cow::Owned(
                IntoIterator::into_iter([Component::from(refs), rad.into(), ids.into(), id.into()])
                    .collect(),
            ))
        }
    }

    impl<'a, T: Into<Component<'a>>, I: Into<Component<'a>>> From<lit::RefsCobs<T, I>>
        for Qualified<'_>
    {
        #[inline]
        fn from((refs, cobs, ty, id): lit::RefsCobs<T, I>) -> Self {
            Self(Cow::Owned(
                IntoIterator::into_iter([Component::from(refs), cobs.into(), ty.into(), id.into()])
                    .collect(),
            ))
        }
    }
}

/// A [`Qualified`] ref under a git namespace.
///
/// A ref is namespaced if it starts with "refs/namespaces/", another path
/// component, and "refs/". Eg.
///
///     refs/namespaces/xyz/refs/heads/main
///
/// Note that namespaces can be nested, so the result of
/// [`Namespaced::strip_namespace`] may be convertible to a [`Namespaced`]
/// again. For example:
///
/// ```no_run
/// let full = refname!("refs/namespaces/a/refs/namespaces/b/refs/heads/main");
/// let namespaced = full.namespaced().unwrap();
/// let strip_first = namespaced.strip_namespace();
/// let nested = strip_first.namespaced().unwrap();
/// let strip_second = nested.strip_namespace();
///
/// assert_eq!("a", namespaced.namespace().as_str());
/// assert_eq!("b", nested.namespace().as_str());
/// assert_eq!("refs/namespaces/b/refs/heads/main", strip_first.as_str());
/// assert_eq!("refs/heads/main", strip_second.as_str());
/// ```
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Namespaced<'a>(Cow<'a, RefStr>);

impl<'a> Namespaced<'a> {
    pub fn namespace(&self) -> Component {
        self.components().nth(2).unwrap()
    }

    pub fn strip_namespace<'b>(&self) -> Qualified<'b> {
        const REFS_NAMESPACES: &RefStr = RefStr::from_str("refs/namespaces");

        Qualified(Cow::Owned(
            self.strip_prefix(REFS_NAMESPACES)
                .unwrap()
                .components()
                .skip(1)
                .collect(),
        ))
    }

    pub fn strip_namespace_recursive<'b>(&self) -> Qualified<'b> {
        let mut strip = self.strip_namespace();
        while let Some(ns) = strip.namespaced() {
            strip = ns.strip_namespace();
        }
        strip
    }

    #[inline]
    pub fn to_owned<'b>(&self) -> Namespaced<'b> {
        Namespaced(Cow::Owned(self.0.clone().into_owned()))
    }

    #[inline]
    pub fn into_owned<'b>(self) -> Namespaced<'b> {
        Namespaced(Cow::Owned(self.0.into_owned()))
    }

    #[inline]
    pub fn into_qualified(self) -> Qualified<'a> {
        self.into()
    }
}

impl Deref for Namespaced<'_> {
    type Target = RefStr;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<RefStr> for Namespaced<'_> {
    #[inline]
    fn as_ref(&self) -> &RefStr {
        self
    }
}

impl AsRef<str> for Namespaced<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl<'a> From<Namespaced<'a>> for Qualified<'a> {
    #[inline]
    fn from(ns: Namespaced<'a>) -> Self {
        Self(ns.0)
    }
}

impl<'a> From<&'a RefStr> for Option<Namespaced<'a>> {
    fn from(rs: &'a RefStr) -> Self {
        let mut cs = rs.iter();
        match (cs.next()?, cs.next()?, cs.next()?, cs.next()?) {
            ("refs", "namespaces", _, "refs") => Some(Namespaced(Cow::from(rs))),

            _ => None,
        }
    }
}

impl<'a, T> From<lit::RefsNamespaces<'_, T>> for Namespaced<'static>
where
    T: Into<Component<'a>>,
{
    #[inline]
    fn from((refs, namespaces, namespace, name): lit::RefsNamespaces<T>) -> Self {
        Self(Cow::Owned(
            IntoIterator::into_iter([refs.into(), namespaces.into(), namespace.into()])
                .collect::<RefString>()
                .and(name),
        ))
    }
}

impl Display for Namespaced<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
