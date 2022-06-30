// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::{EntryContents, History, ObjectId, TypeName};

use link_identities::git::Urn;

use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

/// A CBOR encoding of the change graph which was loaded when the heads of the
/// change graph were `refs`. The `history` contains the bytes of each change
/// and the URN of the identity which made the change.
#[derive(PartialEq, Clone, Debug, minicbor::Encode, minicbor::Decode)]
pub struct CachedChangeGraph {
    #[n(0)]
    pub history: History,
    #[n(1)]
    #[cbor(with = "encoding::oids")]
    pub refs: BTreeSet<git2::Oid>,
    #[n(4)]
    #[cbor(with = "encoding::typename")]
    pub typename: TypeName,
    #[n(5)]
    #[cbor(with = "encoding::objectid")]
    pub object_id: ObjectId,
    #[n(6)]
    pub authorizing_identity_urn: Urn,
}

impl CachedChangeGraph {
    pub fn new(
        tips: impl IntoIterator<Item = git2::Oid>,
        history: History,
        typename: TypeName,
        object_id: ObjectId,
        authorizing_identity_urn: Urn,
    ) -> Rc<RefCell<CachedChangeGraph>> {
        let g = CachedChangeGraph {
            history,
            refs: tips.into_iter().collect(),
            typename,
            object_id,
            authorizing_identity_urn,
        };
        Rc::new(RefCell::new(g))
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn tips(&self) -> BTreeSet<git2::Oid> {
        self.refs.clone()
    }

    pub(crate) fn update_ref(
        &mut self,
        previous: Option<git2::Oid>,
        new: git2::Oid,
        author: Urn,
        changes: EntryContents,
    ) {
        if let Some(previous) = previous {
            self.refs.remove(&previous);
        }
        self.refs.insert(new);
        self.history.extend(new, author, changes);
    }

    pub fn refs(&self) -> &BTreeSet<git2::Oid> {
        &self.refs
    }

    pub fn typename(&self) -> &TypeName {
        &self.typename
    }

    pub fn authorizing_identity_urn(&self) -> &Urn {
        &self.authorizing_identity_urn
    }

    pub fn object_id(&self) -> ObjectId {
        self.object_id
    }
}

mod encoding {

    struct Json(serde_json::Value);

    impl<'b> minicbor::Decode<'b> for Json {
        fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
            let bytes: minicbor::bytes::ByteVec = minicbor::bytes::decode(d)?;
            let value: serde_json::Value = serde_json::from_slice(&bytes[..])
                .map_err(|_| minicbor::decode::Error::Message("invalid json"))?;
            Ok(Json(value))
        }
    }

    impl minicbor::Encode for Json {
        fn encode<W: minicbor::encode::Write>(
            &self,
            e: &mut minicbor::Encoder<W>,
        ) -> Result<(), minicbor::encode::Error<W::Error>> {
            let bvec: minicbor::bytes::ByteVec = serde_json::to_vec(&self.0).unwrap().into();
            e.encode(bvec)?;
            Ok(())
        }
    }

    pub(super) mod oids {
        use minicbor::{
            decode::{Decode, Decoder, Error as DecodeError},
            encode::{Encode, Encoder, Error as EncodeError, Write},
        };
        use radicle_git_ext::Oid;
        use std::collections::BTreeSet;

        pub fn encode<W: Write>(
            v: &BTreeSet<git2::Oid>,
            e: &mut Encoder<W>,
        ) -> Result<(), EncodeError<W::Error>> {
            v.iter()
                .map(|oid| Oid::from(*oid))
                .collect::<BTreeSet<_>>()
                .encode(e)
        }

        pub fn decode(d: &mut Decoder<'_>) -> Result<BTreeSet<git2::Oid>, DecodeError> {
            let exts = BTreeSet::<Oid>::decode(d)?;
            Ok(exts.into_iter().map(|o| o.into()).collect())
        }
    }

    pub(super) mod objectid {
        use std::str::FromStr;

        use crate::ObjectId;
        use minicbor::{
            decode::{Decoder, Error as DecodeError},
            encode::{Encoder, Error as EncodeError, Write},
        };

        pub fn encode<W: Write>(
            v: &ObjectId,
            e: &mut Encoder<W>,
        ) -> Result<(), EncodeError<W::Error>> {
            e.str(v.to_string().as_str())?;
            Ok(())
        }

        pub fn decode(d: &mut Decoder<'_>) -> Result<ObjectId, DecodeError> {
            let s = d.str()?;
            ObjectId::from_str(s).map_err(|_| DecodeError::Message("invalid object ID"))
        }
    }

    pub(super) mod typename {
        use std::str::FromStr;

        use crate::TypeName;
        use minicbor::{
            decode::{Decoder, Error as DecodeError},
            encode::{Encoder, Error as EncodeError, Write},
        };

        pub fn encode<W: Write>(
            v: &TypeName,
            e: &mut Encoder<W>,
        ) -> Result<(), EncodeError<W::Error>> {
            e.str(v.to_string().as_str())?;
            Ok(())
        }

        pub fn decode(d: &mut Decoder<'_>) -> Result<TypeName, DecodeError> {
            let s = d.str()?;
            TypeName::from_str(s).map_err(|_| DecodeError::Message("invalid typename"))
        }
    }
}
