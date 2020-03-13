use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
//use serde_json;

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct EntitySignatureData {
    pub user: Option<String>,
    pub sig: String,
}

#[derive(Clone, Deserialize, Serialize, Debug, Default)]
pub struct EntityData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<HashMap<String, EntitySignatureData>>,

    #[serde(skip_serializing_if = "HashSet::is_empty", default)]
    pub keys: HashSet<String>,
    #[serde(skip_serializing_if = "HashSet::is_empty", default)]
    pub owners: HashSet<String>,
}

impl EntityData {
    pub fn to_json(&self) -> String {
        unimplemented!()
    }
    pub fn from_json(_s: &str) -> Self {
        unimplemented!()
    }
    pub fn canonical_data(&self) -> Vec<u8> {
        unimplemented!()
    }
}
