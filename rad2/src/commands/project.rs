use std::{env, io, path::PathBuf, time::SystemTime};

use failure::Fail;
use git2;
use serde_yaml;
use structopt::StructOpt;

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
    error::Error,
};

#[derive(StructOpt)]
/// Manage projects
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
    pub fn run<K>(self, cfg: Config<K>) -> Result<(), Error<K::Error>>
    where
        K: keystore::Storage<
            PublicKey = device::PublicKey,
            SecretKey = device::Key,
            Metadata = SystemTime,
        >,
        K::Error: Send + Sync,
    {
        match self {
            Self::Init {
                name,
                profile,
                git_dir,
            } => {
                let key = cfg.keystore.get_key().map_err(Error::Keystore)?.secret_key;
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

fn init_project<E: Fail>(
    paths: &Paths,
    key: device::Key,
    project_name: Option<String>,
    profile: meta::UserProfile,
    git_dir: Option<PathBuf>,
) -> Result<(), Error<E>> {
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

    let contrib_meta = {
        let mut contrib = meta::Contributor::new();
        contrib.profile = Some(meta::ProfileRef::UserProfile(profile));
        contrib
    };

    let proj =
        GitProject::builder(&project_name, &key, contrib_meta).init_project(paths, &sources)?;
    println!("Successfully initialised project: {}", proj);

    Ok(())
}
