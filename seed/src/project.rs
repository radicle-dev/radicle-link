// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::HashSet;

use librad::{meta::entity, net::peer::PeerApi, uri::RadUrn};

use crate::{Error, Signer};

#[derive(Debug, Clone)]
pub struct Project {
    pub urn: RadUrn,
    pub name: String,
    pub description: Option<String>,
    pub maintainers: HashSet<RadUrn>,
}

/// Get all local projects.
pub async fn get_projects(api: &PeerApi<Signer>) -> Result<Vec<Project>, Error> {
    api.with_storage(|s| {
        let projs = s
            .all_metadata()?
            .flat_map(|meta| {
                let meta = meta.ok()?;

                meta.try_map(|info| match info {
                    entity::data::EntityInfo::Project(info) => Some(info),
                    _ => None,
                })
                .map(|meta| Project {
                    urn: meta.urn(),
                    name: meta.name().to_owned(),
                    description: meta.description().to_owned(),
                    maintainers: meta.maintainers().clone(),
                })
            })
            .collect::<Vec<Project>>();

        Ok(projs)
    })
    .await?
}
