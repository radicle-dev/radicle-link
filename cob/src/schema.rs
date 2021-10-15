// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::{TryFrom, TryInto},
    fmt,
};

#[derive(Debug)]
pub struct Schema {
    json: serde_json::Value,
    schema: jsonschema::JSONSchema,
}

impl PartialEq for Schema {
    fn eq(&self, other: &Self) -> bool {
        self.json == other.json
    }
}

impl Schema {
    pub fn json_bytes(&self) -> Vec<u8> {
        self.json.to_string().as_bytes().into()
    }

    pub fn validate(&self, value: &serde_json::Value) -> Result<(), error::ValidationErrors> {
        self.schema
            .validate(value)
            .map_err(error::ValidationErrors::from)
    }
}

impl Clone for Schema {
    fn clone(&self) -> Self {
        Schema {
            json: self.json.clone(),
            // The unwrap here is fine as we've already validated the schema during construction
            schema: jsonschema::JSONSchema::compile(&self.json).unwrap(),
        }
    }
}

#[derive(Debug)]
pub struct ValidationError {
    instance_path: jsonschema::paths::JSONPointer,
    description: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.instance_path, self.description)
    }
}

impl<'a> From<jsonschema::ValidationError<'a>> for ValidationError {
    fn from(e: jsonschema::ValidationError<'a>) -> Self {
        ValidationError {
            instance_path: e.instance_path.clone(),
            description: e.to_string(),
        }
    }
}

pub mod error {
    use super::ValidationError;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Parse {
        #[error(transparent)]
        Serde(#[from] serde_json::error::Error),
        #[error("invalid schema: {0}")]
        Validation(String),
    }

    #[derive(Debug, Error)]
    #[error("{errors:?}")]
    pub struct ValidationErrors {
        errors: Vec<ValidationError>,
    }

    impl<'a, I> From<I> for ValidationErrors
    where
        I: Iterator<Item = jsonschema::ValidationError<'a>>,
    {
        fn from(errors: I) -> Self {
            ValidationErrors {
                errors: errors.map(ValidationError::from).collect(),
            }
        }
    }
}

impl TryFrom<&serde_json::Value> for Schema {
    type Error = error::Parse;

    fn try_from(value: &serde_json::Value) -> Result<Self, Self::Error> {
        jsonschema::JSONSchema::compile(value)
            .map(|s| Schema {
                json: value.clone(),
                schema: s,
            })
            .map_err(|e| error::Parse::Validation(e.to_string()))
    }
}

impl TryFrom<&[u8]> for Schema {
    type Error = error::Parse;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let json: serde_json::Value = serde_json::from_slice(bytes)?;
        (&json).try_into()
    }
}
