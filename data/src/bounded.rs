// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Borrow as _,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    iter::{self, FromIterator},
    marker::PhantomData,
    ops::Deref,
};

use thiserror::Error;
use typenum::{IsGreaterOrEqual, IsLessOrEqual, Unsigned, U0, U1};

/// Types which have a length.
pub trait Length {
    fn length(&self) -> usize;
}

impl<T> Length for Vec<T> {
    fn length(&self) -> usize {
        Vec::len(self)
    }
}

impl<T> Length for BTreeSet<T> {
    fn length(&self) -> usize {
        BTreeSet::len(self)
    }
}

impl<K, V> Length for BTreeMap<K, V> {
    fn length(&self) -> usize {
        BTreeMap::len(self)
    }
}

impl<T, S> Length for HashSet<T, S> {
    fn length(&self) -> usize {
        HashSet::len(self)
    }
}

impl<K, V, S> Length for HashMap<K, V, S> {
    fn length(&self) -> usize {
        HashMap::len(self)
    }
}

impl Length for String {
    fn length(&self) -> usize {
        String::len(self)
    }
}

impl Length for &str {
    fn length(&self) -> usize {
        str::len(self)
    }
}

impl<T> Length for &[T] {
    fn length(&self) -> usize {
        (self as &[T]).len()
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("minimum length {min} not reached: {len}")]
    TooSmall { min: usize, len: usize },

    #[error("maximum length {max} exceeded: {len}")]
    TooLarge { max: usize, len: usize },
}

/// Newtype wrapper for types which have a [`Length`], witnessing that the
/// length is within the inclusive range `[N, M]`.
///
/// Note that this type doesn't track the actual length on the type level, and
/// so is immutable. Its main use is for validating untrusted input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Within<N, M, T> {
    inner: T,
    _min: PhantomData<N>,
    _max: PhantomData<M>,
}

impl<N, M, T> Within<N, M, T> {
    /// Construct a value whose [`Length`] is within `[N, M]`.
    pub fn try_from_length(t: T) -> Result<Self, Error>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Length,
    {
        let min = N::USIZE;
        let max = M::USIZE;
        let len = t.length();

        if len < min {
            Err(Error::TooSmall { min, len })
        } else if len > max {
            Err(Error::TooLarge { max, len })
        } else {
            Ok(Self {
                inner: t,
                _min: PhantomData,
                _max: PhantomData,
            })
        }
    }

    /// Construct a value of [`Length`] 1, provided the range bounds allow it.
    pub fn singleton<V>(v: V) -> Self
    where
        N: IsLessOrEqual<U1>,
        M: IsGreaterOrEqual<U1>,
        T: FromIterator<V>,
    {
        iter::once(v).into()
    }

    /// Extend the inner collection with the values of an iterator.
    ///
    /// To maintain the upper bound `M`, the given iterator may not be consumed
    /// fully.
    pub fn extend_fill<I, V>(&mut self, iter: I)
    where
        M: Unsigned,
        I: IntoIterator<Item = V>,
        T: Length + Extend<V>,
    {
        let len = self.inner.length();
        let max = M::USIZE;
        self.inner.extend(iter.into_iter().take(max - len));
    }

    /// Consumes the [`Within`], returning the wrapped value.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<N, M, T> Deref for Within<N, M, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a, N, M, T> IntoIterator for &'a Within<N, M, T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.borrow().into_iter()
    }
}

impl<N, M, T> IntoIterator for Within<N, M, T>
where
    T: IntoIterator,
{
    type Item = <T as IntoIterator>::Item;
    type IntoIter = <T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<N, M, V, T> From<iter::Once<V>> for Within<N, M, T>
where
    N: IsLessOrEqual<U1>,
    M: IsGreaterOrEqual<U1>,
    T: FromIterator<V>,
{
    fn from(once: iter::Once<V>) -> Self {
        Self {
            inner: once.collect(),
            _min: PhantomData,
            _max: PhantomData,
        }
    }
}

/// Alias for types which might be empty (have a [`Length`] of zero), and a
/// _maximum_ [`Length`] of `N`.
pub type Bounded<N, T> = Within<U0, N, T>;

/// Alias for a [`Bounded`] [`Vec`].
pub type BoundedVec<N, T> = Bounded<N, Vec<T>>;

/// Alias for a [`Bounded`] [`BTreeSet`].
pub type BoundedOrderedSet<N, T> = Bounded<N, BTreeSet<T>>;

/// Alias for a [`Bounded`] [`HashSet`].
pub type BoundedHashSet<N, T, S> = Bounded<N, HashSet<T, S>>;

/// Alias for a [`Bounded`] [`BTreeMap`].
pub type BoundedOrderedMap<N, K, V> = Bounded<N, BTreeMap<K, V>>;

/// Alias for a [`Bounded`] [`HashMap`].
pub type BoundedHashMap<N, K, V, S> = Bounded<N, HashMap<K, V, S>>;

impl<N, V, T> From<iter::Empty<V>> for Bounded<N, T>
where
    T: FromIterator<V>,
{
    fn from(empty: iter::Empty<V>) -> Self {
        Self {
            inner: empty.collect(),
            _min: PhantomData,
            _max: PhantomData,
        }
    }
}

/// Instead of returning an error when the deserialized value exceeds the
/// maximum length, truncate it to the maximum length.
///
/// Note that this will deserialize the whole structure into memory before
/// truncating it.
///
/// This function is only available when the `serde` feature is enabled.
///
/// # Example
///
/// ```no_run
/// use radicle_data::BoundedVec;
/// use typenum::U10;
///
/// #[derive(serde::Deserialize)]
/// struct MyStruct {
///     #[serde(deserialize_with = "radicle_data::bounded::deserialize_truncate")]
///     just_ten: BoundedVec<U10, u8>,
/// }
/// ```
#[cfg(feature = "serde")]
pub fn deserialize_truncate<'de, N, T, D>(d: D) -> Result<Bounded<N, T>, D::Error>
where
    N: Unsigned,
    T: serde::Deserialize<'de> + IntoIterator + FromIterator<T::Item>,
    D: serde::Deserializer<'de>,
{
    let inner = T::deserialize(d)?.into_iter().take(N::USIZE).collect::<T>();
    Ok(Bounded {
        inner,
        _min: PhantomData,
        _max: PhantomData,
    })
}

/// Instead of returning an error when the decoded value exceeds the maximum
/// length, truncate it to the maximum length.
///
/// Note that this will decode the whole structure into memory before truncating
/// it.
///
/// This function is only available when the `minicbor` feature is enabled.
///
/// # Example
///
/// ```no_run
/// use radicle_data::BoundedVec;
/// use typenum::U10;
///
/// #[derive(minicbor::Decode)]
/// struct MyStruct {
///     #[n(0)]
///     #[cbor(decode_with = "radicle_data::bounded::decode_truncate")]
///     just_ten: BoundedVec<U10, u8>,
/// }
/// ```
#[cfg(feature = "minicbor")]
pub fn decode_truncate<'de, N, T>(
    d: &mut minicbor::Decoder<'de>,
) -> Result<Bounded<N, T>, minicbor::decode::Error>
where
    N: Unsigned,
    T: minicbor::Decode<'de> + IntoIterator + FromIterator<T::Item>,
{
    let inner = T::decode(d)?.into_iter().take(N::USIZE).collect::<T>();
    Ok(Bounded {
        inner,
        _min: PhantomData,
        _max: PhantomData,
    })
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::*;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl<N, M, T> Serialize for Within<N, M, T>
    where
        T: Serialize,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            self.inner.serialize(serializer)
        }
    }

    impl<'de, N, M, T> Deserialize<'de> for Within<N, M, T>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Deserialize<'de> + Length,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            use serde::de::Error as _;

            let inner = T::deserialize(deserializer)?;
            Within::try_from_length(inner).map_err(D::Error::custom)
        }
    }
}

#[cfg(feature = "minicbor")]
mod minicbor_impls {
    use super::*;
    use std::{collections::hash_map::RandomState, hash::Hash};

    use minicbor::{decode, encode, Decode, Decoder, Encode, Encoder};

    impl<N, M, T> Encode for Within<N, M, T>
    where
        T: Encode,
    {
        fn encode<W: encode::Write>(
            &self,
            e: &mut Encoder<W>,
        ) -> Result<(), encode::Error<W::Error>> {
            self.inner.encode(e)
        }
    }

    impl<'de, N, M, T> Decode<'de> for Within<N, M, Vec<T>>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Decode<'de>,
    {
        fn decode(d: &mut Decoder<'de>) -> Result<Self, decode::Error> {
            decode_array(d)
        }
    }

    impl<'de, N, M, T> Decode<'de> for Within<N, M, BTreeSet<T>>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Ord + Decode<'de>,
    {
        fn decode(d: &mut Decoder<'de>) -> Result<Self, decode::Error> {
            decode_array(d)
        }
    }

    impl<'de, N, M, K, V> Decode<'de> for Within<N, M, BTreeMap<K, V>>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        K: Ord + Decode<'de>,
        V: Decode<'de>,
    {
        fn decode(d: &mut Decoder<'de>) -> Result<Self, decode::Error> {
            decode_map(d)
        }
    }

    impl<'de, N, M, T> Decode<'de> for Within<N, M, HashSet<T, RandomState>>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Eq + Hash + Decode<'de>,
    {
        fn decode(d: &mut Decoder<'de>) -> Result<Self, decode::Error> {
            decode_array(d)
        }
    }

    impl<'de, N, M, K, V> Decode<'de> for Within<N, M, HashMap<K, V, RandomState>>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        K: Eq + Hash + Decode<'de>,
        V: Decode<'de>,
    {
        fn decode(d: &mut Decoder<'de>) -> Result<Self, decode::Error> {
            decode_map(d)
        }
    }

    impl<'de, N, M> Decode<'de> for Within<N, M, String>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
    {
        fn decode(d: &mut Decoder<'de>) -> Result<Self, decode::Error> {
            decode_any(d)
        }
    }

    fn decode_array<'de, N, M, T>(d: &mut Decoder<'de>) -> Result<Within<N, M, T>, decode::Error>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Decode<'de> + Length,
    {
        use decode::Error::Message;

        match d.probe().array()? {
            Some(len) if len < N::U64 => Err(Message("min length not reached")),
            Some(len) if len > M::U64 => Err(Message("max length exceeded")),
            None | Some(_) => decode_any(d),
        }
    }

    fn decode_map<'de, N, M, T>(d: &mut Decoder<'de>) -> Result<Within<N, M, T>, decode::Error>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Decode<'de> + Length,
    {
        use decode::Error::Message;

        match d.probe().map()? {
            Some(len) if len < N::U64 => Err(Message("min length not reached")),
            Some(len) if len > M::U64 => Err(Message("max length exceeded")),
            None | Some(_) => decode_any(d),
        }
    }

    fn decode_any<'de, N, M, T>(d: &mut Decoder<'de>) -> Result<Within<N, M, T>, decode::Error>
    where
        N: Unsigned + IsLessOrEqual<M>,
        M: Unsigned,
        T: Decode<'de> + Length,
    {
        use decode::Error::Message;

        let inner = T::decode(d)?;
        if inner.length() < N::USIZE {
            Err(Message("min length not reached"))
        } else if inner.length() > M::USIZE {
            Err(Message("max length exceeded"))
        } else {
            Ok(Within {
                inner,
                _min: PhantomData,
                _max: PhantomData,
            })
        }
    }
}
