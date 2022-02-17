// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use git_ref_format::{self as refs, Qualified, RefStr, RefString};
use link_canonical::{
    json::{ToCjson, Value},
    Canonical,
    Cstring,
};

use crate::config::{
    self,
    cobs::{self, Policy},
};

pub const DATA_REFS: [refs::Component; 3] = [
    refs::component::HEADS,
    refs::component::TAGS,
    refs::component::NOTES,
];

pub type Config = config::Config<TypeName, ObjectId>;

impl Config {
    /// Evaluate a [`Qualified`] refname against this [`Config`], determining
    /// the [`Policy`] applicable to it.
    ///
    /// The evaluation rules are described in [RFC0699].
    ///
    /// [RFC0699]: https://github.com/radicle-dev/radicle-link/blob/ca4f1856b29fa5d2b469c4f7db33ac81fdec2458/docs/rfc/0699-tracking-storage.adoc
    pub fn policy_for(&self, refname: &Qualified) -> Policy {
        match refname.non_empty_components() {
            (_refs, cobs, ty, mut tail) if refs::name::COBS == cobs.as_ref() => {
                let ty = TypeName::try_from(&ty).ok();
                let id = tail.next().and_then(|id| ObjectId::try_from(&id).ok());
                if tail.next().is_some() {
                    return Policy::Deny;
                }

                match (ty, id) {
                    (Some(ty), Some(id)) => match self.cobs.get(ty) {
                        None => match self.cobs.wildcard() {
                            // Default is allow
                            None => Policy::Allow,
                            // Ignore pattern, as that is rather confusing
                            Some(cobs::Filter { policy, .. }) => *policy,
                        },
                        Some(cobs::Filter { policy, pattern }) => {
                            if pattern.matches(&id) {
                                *policy
                            } else {
                                policy.inverse()
                            }
                        },
                    },
                    _ => Policy::Deny,
                }
            },

            (_refs, cat, _, _) => {
                let cat: &RefStr = cat.as_ref();
                if self.data && DATA_REFS.iter().any(|allowed| allowed.as_ref() == cat) {
                    Policy::Allow
                } else {
                    Policy::Deny
                }
            },
        }
    }
}

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
        ty.as_str().parse().map(Self)
    }
}

impl TryFrom<&refs::Component<'_>> for TypeName {
    type Error = cob::error::TypeNameParse;

    fn try_from(ty: &refs::Component) -> Result<Self, Self::Error> {
        ty.as_str().parse().map(Self)
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

impl From<&TypeName> for refs::Component<'_> {
    fn from(ty: &TypeName) -> Self {
        refs::Component::from_refstring(
            RefString::try_from(ty.0.to_string())
                .expect("`cobs::TypeName` should be a valid ref string"),
        )
        .expect("`cobs::TypeName` should be a valid ref component")
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

impl From<&ObjectId> for refs::Component<'_> {
    fn from(id: &ObjectId) -> Self {
        refs::Component::from_refstring(
            RefString::try_from(id.0.to_string())
                .expect("`cobs::ObjectId` should be a valid ref string"),
        )
        .expect("`cobs::ObjectId` should be a valid ref component")
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
        Ok(id.as_str().parse().map(Self)?)
    }
}

impl TryFrom<&refs::Component<'_>> for ObjectId {
    type Error = error::Object;

    fn try_from(c: &refs::Component) -> Result<Self, Self::Error> {
        Ok(c.as_str().parse().map(Self)?)
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
