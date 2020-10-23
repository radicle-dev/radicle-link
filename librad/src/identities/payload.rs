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

use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    convert::TryFrom,
    fmt::{self, Debug},
    iter::FromIterator,
    marker::PhantomData,
    ops::{Deref, DerefMut, RangeBounds},
};

use either::Either;
use multihash::Multihash;
use serde::ser::SerializeMap;
use thiserror::Error;
use url::Url;

use crate::{internal::canonical::Cstring, keys::PublicKey};

use super::{
    delegation,
    sealed,
    urn::{HasProtocol, Urn},
};

#[cfg(test)]
use proptest::prelude::*;

lazy_static! {
    /// Base [`Url`] for [`User`]
    static ref USER_NAMESPACE_BASE: Url =
        Url::parse("https://radicle.xyz/link/identities/user").unwrap();

    /// Versioned [`Url`] for [`User`], version 1
    static ref USER_NAMESPACE_V1: Url = {
        let mut base = USER_NAMESPACE_BASE.clone();
        base.path_segments_mut().unwrap().extend(&["v1"]);
        base
    };

    /// Base [`Url`] for [`Project`]
    static ref PROJECT_NAMESPACE_BASE: Url =
        Url::parse("https://radicle.xyz/link/identities/project").unwrap();

    /// Versioned [`Url`] for [`Project`], version 1
    static ref PROJECT_NAMESPACE_V1: Url = {
        let mut base = PROJECT_NAMESPACE_BASE.clone();
        base.path_segments_mut().unwrap().extend(&["v1"]);
        base
    };
}

/// Structure `radicle-link` expects to be part of a [`Payload`] describing a
/// personal identity.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub name: Cstring,
}

impl sealed::Sealed for User {}

#[cfg(test)]
impl Arbitrary for User {
    type Parameters = ();
    type Strategy = prop::strategy::Map<<Cstring as Arbitrary>::Strategy, fn(Cstring) -> Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        any::<Cstring>().prop_map(|name| User { name })
    }
}

/// Structure `radicle-link` expects to be part of a [`Payload`] describing a
/// project identity.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub name: Cstring,
    pub description: Option<Cstring>,
    pub default_branch: Option<Cstring>,
}

impl sealed::Sealed for Project {}

#[cfg(test)]
impl Arbitrary for Project {
    type Parameters = ();
    // Silly clippy: this _is_ a type definition
    #[allow(clippy::type_complexity)]
    type Strategy = prop::strategy::Map<
        (
            <Cstring as Arbitrary>::Strategy,
            <Option<Cstring> as Arbitrary>::Strategy,
            <Option<Cstring> as Arbitrary>::Strategy,
        ),
        fn((Cstring, Option<Cstring>, Option<Cstring>)) -> Self,
    >;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        Strategy::prop_map(
            (
                any::<Cstring>(),
                any::<Option<Cstring>>(),
                any::<Option<Cstring>>(),
            ),
            |(name, description, default_branch)| Project {
                name,
                description,
                default_branch,
            },
        )
    }
}

/// Namespace attached to a member type of the [`Payload`] "open" coproduct.
///
/// This is morally a constant -- we cannot, however, construct a [`Url`] in
/// `const` form, so the recommended use is like the following:
///
/// ```rust
/// use lazy_static::lazy_static;
/// use url::Url;
///
/// use librad::identities::payload::HasNamespace;
///
/// lazy_static! {
///     static ref MY_NAMESPACE: Url = Url::parse("https://semantic.me/mytype/v1").unwrap();
/// }
///
/// struct MyType {
///     something: u32,
/// }
///
/// impl HasNamespace for MyType {
///     fn namespace() -> &'static Url {
///         &MY_NAMESPACE
///     }
/// }
/// ```
pub trait HasNamespace {
    fn namespace() -> &'static Url;
}

impl HasNamespace for User {
    fn namespace() -> &'static Url {
        &USER_NAMESPACE_V1
    }
}

impl HasNamespace for Project {
    fn namespace() -> &'static Url {
        &PROJECT_NAMESPACE_V1
    }
}

/// Internal trait which helps deal with future versions
pub trait Subject: HasNamespace + sealed::Sealed {
    fn namespace_matches(url: &Url) -> bool;
}

impl Subject for User {
    fn namespace_matches(url: &Url) -> bool {
        url.as_str().starts_with(USER_NAMESPACE_BASE.as_str())
    }
}

impl Subject for Project {
    fn namespace_matches(url: &Url) -> bool {
        url.as_str().starts_with(PROJECT_NAMESPACE_BASE.as_str())
    }
}

pub type UserPayload = Payload<User>;
pub type ProjectPayload = Payload<Project>;

/// [`Payload`] for which the type is not known statically.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SomePayload {
    User(UserPayload),
    Project(ProjectPayload),
}

/// Payload of an identity document.
///
/// This type is a simple formulation of an "open sum", where the type of one
/// member is known. Additional members (extensions) are represented as
/// [`serde_json::Value`]s. Every member is namespaced by a [`Url`], as
/// described by its [`HasNamespace`] impl.
///
/// Note that it is an error during deserialisation if duplicate namespaces are
/// found in the input -- this is unlike normal JSON deserialisation, which
/// would just treat objects as maps, retaining the last key found in the input.
#[derive(Clone, Debug, PartialEq)]
pub struct Payload<T> {
    pub subject: T,
    ext: BTreeMap<Url, serde_json::Value>,
}

#[derive(Debug, Error)]
pub enum ExtError {
    #[error("extension namespace can not be the subject namespace")]
    ExtensionIsSubject,

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl<T> Payload<T>
where
    T: Subject,
{
    pub fn new(subject: T) -> Self {
        Self {
            subject,
            ext: BTreeMap::new(),
        }
    }

    pub fn with_ext<U>(mut self, val: U) -> Result<Self, ExtError>
    where
        U: HasNamespace + serde::Serialize,
    {
        self.set_ext(val)?;
        Ok(self)
    }

    pub fn set_ext<U>(&mut self, val: U) -> Result<(), ExtError>
    where
        U: HasNamespace + serde::Serialize,
    {
        if T::namespace_matches(U::namespace()) {
            return Err(ExtError::ExtensionIsSubject);
        }

        let val = serde_json::to_value(val)?;
        self.ext.insert(U::namespace().clone(), val);

        Ok(())
    }

    pub fn get_ext<U>(&self) -> Result<Option<U>, serde_json::Error>
    where
        U: HasNamespace + serde::de::DeserializeOwned,
    {
        self.ext
            .get(U::namespace())
            .map(|val| serde_json::from_value(val.clone()))
            .transpose()
    }

    pub fn query_ext<R>(&self, range: R) -> impl Iterator<Item = (&Url, &serde_json::Value)>
    where
        R: RangeBounds<Url>,
    {
        self.ext.range(range)
    }
}

impl<T> serde::Serialize for Payload<T>
where
    T: Subject + serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.ext.len() + 1))?;
        map.serialize_entry(T::namespace(), &self.subject)?;
        self.ext
            .iter()
            .try_for_each(|(k, v)| map.serialize_entry(k, v))?;
        map.end()
    }
}

impl<'de, T> serde::Deserialize<'de> for Payload<T>
where
    T: Subject + serde::de::DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor<T>(PhantomData<T>);

        impl<'de, T> serde::de::Visitor<'de> for Visitor<T>
        where
            T: Subject + serde::de::DeserializeOwned,
        {
            type Value = Payload<T>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an identity doc payload")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut subject = None;
                let mut ext: BTreeMap<Url, serde_json::Value> = BTreeMap::new();
                while let Some(k) = access.next_key()? {
                    if T::namespace_matches(&k) {
                        match subject {
                            // Once we have more subject versions, we'll want to
                            // dispatch here using `MapAccess::next_value_seed()`,
                            // passing the Url key
                            None => subject = access.next_value()?,
                            Some(_) => {
                                // FIXME(kim): Make this convey what version we already saw
                                return Err(serde::de::Error::custom(
                                    "multiple subject versions in document",
                                ));
                            },
                        }
                    } else {
                        match ext.entry(k) {
                            Entry::Vacant(entry) => {
                                entry.insert(access.next_value()?);
                            },
                            Entry::Occupied(entry) => {
                                return Err(serde::de::Error::custom(format!(
                                    "duplicate field `{}`",
                                    entry.key().clone()
                                )));
                            },
                        }
                    }
                }

                subject
                    .ok_or_else(|| serde::de::Error::missing_field("subject"))
                    .map(|subject| Payload { subject, ext })
            }
        }

        deserializer.deserialize_map(Visitor(PhantomData))
    }
}

impl<T> From<T> for Payload<T>
where
    T: Subject,
{
    fn from(subject: T) -> Self {
        Self::new(subject)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum SomeDelegations<R, E>
where
    R: Debug + Ord + HasProtocol + TryFrom<Multihash, Error = E>,
    E: std::error::Error + 'static,
{
    User(UserDelegations),
    Project(ProjectDelegations<R>),
}

/// Delegations of a [`UserPayload`] identity document.
///
/// This is just a set of [`PublicKey`]s. Note that it is a deserialisation
/// error if duplicate elements are found in the input.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct UserDelegations(BTreeSet<PublicKey>);

impl Deref for UserDelegations {
    type Target = BTreeSet<PublicKey>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for UserDelegations {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<delegation::Direct> for UserDelegations {
    fn from(d: delegation::Direct) -> Self {
        Self(d.into())
    }
}

impl From<UserDelegations> for BTreeSet<PublicKey> {
    fn from(UserDelegations(set): UserDelegations) -> Self {
        set
    }
}

impl<'de> serde::Deserialize<'de> for UserDelegations {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UserDelegations;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a UserDelegations set")
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut set = BTreeSet::new();
                while let Some(key) = seq.next_element::<PublicKey>()? {
                    if set.contains(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate delegation `{}`",
                            key
                        )));
                    }

                    set.insert(key);
                }

                if set.is_empty() {
                    Err(serde::de::Error::custom("no delegations"))
                } else {
                    Ok(UserDelegations(set))
                }
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

/// Helper for [`ProjectDelegations`], isomorphic to [`Either<PublicKey,
/// Urn<R>>`].
///
/// Note that the type is serialised without any variant tagging.
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct KeyOrUrn<R> {
    inner: Either<PublicKey, Urn<R>>,
}

impl<R> From<Either<PublicKey, Urn<R>>> for KeyOrUrn<R> {
    fn from(inner: Either<PublicKey, Urn<R>>) -> Self {
        Self { inner }
    }
}

impl<R> From<KeyOrUrn<R>> for Either<PublicKey, Urn<R>> {
    fn from(KeyOrUrn { inner }: KeyOrUrn<R>) -> Self {
        inner
    }
}

impl<R> serde::Serialize for KeyOrUrn<R>
where
    R: HasProtocol + serde::Serialize,
    for<'a> &'a R: Into<Multihash>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        either::serde_untagged::serialize(&self.inner, serializer)
    }
}

impl<'de, R, E> serde::Deserialize<'de> for KeyOrUrn<R>
where
    R: HasProtocol + TryFrom<Multihash, Error = E>,
    E: std::error::Error + 'static,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        either::serde_untagged::deserialize(deserializer).map(|inner| Self { inner })
    }
}

/// Delegations of a [`ProjectPayload`] identity document.
///
/// Note that this represents the _serialised_ form, and is not intended to be
/// manipulated directly.
///
/// It is a deserialisation error if the input contains duplicate elements.
/// Note, however, that the specification requires an additional validation step
/// after resolving any [`Urn`] pointers -- the identity document is invalid if
/// it contains duplicate _keys_.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectDelegations<R: Ord> {
    inner: BTreeSet<KeyOrUrn<R>>,
}

impl<R: Ord> IntoIterator for ProjectDelegations<R> {
    type Item = KeyOrUrn<R>;
    type IntoIter = <BTreeSet<KeyOrUrn<R>> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<R: Ord> FromIterator<Either<PublicKey, Urn<R>>> for ProjectDelegations<R> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Either<PublicKey, Urn<R>>>,
    {
        Self {
            inner: iter.into_iter().map(KeyOrUrn::from).collect(),
        }
    }
}

impl<R> serde::Serialize for ProjectDelegations<R>
where
    R: Ord + HasProtocol + serde::Serialize,
    for<'a> &'a R: Into<Multihash>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.serialize(serializer)
    }
}

impl<'de, R, E> serde::Deserialize<'de> for ProjectDelegations<R>
where
    R: Debug + Ord + HasProtocol + TryFrom<Multihash, Error = E>,
    E: std::error::Error + 'static,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor<R>(PhantomData<R>);

        impl<'de, R, E> serde::de::Visitor<'de> for Visitor<R>
        where
            R: Debug + Ord + HasProtocol + TryFrom<Multihash, Error = E>,
            E: std::error::Error + 'static,
        {
            type Value = ProjectDelegations<R>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a ProjectDelegations set")
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                let mut set = BTreeSet::new();
                while let Some(key_or_urn) = seq.next_element::<KeyOrUrn<R>>()? {
                    if set.contains(&key_or_urn) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate delegation `{:?}`",
                            key_or_urn
                        )));
                    }

                    set.insert(key_or_urn);
                }

                if set.is_empty() {
                    Err(serde::de::Error::custom("no delegations"))
                } else {
                    Ok(ProjectDelegations { inner: set })
                }
            }
        }

        deserializer.deserialize_seq(Visitor(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use git_ext::Oid;
    use librad_test::roundtrip::*;
    use pretty_assertions::assert_eq;

    use crate::{
        identities::gen::gen_oid,
        keys::{tests::gen_public_key, SecretKey},
    };

    lazy_static! {
        static ref UPSTREAM_USER_NAMESPACE: Url =
            Url::parse("https://radicle.xyz/upstream/user/v1").unwrap();
        static ref UPSTREAM_PROJECT_NAMESPACE: Url =
            Url::parse("https://radicle.xyz/upstream/project/v1").unwrap();
    }

    #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct UpstreamUser {
        #[serde(rename = "radicle-registry-name")]
        registered_as: Cstring,
    }

    impl Arbitrary for UpstreamUser {
        type Parameters = ();
        type Strategy = prop::strategy::Map<<Cstring as Arbitrary>::Strategy, fn(Cstring) -> Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            any::<Cstring>().prop_map(|registered_as| Self { registered_as })
        }
    }

    impl HasNamespace for UpstreamUser {
        fn namespace() -> &'static Url {
            &UPSTREAM_USER_NAMESPACE
        }
    }

    #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct UpstreamProject {
        #[serde(rename = "radicle-registry-name")]
        registered_as: Cstring,
    }

    impl Arbitrary for UpstreamProject {
        type Parameters = ();
        type Strategy = prop::strategy::Map<<Cstring as Arbitrary>::Strategy, fn(Cstring) -> Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            any::<Cstring>().prop_map(|registered_as| Self { registered_as })
        }
    }

    impl HasNamespace for UpstreamProject {
        fn namespace() -> &'static Url {
            &UPSTREAM_PROJECT_NAMESPACE
        }
    }

    fn gen_user_payload() -> impl Strategy<Value = UserPayload> {
        (any::<User>(), proptest::option::of(any::<UpstreamUser>())).prop_map(|(user, up)| {
            let mut u = UserPayload::new(user);
            if let Some(up) = up {
                u.set_ext(up).unwrap();
            }
            u
        })
    }

    fn gen_project_payload() -> impl Strategy<Value = ProjectPayload> {
        (
            any::<Project>(),
            proptest::option::of(any::<UpstreamProject>()),
        )
            .prop_map(|(project, up)| {
                let mut p = ProjectPayload::new(project);
                if let Some(up) = up {
                    p.set_ext(up).unwrap();
                }
                p
            })
    }

    fn gen_payload() -> impl Strategy<Value = SomePayload> {
        prop_oneof![
            gen_user_payload().prop_map(SomePayload::User),
            gen_project_payload().prop_map(SomePayload::Project)
        ]
    }

    fn gen_user_delegations() -> impl Strategy<Value = UserDelegations> {
        proptest::collection::btree_set(gen_public_key(), 1..32).prop_map(UserDelegations)
    }

    fn gen_key_or_urn() -> impl Strategy<Value = KeyOrUrn<Oid>> {
        prop_oneof![
            gen_public_key().prop_map(|pk| KeyOrUrn {
                inner: Either::Left(pk)
            }),
            gen_oid(git2::ObjectType::Tree).prop_map(|oid| KeyOrUrn {
                inner: Either::Right(Urn::new(oid))
            })
        ]
    }

    fn gen_project_delegations() -> impl Strategy<Value = ProjectDelegations<Oid>> {
        proptest::collection::btree_set(gen_key_or_urn(), 1..64)
            .prop_map(|inner| ProjectDelegations { inner })
    }

    #[test]
    fn user_example() {
        let payload = UserPayload::new(User {
            name: "cloudhead".into(),
        })
        .with_ext(UpstreamUser {
            registered_as: "cloudhead".into(),
        })
        .unwrap();

        let json_pretty = r#"{
  "https://radicle.xyz/link/identities/user/v1": {
    "name": "cloudhead"
  },
  "https://radicle.xyz/upstream/user/v1": {
    "radicle-registry-name": "cloudhead"
  }
}"#;

        assert_eq!(
            serde_json::to_string_pretty(&payload).unwrap(),
            json_pretty.to_owned()
        )
    }

    #[test]
    fn project_example() {
        let descr = "nom, eating data byte by byte

nom is a parser combinators library written in Rust.";

        let payload = ProjectPayload::new(Project {
            name: "nom".into(),
            description: Some(descr.into()),
            default_branch: Some("mistress".into()),
        })
        .with_ext(UpstreamProject {
            registered_as: "nomnomnom".into(),
        })
        .unwrap();

        let json_pretty = r#"{
  "https://radicle.xyz/link/identities/project/v1": {
    "name": "nom",
    "description": "nom, eating data byte by byte\n\nnom is a parser combinators library written in Rust.",
    "default_branch": "mistress"
  },
  "https://radicle.xyz/upstream/project/v1": {
    "radicle-registry-name": "nomnomnom"
  }
}"#;

        assert_eq!(
            serde_json::to_string_pretty(&payload).unwrap(),
            json_pretty.to_owned()
        );

        cjson_roundtrip(payload)
    }

    fn duplicate_delegation<T>()
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let k = SecretKey::new().public();
        let d = vec![k, k];

        let ser = serde_json::to_string(&d).unwrap();
        assert!(matches!(
            serde_json::from_str::<T>(&ser),
            Err(e) if e.to_string().starts_with("duplicate delegation")
        ))
    }

    #[test]
    fn duplicate_user_delegation() {
        duplicate_delegation::<UserDelegations>()
    }

    #[test]
    fn duplicate_project_delegation() {
        duplicate_delegation::<ProjectDelegations<Oid>>()
    }

    fn empty_delegations<T>()
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let ser = serde_json::to_string(&[] as &[T]).unwrap();
        assert!(matches!(
            serde_json::from_str::<T>(&ser),
            Err(e) if e.to_string().starts_with("no delegations")
        ))
    }

    #[test]
    fn empty_user_delegations() {
        empty_delegations::<UserDelegations>()
    }

    #[test]
    fn empty_project_delegations() {
        empty_delegations::<ProjectDelegations<Oid>>()
    }

    #[test]
    fn duplicate_namespace() {
        let json = r#"{
            "https://radicle.xyz/link/identities/project/v1": {
                "name": "foo"
            },
            "https://semantic.me/ld": {},
            "https://semantic.me/ld": {},
        }"#;

        assert!(matches!(
            serde_json::from_str::<ProjectPayload>(json),
            Err(e) if e.to_string().starts_with("duplicate field")
        ))
    }

    #[test]
    fn subject_from_the_future() {
        let json = r#"{
            "https://radicle.xyz/link/identities/project/v1": {
                "name": "foo"
            },
            "https://radicle.xyz/link/identities/project/v2": {
                "name": "foo",
                "bdfl": "dylan"
            }
        }"#;

        assert!(matches!(
            serde_json::from_str::<ProjectPayload>(json),
            Err(e) if e.to_string().starts_with("multiple subject versions in document")
        ))
    }

    /// All serialisation roundtrips required for payload types
    fn trippin<A>(a: A)
    where
        A: Clone + Debug + PartialEq + serde::Serialize + serde::de::DeserializeOwned,
    {
        cjson_roundtrip(a.clone());
        json_roundtrip(a)
    }

    proptest! {
        #[test]
        fn any_payload_roundtrip(payload in gen_payload()) {
            trippin(payload)
        }

        #[test]
        fn user_delegations_roundtrip(delegations in gen_user_delegations()) {
            trippin(delegations)
        }

        #[test]
        fn project_delegations_roundtrip(delegations in gen_project_delegations()) {
            trippin(delegations)
        }

        #[test]
        fn key_or_urn(key_or_urn in gen_key_or_urn()) {
            json_roundtrip(key_or_urn)
        }
    }
}
