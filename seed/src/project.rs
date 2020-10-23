// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.
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
