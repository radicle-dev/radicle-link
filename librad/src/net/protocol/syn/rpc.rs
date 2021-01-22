// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, hash::Hash};

use crate::identities::SomeUrn;

#[derive(Clone, Debug, minicbor::Encode, minicbor::Decode)]
pub enum Request {
    #[n(0)]
    #[cbor(array)]
    ListNamespaces {
        #[n(0)]
        filter: Option<BloomFilter>,
    },
}

#[derive(Clone, Debug, minicbor::Encode, minicbor::Decode)]
pub enum Response {
    #[n(0)]
    #[cbor(array)]
    OfferNamespaces {
        #[n(0)]
        batch: Offer,
    },
}

pub const MAX_OFFER_BATCH_SIZE: usize = 16;

#[derive(Clone, Debug, minicbor::Encode)]
#[cbor(transparent)]
pub struct Offer(#[n(0)] Vec<SomeUrn>);

impl TryFrom<Vec<SomeUrn>> for Offer {
    type Error = &'static str;

    fn try_from(v: Vec<SomeUrn>) -> Result<Self, Self::Error> {
        if v.len() > MAX_OFFER_BATCH_SIZE {
            Err("max batch size exceeded")
        } else {
            Ok(Self(v))
        }
    }
}

impl<'b> minicbor::Decode<'b> for Offer {
    fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
        use minicbor::decode::Error::Message as Error;

        match d.array()? {
            None => Err(Error("expected definite-size array")),
            Some(len) => {
                if len as usize > MAX_OFFER_BATCH_SIZE {
                    Err(Error("max batch size exceeded"))
                } else {
                    d.array_iter()?.collect::<Result<Vec<_>, _>>().map(Self)
                }
            },
        }
    }
}

impl IntoIterator for Offer {
    type Item = SomeUrn;
    type IntoIter = <Vec<SomeUrn> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Clone, Debug, PartialEq, minicbor::Encode, minicbor::Decode)]
#[cbor(array)]
pub struct BloomFilter {
    #[n(0)]
    pub flavour: bloom::Flavour,
    #[n(1)]
    pub hashers: usize,
    #[n(2)]
    pub filter: bloom::Filter,
}

impl<T: Hash> TryFrom<&crate::bloom::BloomFilter<T>> for BloomFilter {
    type Error = <bloom::Filter as TryFrom<Vec<u8>>>::Error;

    fn try_from(b: &crate::bloom::BloomFilter<T>) -> Result<Self, Self::Error> {
        let filter = bloom::Filter::try_from(b.filter().to_owned())?;
        Ok(Self {
            flavour: bloom::Flavour::default(),
            hashers: b.hashers(),
            filter,
        })
    }
}

impl<T: Hash> TryFrom<BloomFilter> for crate::bloom::BloomFilter<T> {
    type Error = &'static str;

    fn try_from(b: BloomFilter) -> Result<Self, Self::Error> {
        match b.flavour {
            bloom::Flavour::KirschMitzenmacher {
                hash_function_1: bloom::HashFunction::Xxh3,
                hash_function_2: bloom::HashFunction::Sip24,
            } => crate::bloom::BloomFilter::load(b.hashers, b.filter.into())
                .ok_or("invalid parameters"),
            _ => Err("unknown flavour"),
        }
    }
}

pub mod bloom {
    use std::{convert::TryFrom, ops::Deref};

    #[derive(Clone, Copy, Debug, PartialEq, minicbor::Encode, minicbor::Decode)]
    pub enum Flavour {
        #[n(0)]
        #[cbor(array)]
        KirschMitzenmacher {
            #[n(0)]
            hash_function_1: HashFunction,
            #[n(1)]
            hash_function_2: HashFunction,
        },
    }

    impl Default for Flavour {
        fn default() -> Self {
            Self::KirschMitzenmacher {
                hash_function_1: HashFunction::Xxh3,
                hash_function_2: HashFunction::Sip24,
            }
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, minicbor::Encode, minicbor::Decode)]
    #[cbor(index_only)]
    pub enum HashFunction {
        #[n(0)]
        Xxh3,
        #[n(1)]
        Sip24,
    }

    pub const MAX_FILTER_LEN: usize = 36_000;

    // TODO: should we borrow?
    #[derive(Debug, PartialEq, minicbor::Encode)]
    #[cbor(transparent)]
    pub struct Filter(#[n(0)] minicbor::bytes::ByteVec);

    impl Clone for Filter {
        fn clone(&self) -> Self {
            Self((*self.0).clone().into())
        }
    }

    impl Deref for Filter {
        type Target = [u8];

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl TryFrom<Vec<u8>> for Filter {
        type Error = &'static str;

        fn try_from(v: Vec<u8>) -> Result<Self, Self::Error> {
            if v.len() > MAX_FILTER_LEN {
                Err("maximum length of bloom filter exceeded")
            } else {
                Ok(Self(v.into()))
            }
        }
    }

    impl From<Filter> for Vec<u8> {
        fn from(f: Filter) -> Self {
            f.0.into()
        }
    }

    impl<'b> minicbor::Decode<'b> for Filter {
        fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
            let bytes: Vec<u8> = minicbor::bytes::ByteVec::decode(d)?.into();
            Self::try_from(bytes).map_err(minicbor::decode::Error::Message)
        }
    }
}
