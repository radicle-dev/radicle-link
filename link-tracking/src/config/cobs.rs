// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{btree_map, BTreeMap, BTreeSet},
    iter::FromIterator,
};

/// Serialisation and deserialisation of [`Cobs`] et al.
pub mod cjson;

/// A set of filters of the form:
///
/// ```ignore
/// ("*" | <typename>): {
///   "policy": ("allow" | "deny")
///   "pattern": ("*" | [<object id>])
/// }
/// ```
///
/// The `<typename>` is the type identifier for the collaborative object, the
/// `<object id>` is the identifier for a particular object of the given type,
/// and `*` signifies a wildcard.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cobs<Type, ObjectId: Ord>(BTreeMap<TypeName<Type>, Filter<ObjectId>>);

impl<Ty: Ord, Id: Ord> Default for Cobs<Ty, Id> {
    fn default() -> Self {
        Self::allow_all()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeName<Type> {
    Wildcard,
    Type(Type),
}

/// The filtering policy for a set of collaborative objects.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ToCjson)]
pub struct Filter<ObjectId: Ord> {
    /// Allow or deny the [`Pattern`]s
    pub policy: Policy,
    pub pattern: Pattern<ObjectId>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pattern<ObjectId> {
    Wildcard,
    Objects(BTreeSet<ObjectId>),
}

impl<ObjectId: Ord> Pattern<ObjectId> {
    pub fn matches(&self, oid: &ObjectId) -> bool {
        match self {
            Self::Wildcard => true,
            Self::Objects(objs) => objs.contains(oid),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Policy {
    Allow,
    Deny,
}

impl Policy {
    pub fn inverse(&self) -> Self {
        match self {
            Self::Allow => Self::Deny,
            Self::Deny => Self::Allow,
        }
    }
}

impl<Ty: Ord, Id: Ord> Cobs<Ty, Id> {
    /// Creates the following cobs configuration:
    /// ```ignore
    /// { "*": { "policy": "allow", "pattern": "*" } }
    /// ```
    pub fn allow_all() -> Self {
        [(
            TypeName::Wildcard,
            Filter {
                policy: Policy::Allow,
                pattern: Pattern::Wildcard,
            },
        )]
        .into()
    }

    /// Creates the following cobs configuration:
    /// ```ignore
    /// { "*": { "policy": "deny", "pattern": "*" } }
    /// ```
    pub fn deny_all() -> Self {
        [(
            TypeName::Wildcard,
            Filter {
                policy: Policy::Deny,
                pattern: Pattern::Wildcard,
            },
        )]
        .into()
    }

    /// Create an empty `Cobs` filter.
    pub fn empty() -> Self {
        Self(BTreeMap::default())
    }

    /// Retrieve the [`Filter`] which applies to the given type name, or `None`
    /// if no filter is registered for this type name.
    pub fn get(&self, ty: Ty) -> Option<&Filter<Id>> {
        self.0.get(&TypeName::Type(ty))
    }

    /// Retrieve the [`Filter`] matching the default (wildcard) entry, or `None`
    /// if no wildcard entry is registered.
    pub fn wildcard(&self) -> Option<&Filter<Id>> {
        self.0.get(&TypeName::Wildcard)
    }

    /// Insert the given `typename` and `filter`. If the entry already existed,
    /// the old [`Filter`] is replaced and returned.
    pub fn insert(&mut self, typename: TypeName<Ty>, filter: Filter<Id>) -> Option<Filter<Id>> {
        self.0.insert(typename, filter)
    }

    /// Remove the given `typename` from the filters.
    pub fn remove(&mut self, typename: &TypeName<Ty>) {
        self.0.remove(typename);
    }

    /// Access the [`Entry`] for the given `typename`.
    pub fn entry(&mut self, typename: TypeName<Ty>) -> Entry<'_, Ty, Id> {
        Entry(self.0.entry(typename))
    }
}

impl<Ty: Ord, Id: Ord> FromIterator<(TypeName<Ty>, Filter<Id>)> for Cobs<Ty, Id> {
    fn from_iter<T: IntoIterator<Item = (TypeName<Ty>, Filter<Id>)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<Ty: Ord, Id: Ord, const N: usize> From<[(TypeName<Ty>, Filter<Id>); N]> for Cobs<Ty, Id> {
    fn from(kvs: [(TypeName<Ty>, Filter<Id>); N]) -> Self {
        Self(kvs.into())
    }
}

pub struct Entry<'a, Ty, Id: Ord>(btree_map::Entry<'a, TypeName<Ty>, Filter<Id>>);

impl<'a, Ty: Ord, Id: Ord> Entry<'a, Ty, Id> {
    /// Set the [`Policy`] for the given `Entry`.
    pub fn set_policy(self, policy: Policy) -> Self {
        self.and_modify(|filter| {
            filter.policy = policy;
        })
    }

    /// Set the [`Pattern`] for the given `Entry`.
    pub fn set_pattern(self, pattern: Pattern<Id>) -> Self {
        self.and_modify(|filter| {
            filter.pattern = pattern;
        })
    }

    /// Insert the given `Id`s for the given `Entry`. If the previous
    /// [`Pattern`] was a `Wildcard` then this operation is a no-op.
    pub fn insert_objects<I>(self, ids: I) -> Self
    where
        I: IntoIterator<Item = Id>,
    {
        self.and_modify(|filter| {
            match &mut filter.pattern {
                Pattern::Wildcard => { /* no-op */ },
                Pattern::Objects(objs) => objs.extend(ids),
            }
        })
    }

    /// Remove the given `Id`s for the given `Entry`. If the previous
    /// [`Pattern`] was a `Wildcard` then this operation is a no-op.
    pub fn remove_objects<I>(self, ids: I) -> Self
    where
        I: IntoIterator<Item = Id>,
    {
        self.and_modify(|filter| {
            match &mut filter.pattern {
                Pattern::Wildcard => { /* no-op */ },
                Pattern::Objects(objs) => {
                    for id in ids {
                        objs.remove(&id);
                    }
                },
            }
        })
    }

    pub fn and_modify<F>(self, f: F) -> Self
    where
        F: FnOnce(&mut Filter<Id>),
    {
        Self(self.0.and_modify(f))
    }

    pub fn or_insert(self, default: Filter<Id>) -> &'a mut Filter<Id> {
        self.0.or_insert(default)
    }

    pub fn or_insert_with<F>(self, default: F) -> &'a mut Filter<Id>
    where
        F: FnOnce() -> Filter<Id>,
    {
        self.0.or_insert_with(default)
    }

    pub fn or_insert_with_key<F>(self, default: F) -> &'a mut Filter<Id>
    where
        F: FnOnce(&TypeName<Ty>) -> Filter<Id>,
    {
        self.0.or_insert_with_key(default)
    }
}
