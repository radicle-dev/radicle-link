// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::{
    validated_automerge::error::ProposalError,
    History,
    ObjectId,
    Schema,
    TypeName,
    ValidatedAutomerge,
};

use link_identities::git::Urn;

pub use minicbor_impls::forward_compatible_decode;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

/// A representation of a change graph which contains only the history generated
/// by fully evaluating the change graph and the OIDs that were pointed at by
/// known references to the object that were used to load the change graph.
#[derive(Debug)]
pub struct ThinChangeGraph {
    // This is an `Option` because often we never actually need to evaluate the automerge document
    // at all. If we are loading objects from the cache to return in response to a read request
    // then we already know that the document is valid (otherwise it would never be in the
    // cache in the first place) and we can just return the raw history we read from the cache.
    // When we do need the full history (e.g when we need to make an update and therefore need
    // to validate the update with respect to the schema) then we generate the
    // `ValidatedAutomerge` from the `raw_history`.
    pub validated_history: Option<ValidatedAutomerge>,
    pub history: History,
    pub refs: BTreeSet<git2::Oid>,
    pub schema_commit: git2::Oid,
    pub schema: Schema,
    pub state: serde_json::Value,
    pub typename: TypeName,
    pub object_id: ObjectId,
    pub authorizing_identity_urn: Urn,
}

impl ThinChangeGraph {
    pub fn new(
        tips: impl IntoIterator<Item = git2::Oid>,
        schema: Schema,
        schema_commit: git2::Oid,
        history: ValidatedAutomerge,
        typename: TypeName,
        object_id: ObjectId,
        authorizing_identity_urn: Urn,
    ) -> Rc<RefCell<ThinChangeGraph>> {
        let state = history.state();
        let compressed_history = history.compressed_valid_history();
        let g = ThinChangeGraph {
            validated_history: Some(history),
            history: compressed_history,
            schema,
            refs: tips.into_iter().collect(),
            schema_commit,
            state,
            typename,
            object_id,
            authorizing_identity_urn,
        };
        Rc::new(RefCell::new(g))
    }

    pub(crate) fn new_from_single_change(
        change: git2::Oid,
        schema: Schema,
        schema_commit: git2::Oid,
        history: ValidatedAutomerge,
        typename: TypeName,
        object_id: ObjectId,
        authorizing_identity_urn: Urn,
    ) -> Rc<RefCell<ThinChangeGraph>> {
        let mut tips = BTreeSet::new();
        let state = history.state();
        tips.insert(change);
        let compressed_history = history.compressed_valid_history();
        Rc::new(RefCell::new(ThinChangeGraph {
            validated_history: Some(history),
            history: compressed_history,
            schema,
            refs: tips,
            schema_commit,
            state,
            typename,
            object_id,
            authorizing_identity_urn,
        }))
    }

    pub fn history(&self) -> History {
        if let Some(history) = &self.validated_history {
            history.valid_history()
        } else {
            self.history.clone()
        }
    }

    pub(crate) fn propose_change(&mut self, change_bytes: &[u8]) -> Result<(), ProposalError> {
        if let Some(history) = &mut self.validated_history {
            history.propose_change(change_bytes)?;
        } else {
            // This unwrap should be safe as we only save things in the cache when we've
            // validated them
            let mut history =
                ValidatedAutomerge::new_with_history(self.schema.clone(), self.history.clone())
                    .unwrap();
            history.propose_change(change_bytes)?;
            self.validated_history = Some(history);
        }
        Ok(())
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn schema_commit(&self) -> git2::Oid {
        self.schema_commit
    }

    pub fn tips(&self) -> BTreeSet<git2::Oid> {
        self.refs.clone()
    }

    pub(crate) fn update_ref(&mut self, previous: Option<git2::Oid>, new: git2::Oid) {
        if let Some(previous) = previous {
            self.refs.remove(&previous);
        }
        self.refs.insert(new);
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

    pub fn state(&self) -> &serde_json::Value {
        &self.state
    }
}

mod minicbor_impls {
    use super::ThinChangeGraph;
    use crate::{History, HistoryType, ObjectId, Schema, TypeName};
    use link_identities::git::Urn;
    use radicle_git_ext as ext;
    use std::{
        collections::BTreeSet,
        convert::{TryFrom, TryInto},
    };

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

    impl minicbor::Encode for Schema {
        fn encode<W: minicbor::encode::Write>(
            &self,
            e: &mut minicbor::Encoder<W>,
        ) -> Result<(), minicbor::encode::Error<W::Error>> {
            e.encode(self.json_bytes())?;
            Ok(())
        }
    }

    impl<'b> minicbor::Decode<'b> for Schema {
        fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
            let bytes: Vec<u8> = d.decode()?;
            Schema::try_from(&bytes[..])
                .map_err(|_| minicbor::decode::Error::Message("invalid schema JSON"))
        }
    }

    mod objectid {
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

    mod typename {
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

    #[derive(minicbor::Encode, minicbor::Decode)]
    #[cbor(map)]
    pub struct Encoding {
        #[n(0)]
        #[cbor(encode_with = "minicbor::bytes::encode")]
        #[cbor(decode_with = "minicbor::bytes::decode")]
        raw_history: Vec<u8>,
        // Note that this is an `Option<HistoryType>` because that allows minicbor to decode future
        // versions of HistoryType which have unknown variants as a `None`. We make use of this in
        // `forward_compatible_decode`.
        #[n(1)]
        history_type: Option<HistoryType>,
        #[n(2)]
        refs: BTreeSet<ext::Oid>,
        #[n(3)]
        schema_commit: ext::Oid,
        #[n(4)]
        schema: Schema,
        #[n(5)]
        state: Json,
        #[n(6)]
        #[cbor(with = "typename")]
        typename: TypeName,
        #[n(7)]
        #[cbor(with = "objectid")]
        object_id: ObjectId,
        #[n(8)]
        authorizing_identity_urn: Urn,
    }

    impl<'a> From<&'a ThinChangeGraph> for Encoding {
        fn from(g: &'a ThinChangeGraph) -> Self {
            let raw = if let Some(h) = &g.validated_history {
                h.valid_history().as_bytes().to_vec()
            } else {
                g.history.as_bytes().to_vec()
            };
            Encoding {
                raw_history: raw,
                history_type: Some(HistoryType::Automerge),
                refs: g.refs.iter().map(|oid| (*oid).into()).collect(),
                schema_commit: g.schema_commit.into(),
                schema: g.schema.clone(),
                state: Json(g.state.clone()),
                typename: g.typename.clone(),
                object_id: g.object_id,
                authorizing_identity_urn: g.authorizing_identity_urn.clone(),
            }
        }
    }

    #[derive(thiserror::Error, Debug)]
    pub enum DecodeThinChangeGraphError {
        #[error("unknown history type, most likely due to an out of date librad")]
        UnknownHistoryType,
    }

    impl TryFrom<Encoding> for ThinChangeGraph {
        type Error = DecodeThinChangeGraphError;

        fn try_from(e: Encoding) -> Result<Self, Self::Error> {
            if e.history_type.is_none() {
                Err(DecodeThinChangeGraphError::UnknownHistoryType)
            } else {
                Ok(ThinChangeGraph {
                    validated_history: None,
                    history: History::Automerge(e.raw_history.to_vec()),
                    refs: e.refs.into_iter().map(|eoid| eoid.into()).collect(),
                    schema_commit: e.schema_commit.into(),
                    schema: e.schema,
                    state: e.state.0,
                    typename: e.typename,
                    object_id: e.object_id,
                    authorizing_identity_urn: e.authorizing_identity_urn,
                })
            }
        }
    }

    impl minicbor::Encode for ThinChangeGraph {
        fn encode<W: minicbor::encode::Write>(
            &self,
            e: &mut minicbor::Encoder<W>,
        ) -> Result<(), minicbor::encode::Error<W::Error>> {
            Encoding::from(self).encode(e)
        }
    }

    /// Decode a [`ThinChangeGraph`], but return `None` if the history type of
    /// the encoded change graph is unknown. This means that the
    /// introduction of future history types will result in the cache being
    /// regenerated, rather than an error being thrown.
    pub fn forward_compatible_decode(
        d: &mut minicbor::decode::Decoder<'_>,
    ) -> Result<Option<ThinChangeGraph>, minicbor::decode::Error> {
        let encoded: Encoding = d.decode()?;
        let tg: Result<ThinChangeGraph, DecodeThinChangeGraphError> = encoded.try_into();
        match tg {
            Ok(tg) => Ok(Some(tg)),
            Err(DecodeThinChangeGraphError::UnknownHistoryType) => Ok(None),
        }
    }
}

impl PartialEq for ThinChangeGraph {
    fn eq(&self, other: &Self) -> bool {
        self.history == other.history
            && self.schema_commit == other.schema_commit
            && self.refs == other.refs
            && self.schema == other.schema
            && self.state == other.state
    }
}

impl Clone for ThinChangeGraph {
    fn clone(&self) -> Self {
        ThinChangeGraph {
            // `ValidatedHistory` is not `Clone` because it contains an `automerge::Frontend`,
            // which in turn is not `Clone` because it is a representation of an "actor
            // ID", and it is invalid to make concurrent changes with the same actor
            // ID in automerge.
            validated_history: None,
            history: self.history.clone(),
            refs: self.refs.clone(),
            schema_commit: self.schema_commit,
            schema: self.schema.clone(),
            state: self.state.clone(),
            typename: self.typename.clone(),
            object_id: self.object_id,
            authorizing_identity_urn: self.authorizing_identity_urn.clone(),
        }
    }
}
