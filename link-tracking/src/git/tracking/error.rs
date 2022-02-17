// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_ref_format::refspec;
use radicle_git_ext::Oid;
use thiserror::Error;

use super::RefName;

#[derive(Debug, Error)]
pub enum Batch {
    #[error("failed to find `{name}` during batch")]
    FindRef {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("`{name}` did not exist, invalidating the given policy")]
    MissingRef { name: RefName<'static, Oid> },
    #[error("failed to write new configuration to `{name}` during batch")]
    WriteObj {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("transaction failed during batch tracking updates")]
    Txn {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum Modify {
    #[error("failed to find reference `{name}` during modify")]
    DidNotExist { name: RefName<'static, Oid> },
    #[error("failed to while attempting to find `{name}` during modify")]
    FindRef {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to modify configuration found at `{name}@{target}`")]
    ModifyObj {
        name: RefName<'static, Oid>,
        target: Oid,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to point `{name}` to new configuration `{object}` during modify")]
    WriteRef {
        object: Oid,
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum Track {
    #[error("failed to create reference `{name}` during track")]
    Create {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to write new configuration to `{name}` during track")]
    WriteObj {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum Untrack {
    #[error("failed to remove configuration at `{name}` during untrack")]
    Delete {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to prune branches related to `{name}` during untrack")]
    Prune {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum UntrackAll {
    #[error("failed to get entries for `{spec}` during untrack all")]
    References {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to unpack a reference for `{spec}` during untrack all")]
    Iter {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to remove configurations for `{spec}` during untrack all")]
    Delete {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to prune branches related to `{spec}` during untrack all")]
    Prune {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum Tracked {
    #[error("failed to get configuration for `{name}@{target}` while getting tracked entries")]
    FindObj {
        name: RefName<'static, Oid>,
        target: Oid,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed to unpack a reference entry while getting tracked entries for `{spec}`")]
    Iter {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed getting tracked entries for `{spec}`")]
    References {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum TrackedPeers {
    #[error("failed to unpack a reference entry while getting tracked entries for `{spec}`")]
    Iter {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed getting tracked entries for `{spec}`")]
    References {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum Get {
    #[error("failed to get configuration for `{name}@{target}` while getting entry")]
    FindObj {
        name: RefName<'static, Oid>,
        target: Oid,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed while attempting to find `{name}` during get")]
    FindRef {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum IsTracked {
    #[error("failed while attempting to find `{name}` during is_tracked")]
    FindRef {
        name: RefName<'static, Oid>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Debug, Error)]
pub enum DefaultOnly {
    #[error("failed to unpack a reference entry while getting tracked entries for `{spec}`")]
    Iter {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("failed getting tracked entries for `{spec}`")]
    References {
        spec: refspec::PatternString,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}
