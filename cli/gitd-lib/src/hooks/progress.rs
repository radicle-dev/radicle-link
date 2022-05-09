// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt;

use git_ref_format::{name, Component, RefString};
use librad::{
    git::Urn,
    net::{protocol::request_pull, replication},
};

use super::error;

pub(crate) struct Progress(String);

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for Progress {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for Progress {
    fn from(s: String) -> Self {
        Self(s)
    }
}

pub(crate) trait ProgressReporter {
    type Error;
    fn report(&mut self, progress: Progress)
        -> futures::future::BoxFuture<Result<(), Self::Error>>;
}

pub(crate) async fn report<
    E: std::error::Error + Send + 'static,
    P: ProgressReporter<Error = E>,
>(
    reporter: &mut P,
    msg: impl Into<Progress>,
) -> Result<(), error::Progress<E>> {
    reporter.report(msg.into()).await.map_err(error::Progress)
}

pub(super) struct Namespaced<'a, T> {
    urn: &'a Urn,
    payload: &'a T,
}

impl<'a, T> Namespaced<'a, T> {
    pub(super) fn new(urn: &'a Urn, payload: &'a T) -> Self {
        Self { urn, payload }
    }

    fn prefix(&self) -> RefString {
        name::REFS
            .join(name::NAMESPACES)
            .join(Component::from(self.urn))
    }
}

impl<'a> From<Namespaced<'a, request_pull::Success>> for Progress {
    fn from(ns: Namespaced<request_pull::Success>) -> Self {
        let mut progress = String::new();

        let prefix = ns.prefix();
        if !ns.payload.refs.is_empty() {
            progress.push_str("updated references:\n");
            for updated in &ns.payload.refs {
                let name = updated.name.strip_prefix(&prefix).unwrap_or(&updated.name);
                let target = updated.oid;
                progress.push_str(&format!("+{name}->{target}\n"))
            }
        }

        if !ns.payload.pruned.is_empty() {
            progress.push('\n');
            progress.push_str("pruned references:\n");
            for pruned in &ns.payload.pruned {
                let name = pruned.strip_prefix(&prefix).unwrap_or(pruned);
                progress.push_str(&format!("-{name}\n"));
            }
        }

        progress.into()
    }
}

impl<'a> From<Namespaced<'a, replication::Success>> for Progress {
    fn from(ns: Namespaced<'a, replication::Success>) -> Self {
        use either::Either;
        use link_replication::{SymrefTarget, Update, Updated};

        let s = ns.payload;
        let prefix = ns.prefix();
        let mut progress = String::new();

        let updates = s.updated_refs();
        if !updates.is_empty() {
            progress.push_str("updated references:\n");
            for updated in updates {
                let update = match updated {
                    Updated::Direct { name, target } => {
                        let name = name.strip_prefix(&prefix).unwrap_or(name);
                        format!("+{name}->{target}\n")
                    },
                    Updated::Symbolic { name, target } => {
                        let name = name.strip_prefix(&prefix).unwrap_or(name);
                        format!("+{name}->{target}\n")
                    },
                    Updated::Prune { name } => {
                        let name = name.strip_prefix(&prefix).unwrap_or(name);
                        format!("-{name}\n")
                    },
                };
                progress.push_str(&update);
            }
        }

        let rejections = s.rejected_updates();
        if !rejections.is_empty() {
            progress.push('\n');
            progress.push_str("rejected updates:\n");
            for rejection in rejections {
                let rejection = match rejection {
                    Update::Direct { name, target, .. } => {
                        let name = name.strip_prefix(&prefix).unwrap_or(name);
                        format!("+{name}->{target}\n")
                    },
                    Update::Symbolic {
                        name,
                        target: SymrefTarget { target, .. },
                        ..
                    } => {
                        let name = name.strip_prefix(&prefix).unwrap_or(name);
                        format!("+{name}->{target}\n")
                    },
                    Update::Prune { name, .. } => {
                        let name = name.strip_prefix(&prefix).unwrap_or(name);
                        format!("-{name}\n")
                    },
                };
                progress.push_str(&rejection);
            }
        }

        let trackings = s.tracked();
        if !trackings.is_empty() {
            progress.push('\n');
            progress.push_str("tracked peers:\n");
            for tracked in trackings {
                match tracked {
                    Either::Left(peer) => progress.push_str(&peer.to_string()),
                    Either::Right(urn) => progress.push_str(&urn.to_string()),
                }
            }
        }

        let errors = s.validation_errors();
        if !errors.is_empty() {
            progress.push('\n');
            progress.push_str("storage validation errors:\n");
            for error in errors {
                progress.push_str(&format!("{}\n", error));
            }
        }

        let urns = s.urns_created().collect::<Vec<_>>();
        if !urns.is_empty() {
            progress.push('\n');
            progress.push_str("new identities discovered:\n");
            for urn in urns {
                progress.push_str(&format!("{}\n", *urn));
            }
        }

        if s.requires_confirmation() {
            progress.push('\n');
            progress.push_str(
                "as a delegate of this identity, changes to the identity require review from you\n",
            )
        }

        progress.into()
    }
}
