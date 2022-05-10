// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::marker::PhantomData;

use either::Either;

use crate::{error, ids, Applied, PeerId, Update, Updated};

#[derive(Debug)]
pub struct Success<Urn> {
    pub applied: Applied<'static>,
    pub tracked: Vec<Either<PeerId, Urn>>,
    pub requires_confirmation: bool,
    pub validation: Vec<error::Validation>,
    pub(crate) _marker: PhantomData<Urn>,
}

impl<Urn> Success<Urn>
where
    Urn: ids::Urn,
{
    /// All refs which have been created or updated as a result of the
    /// replication run.
    pub fn updated_refs(&self) -> &[Updated] {
        &self.applied.updated
    }

    /// Ref updates which have been rejected, eg. due to not being fast-forwards
    /// when required.
    pub fn rejected_updates(&self) -> &[Update<'static>] {
        &self.applied.rejected
    }

    /// New tracking relationships which have been established as a result of
    /// the replication run.
    ///
    /// New trackings are established when new delegations or `refs/rad/self`
    /// are discovered.
    pub fn tracked(&self) -> &[Either<PeerId, Urn>] {
        &self.tracked
    }

    /// Top-level URNs created as a result of the replication run.
    ///
    /// This happens due to new `refs/rad/ids/*` being discovered, which are
    /// tracked automatically.
    pub fn urns_created(&self) -> impl Iterator<Item = Urn> + '_ {
        use git_ref_format::name::str::*;

        self.applied
            .updated
            .iter()
            .filter_map(|update| match update {
                Updated::Symbolic { target, .. } => {
                    let id = match target.iter().take(7).collect::<Vec<_>>()[..] {
                        [REFS, NAMESPACES, id, REFS, RAD, ID] => Some(id),
                        _ => None,
                    }?;
                    Urn::try_from_id(id).ok()
                },

                _ => None,
            })
    }

    /// Whether the identity for the replicated URN requires confirmation.
    ///
    /// `true` if the local peer is in the set of delegations, and another
    /// delegate has proposed an update.
    pub fn requires_confirmation(&self) -> bool {
        self.requires_confirmation
    }

    /// Any post-validation errors.
    pub fn validation_errors(&self) -> &[error::Validation] {
        &self.validation
    }
}
