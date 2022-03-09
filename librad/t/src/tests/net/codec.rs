// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use futures::{AsyncReadExt as _, SinkExt as _, TryStreamExt as _};
use futures_codec::{FramedRead, FramedWrite};
use librad::net::codec::{CborCodec, CborCodecError, CborError};
use minicbor::{Decode, Encode};

#[derive(Clone, Debug, PartialEq, Encode, Decode)]
struct Data {
    #[n(0)]
    field0: usize,
    #[n(1)]
    field1: Vec<char>,
    #[n(2)]
    #[cbor(with = "minicbor::bytes")]
    field2: Vec<u8>,
}

#[async_test]
async fn roundtrip() {
    let data = Data {
        field0: 42,
        field1: "abc".chars().collect(),
        field2: b"xyz".to_vec(),
    };

    let mut buf = Vec::new();
    let mut framed = FramedWrite::new(&mut buf, CborCodec::<Data, Data>::new());
    framed.send(data.clone()).await.unwrap();
    let (buf, codec) = framed.release();

    let mut framed = FramedRead::new(buf.as_slice(), codec);
    let data0 = framed.try_next().await.unwrap();
    assert_eq!(Some(data), data0)
}

#[async_test]
async fn sequence() {
    let data1 = Data {
        field0: 42,
        field1: "abc".chars().collect(),
        field2: b"xyz".to_vec(),
    };
    let data2 = Data {
        field0: 32,
        field1: "cde".chars().collect(),
        field2: b"zyx".to_vec(),
    };

    let mut buf = Vec::new();
    let mut framed = FramedWrite::new(&mut buf, CborCodec::<Data, Data>::new());
    framed.send(data1.clone()).await.unwrap();
    framed.send(data2.clone()).await.unwrap();
    let (buf, codec) = framed.release();

    let framed = FramedRead::new(buf.as_slice(), codec);
    let out: Result<Vec<Data>, CborCodecError> = framed.try_collect().await;
    assert_eq!(&out.unwrap(), &[data1, data2])
}

#[async_test]
async fn decode_early_eof() {
    let mut buf = minicbor::to_vec(&Data {
        field0: 69,
        field1: "hello".chars().collect(),
        field2: b"asdf".to_vec(),
    })
    .unwrap();
    buf.truncate(buf.len() / 2);

    let mut framed = FramedRead::new(buf.as_slice(), CborCodec::<Data, Data>::new());
    assert!(matches!(
        framed.try_next().await,
        Err(CborCodecError::Cbor(CborError::Decode(
            minicbor::decode::Error::EndOfInput
        )))
    ))
}

#[async_test]
async fn incremental() {
    let data = Data {
        field0: 42,
        field1: "abc".chars().collect(),
        field2: b"xyz".to_vec(),
    };

    let mut buf = minicbor::to_vec(&data).unwrap();
    let snd = buf.split_off(buf.len() / 2);
    let mut framed = FramedRead::new(
        buf.as_slice().chain(snd.as_slice()),
        CborCodec::<Data, Data>::new(),
    );
    let out = framed.try_next().await.unwrap().unwrap();
    assert_eq!(data, out)
}
