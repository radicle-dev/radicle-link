// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::Cstring;

use super::{Number, Value};

impl Value {
    pub(super) fn to_bytes(&self) -> Vec<u8> {
        match self {
            Value::Object(obj) => {
                let mut buf = vec![];
                between(&mut buf, b'{', b'}', |buf| {
                    intercalate(buf, obj.iter(), |buf, (key, val)| {
                        string(buf, key);
                        buf.push(b':');
                        buf.extend(val.to_bytes());
                    })
                });
                buf
            },
            Value::Array(array) => {
                let mut buf = vec![];
                between(&mut buf, b'[', b']', |buf| {
                    intercalate(buf, array.iter(), |buf, v| buf.extend(v.to_bytes()))
                });
                buf
            },
            Value::String(s) => {
                let mut buf = vec![];
                string(&mut buf, s);
                buf
            },
            Value::Number(n) => n.to_bytes(),
            Value::Bool(b) => match b {
                true => b"true".to_vec(),
                false => b"false".to_vec(),
            },
            Value::Null => b"null".to_vec(),
        }
    }
}

impl Number {
    pub(super) fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::U64(x) => format!("{}", x).as_bytes().to_vec(),
            Self::I64(x) => format!("{}", x).as_bytes().to_vec(),
        }
    }
}

fn between<F>(buf: &mut Vec<u8>, before: u8, after: u8, callback: F)
where
    F: FnOnce(&mut Vec<u8>),
{
    buf.push(before);
    callback(buf);
    buf.push(after);
}

fn string(buf: &mut Vec<u8>, string: &Cstring) {
    between(buf, b'"', b'"', |buf| {
        buf.extend(string.as_bytes());
    });
}

fn intercalate<F, T>(buf: &mut Vec<u8>, collection: impl ExactSizeIterator<Item = T>, callback: F)
where
    F: Fn(&mut Vec<u8>, T),
{
    let length = collection.len();
    for (i, v) in collection.enumerate() {
        callback(buf, v);
        if i + 1 != length {
            buf.push(b',');
        }
    }
}
