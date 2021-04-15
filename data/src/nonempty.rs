// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    hash::{BuildHasher, Hash},
    ops::Deref,
};

pub use nonempty::NonEmpty as NonEmptyVec;

/// Alias for a [`NonEmpty`] backed by a [`HashSet`]
pub type NonEmptyHashSet<T> = NonEmpty<HashSet<T>>;

/// Alias for a [`NonEmpty`] backed by a [`BTreeSet`]
pub type NonEmptyOrderedSet<T> = NonEmpty<BTreeSet<T>>;

/// Alias for a [`NonEmpty`] backed by a [`HashMap`]
pub type NonEmptyHashMap<K, V, S> = NonEmpty<HashMap<K, V, S>>;

/// Alias for a [`NonEmpty`] backed by a [`BTreeMap`]
pub type NonEmptyOrderedMap<K, V> = NonEmpty<BTreeMap<K, V>>;

/// Types which may be empty.
///
/// A [`NonEmpty`] can only be constructed from implementors when `is_empty`
/// returns `false`.
pub trait MaybeEmpty {
    /// `true` if this type does not contain any elements
    fn is_empty(&self) -> bool;
}

impl<T> MaybeEmpty for BTreeSet<T> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<T> MaybeEmpty for HashSet<T> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<K, V> MaybeEmpty for BTreeMap<K, V> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<K, V, S> MaybeEmpty for HashMap<K, V, S> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

/// Mutable set operations which don't decrease the size of the set.
pub trait Set {
    type Value;

    /// Adds a value to the set.
    ///
    /// If the set did not have this value present, true is returned. If the set
    /// did have this value present, false is returned, and the entry is not
    /// updated.
    fn insert(&mut self, value: Self::Value) -> bool;

    /// Adds a value to the set, replacing the existing value, if any, that is
    /// equal to the given one. Returns the replaced value.
    fn replace(&mut self, value: Self::Value) -> Option<Self::Value>;
}

impl<T> Set for BTreeSet<T>
where
    T: Ord,
{
    type Value = T;

    fn insert(&mut self, value: T) -> bool {
        self.insert(value)
    }

    fn replace(&mut self, value: T) -> Option<T> {
        self.replace(value)
    }
}

impl<T, S> Set for HashSet<T, S>
where
    T: Eq + Hash,
    S: BuildHasher,
{
    type Value = T;

    fn insert(&mut self, value: T) -> bool {
        self.insert(value)
    }

    fn replace(&mut self, value: T) -> Option<T> {
        self.replace(value)
    }
}

/// Mutable map operations which don't decrease the size of the map.
pub trait Map {
    type Key;
    type Value;

    /// Inserts a key-value pair into the map.
    ///
    /// If the map did not have this key present, None is returned. If the map
    /// did have this key present, the value is updated, and the old value
    /// is returned. The key is not updated, though; this matters for types
    /// that can be == without being identical.
    fn insert(&mut self, key: Self::Key, value: Self::Value) -> Option<Self::Value>;
}

impl<K, V> Map for BTreeMap<K, V>
where
    K: Ord,
{
    type Key = K;
    type Value = V;

    fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.insert(key, value)
    }
}

/// Containers that implement an insert function.
pub trait Insert {
    type Value;
    type Result;

    /// Insert a value into the container and returns the result.
    fn insert(&mut self, value: Self::Value) -> Self::Result;
}

impl<V> Insert for HashSet<V>
where
    V: Eq + Hash,
{
    type Value = V;
    type Result = bool;

    fn insert(&mut self, value: Self::Value) -> Self::Result {
        HashSet::insert(self, value)
    }
}

impl<V> Insert for BTreeSet<V>
where
    V: Ord,
{
    type Value = V;
    type Result = bool;

    fn insert(&mut self, value: Self::Value) -> Self::Result {
        BTreeSet::insert(self, value)
    }
}

impl<K, V> Insert for HashMap<K, V>
where
    K: Eq + Hash,
{
    type Value = (K, V);
    type Result = Option<V>;

    fn insert(&mut self, (key, value): Self::Value) -> Self::Result {
        HashMap::insert(self, key, value)
    }
}

impl<K, V> Insert for BTreeMap<K, V>
where
    K: Ord,
{
    type Value = (K, V);
    type Result = Option<V>;

    fn insert(&mut self, (key, value): Self::Value) -> Self::Result {
        BTreeMap::insert(self, key, value)
    }
}

/// Newtype wrapper around container types, which witnesses that the container
/// contains at least one element.
///
/// Non-mutating methods of the underlying container are available through the
/// [`Deref`] impl.
///
/// Mutating methods which either grow the container or don't change its size
/// are provided via the [`Set`] and [`Map`] impls, respectively. Additionally,
/// [`Extend`] is implemented if the container implements it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NonEmpty<T>(T);

impl<T> NonEmpty<T> {
    /// Construct a [`NonEmpty`] with exactly one element.
    pub fn new(v: T::Value) -> Self
    where
        T: Default + Insert,
    {
        let mut container = T::default();
        container.insert(v);
        Self(container)
    }
    /// Construct a [`NonEmpty`] from a possibly empty type.
    ///
    /// If the argument is empty, ie. [`MaybeEmpty::is_empty`] evaluates to
    /// `true`, [`None`] is returned.
    pub fn from_maybe_empty(maybe_empty: T) -> Option<Self>
    where
        T: MaybeEmpty,
    {
        if maybe_empty.is_empty() {
            None
        } else {
            Some(Self(maybe_empty))
        }
    }

    /// Construct a [`NonEmpty`] with an inner type satisfying [`Set`], and
    /// exactly one element.
    pub fn singleton_set<V>(v: V) -> Self
    where
        T: Set<Value = V> + Default,
    {
        let mut inner = T::default();
        inner.insert(v);
        Self(inner)
    }

    /// Construct a [`NonEmpty`] with an inner type satisfying [`Map`], and
    /// exactly one element.
    pub fn singleton_map<K, V>(k: K, v: V) -> Self
    where
        T: Map<Key = K, Value = V> + Default,
    {
        let mut inner = T::default();
        inner.insert(k, v);
        Self(inner)
    }

    /// Consumes the [`NonEmpty`], returning the wrapped value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: Set> Set for NonEmpty<T> {
    type Value = <T as Set>::Value;

    fn insert(&mut self, value: Self::Value) -> bool {
        self.0.insert(value)
    }

    fn replace(&mut self, value: Self::Value) -> Option<Self::Value> {
        self.0.replace(value)
    }
}

impl<T: Map> Map for NonEmpty<T> {
    type Key = <T as Map>::Key;
    type Value = <T as Map>::Value;

    fn insert(&mut self, key: Self::Key, value: Self::Value) -> Option<Self::Value> {
        self.0.insert(key, value)
    }
}

impl<T> Deref for NonEmpty<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, U> Extend<U> for NonEmpty<T>
where
    T: Extend<U>,
{
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = U>,
    {
        self.0.extend(iter)
    }
}

impl<'a, T> IntoIterator for &'a NonEmpty<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.borrow().into_iter()
    }
}

impl<T> IntoIterator for NonEmpty<T>
where
    T: IntoIterator,
{
    type Item = <T as IntoIterator>::Item;
    type IntoIter = <T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::*;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl<T> Serialize for NonEmpty<T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            self.0.serialize(serializer)
        }
    }

    impl<'de, T> Deserialize<'de> for NonEmpty<T>
    where
        T: Deserialize<'de> + MaybeEmpty,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            use serde::de::Error as _;

            let inner = T::deserialize(deserializer)?;
            NonEmpty::from_maybe_empty(inner)
                .ok_or_else(|| D::Error::custom("attempt to deserialize from an empty container"))
        }
    }
}

#[cfg(feature = "minicbor")]
mod minicbor_impls {
    use super::*;

    use minicbor::{decode, encode, Decode, Decoder, Encode, Encoder};

    impl<T> Encode for NonEmpty<T>
    where
        T: Encode,
    {
        fn encode<W: encode::Write>(
            &self,
            e: &mut Encoder<W>,
        ) -> Result<(), encode::Error<W::Error>> {
            self.0.encode(e)
        }
    }

    impl<'de, T> Decode<'de> for NonEmpty<T>
    where
        T: Decode<'de> + MaybeEmpty,
    {
        fn decode(d: &mut Decoder<'de>) -> Result<Self, decode::Error> {
            let inner = T::decode(d)?;
            NonEmpty::from_maybe_empty(inner).ok_or(decode::Error::Message(
                "attempt to decode from an empty container",
            ))
        }
    }
}
