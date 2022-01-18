// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::{TryFrom, TryInto};

use link_canonical::{
    json::{ToCjson, Value},
    Cstring,
};

use super::{Cobs, Filter, Pattern, Policy, TypeName};

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
    pub enum Pattern {
        #[error("expected wildcard `*`")]
        ExpectedWildcard,
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
        Pattern(#[from] Pattern),
        #[error(transparent)]
        Policy(#[from] Policy),
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

impl<Ty: Into<Cstring>> ToCjson for TypeName<Ty> {
    fn into_cjson(self) -> Value {
        Cstring::from(self).into_cjson()
    }
}

impl<Ty: Into<Cstring>> From<TypeName<Ty>> for Cstring {
    fn from(typename: TypeName<Ty>) -> Self {
        match typename {
            TypeName::Wildcard => "*".into(),
            TypeName::Type(ty) => ty.into(),
        }
    }
}

impl<Id: Ord + ToCjson> ToCjson for Pattern<Id> {
    fn into_cjson(self) -> Value {
        match self {
            Self::Wildcard => "*".into_cjson(),
            Self::Objects(objs) => objs.into_cjson(),
        }
    }
}

impl<Ty: Into<Cstring> + Ord, ObjectId: ToCjson + Ord> ToCjson for Cobs<Ty, ObjectId> {
    fn into_cjson(self) -> Value {
        self.0.into_cjson()
    }
}

impl<Id> TryFrom<Value> for Filter<Id>
where
    Id: Ord,
    Cstring: TryInto<Id>,
    <Cstring as TryInto<Id>>::Error: std::error::Error + Send + Sync + 'static,
{
    type Error = error::Filter;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Object(mut map) => {
                let policy = map
                    .remove(&POLICY.into())
                    .ok_or(error::Filter::Missing(POLICY))?;
                let pattern = map
                    .remove(&PATTERN.into())
                    .ok_or(error::Filter::Missing(PATTERN))?;

                Ok(Self {
                    policy: Policy::try_from(policy)?,
                    pattern: Pattern::try_from(pattern)?,
                })
            },
            val => Err(error::Filter::MismatchedTy {
                expected: "expected string 'allow' or 'deny'".to_string(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}

impl TryFrom<Value> for Policy {
    type Error = error::Policy;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(policy) => match policy.as_str() {
                "allow" => Ok(Self::Allow),
                "deny" => Ok(Self::Deny),
                _ => Err(error::Policy::Unexpected(policy)),
            },
            val => Err(error::Policy::MismatchedTy {
                expected: "expected string 'allow' or 'deny'".to_string(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}

impl<Id> TryFrom<Value> for Pattern<Id>
where
    Id: Ord,
    Cstring: TryInto<Id>,
    <Cstring as TryInto<Id>>::Error: std::error::Error + Send + Sync + 'static,
{
    type Error = error::Pattern;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Array(objs) => objs
                .into_iter()
                .map(|val| match val {
                    Value::String(s) => s
                        .try_into()
                        .map_err(|err| error::Pattern::Identifier(err.into())),
                    val => Err(error::Pattern::MismatchedTy {
                        expected: "<object id>".into(),
                        found: val.ty_name().to_string(),
                    }),
                })
                .collect::<Result<_, _>>()
                .map(Self::Objects),
            Value::String(s) => match s.as_str() {
                "*" => Ok(Self::Wildcard),
                _ => Err(error::Pattern::ExpectedWildcard),
            },
            val => Err(error::Pattern::MismatchedTy {
                expected: "string of '*' or '[<object id> ..]'".into(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}

impl<Ty, Id> TryFrom<Value> for Cobs<Ty, Id>
where
    Ty: Ord,
    Id: Ord,
    Cstring: TryInto<Ty> + TryInto<Id>,
    <Cstring as TryInto<Ty>>::Error: std::error::Error + Send + Sync + 'static,
    <Cstring as TryInto<Id>>::Error: std::error::Error + Send + Sync + 'static,
{
    type Error = error::Cobs;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let try_typename = |typename: Cstring| match typename.as_str() {
            "*" => Ok(TypeName::Wildcard),
            _ => typename
                .try_into()
                .map(TypeName::Type)
                .map_err(|err: <Cstring as TryInto<Ty>>::Error| error::Cobs::TypeName(err.into())),
        };
        match value {
            Value::Object(cobs) => cobs
                .into_iter()
                .map(|(typename, filter)| {
                    let typename = try_typename(typename);
                    let filter = Filter::try_from(filter).map_err(error::Cobs::from);
                    typename.and_then(|ty| filter.map(|f| (ty, f)))
                })
                .collect::<Result<Cobs<Ty, Id>, _>>(),
            val => Err(error::Cobs::MismatchedTy {
                expected: r#"{"<typename>": [<object id>...]}"#.to_string(),
                found: val.ty_name().to_string(),
            }),
        }
    }
}
