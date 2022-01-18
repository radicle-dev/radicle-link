// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use link_canonical::{
    json::{ToCjson, Value},
    Canonical,
    Cstring,
};

use crate::config;

pub type Config = config::Config<TypeName, ObjectId>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeName(pub cob::TypeName);

impl From<&TypeName> for Cstring {
    fn from(ty: &TypeName) -> Self {
        Self::from(ty.0.to_string())
    }
}

impl TryFrom<Cstring> for TypeName {
    type Error = cob::error::TypeNameParse;

    fn try_from(ty: Cstring) -> Result<Self, Self::Error> {
        ty.as_str().parse().map(TypeName)
    }
}

impl From<TypeName> for Cstring {
    fn from(ty: TypeName) -> Self {
        Self::from(ty.0.to_string())
    }
}

impl From<&TypeName> for Value {
    fn from(ty: &TypeName) -> Self {
        Value::String(Cstring::from(ty))
    }
}

impl From<TypeName> for Value {
    fn from(ty: TypeName) -> Self {
        Value::String(Cstring::from(ty))
    }
}

impl Canonical for TypeName {
    type Error = <Value as Canonical>::Error;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error> {
        Value::from(self).canonical_form()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub cob::ObjectId);

impl From<&ObjectId> for Cstring {
    fn from(ty: &ObjectId) -> Self {
        Self::from(ty.0.to_string())
    }
}

impl From<ObjectId> for Cstring {
    fn from(ty: ObjectId) -> Self {
        Self::from(ty.0.to_string())
    }
}

impl From<&ObjectId> for Value {
    fn from(ty: &ObjectId) -> Self {
        Value::String(Cstring::from(ty))
    }
}

impl From<ObjectId> for Value {
    fn from(ty: ObjectId) -> Self {
        Value::String(Cstring::from(ty))
    }
}

impl TryFrom<Value> for ObjectId {
    type Error = error::Object;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(id) => Self::try_from(id),
            val => Err(error::Object::MismatchedTy {
                expected: "string representing object identifier".into(),
                found: val.ty_name().into(),
            }),
        }
    }
}

impl TryFrom<Cstring> for ObjectId {
    type Error = error::Object;

    fn try_from(id: Cstring) -> Result<Self, Self::Error> {
        Ok(id.as_str().parse().map(ObjectId)?)
    }
}

impl Canonical for ObjectId {
    type Error = <Value as Canonical>::Error;

    fn canonical_form(&self) -> Result<Vec<u8>, Self::Error> {
        Value::from(self).canonical_form()
    }
}

impl ToCjson for ObjectId {
    fn into_cjson(self) -> Value {
        Value::from(self)
    }
}

mod error {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Object {
        #[error("expected type {expected}, but found {found}")]
        MismatchedTy { expected: String, found: String },
        #[error(transparent)]
        Parse(#[from] cob::error::ParseObjectId),
    }
}
