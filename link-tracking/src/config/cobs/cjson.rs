// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
};

use link_canonical::{
    json::{ToCjson, Value},
    Cstring,
};

use super::{Cobs, Filter, Object, Policy};

const POLICY: &str = "policy";
const PATTERN: &str = "pattern";

pub mod error {
    use thiserror::Error;

    use link_canonical::Cstring;

    #[derive(Debug, Error)]
    pub enum Policy {
        #[error("expected 'allow' or 'deny', but found {0}")]
        Unexpected(Cstring),
        #[error("expected type {expected}, but found {found}")]
        MismatchedTy { expected: String, found: String },
    }

    #[derive(Debug, Error)]
    pub enum Object {
        #[error("expected type {expected}, but found {found}")]
        MismatchedTy { expected: String, found: String },
        #[error("failed to parse the object identifier")]
        Identifier(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    }

    #[derive(Debug, Error)]
    pub enum Filter {
        #[error("missing key '{0}'")]
        Missing(&'static str),
        #[error("expected type {expected}, but found {found}")]
        MismatchedTy { expected: String, found: String },
        #[error(transparent)]
        Policy(#[from] Policy),
        #[error(transparent)]
        Object(#[from] Object),
    }

    #[derive(Debug, Error)]
    pub enum Cobs {
        #[error("expected type {expected}, but found {found}")]
        MismatchedTy { expected: String, found: String },
        #[error("expected '*', but found {0}")]
        MismatchedStr(String),
        #[error(transparent)]
        Filter(#[from] Filter),
        #[error("failed to parse the object's type name")]
        TypeName(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    }
}

impl ToCjson for Policy {
    fn into_cjson(self) -> Value {
        match self {
            Self::Allow => "allow".into_cjson(),
            Self::Deny => "deny".into_cjson(),
        }
    }
}

impl<Id: ToCjson> ToCjson for Object<Id> {
    fn into_cjson(self) -> Value {
        match self {
            Self::Wildcard => "*".into_cjson(),
            Self::Identifier(id) => id.into_cjson(),
        }
    }
}

impl<Ty: Into<Cstring> + Ord, ObjectId: ToCjson + Ord> ToCjson for Cobs<Ty, ObjectId> {
    fn into_cjson(self) -> Value {
        match self {
            Self::Wildcard => Value::String("*".into()),
            Self::Filters(filters) => filters.into_cjson(),
        }
    }
}

impl<Id> TryFrom<Value> for Filter<Id>
where
    Value: TryInto<Id>,
    <Value as TryInto<Id>>::Error: std::error::Error + Send + Sync + 'static,
{
    type Error = error::Filter;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Object(map) => {
                let policy = map
                    .get(&POLICY.into())
                    .ok_or(error::Filter::Missing(POLICY))?;
                let pattern = map
                    .get(&PATTERN.into())
                    .ok_or(error::Filter::Missing(PATTERN))?;

                Ok(Self {
                    policy: Policy::try_from(policy)?,
                    pattern: Object::try_from(pattern.clone())?,
                })
            },
            val => Err(error::Filter::MismatchedTy {
                expected: "expected string 'allow' or 'deny'".to_string(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}

impl TryFrom<&Value> for Policy {
    type Error = error::Policy;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(policy) => match policy.as_str() {
                "allow" => Ok(Self::Allow),
                "deny" => Ok(Self::Deny),
                _ => Err(error::Policy::Unexpected(policy.clone())),
            },
            val => Err(error::Policy::MismatchedTy {
                expected: "expected string 'allow' or 'deny'".to_string(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}

impl<Id> TryFrom<Value> for Object<Id>
where
    Value: TryInto<Id>,
    <Value as TryInto<Id>>::Error: std::error::Error + Send + Sync + 'static,
{
    type Error = error::Object;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match &value {
            Value::String(s) => match s.as_str() {
                "*" => Ok(Self::Wildcard),
                _ => value
                    .try_into()
                    .map(Self::Identifier)
                    .map_err(|err| error::Object::Identifier(err.into())),
            },
            val => Err(error::Object::MismatchedTy {
                expected: "string of '*' or '<object id>'".into(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}

impl<Ty, Id> TryFrom<&Value> for Cobs<Ty, Id>
where
    Ty: Ord,
    Id: Ord,
    Value: TryInto<Id>,
    Cstring: TryInto<Ty>,
    <Cstring as TryInto<Ty>>::Error: std::error::Error + Send + Sync + 'static,
    <Value as TryInto<Id>>::Error: std::error::Error + Send + Sync + 'static,
{
    type Error = error::Cobs;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Object(cobs) => cobs
                .iter()
                .map(|(typename, objects)| match objects {
                    Value::Array(objs) => {
                        let typename = typename
                            .clone()
                            .try_into()
                            .map_err(|err| error::Cobs::TypeName(err.into()));
                        typename.and_then(|ty| {
                            objs.iter()
                                .cloned()
                                .map(Filter::try_from)
                                .collect::<Result<BTreeSet<_>, _>>()
                                .map(|objs| (ty, objs))
                                .map_err(error::Cobs::from)
                        })
                    },
                    val => Err(error::Cobs::MismatchedTy {
                        expected: "[<object id>...]".to_string(),
                        found: val.ty_name().to_string(),
                    }),
                })
                .collect::<Result<Cobs<Ty, Id>, _>>(),
            Value::String(s) => match s.as_str() {
                "*" => Ok(Self::Wildcard),
                _ => Err(error::Cobs::MismatchedStr(s.to_string())),
            },
            val => Err(error::Cobs::MismatchedTy {
                expected: r#"{"<typename>": [<object id>...]}"#.to_string(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}
