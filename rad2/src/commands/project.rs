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

use std::{env, io, path::PathBuf, time::SystemTime};

use anyhow::Error;
use git2;
use serde_yaml;
use structopt::StructOpt;

use keystore::Keystore;
use librad::{
    git::GitProject,
    keys::device,
    meta,
    paths::Paths,
    project::{Project, ProjectId},
};

use crate::{
    commands::profiles::{load_profile, ProfilePath},
    config::Config,
};

/// Manage projects
#[derive(StructOpt)]
pub enum Commands {
    /// List available projects
    List,
    /// Display information about project <project>
    Show { project: ProjectId },
    /// Initialise a new project
    Init {
        /// Name of the project. Defaults to the directory name of the source
        /// repository.
        #[structopt(short, long)]
        name: Option<String>,

        /// User profile to use for this project
        #[structopt(short, long)]
        profile: String,

        /// `.git` directory of the repository to initialise as a project.
        /// Discovered from the current directory by default.
        #[structopt(short, long)]
        git_dir: Option<PathBuf>,
    },
    /// Update project <project>
    Update { project: ProjectId },
}

impl Commands {
    pub fn run<K>(self, cfg: Config<K>) -> Result<(), Error>
    where
        K: Keystore<PublicKey = device::PublicKey, SecretKey = device::Key, Metadata = SystemTime>,
        K::Error: Send + Sync + 'static,
    {
        match self {
            Self::Init {
                name,
                profile,
                git_dir,
            } => {
                let key = cfg.keystore.get_key()?.secret_key;
                let profile = load_profile(ProfilePath::new(&cfg.paths, &profile))?;
                init_project(&cfg.paths, key, name, profile, git_dir)
            },

            Self::Show { project } => {
                let proj = Project::show(&cfg.paths, &project)?;
                serde_yaml::to_writer(io::stdout(), &proj).expect("I/O error");
                Ok(())
            },

            Self::List => {
                for pid in Project::list(&cfg.paths) {
                    if cfg.verbose {
                        println!(
                            "{} ({:?})",
                            pid,
                            cfg.paths.projects_dir().join(pid.to_string())
                        )
                    } else {
                        println!("{}", pid)
                    }
                }
                Ok(())
            },

            Self::Update { .. } => unimplemented!(),
        }
    }
}

fn init_project(
    paths: &Paths,
    key: device::Key,
    project_name: Option<String>,
    profile: meta::UserProfile,
    git_dir: Option<PathBuf>,
) -> Result<(), Error> {
    let cwd = env::current_dir()?;
    let sources = match git_dir {
        Some(dir) => git2::Repository::open(dir)?,
        None => git2::Repository::open(&cwd)?,
    };

    // Guess the project name if none was given
    let project_name = project_name.unwrap_or({
        let src_path = sources.path();
        let file_name = if src_path.ends_with(".git") {
            src_path.parent().and_then(|parent| parent.file_name())
        } else {
            src_path.file_name()
        };
        file_name.map(|f| f.to_string_lossy().to_string()).unwrap()
    });

    let contrib_meta = meta::UserData::default().set_profile(profile).build()?;

    let proj =
        GitProject::builder(&project_name, &key, contrib_meta).init_project(paths, &sources)?;
    println!("Successfully initialised project: {}", proj);

    Ok(())
}
