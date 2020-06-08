// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{io, marker::PhantomData};

use bytes::{Buf, BufMut, BytesMut};
use futures_codec::{Decoder, Encoder};
use minicbor::{Decode, Encode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CborCodecError {
    #[error(transparent)]
    Encode(#[from] minicbor::encode::Error<io::Error>),

    #[error(transparent)]
    Decode(#[from] minicbor::decode::Error),

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
        let bytes = minicbor::to_vec(&item)?;

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
            Err(e) => Err(e.into()),
        };

        let offset = decoder.position();
        src.advance(offset);

        res
    }
}
