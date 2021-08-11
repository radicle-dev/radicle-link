// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::marker::PhantomData;

use crate::{error, ids, refs, Applied, PeerId, Update, Updated};

#[derive(Debug)]
pub struct Success<Urn> {
    pub(crate) applied: Applied<'static>,
    pub(crate) tracked: Vec<(PeerId, Option<Urn>)>,
    pub(crate) requires_confirmation: bool,
    pub(crate) validation: Vec<error::Validation>,
    pub(crate) _marker: PhantomData<Urn>,
}

impl<Urn> Success<Urn>
where
    Urn: ids::Urn,
{
    pub fn updated_refs(&self) -> &[Updated] {
        &self.applied.updated
    }

    pub fn rejected_updates(&self) -> &[Update<'static>] {
        &self.applied.rejected
    }

    pub fn tracked(&self) -> &[(PeerId, Option<Urn>)] {
        &self.tracked
    }

    pub fn urns_created(&self) -> impl Iterator<Item = Urn> + '_ {
        use refs::component::*;

        self.applied
            .updated
            .iter()
            .filter_map(|update| match update {
                Updated::Symbolic { target, .. } => {
                    let id = match target.splitn(7, refs::is_separator).collect::<Vec<_>>()[..] {
                        [REFS, NAMESPACES, id, REFS, RAD, ID] => Some(id),
                        _ => None,
                    }?;
                    let id = std::str::from_utf8(id).ok()?;
                    Urn::try_from_id(id).ok()
                },

                _ => None,
            })
    }

    pub fn requires_confirmation(&self) -> bool {
        self.requires_confirmation
    }

    pub fn validation_errors(&self) -> &[error::Validation] {
        &self.validation
    }
}
