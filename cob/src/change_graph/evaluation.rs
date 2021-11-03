// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::{
    change::Change,
    identity_storage::{lookup_authorizing_identity, lookup_person},
    validated_automerge::{error::ProposalError, ValidatedAutomerge},
    AuthDecision,
    AuthorizingIdentity,
    History,
    IdentityStorage,
    Schema,
};
use std::collections::{BTreeSet, HashMap};

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
    InvalidChange(ProposalError),
}

struct RejectedChanges {
    /// Changes which are rejcted due to a problem with the change commit
    direct: BTreeSet<git2::Oid>,
    /// A map from change commit IDs to the IDs of ancestor commits which are
    /// direct rejections
    transitive: HashMap<git2::Oid, git2::Oid>,
}

impl RejectedChanges {
    fn new() -> RejectedChanges {
        RejectedChanges {
            direct: BTreeSet::new(),
            transitive: HashMap::new(),
        }
    }

    fn rejected_ancestor(&self, change: &Change) -> Option<git2::Oid> {
        self.transitive.get(&change.commit()).copied()
    }

    fn is_rejected(&self, change: &Change) -> bool {
        self.direct.contains(&change.commit())
    }

    fn directly_reject(&mut self, change: git2::Oid, children: Vec<git2::Oid>) {
        self.direct.insert(change);
        for child in children {
            self.transitive.insert(child, change);
        }
    }

    fn transitively_reject(&mut self, child: git2::Oid, rejected_ancestor: git2::Oid) {
        self.transitive.insert(child, rejected_ancestor);
    }
}

pub struct Evaluating<'a, I: IdentityStorage> {
    identities: &'a I,
    authorizing_identity: &'a dyn AuthorizingIdentity,
    repo: &'a git2::Repository,
    rejected: RejectedChanges,
    in_progress_history: ValidatedAutomerge,
}

impl<'a, I: IdentityStorage> Evaluating<'a, I> {
    pub fn new(
        identities: &'a I,
        authorizer: &'a dyn AuthorizingIdentity,
        repo: &'a git2::Repository,
        schema: Schema,
    ) -> Evaluating<'a, I> {
        Evaluating {
            identities,
            authorizing_identity: authorizer,
            repo,
            rejected: RejectedChanges::new(),
            in_progress_history: ValidatedAutomerge::new(schema),
        }
    }

    pub fn evaluate<'b, It: Iterator<Item = (&'b Change, Vec<git2::Oid>)>>(
        mut self,
        items: It,
    ) -> ValidatedAutomerge {
        for (change, child_commits) in items {
            // There can be multiple paths to a change so in a topological traversal we
            // might encounter a change which we have already rejected
            // previously
            if self.rejected.is_rejected(change) {
                continue;
            }
            if let Some(rejected_ancestor) = self.rejected.rejected_ancestor(change) {
                tracing::warn!(commit=?change.commit(), ?rejected_ancestor, "rejecting change because an ancestor change was rejected");
                for child in child_commits {
                    self.rejected.transitively_reject(child, rejected_ancestor);
                }
                continue;
            }
            if let Some(reason) = self.evaluate_change(change) {
                self.rejected
                    .directly_reject(change.commit(), child_commits);
                match reason {
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
                    RejectionReason::InvalidChange(error) => {
                        tracing::warn!(
                            err=?error,
                            "rejecting invalid change"
                        );
                    },
                }
            } else {
                tracing::trace!(commit=?change.commit(), "change accepted");
            }
        }
        self.in_progress_history
    }

    fn evaluate_change(&mut self, change: &Change) -> Option<RejectionReason> {
        // Check the change signatures are valid
        if !change.valid_signatures() {
            return Some(RejectionReason::InvalidSignatures);
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
                return Some(RejectionReason::InvalidAuthorizer(Box::new(e)));
            },
        };
        if referenced_auth_identity.urn() != self.authorizing_identity.urn() {
            return Some(RejectionReason::WrongAuthorizer);
        }

        // Check that the authorizing identity allows this change
        match lookup_person(self.repo, change.author_commit()) {
            Ok(Some(author)) => match referenced_auth_identity.check_authorization(&author) {
                AuthDecision::Authorized => {},
                AuthDecision::NotAuthorized { reason } => {
                    return Some(RejectionReason::Unauthorized { reason });
                },
            },
            Ok(None) => {
                return Some(RejectionReason::MissingAuthor {
                    missing_author_oid: change.author_commit(),
                });
            },
            Err(e) => {
                return Some(RejectionReason::ErrorFindingAuthor {
                    author_commit_oid: change.author_commit(),
                    error: Box::new(e),
                });
            },
        };

        // Check that the history the change carries is well formed and does not violate
        // the schema
        match &change.history() {
            History::Automerge(bytes) => match self.in_progress_history.propose_change(bytes) {
                Ok(()) => {},
                Err(e) => {
                    return Some(RejectionReason::InvalidChange(e));
                },
            },
        };

        None
    }
}
