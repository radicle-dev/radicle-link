// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::HashSet;

use librad::{
    git::{
        identities::{self, SomeIdentity},
        Urn,
    },
    net::peer::PeerApi,
};

use crate::Error;

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
pub async fn get_projects(api: &PeerApi) -> Result<Vec<Project>, Error> {
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
