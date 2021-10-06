// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{btree_map, btree_set, BTreeMap, BTreeSet},
    convert::Infallible,
    iter::FromIterator,
};

use crate::{Canonical, Cstring};

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

impl<T: ToCjson> ToCjson for BTreeMap<Cstring, T> {
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
    T: Ord + ToCjson,
{
    Value::Array(it.map(ToCjson::into_cjson).collect())
}

fn into_object<I, T>(it: I) -> Value
where
    I: Iterator<Item = (Cstring, T)>,
    T: ToCjson,
{
    Value::Object(
        it.map(|(key, value)| (key, ToCjson::into_cjson(value)))
            .collect(),
    )
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

pub struct MapIter<'a> {
    iter: btree_map::Iter<'a, Cstring, Value>,
}

impl<'a> Iterator for MapIter<'a> {
    type Item = (&'a Cstring, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl Default for Map {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: ToCjson> FromIterator<(Cstring, A)> for Map {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (Cstring, A)>,
    {
        Self(iter.into_iter().map(|(k, v)| (k, v.into_cjson())).collect())
    }
}

impl ToCjson for Map {
    fn into_cjson(self) -> Value {
        Value::Object(self)
    }
}

// Array

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Array(BTreeSet<Value>);

impl Array {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    pub fn insert(&mut self, val: Value) -> bool {
        self.0.insert(val)
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

pub struct ArrayIter<'a> {
    iter: btree_set::Iter<'a, Value>,
}

impl<'a> Iterator for ArrayIter<'a> {
    type Item = &'a Value;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
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
