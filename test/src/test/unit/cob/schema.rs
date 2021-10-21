// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

#[test]
fn valid_schema_can_be_parsed() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
        },
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    assert!(cob::Schema::try_from(&schema).is_ok());
}

#[test]
fn missing_vocab_fails() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    assert!(matches!(
        cob::Schema::try_from(&schema),
        Err(cob::schema::error::Parse::InvalidVocabulary)
    ));
}

#[test]
fn non_required_automerge_vocab_fails() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": false,
        },
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    assert!(matches!(
        cob::Schema::try_from(&schema),
        Err(cob::schema::error::Parse::InvalidVocabulary)
    ));
}

#[test]
fn other_vocabs_fails() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
            "https://json-schema.org/draft/2020-12/schema": true
        },
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    assert!(matches!(
        cob::Schema::try_from(&schema),
        Err(cob::schema::error::Parse::InvalidVocabulary)
    ));
}

#[test]
fn invalid_keywords_raise_error() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
        },
        "type": "object",
        "properties": {
            "addresses": {
                "type": "array",
                "maxLength": 10,
                "items": {
                    "type": "object",
                    "properties": {
                        "line_one": {"type": "string"}
                    }
                }
            }
        }
    });
    let err = cob::schema::Schema::try_from(&schema).err();
    if let Some(cob::schema::error::Parse::InvalidKeyword { path, keyword }) = err {
        assert_eq!(path, "properties/addresses/maxLength".to_string());
        assert_eq!(keyword, "maxLength".to_string());
    } else {
        panic!("expected an InvalidKeyword error, got {:?}", err);
    }
}

#[test]
fn invalid_keywords_in_all_of_raises_error() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
        },
        "type": "object",
        "properties": {
            "addresses": {
                "allOf": [
                    {"type": "array"},
                    {"maxLength": 10 },
                ]
            }
        }
    });
    let err = cob::schema::Schema::try_from(&schema).err();
    if let Some(cob::schema::error::Parse::InvalidKeyword { path, keyword }) = err {
        assert_eq!(path, "properties/addresses/allOf/1/maxLength".to_string());
        assert_eq!(keyword, "maxLength".to_string());
    } else {
        panic!("expected an InvalidKeyword error, got {:?}", err);
    }
}

#[test]
fn invalid_keywords_in_definitions_raises_error() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
        },
        "type": "object",
        "$defs": {
            "address": {
                "type": "array",
                "maxLength": 10
            }
        },
        "properties": {
            "addresses": {
                "$ref": "#/$defs/address"
            }
        }
    });
    let err = cob::schema::Schema::try_from(&schema).err();
    if let Some(cob::schema::error::Parse::InvalidKeyword { path, keyword }) = err {
        assert_eq!(path, "$defs/address/maxLength".to_string());
        assert_eq!(keyword, "maxLength".to_string());
    } else {
        panic!("expected an InvalidKeyword error, got {:?}", err);
    }
}

#[test]
fn string_validation_keywords_valid_if_automerge_type_string() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
        },
        "type": "object",
        "properties": {
            "name": {
                "automerge_type": "string",
                "type": "string",
                "maxLength": 10
            }
        }
    });
    assert!(cob::schema::Schema::try_from(&schema).is_ok())
}

#[test]
fn string_validation_keywords_invalid_if_not_automerge_type_string() {
    let schema = serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
        },
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "maxLength": 10
            }
        }
    });
    let err = cob::schema::Schema::try_from(&schema).err();
    if let Some(cob::schema::error::Parse::InvalidKeyword { path, keyword }) = err {
        assert_eq!(path, "properties/name/maxLength".to_string());
        assert_eq!(keyword, "maxLength".to_string());
    } else {
        panic!("expected an InvalidKeyword error, got {:?}", err);
    }
}

#[test]
fn automerge_document_with_automerge_type_string() {
    let schema = cob::schema::Schema::try_from(&serde_json::json!({
        "$vocabulary": {
            "https://alexjg.github.io/automerge-jsonschema/spec": true,
        },
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "automerge_type": "string",
                "maxLength": 10
            }
        }
    }))
    .unwrap();
    let mut good_doc = automerge::Frontend::new();
    let mut good_doc_backend = automerge::Backend::new();
    good_doc
        .apply_patch(good_doc_backend.get_patch().unwrap())
        .unwrap();
    let (_, change) = good_doc
        .change::<_, _, automerge::InvalidChangeRequest>(None, |doc| {
            doc.add_change(automerge::LocalChange::set(
                automerge::Path::root().key("name"),
                automerge::Value::Primitive(automerge::Primitive::Str("somename".into())),
            ))?;
            Ok(())
        })
        .unwrap();
    let (patch, _) = good_doc_backend
        .apply_local_change(change.unwrap())
        .unwrap();
    good_doc.apply_patch(patch).unwrap();
    assert!(schema.validate(&mut good_doc).is_ok());

    let mut bad_doc = automerge::Frontend::new();
    let mut bad_doc_backend = automerge::Backend::new();
    bad_doc
        .apply_patch(bad_doc_backend.get_patch().unwrap())
        .unwrap();
    let (_, change) = bad_doc
        .change::<_, _, automerge::InvalidChangeRequest>(None, |doc| {
            doc.add_change(automerge::LocalChange::set(
                automerge::Path::root().key("name"),
                automerge::Value::Text("some name".chars().map(|c| c.to_string().into()).collect()),
            ))?;
            Ok(())
        })
        .unwrap();
    let (patch, _) = bad_doc_backend.apply_local_change(change.unwrap()).unwrap();
    bad_doc.apply_patch(patch).unwrap();
    assert!(schema.validate(&mut bad_doc).is_err());

    //let mut bad_doc = automerge::Automerge::new();
    //bad_doc.change::<_, _, automerge::InvalidChangeRequest>(None, |doc| {
    //doc.add_change(automerge::LocalChange::set(
    //automerge::Path::root().key("name"),
    //automerge::Value::Text("some name".chars().map(|c|
    // c.to_string().into()).collect()),
    //))?;
    //Ok(())
    //}).unwrap();
    //assert!(schema.validate(&bad_doc).is_err());
}
