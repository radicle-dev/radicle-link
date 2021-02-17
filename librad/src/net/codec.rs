// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, marker::PhantomData};

use bytes::{Buf, BufMut, BytesMut};
use futures_codec::{Decoder, Encoder};
use minicbor::{Decode, Encode};
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CborError {
    // Note: the error will not actually contain any io, the type parameter is
    // there due to the `Write` impl of `Vec`
    #[error(transparent)]
    Encode(#[from] minicbor::encode::Error<io::Error>),

    #[error(transparent)]
    Decode(#[from] minicbor::decode::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CborCodecError {
    #[error(transparent)]
    Cbor(#[from] CborError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Copy, Default)]
pub struct CborCodec<Enc, Dec> {
    enc: PhantomData<Enc>,
    dec: PhantomData<Dec>,
}

impl<Enc, Dec> CborCodec<Enc, Dec> {
    pub fn new() -> Self {
        Self {
            enc: PhantomData,
            dec: PhantomData,
        }
    }
}

impl<Enc, Dec> Encoder for CborCodec<Enc, Dec>
where
    Enc: Encode,
{
    type Item = Enc;
    type Error = CborCodecError;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = minicbor::to_vec(&item).map_err(CborError::from)?;

        dst.reserve(bytes.len());
        dst.put_slice(&bytes);

        Ok(())
    }
}

impl<Enc, Dec> Decoder for CborCodec<Enc, Dec>
where
    for<'b> Dec: Decode<'b>,
{
    type Item = Dec;
    type Error = CborCodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut decoder = minicbor::Decoder::new(src);

        let res = match decoder.decode() {
            Ok(v) => Ok(Some(v)),
            // try later if we reach EOF prematurely
            Err(minicbor::decode::Error::EndOfInput) => Ok(None),
            Err(e) => Err(CborError::from(e).into()),
        };

        let offset = decoder.position();
        src.advance(offset);

        res
    }
}
