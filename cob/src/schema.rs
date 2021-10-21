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

    pub fn validate(&self, doc: &mut automerge::Frontend) -> Result<(), error::ValidationErrors> {
        let value = doc.state().to_json();
        let output = self.schema.apply(&value).basic();
        match output {
            jsonschema::output::BasicOutput::Invalid(_) => self
                .schema
                .validate(&value)
                .map_err(error::ValidationErrors::from),
            jsonschema::output::BasicOutput::Valid(annotations) => {
                for annotation in annotations {
                    if let serde_json::Value::Object(kvs) = annotation.value().as_ref() {
                        if let Some(serde_json::Value::String(s)) = kvs.get("automerge_type") {
                            if s.as_str() == "string" {
                                let value = lookup_value(doc, annotation.instance_location());
                                if !matches!(
                                    value,
                                    Some(automerge::Value::Primitive(automerge::Primitive::Str(_)))
                                ) {
                                    return Err(error::ValidationErrors {
                                        errors: vec![ValidationError {
                                            instance_path: annotation.instance_location().clone(),
                                            description: "Value must be of type 'string'"
                                                .to_string(),
                                        }],
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(())
            },
        }
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
        #[error("schemas must have exactly one $vocabulary: https://alexjg.github.io/automerge-jsonschema/spec")]
        InvalidVocabulary,
        #[error("invalid keyword {keyword} at {path}")]
        InvalidKeyword { path: String, keyword: String },
    }

    #[derive(Debug, Error)]
    #[error("{errors:?}")]
    pub struct ValidationErrors {
        pub(super) errors: Vec<ValidationError>,
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
        if let serde_json::Value::Object(kvs) = value {
            if let Some(serde_json::Value::Object(vocabs)) = kvs.get("$vocabulary") {
                if vocabs.len() != 1 {
                    return Err(error::Parse::InvalidVocabulary);
                }
                if let Some(serde_json::Value::Bool(true)) =
                    vocabs.get("https://alexjg.github.io/automerge-jsonschema/spec")
                {
                } else {
                    return Err(error::Parse::InvalidVocabulary);
                }
                validate_keywords(Path::Root, value)?;
            } else {
                return Err(error::Parse::InvalidVocabulary);
            }
        }
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

#[derive(Clone)]
enum PathChunk<'a> {
    Keyword(&'static str),
    ArrayIndex(usize),
    ObjectProperty(&'a String),
}

impl<'a> fmt::Display for PathChunk<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keyword(k) => write!(f, "{}", k),
            Self::ArrayIndex(i) => write!(f, "{}", i),
            Self::ObjectProperty(p) => write!(f, "{}", p),
        }
    }
}

#[derive(Clone)]
enum Path<'a> {
    Root,
    Child {
        chunk: PathChunk<'a>,
        parent: &'a Path<'a>,
    },
}

impl<'a> Path<'a> {
    fn push(&'a self, chunk: PathChunk<'a>) -> Path<'a> {
        Path::Child {
            parent: self,
            chunk,
        }
    }

    fn to_vec(&'a self) -> Vec<&'a PathChunk<'a>> {
        match self {
            Self::Root => Vec::new(),
            Self::Child { chunk, parent } => {
                let mut result = vec![chunk];
                let mut current_parent = parent;
                while let Path::Child { chunk, parent } = current_parent {
                    current_parent = parent;
                    result.push(chunk);
                }
                result.reverse();
                result
            },
        }
    }
}

impl<'a> fmt::Display for Path<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let as_str = self
            .to_vec()
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<String>>()
            .join("/");
        write!(f, "{}", as_str)
    }
}

/// This enum represents all the applicator keywords defined in the core json
/// vocabulary with the exception of "contains" and "prefixItems", which are not
/// allowed by the automerge vocabulary.
#[derive(Debug)]
enum Applicator<'a> {
    AllOf(&'a [serde_json::Value]),
    AnyOf(&'a [serde_json::Value]),
    OneOf(&'a [serde_json::Value]),
    Not(&'a serde_json::Value),
    IfCondition(&'a serde_json::Value),
    ThenClause(&'a serde_json::Value),
    ElseClause(&'a serde_json::Value),
    DependentSchemas(&'a serde_json::Value),
    Items(&'a serde_json::Value),
    Properties(&'a serde_json::Map<String, serde_json::Value>),
    PatternProperties(&'a serde_json::Value),
    AdditionalProperties(&'a serde_json::Value),
    PropertyNames(&'a serde_json::Value),
    UnevaluatedItems(&'a serde_json::Value),
}

impl<'a> Applicator<'a> {
    fn from_keyword<A: AsRef<str>>(
        keyword: A,
        value: &'a serde_json::Value,
    ) -> Option<Applicator<'a>> {
        match (keyword.as_ref(), value) {
            ("allOf", serde_json::Value::Array(vals)) => Some(Applicator::AllOf(vals)),
            ("anyOf", serde_json::Value::Array(vals)) => Some(Applicator::AnyOf(vals)),
            ("OneOf", serde_json::Value::Array(vals)) => Some(Applicator::OneOf(vals)),
            ("not", props) => Some(Applicator::Not(props)),
            ("if", value) => Some(Applicator::IfCondition(value)),
            ("then", value) => Some(Applicator::ThenClause(value)),
            ("else", value) => Some(Applicator::ElseClause(value)),
            ("dependentSchemas", props) => Some(Applicator::DependentSchemas(props)),
            ("items", value) => Some(Applicator::Items(value)),
            ("properties", serde_json::Value::Object(props)) => Some(Applicator::Properties(props)),
            ("patternProperties", props) => Some(Applicator::PatternProperties(props)),
            ("additionalProperties", props) => Some(Applicator::AdditionalProperties(props)),
            ("propertyNames", props) => Some(Applicator::PropertyNames(props)),
            ("unevaluatedItems", props) => Some(Applicator::UnevaluatedItems(props)),
            _ => None,
        }
    }

    fn keyword(&self) -> &'static str {
        match self {
            Self::AllOf(..) => "allOf",
            Self::AnyOf(..) => "anyOf",
            Self::OneOf(..) => "oneOf",
            Self::Not(..) => "not",
            Self::IfCondition(..) => "ifCondition",
            Self::ThenClause(..) => "thenClause",
            Self::ElseClause(..) => "elseClause",
            Self::DependentSchemas(..) => "dependentSchemas",
            Self::Items(..) => "items",
            Self::Properties(..) => "properties",
            Self::PatternProperties(..) => "patternProperties",
            Self::AdditionalProperties(..) => "additionalProperties",
            Self::PropertyNames(..) => "propertyNames",
            Self::UnevaluatedItems(..) => "unevaluatedItems",
        }
    }

    fn children(&'a self) -> ApplicatorChildren<'a> {
        match self {
            Applicator::AllOf(values) => self.array_children(values),
            Applicator::AnyOf(values) => self.array_children(values),
            Applicator::OneOf(values) => self.array_children(values),
            Applicator::Not(value) => self.object_children(value),
            Applicator::IfCondition(cond) => self.object_children(cond),
            Applicator::ThenClause(clause) => self.object_children(clause),
            Applicator::ElseClause(clause) => self.object_children(clause),
            Applicator::DependentSchemas(value) => self.object_children(value),
            Applicator::Items(items) => self.object_children(items),
            Applicator::Properties(kvs) => ApplicatorChildren::Multiple(Box::new(
                kvs.iter().map(|(k, v)| (PathChunk::ObjectProperty(k), v)),
            )),
            Applicator::PatternProperties(kvs) => self.object_children(kvs),
            Applicator::AdditionalProperties(schema) => self.object_children(schema),
            Applicator::PropertyNames(schema) => self.object_children(schema),
            Applicator::UnevaluatedItems(schema) => self.object_children(schema),
        }
    }

    fn object_children(&'a self, props: &'a serde_json::Value) -> ApplicatorChildren<'a> {
        ApplicatorChildren::Single(props)
    }

    fn array_children(&'a self, values: &'a [serde_json::Value]) -> ApplicatorChildren<'a> {
        ApplicatorChildren::Multiple(Box::new(
            values
                .iter()
                .enumerate()
                .map(|(i, v)| (PathChunk::ArrayIndex(i), v)),
        ))
    }
}

enum ApplicatorChildren<'a> {
    Multiple(Box<dyn Iterator<Item = (PathChunk<'a>, &'a serde_json::Value)> + 'a>),
    Single(&'a serde_json::Value),
}

/// Validator keywords allowed by <https://alexjg.github.io/automerge-jsonschema/spec>
enum Validator {
    Type,
    Enum,
    Const,
    MultipleOf,
    Maximum,
    ExclusiveMaximum,
    Minimum,
    ExclusiveMinimum,
    Required,
    DependentRequired,
    AutomergeType,
}

impl Validator {
    fn from_keyword<A: AsRef<str>>(keyword: A) -> Option<Validator> {
        match keyword.as_ref() {
            "type" => Some(Validator::Type),
            "enum" => Some(Validator::Enum),
            "const" => Some(Validator::Const),
            "multipleOf" => Some(Validator::MultipleOf),
            "maximum" => Some(Validator::Maximum),
            "exclusiveMaximum" => Some(Validator::ExclusiveMaximum),
            "minimum" => Some(Validator::Minimum),
            "exclusiveMinimum" => Some(Validator::ExclusiveMinimum),
            "required" => Some(Validator::Required),
            "dependentRequired" => Some(Validator::DependentRequired),
            "automerge_type" => Some(Validator::AutomergeType),
            _ => None,
        }
    }
}

/// Validator keywords which are allowed provided the underlying automerge type
/// is "string"
enum StringValidator {
    MaxLength,
    MinLength,
    Pattern,
    Format,
    ContentEncoding,
    ContentMediaType,
    ContentSchema,
}

impl StringValidator {
    fn from_keyword<A: AsRef<str>>(keyword: A) -> Option<StringValidator> {
        match keyword.as_ref() {
            "maxLength" => Some(StringValidator::MaxLength),
            "minLength" => Some(StringValidator::MinLength),
            "pattern" => Some(StringValidator::Pattern),
            "format" => Some(StringValidator::Format),
            "contentEncoding" => Some(StringValidator::ContentEncoding),
            "contentMediaType" => Some(StringValidator::ContentMediaType),
            "contentSchema" => Some(StringValidator::ContentSchema),
            _ => None,
        }
    }
}

enum MetaKeyword {
    Schema,
    Vocabulary,
    Id,
    Defs,
    Ref,
    DynamicRef,
    Comment,
}

impl MetaKeyword {
    fn from_keyword<A: AsRef<str>>(keyword: A) -> Option<MetaKeyword> {
        match keyword.as_ref() {
            "$schema" => Some(Self::Schema),
            "$vocabulary" => Some(Self::Vocabulary),
            "$id" => Some(Self::Id),
            "$defs" => Some(Self::Defs),
            "$ref" => Some(Self::Ref),
            "$dynamicRef" => Some(Self::DynamicRef),
            "$comment" => Some(Self::Comment),
            _ => None,
        }
    }
}

/// Check that the schema is a valid <https://alexjg.github.io/automerge-jsonschema/spec> schema. We
/// iterate over each of the keys in the object and:
///
/// - If we encounter a validator keyword (i.e a keyword which is not an
///   applicator) we check that it is one of the keywords allowed by the
///   vocabulary. Some keywords are only allowed if the underlying automerge
///   type is "string", which is asserted by a sibling keyword "automerge_type",
///   so we check that sibling is present for the relevant keywords.
/// - If we encounter an applicator keyword (a keyword which composes
///   subschemas) we check that the applicator is allowed by the vocabulary.
///   Then we check that the subschemas it is composed of are valid with respect
///   to the vocabulary
fn validate_keywords(path: Path<'_>, value: &serde_json::Value) -> Result<(), error::Parse> {
    if let serde_json::Value::Object(props) = value {
        for (prop, value) in props {
            if Validator::from_keyword(prop).is_some() {
                continue;
            }
            if let Some(meta_kw) = MetaKeyword::from_keyword(prop) {
                if let MetaKeyword::Defs = meta_kw {
                    if let serde_json::Value::Object(kvs) = value {
                        let path = path.push(PathChunk::Keyword("$defs"));
                        for (prop, value) in kvs {
                            validate_keywords(path.push(PathChunk::ObjectProperty(prop)), value)?
                        }
                    }
                };
                continue;
            }
            if StringValidator::from_keyword(prop).is_some() {
                if let Some("string") = props.get("automerge_type").and_then(|v| v.as_str()) {
                    continue;
                }
            }
            if let Some(applicator) = Applicator::from_keyword(prop, value) {
                let path = path.push(PathChunk::Keyword(applicator.keyword()));
                match applicator.children() {
                    ApplicatorChildren::Single(props) => {
                        validate_keywords(path.push(PathChunk::ObjectProperty(prop)), props)?;
                    },
                    ApplicatorChildren::Multiple(values) => {
                        for (chunk, value) in values {
                            validate_keywords(path.push(chunk), value)?;
                        }
                    },
                }
                continue;
            }
            return Err(error::Parse::InvalidKeyword {
                path: path.push(PathChunk::ObjectProperty(prop)).to_string(),
                keyword: prop.clone(),
            });
        }
    }
    Ok(())
}

fn lookup_value(
    doc: &automerge::Frontend,
    path: &jsonschema::paths::JSONPointer,
) -> Option<automerge::Value> {
    let mut automerge_path = automerge::Path::root();
    for chunk in path.iter() {
        match chunk {
            jsonschema::paths::PathChunk::Keyword(s) => {
                automerge_path = automerge_path.key(*s);
            },
            jsonschema::paths::PathChunk::Property(s) => {
                automerge_path = automerge_path.key(s.as_ref());
            },
            jsonschema::paths::PathChunk::Index(i) => {
                automerge_path = automerge_path.index((*i) as u32);
            },
        }
    }
    doc.get_value(&automerge_path)
}
