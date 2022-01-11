// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    iter::FromIterator,
};

/// Serialisation and deserialisation of [`Cobs`] et al.
pub mod cjson;

/// Either a wildcard `*` or a set of filters.
///
/// The filters are keyed by the Collaborative Object typename where the value
/// is a set of Collaborative Object Identifiers or a wildcard `*`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cobs<Type, ObjectId> {
    Wildcard,
    Filters(BTreeMap<Type, BTreeSet<Filter<ObjectId>>>),
}

/// The filtering policy of a Collaborative Object.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ToCjson)]
pub struct Filter<ObjectId> {
    /// Allow or deny the specified [`Object`]
    pub policy: Policy,
    pub pattern: Object<ObjectId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Policy {
    Allow,
    Deny,
}

impl<Ty: Ord, Id: Ord> Cobs<Ty, Id> {
    /// Insert a [`Filter`] for the given `typename`.
    ///
    /// # Note
    ///
    /// If `self` is [`Cobs::Wildcard`], it will be turned into a
    /// [`Cobs::Filters`] using the the `typename` and `Object` as the
    /// initial entries.
    pub fn insert(&mut self, typename: Ty, filter: Filter<Id>)
    where
        Id: Clone,
    {
        match self {
            Self::Wildcard => {
                let mut filters = BTreeMap::new();
                filters.insert(typename, vec![filter].into_iter().collect());
                *self = Self::Filters(filters);
            },
            Self::Filters(filters) => {
                filters
                    .entry(typename)
                    .and_modify(|objs| {
                        objs.insert(filter.clone());
                    })
                    .or_insert_with(|| vec![filter].into_iter().collect());
            },
        }
    }

    /// Remove the [`Filter`] for the given `typename`.
    ///
    /// # Note
    ///
    /// If `self` is [`Cobs::Wildcard`] then this is a no-op.
    /// If the resulting set of objects is empty we remove the `typename` from
    /// the filters.
    pub fn remove(&mut self, typename: &Ty, filter: &Filter<Id>) {
        match self {
            Self::Wildcard => { /* no-op */ },
            Self::Filters(filters) => {
                if let Some(objs) = filters.get_mut(typename) {
                    objs.remove(filter);
                    if objs.is_empty() {
                        filters.remove(typename);
                    }
                }
            },
        }
    }

    /// Remove the given `typename` from the filters.
    pub fn remove_type(&mut self, typename: &Ty) {
        match self {
            Self::Wildcard => { /* no-op */ },
            Self::Filters(filters) => {
                filters.remove(typename);
            },
        }
    }
}

impl<Ty: Ord, Id: Ord> FromIterator<(Ty, BTreeSet<Filter<Id>>)> for Cobs<Ty, Id> {
    fn from_iter<T: IntoIterator<Item = (Ty, BTreeSet<Filter<Id>>)>>(iter: T) -> Self {
        Self::Filters(iter.into_iter().collect())
    }
}

/// Either a wildcard `*` or a Collaborative Object Identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Object<Id> {
    Wildcard,
    Identifier(Id),
}

impl<Id> Object<Id> {
    pub fn map<O>(self, f: impl FnOnce(Id) -> O) -> Object<O> {
        match self {
            Self::Wildcard => Object::Wildcard,
            Self::Identifier(id) => Object::Identifier(f(id)),
        }
    }
}
