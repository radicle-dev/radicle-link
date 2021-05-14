// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::internal::canonical::formatter::CanonicalFormatter;

use std::io::Result;

use serde::Serialize;
use serde_json::Serializer;

macro_rules! encode {
        ($($tt:tt)+) => {
            (|v: serde_json::Value| -> Result<Vec<u8>> {
                let mut buf = Vec::new();
                let mut ser = Serializer::with_formatter(&mut buf, CanonicalFormatter::new());
                v.serialize(&mut ser)?;
                Ok(buf)
            })(serde_json::json!($($tt)+))
        };
    }

macro_rules! encode_string {
        ($($tt:tt)+) => {
            (|v: serde_json::Value| -> Result<String> {
                let bytes = encode!(v)?;
                let string = unsafe { String::from_utf8_unchecked(bytes) };
                Ok(string)
            })(serde_json::json!($($tt)+))
        };
    }

#[test]
fn securesystemslib_asserts() -> Result<()> {
    assert_eq!(encode!([1, 2, 3])?, b"[1,2,3]");
    assert_eq!(encode!([1, 2, 3])?, b"[1,2,3]");
    assert_eq!(encode!([])?, b"[]");
    assert_eq!(encode!({})?, b"{}");
    assert_eq!(encode!({"A": [99]})?, br#"{"A":[99]}"#);
    assert_eq!(encode!({"A": true})?, br#"{"A":true}"#);
    assert_eq!(encode!({"B": false})?, br#"{"B":false}"#);
    assert_eq!(encode!({"x": 3, "y": 2})?, br#"{"x":3,"y":2}"#);
    assert_eq!(encode!({"x": 3, "y": null})?, br#"{"x":3,"y":null}"#);

    // Test conditions for invalid arguments.
    assert!(encode!(8.0).is_err());
    assert!(encode!({"x": 8.0}).is_err());

    Ok(())
}

#[test]
fn ascii_control_characters() -> Result<()> {
    assert_eq!(encode_string!("\x00")?, r#""\u0000""#);
    assert_eq!(encode_string!("\x01")?, r#""\u0001""#);
    assert_eq!(encode_string!("\x02")?, r#""\u0002""#);
    assert_eq!(encode_string!("\x03")?, r#""\u0003""#);
    assert_eq!(encode_string!("\x04")?, r#""\u0004""#);
    assert_eq!(encode_string!("\x05")?, r#""\u0005""#);
    assert_eq!(encode_string!("\x06")?, r#""\u0006""#);
    assert_eq!(encode_string!("\x07")?, r#""\u0007""#);
    assert_eq!(encode_string!("\x08")?, r#""\b""#);
    assert_eq!(encode_string!("\x09")?, r#""\t""#);
    assert_eq!(encode_string!("\x0a")?, r#""\n""#);
    assert_eq!(encode_string!("\x0b")?, r#""\u000b""#);
    assert_eq!(encode_string!("\x0c")?, r#""\f""#);
    assert_eq!(encode_string!("\x0d")?, r#""\r""#);
    assert_eq!(encode_string!("\x0e")?, r#""\u000e""#);
    assert_eq!(encode_string!("\x0f")?, r#""\u000f""#);
    assert_eq!(encode_string!("\x10")?, r#""\u0010""#);
    assert_eq!(encode_string!("\x11")?, r#""\u0011""#);
    assert_eq!(encode_string!("\x12")?, r#""\u0012""#);
    assert_eq!(encode_string!("\x13")?, r#""\u0013""#);
    assert_eq!(encode_string!("\x14")?, r#""\u0014""#);
    assert_eq!(encode_string!("\x15")?, r#""\u0015""#);
    assert_eq!(encode_string!("\x16")?, r#""\u0016""#);
    assert_eq!(encode_string!("\x17")?, r#""\u0017""#);
    assert_eq!(encode_string!("\x18")?, r#""\u0018""#);
    assert_eq!(encode_string!("\x19")?, r#""\u0019""#);
    assert_eq!(encode_string!("\x1a")?, r#""\u001a""#);
    assert_eq!(encode_string!("\x1b")?, r#""\u001b""#);
    assert_eq!(encode_string!("\x1c")?, r#""\u001c""#);
    assert_eq!(encode_string!("\x1d")?, r#""\u001d""#);
    assert_eq!(encode_string!("\x1e")?, r#""\u001e""#);
    assert_eq!(encode_string!("\x1f")?, r#""\u001f""#);

    pretty_assertions::assert_eq!(encode_string!({"\t": "\n"})?, r#"{"\t":"\n"}"#);
    assert_eq!(encode_string!("\\")?, r#""\\""#);
    assert_eq!(encode_string!("\"")?, r#""\"""#);

    Ok(())
}

#[test]
fn ordered_nested_object() -> Result<()> {
    assert_eq!(
            encode!({
                "nested": {
                    "good": false,
                    "bad": true
                },
                "b": 2,
                "a": 1,
                "c": {
                    "h": {
                        "h": -5,
                        "i": 3
                    },
                    "a": null,
                    "x": {}
                },
                "zzz": "I have a newline\n"
            })?,
            br#"{"a":1,"b":2,"c":{"a":null,"h":{"h":-5,"i":3},"x":{}},"nested":{"bad":true,"good":false},"zzz":"I have a newline\n"}"#.to_vec(),
        );

    Ok(())
}
