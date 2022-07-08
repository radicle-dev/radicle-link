// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::HashMap, ops::ControlFlow};

use crate::{
    change::Change,
    history,
    identity_storage::{lookup_authorizing_identity, lookup_person},
    pruning_fold,
    AuthDecision,
    AuthorizingIdentity,
    IdentityStorage,
};

pub struct Evaluating<'a, I: IdentityStorage> {
    identities: &'a I,
    authorizing_identity: &'a dyn AuthorizingIdentity,
    repo: &'a git2::Repository,
}

impl<'a, I: IdentityStorage> Evaluating<'a, I> {
    pub fn new(
        identities: &'a I,
        authorizer: &'a dyn AuthorizingIdentity,
        repo: &'a git2::Repository,
    ) -> Evaluating<'a, I> {
        Evaluating {
            identities,
            authorizing_identity: authorizer,
            repo,
        }
    }

    /// # Panics
    ///
    /// If the change corresponding to the root OID is not in `items`
    pub fn evaluate<'b, It: Iterator<Item = (&'b Change, Vec<git2::Oid>)>>(
        mut self,
        root: git2::Oid,
        items: It,
    ) -> history::History {
        let entries = pruning_fold::pruning_fold(
            HashMap::new(),
            items.map(|(change, children)| ChangeWithChildren {
                change,
                child_commits: children,
            }),
            |mut entries, c| match self.evaluate_change(c.change, &c.child_commits) {
                Err(reason) => {
                    reason.log(c.change);
                    ControlFlow::Break(entries)
                },
                Ok(entry) => {
                    tracing::trace!(commit=?c.change.commit(), "change accepted");
                    entries.insert((*c.change.commit()).into(), entry);
                    ControlFlow::Continue(entries)
                },
            },
        );
        // SAFETY: The caller must guarantee that `root` is in `items`
        history::History::new(root, entries).unwrap()
    }

    fn evaluate_change(
        &mut self,
        change: &Change,
        child_commits: &[git2::Oid],
    ) -> Result<history::HistoryEntry, RejectionReason> {
        // Check the change signatures are valid
        if !change.valid_signatures() {
            return Err(RejectionReason::InvalidSignatures);
        }

        // Check that the authorizing identity refernced by the change is a valid
        // version of the identity we are authorizing with respect to
        let referenced_auth_identity = match lookup_authorizing_identity(
            self.identities,
            self.repo,
            change.authorizing_identity_commit(),
        ) {
            Ok(id) => id,
            Err(e) => {
                return Err(RejectionReason::InvalidAuthorizer(Box::new(e)));
            },
        };
        if referenced_auth_identity.urn() != self.authorizing_identity.urn() {
            return Err(RejectionReason::WrongAuthorizer);
        }

        let author = lookup_person(self.repo, change.author_commit())
            .map_err(|e| RejectionReason::ErrorFindingAuthor {
                author_commit_oid: change.author_commit(),
                error: Box::new(e),
            })?
            .ok_or_else(|| RejectionReason::MissingAuthor {
                missing_author_oid: change.author_commit(),
            })?;
        // Check that the authorizing identity allows this change
        match referenced_auth_identity.check_authorization(&author) {
            AuthDecision::Authorized => {},
            AuthDecision::NotAuthorized { reason } => {
                return Err(RejectionReason::Unauthorized { reason });
            },
        };

        Ok(history::HistoryEntry::new(
            *change.commit(),
            author.urn(),
            child_commits.iter().cloned(),
            change.contents().clone(),
        ))
    }
}

struct ChangeWithChildren<'a> {
    change: &'a Change,
    child_commits: Vec<git2::Oid>,
}

impl<'a> pruning_fold::GraphNode for ChangeWithChildren<'a> {
    type Id = git2::Oid;

    fn id(&self) -> &Self::Id {
        self.change.commit()
    }

    fn child_ids(&self) -> &[Self::Id] {
        &self.child_commits
    }
}

enum RejectionReason {
    InvalidSignatures,
    InvalidAuthorizer(Box<dyn std::error::Error>),
    WrongAuthorizer,
    MissingAuthor {
        missing_author_oid: git2::Oid,
    },
    ErrorFindingAuthor {
        author_commit_oid: git2::Oid,
        error: Box<dyn std::error::Error>,
    },
    Unauthorized {
        reason: &'static str,
    },
}

impl RejectionReason {
    fn log(&self, change: &Change) {
        match self {
            RejectionReason::InvalidSignatures => {
                tracing::warn!(
                    commit=?change.commit(),
                    "rejecting change because its signatures were invalid"
                );
            },
            RejectionReason::InvalidAuthorizer(error) => {
                tracing::warn!(
                    commit=?change.commit(),
                    err=?error,
                    "rejecting change due to an error looking up the authorizing identity"
                );
            },
            RejectionReason::WrongAuthorizer => {
                tracing::warn!(
                    commit=?change.commit(),
                    "rejecting change which points to an authorizing identity this reference is not stored under"
                );
            },
            RejectionReason::MissingAuthor { missing_author_oid } => {
                tracing::warn!(
                    commit=?change.commit(),
                    missing_author_oid=?missing_author_oid,
                    "rejecting change due to a missing author identity"
                );
            },
            RejectionReason::ErrorFindingAuthor {
                author_commit_oid,
                error,
            } => {
                tracing::warn!(
                    commit=?change.commit(),
                    ?author_commit_oid,
                    err=?error,
                    "rejecting change due to an error lookup up the author identity"
                );
            },
            RejectionReason::Unauthorized { reason } => {
                tracing::warn!(
                    commit=?change.commit(),
                    reason,
                    "rejecting change as it was not authorized"
                );
            },
        }
    }
}
