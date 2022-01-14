// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{btree_map, BTreeMap, BTreeSet},
    convert::{Infallible, TryFrom},
    iter::FromIterator,
    slice,
    str::{self, FromStr},
};

use crate::{Canonical, Cstring};

mod parser;
mod ser;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Object(Map),
    Array(Array),
    String(Cstring),
    Number(Number),
    Bool(bool),
    Null,
}

impl Value {
    pub fn ty_name(&self) -> &'static str {
        match self {
            Value::Object(_) => "object",
            Value::Array(_) => "array",
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Bool(_) => "bool",
            Value::Null => "null",
        }
    }
}

impl<K: Into<Cstring>, A: ToCjson> FromIterator<(K, A)> for Value {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (K, A)>,
    {
        Self::Object(Map::from_iter(iter))
    }
}

impl FromStr for Value {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use nom::{
            error::{convert_error, VerboseError},
            Err::{Error, Failure, Incomplete},
        };

        match parser::json::<VerboseError<&str>>(s) {
            Ok((rem, value)) => {
                if rem.trim().is_empty() {
                    Ok(value)
                } else {
                    Err(format!("expected EOF, found: {}", rem))
                }
            },
            Err(Error(e)) | Err(Failure(e)) => Err(convert_error(s, e)),
            Err(Incomplete(_)) => Err("unexpected end of input".to_string()),
        }
    }
}

impl TryFrom<&[u8]> for Value {
    type Error = String;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        // XXX: could make the `parser` generic over input
        str::from_utf8(bytes)
            .map_err(|err| err.to_string())
            .and_then(|s| s.parse())
    }
}

impl Canonical for Value {
    type Error = Infallible;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error> {
        Ok(self.to_bytes())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Number {
    U64(u64),
    I64(i64),
}

impl Canonical for Number {
    type Error = Infallible;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error> {
        Ok(self.to_bytes())
    }
}

pub trait ToCjson {
    fn into_cjson(self) -> Value;
}

// Identity

impl ToCjson for Value {
    fn into_cjson(self) -> Value {
        self
    }
}

// Object

impl<K: Into<Cstring>, T: ToCjson> ToCjson for BTreeMap<K, T> {
    fn into_cjson(self) -> Value {
        into_object(self.into_iter())
    }
}

// Array

impl<T: ToCjson + Ord> ToCjson for BTreeSet<T> {
    fn into_cjson(self) -> Value {
        into_array(self.into_iter())
    }
}

impl<T: ToCjson> ToCjson for Vec<T> {
    fn into_cjson(self) -> Value {
        into_array(self.into_iter())
    }
}

// Option

impl<T: ToCjson> ToCjson for Option<T> {
    fn into_cjson(self) -> Value {
        match self {
            None => Value::Null,
            Some(t) => t.into_cjson(),
        }
    }
}

// Strings

impl ToCjson for Cstring {
    fn into_cjson(self) -> Value {
        Value::String(self)
    }
}

impl ToCjson for String {
    fn into_cjson(self) -> Value {
        Cstring::from(self).into_cjson()
    }
}

impl ToCjson for &str {
    fn into_cjson(self) -> Value {
        Cstring::from(self).into_cjson()
    }
}

// Numbers

impl ToCjson for u64 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::U64(self))
    }
}

impl ToCjson for u32 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::U64(self as u64))
    }
}

impl ToCjson for u16 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::U64(self as u64))
    }
}

impl ToCjson for u8 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::U64(self as u64))
    }
}

impl ToCjson for i64 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::I64(self))
    }
}

impl ToCjson for i32 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::I64(self as i64))
    }
}

impl ToCjson for i16 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::I64(self as i64))
    }
}

impl ToCjson for i8 {
    fn into_cjson(self) -> Value {
        Value::Number(Number::I64(self as i64))
    }
}

// Bool

impl ToCjson for bool {
    fn into_cjson(self) -> Value {
        Value::Bool(self)
    }
}

// Iterator helpers

fn into_array<I, T>(it: I) -> Value
where
    I: Iterator<Item = T>,
    T: ToCjson,
{
    Value::Array(it.map(ToCjson::into_cjson).collect())
}

fn into_object<I, K, T>(it: I) -> Value
where
    I: Iterator<Item = (K, T)>,
    K: Into<Cstring>,
    T: ToCjson,
{
    Value::Object(it.collect())
}

// Map

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Map(BTreeMap<Cstring, Value>);

impl Map {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn insert(&mut self, key: Cstring, val: Value) -> Option<Value> {
        self.0.insert(key, val)
    }

    pub fn get(&self, key: &Cstring) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn remove(&mut self, key: &Cstring) -> Option<Value> {
        self.0.remove(key)
    }

    pub fn entry(&mut self, key: Cstring) -> Entry<'_> {
        Entry(self.0.entry(key))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> MapIter<'_> {
        MapIter {
            iter: self.0.iter(),
        }
    }
}

pub struct Entry<'a>(btree_map::Entry<'a, Cstring, Value>);

impl<'a> Entry<'a> {
    pub fn and_modify<F>(self, f: F) -> Self
    where
        F: FnOnce(&mut Value),
    {
        Self(self.0.and_modify(f))
    }

    pub fn or_insert(self, default: Value) -> &'a mut Value {
        self.0.or_insert(default)
    }

    pub fn or_insert_with<F>(self, default: F) -> &'a mut Value
    where
        F: FnOnce() -> Value,
    {
        self.0.or_insert_with(default)
    }

    pub fn or_insert_with_key<F>(self, default: F) -> &'a mut Value
    where
        F: FnOnce(&Cstring) -> Value,
    {
        self.0.or_insert_with_key(default)
    }
}

pub struct MapIter<'a> {
    iter: btree_map::Iter<'a, Cstring, Value>,
}

impl<'a> Iterator for MapIter<'a> {
    type Item = (&'a Cstring, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl<'a> ExactSizeIterator for MapIter<'a> {
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl Default for Map {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Into<Cstring>, A: ToCjson> FromIterator<(K, A)> for Map {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (K, A)>,
    {
        Self(
            iter.into_iter()
                .map(|(k, v)| (k.into(), v.into_cjson()))
                .collect(),
        )
    }
}

impl ToCjson for Map {
    fn into_cjson(self) -> Value {
        Value::Object(self)
    }
}

// Array

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Array(Vec<Value>);

impl Array {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn insert(&mut self, val: Value) {
        self.0.push(val)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> ArrayIter<'_> {
        ArrayIter {
            iter: self.0.iter(),
        }
    }
}

impl IntoIterator for Array {
    type Item = Value;

    type IntoIter = <Vec<Value> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

pub struct ArrayIter<'a> {
    iter: slice::Iter<'a, Value>,
}

impl<'a> Iterator for ArrayIter<'a> {
    type Item = &'a Value;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl<'a> ExactSizeIterator for ArrayIter<'a> {
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl Default for Array {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: ToCjson> FromIterator<A> for Array {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = A>,
    {
        Self(iter.into_iter().map(|val| val.into_cjson()).collect())
    }
}

impl ToCjson for Array {
    fn into_cjson(self) -> Value {
        Value::Array(self)
    }
}
