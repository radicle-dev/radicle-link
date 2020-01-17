use std::{env, io, path::PathBuf};

use failure::Fail;
use git2;
use serde_yaml;
use structopt::StructOpt;

use librad::{
    git::GitProject,
    keys::{
        device,
        storage::{FileStorage, Pinentry, Storage},
    },
    meta,
    paths::Paths,
    project::{Project, ProjectId},
};

use crate::{
    commands::profiles::{load_profile, ProfilePath},
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
        /// Name of the project. Defaults to the directory name of the source repository.
        #[structopt(short, long)]
        name: Option<String>,

        /// User profile to use for this project
        #[structopt(short, long)]
        profile: String,

        /// `.git` directory of the repository to initialise as a project. Discovered from the
        /// current directory by default.
        #[structopt(short, long)]
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
        Commands::Init {
            name,
            profile,
            git_dir,
        } => {
            let key =
                FileStorage::new(paths.clone()).get_device_key(pin("Unlock your key store:"))?;
            let profile = load_profile(ProfilePath::new(&paths, &profile))?;
            init_project(&paths, &key, name, profile, git_dir)
        }

        Commands::Show { project } => {
            let proj = Project::show(&paths, &project)?;
            serde_yaml::to_writer(io::stdout(), &proj).expect("I/O error");
            Ok(())
        }

        Commands::List => {
            for pid in Project::list(&paths) {
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
        GitProject::builder(&project_name, key, contrib_meta).init_project(paths, &sources)?;
    println!("Successfully initialised project: {}", proj);

    Ok(())
}
