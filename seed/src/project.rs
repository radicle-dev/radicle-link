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

use librad::{
    git::{
        identities::{self, SomeIdentity},
        Urn,
    },
    net::peer::PeerApi,
};

use crate::{Error, Signer};

#[derive(Debug, Clone)]
pub struct Project {
    pub urn: Urn,
    pub name: String,
    pub description: Option<String>,
    pub maintainers: HashSet<Urn>,
}

impl From<identities::Project> for Project {
    fn from(proj: identities::Project) -> Self {
        Self {
            urn: proj.urn(),
            name: proj.subject().name.to_string(),
            description: proj
                .doc
                .payload
                .subject
                .description
                .map(|desc| desc.to_string()),
            maintainers: proj
                .doc
                .delegations
                .into_iter()
                .indirect()
                .map(|id| id.urn())
                .collect(),
        }
    }
}

/// Get all local projects.
pub async fn get_projects(api: &PeerApi<Signer>) -> Result<Vec<Project>, Error> {
    api.with_storage(|s| {
        identities::any::list(&s)?
            .filter_map(|res| {
                res.map(|id| match id {
                    SomeIdentity::Project(proj) => Some(Project::from(proj)),
                    _ => None,
                })
                .map_err(Error::from)
                .transpose()
            })
            .collect::<Result<Vec<_>, _>>()
    })
    .await?
}
