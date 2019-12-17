use std::env;
use std::io;
use std::path::PathBuf;

use failure::Fail;
use git2;
use serde_yaml;
use structopt::StructOpt;

use librad::git::GitProject;
use librad::keys::device;
use librad::keys::storage::{FileStorage, Pinentry, Storage};
use librad::meta::profile::UserProfile;
use librad::paths::Paths;
use librad::project::{list_projects, show_project, ProjectId};

use crate::commands::profiles::{load_profile, ProfilePath};
use crate::error::Error;

#[derive(StructOpt)]
/// Manage projects
pub enum Commands {
    /// List available projects
    List,
    /// Display information about project <project>
    Show { project: ProjectId },
    /// Initialise a new project
    Init {
        #[structopt(short, long)]
        /// User profile to use for this project
        profile: String,

        #[structopt(short, long)]
        /// `.git` directory of the repository to initialise as a project. Discovered from the
        /// current directory by default.
        git_dir: Option<PathBuf>,
    },
    /// Update project <project>
    Update { project: ProjectId },
}

pub fn run<F, P>(paths: Paths, cmd: Commands, verbose: bool, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    match cmd {
        Commands::Init { profile, git_dir } => {
            let key =
                FileStorage::new(paths.clone()).get_device_key(pin("Unlock your key store:"))?;
            let profile = load_profile(ProfilePath::new(&paths, &profile))?;
            init_project(&paths, &key, &profile, git_dir)
        }

        Commands::Show { project } => {
            let proj = show_project(&paths, &project)?;
            serde_yaml::to_writer(io::stdout(), &proj).expect("I/O error");
            Ok(())
        }

        Commands::List => {
            for pid in list_projects(&paths) {
                if verbose {
                    println!("{} ({:?})", pid, paths.projects_dir().join(pid.to_string()))
                } else {
                    println!("{}", pid)
                }
            }
            Ok(())
        }

        Commands::Update { .. } => unimplemented!(),
    }
}

fn init_project<E: Fail>(
    paths: &Paths,
    key: &device::Key,
    profile: &UserProfile,
    git_dir: Option<PathBuf>,
) -> Result<(), Error<E>> {
    let cwd = env::current_dir()?;
    let sources = match git_dir {
        Some(dir) => git2::Repository::open(dir)?,
        None => git2::Repository::open(&cwd)?,
    };
    let proj = GitProject::init(paths, key, profile, &sources)?;
    println!("Successfully initialised project: {}", proj);

    Ok(())
}
