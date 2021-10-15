// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{History, Schema};

use std::convert::TryFrom;

pub mod error {
    use super::super::schema::error::Parse as SchemaParseError;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum LoadError {
        #[error(transparent)]
        AutomergeBackend(#[from] automerge::BackendError),
        #[error(transparent)]
        AutomergeFrontend(#[from] automerge::FrontendError),
        #[error(transparent)]
        InvalidPatch(#[from] automerge::InvalidPatch),
        #[error(transparent)]
        SchemaParse(#[from] SchemaParseError),
    }

    #[derive(Debug, Error)]
    pub enum ProposalError {
        #[error("invalid change: {0}")]
        InvalidChange(Box<dyn std::error::Error>),
        #[error("invalidates schema: {0}")]
        InvalidatesSchema(Box<dyn std::error::Error>),
        #[error("there are missing dependencies: {missing:?}")]
        MissingDependencies { missing: Vec<automerge::ChangeHash> },
    }
}

/// A history which is valid with respect to a schema and allows fallibly
/// proposing a new change
///
/// The main purpose of this is to cache the backend and frontend for use when
/// the change does not invalidate the schema (presumably the common case). This
/// is necessary because loading a schema invalidating change requires throwing
/// away the backend and reloading it, which is very wasteful for the happy
/// path.
///
/// There are a number of unwraps which are currently unavoidable due to
/// deficiencies in Automerge's API. Let me explain. Automerge is currently
/// architected as a "frontend" and a "backend". These are components which can
/// run in different processes and even in different languages, so they
/// communicate with each other via (possibly) serialized data structures. The
/// backend stores the entire history of the document and emits patch to the
/// frontend which just has the currently realized state, the frontend in turn
/// generates "changes", which are sent to the backend.  Consequently there are
/// a lot of methods on the frontend and backend which are fallible, even though
/// in our case (everything on one thread, with no serialization or other
/// messing with the data structures involved) there is no possibility of an
/// error.
///
/// This is not an ideal situation and there are plans to update the automerge
/// API to fix this unfortunate state of affairs, as well as increasing
/// performance by an order of magnitude or more. Until then we must make do
/// with a long prose explanation of why the unwraps are okay.
#[derive(Debug)]
pub struct ValidatedAutomerge {
    backend: automerge::Backend,
    frontend: automerge::Frontend,
    schema: Schema,
    valid_history: Vec<u8>,
}

impl ValidatedAutomerge {
    pub(crate) fn new(schema: Schema) -> ValidatedAutomerge {
        ValidatedAutomerge {
            backend: automerge::Backend::new(),
            frontend: automerge::Frontend::new(),
            valid_history: Vec::new(),
            schema,
        }
    }

    pub fn new_with_history(
        schema: Schema,
        history: History,
    ) -> Result<ValidatedAutomerge, error::LoadError> {
        let hist_bytes = history.as_bytes().to_vec();
        let backend = automerge::Backend::load(hist_bytes.clone())?;
        let mut frontend = automerge::Frontend::new();
        // Unwraps are fine as fallibility is due to errors which can occur in
        // serialization or due to out of order delivery of patches
        frontend.apply_patch(backend.get_patch().unwrap()).unwrap();
        Ok(ValidatedAutomerge {
            backend,
            frontend,
            valid_history: hist_bytes,
            schema,
        })
    }

    pub(crate) fn propose_change(
        &mut self,
        change_bytes: &[u8],
    ) -> Result<(), error::ProposalError> {
        let change = automerge::Change::try_from(change_bytes)
            .map_err(|e| error::ProposalError::InvalidChange(Box::new(e)))?;
        let old_backend = self.backend.clone();
        let patch = self
            .backend
            .apply_changes(vec![change])
            .map_err(|e| error::ProposalError::InvalidChange(Box::new(e)))?;
        // This can only go wrong if the patch is delivered out of order, which we
        // promise we aren't doing
        self.frontend.apply_patch(patch).unwrap();
        let value = self.frontend.state();
        let validation_error = self.schema.validate(&value.to_json()).err();
        match validation_error {
            None => {
                self.valid_history.extend(change_bytes);
            },
            Some(e) => {
                tracing::debug!(invalid_json=?value.to_json().to_string(), "change invalidated schema");
                self.reset(old_backend);
                return Err(error::ProposalError::InvalidatesSchema(Box::new(e)));
            },
        }
        let missing_deps = self.backend.get_missing_deps(&[]);
        if !missing_deps.is_empty() {
            self.reset(old_backend);
            return Err(error::ProposalError::MissingDependencies {
                missing: missing_deps,
            });
        }
        self.valid_history = self.backend.save().unwrap();
        Ok(())
    }

    fn reset(&mut self, old_backend: automerge::Backend) {
        self.backend = old_backend;
        let mut old_frontend = automerge::Frontend::new();
        // This can only happen if an invalid document is loaded, but we know the
        // backend is in a good state as we had already previously generated a
        // patch from it.
        let patch = self.backend.get_patch().unwrap();
        old_frontend.apply_patch(patch).unwrap();
        self.frontend = old_frontend;
    }

    pub(crate) fn state(&self) -> serde_json::Value {
        self.frontend
            .get_value(&automerge::Path::root())
            // this can only fail if the path does not exist, but the root always exists
            .unwrap()
            .to_json()
    }

    pub(crate) fn valid_history(&self) -> History {
        History::Automerge(self.valid_history.clone())
    }

    pub(crate) fn compressed_valid_history(&self) -> History {
        // This error is a red herring, it can only occur in an OOM situation.
        History::Automerge(self.backend.save().unwrap())
    }
}
