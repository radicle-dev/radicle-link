// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_canonical::{
    json::{Array, Map, ToCjson, Value},
    Canonical,
    Cstring,
};

#[derive(ToCjson)]
#[cjson(rename_all = "camelCase")]
struct Foo {
    x_foo: u64,
    y_foo: Option<Cstring>,
}

#[derive(ToCjson)]
struct Bar(bool, bool);

#[derive(ToCjson)]
struct Baz;

#[derive(ToCjson)]
struct Newtype(bool);

#[derive(ToCjson)]
#[cjson(tag = "type", content = "payload")]
enum E {
    W { a: u32, b: i32 },
    X(u32, i32),
    Y(i32),
    Z,
}

#[derive(ToCjson)]
#[cjson(tag = "t")]
enum F {
    I(u64),
    N(bool, bool),
    T { x: String },
    O,
}

fn roundtrip(s: &str) -> Result<(), String> {
    let val = s.parse::<Value>()?;
    assert_eq!(val.canonical_form().unwrap(), s.as_bytes());
    Ok(())
}

fn encode_string(s: &str) -> Result<String, String> {
    let bs = s.parse::<Value>()?.canonical_form().unwrap();
    Ok(std::str::from_utf8(&bs).unwrap().to_string())
}

#[test]
fn securesystemslib_asserts() -> Result<(), String> {
    roundtrip("[1,2,3]")?;
    roundtrip("[]")?;
    roundtrip("{}")?;
    roundtrip(r#"{"A":[99]}"#)?;
    roundtrip(r#"{"A":true}"#)?;
    roundtrip(r#"{"B":false}"#)?;
    roundtrip(r#"{"x":3,"y":2}"#)?;
    roundtrip(r#"{"x":3,"y":null}"#)?;

    // Test conditions for invalid arguments.
    assert!(roundtrip("8.0").is_err());
    assert!(roundtrip(r#"{"x": 8.0}"#).is_err());

    Ok(())
}

#[test]
fn ascii_control_characters() -> Result<(), String> {
    for i in 0x00..0x80 {
        assert!(encode_string(&format!("\\x{:02x}", i)).is_err());
    }

    pretty_assertions::assert_eq!(&encode_string(r#"{"\t": "\n"}"#)?, r#"{"\t":"\n"}"#);
    assert_eq!(&encode_string(r#""\\""#)?, r#""\\""#);
    assert_eq!(&encode_string(r#""\"""#)?, r#""\"""#);

    Ok(())
}

#[test]
fn ordered_nested_object() -> Result<(), String> {
    roundtrip(
        r#"{"a":1,"b":2,"c":{"a":null,"h":{"h":-5,"i":3},"x":{}},"nested":{"bad":true,"good":false},"zzz":"I have a newline\n"}"#,
    )?;

    assert_eq!(
            r#"{
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
            }"#.parse::<Value>()?.canonical_form().unwrap(),
            br#"{"a":1,"b":2,"c":{"a":null,"h":{"h":-5,"i":3},"x":{}},"nested":{"bad":true,"good":false},"zzz":"I have a newline\n"}"#.to_vec(),
        );

    Ok(())
}

#[test]
fn foo_canon() {
    let val = Foo {
        x_foo: 42,
        y_foo: Some("hello".into()),
    };
    assert_eq!(
        val.into_cjson(),
        vec![
            ("xFoo".into(), 42u64.into_cjson()),
            ("yFoo".into(), "hello".into_cjson())
        ]
        .into_iter()
        .collect::<Value>()
    );
}

#[test]
fn bar_canon() {
    let val = Bar(true, false);
    assert_eq!(
        val.into_cjson(),
        vec![true, false]
            .into_iter()
            .collect::<Array>()
            .into_cjson()
    );
}

#[test]
fn newtype_canon() {
    assert_eq!(Newtype(true).into_cjson(), Value::Bool(true));
}

#[test]
fn baz_canon() {
    assert_eq!(Baz.into_cjson(), Value::Null);
}

#[test]
fn e_canon() {
    let val = E::W { a: 42, b: -3 };
    assert_eq!(
        val.into_cjson(),
        vec![
            ("type".into(), "W".into_cjson()),
            (
                "payload".into(),
                vec![
                    ("a".into(), 42u64.into_cjson()),
                    ("b".into(), (-3).into_cjson()),
                ]
                .into_iter()
                .collect::<Map>()
                .into_cjson()
            )
        ]
        .into_iter()
        .collect::<Value>()
    );

    let val = E::X(42, 3);
    assert_eq!(
        val.into_cjson(),
        vec![
            ("type".into(), "X".into_cjson()),
            (
                "payload".into(),
                vec![42u64.into_cjson(), 3.into_cjson()].into_cjson()
            ),
        ]
        .into_iter()
        .collect::<Value>()
    );

    let val = E::Y(42);
    assert_eq!(
        val.into_cjson(),
        vec![
            ("type".into(), "Y".into_cjson()),
            ("payload".into(), 42.into_cjson())
        ]
        .into_iter()
        .collect::<Value>()
    );

    assert_eq!(
        E::Z.into_cjson(),
        vec![("type".into(), "Z".into_cjson())]
            .into_iter()
            .collect::<Value>()
    )
}

#[test]
fn f_canon() {
    let val = F::I(42);
    assert_eq!(
        val.into_cjson(),
        vec![
            ("t".into(), "I".into_cjson()),
            ("0".into(), 42u64.into_cjson())
        ]
        .into_iter()
        .collect::<Value>()
    );

    let val = F::N(true, false);
    assert_eq!(
        val.into_cjson(),
        vec![
            ("t".into(), "N".into_cjson()),
            ("0".into(), true.into_cjson()),
            ("1".into(), false.into_cjson())
        ]
        .into_iter()
        .collect::<Value>()
    );

    let val = F::T {
        x: "isolation station".to_string(),
    };
    assert_eq!(
        val.into_cjson(),
        vec![
            ("t".into(), "T".into_cjson()),
            ("x".into(), "isolation station".into_cjson()),
        ]
        .into_iter()
        .collect::<Value>()
    );

    assert_eq!(
        F::O.into_cjson(),
        vec![("t".into(), "O".into_cjson())]
            .into_iter()
            .collect::<Value>()
    );
}
