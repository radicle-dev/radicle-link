// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use minicbor::{
    decode::{self, Decoder},
    encode::{self, Encoder, Write},
};

pub mod url {
    use ::url::Url;

    use super::*;

    pub struct Encode(pub Url);

    impl From<Url> for Encode {
        fn from(url: Url) -> Self {
            Self(url)
        }
    }

    impl minicbor::Encode for Encode {
        fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), encode::Error<W::Error>> {
            e.str(self.0.as_str()).map(|_| ())
        }
    }

    pub struct Decode(pub Url);

    impl From<Decode> for Url {
        fn from(d: Decode) -> Self {
            d.0
        }
    }

    impl minicbor::Decode<'_> for Decode {
        fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
            let url = d.str()?;
            Url::try_from(url)
                .map_err(|_| decode::Error::Message("failed to parse Url"))
                .map(Decode)
        }
    }
}

pub mod json {
    use minicbor::data::Type;
    use serde_json::{value::Number, Map, Value};

    use super::*;

    pub struct Encode<'a>(pub &'a Value);

    impl<'a> From<&'a Value> for Encode<'a> {
        fn from(val: &'a Value) -> Self {
            Encode(val)
        }
    }

    impl<'a> minicbor::Encode for Encode<'a> {
        fn encode<W: Write>(&self, e: &mut Encoder<W>) -> Result<(), encode::Error<W::Error>> {
            match &self.0 {
                Value::Null => e.null().map(|_| ()),
                Value::Bool(b) => e.bool(*b).map(|_| ()),
                Value::Number(n) => {
                    if let Some(n) = n.as_u64() {
                        e.u64(n).map(|_| ())
                    } else if let Some(n) = n.as_i64() {
                        e.i64(n).map(|_| ())
                    } else if n.is_f64() {
                        panic!("floating point is not supported in canonical json")
                    } else {
                        panic!("unknown serde_json::Number value encountered")
                    }
                },
                Value::String(s) => e.str(s).map(|_| ()),
                Value::Array(array) => {
                    e.array(array.len() as u64)?;
                    for x in array {
                        e.encode(Encode(x))?;
                    }
                    Ok(())
                },
                Value::Object(map) => {
                    e.map(map.len() as u64)?;
                    for (k, v) in map.iter() {
                        e.str(k)?.encode(Encode(v))?;
                    }
                    Ok(())
                },
            }
        }
    }

    pub struct Decode(pub Value);

    impl From<Decode> for Value {
        fn from(d: Decode) -> Self {
            d.0
        }
    }

    impl minicbor::Decode<'_> for Decode {
        fn decode(d: &mut Decoder) -> Result<Self, decode::Error> {
            match d.datatype()? {
                Type::Bool => d.bool().map(|b| Decode(Value::Bool(b))),
                Type::Null => Ok(Decode(Value::Null)),
                Type::U8 => d.u8().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::U16 => d.u16().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::U32 => d.u8().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::U64 => d.u8().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::I8 => d.i8().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::I16 => d.i16().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::I32 => d.i32().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::I64 => d.i64().map(|x| Decode(Value::Number(Number::from(x)))),
                Type::String => d.str().map(|x| Decode(Value::String(x.to_string()))),
                Type::Array => d.array_iter().and_then(|x| {
                    x.map(|d: Result<Decode, _>| d.map(|d| d.0))
                        .collect::<Result<Vec<Value>, _>>()
                        .map(|x| Decode(Value::Array(x)))
                }),
                Type::Map => d.map_iter().and_then(|x| {
                    x.map(|d: Result<(&str, Decode), _>| d.map(|(k, d)| (k.to_string(), d.0)))
                        .collect::<Result<Map<String, Value>, _>>()
                        .map(|x| Decode(Value::Object(x)))
                }),
                t => Err(decode::Error::TypeMismatch(
                    t,
                    "not a valid Canonical JSON value",
                )),
            }
        }
    }
}
