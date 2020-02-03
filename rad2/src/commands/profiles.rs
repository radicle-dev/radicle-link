use std::{
    fs::{remove_file, File},
    io,
    path::PathBuf,
};

use failure::Fail;
use serde_yaml as yaml;
use structopt::StructOpt;

use librad::{
    meta::{EmailAddr, UserProfile},
    paths::Paths,
};
use radicle_keystore::Storage;

use crate::{config::Config, editor};

#[derive(StructOpt)]
/// Manage user profiles
pub enum Commands {
    /// Create a new profile
    New { name: String },
    /// Edit an existing profile
    Edit { name: String },
    /// Show an existing profile
    Show { name: String },
    /// Delete an existing profile
    Delete { name: String },
    /// List all profiles
    List,
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Profile `{}` already exists", 0)]
    AlreadyExists(String),

    #[fail(display = "Profile `{}` does not exist", 0)]
    DoesNotExist(String),

    #[fail(display = "{}", 0)]
    Editor(editor::Error),

    #[fail(display = "{}", 0)]
    Yaml(yaml::Error),

    #[fail(display = "{}", 0)]
    Io(io::Error),
}

impl From<editor::Error> for Error {
    fn from(err: editor::Error) -> Self {
        Self::Editor(err)
    }
}

impl From<yaml::Error> for Error {
    fn from(err: yaml::Error) -> Self {
        Self::Yaml(err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

// TODO: generalise this
pub struct ProfilePath {
    path: PathBuf,
    name: String,
}

impl ProfilePath {
    pub fn new(paths: &Paths, profile_name: &str) -> Self {
        Self {
            path: paths
                .profiles_dir()
                .join(profile_name)
                .with_extension("yaml"),
            name: profile_name.into(),
        }
    }

    pub fn must_exist(self) -> Result<Self, Error> {
        if !self.path.exists() {
            Err(Error::DoesNotExist(self.name))
        } else {
            Ok(self)
        }
    }

    pub fn must_not_exist(self) -> Result<Self, Error> {
        if self.path.exists() {
            Err(Error::AlreadyExists(self.name))
        } else {
            Ok(self)
        }
    }
}

impl Commands {
    pub fn run<K>(self, cfg: Config<K>) -> Result<(), Error>
    where
        K: Storage,
    {
        match self {
            Self::New { name } => create_profile(&cfg.paths, &name),
            Self::Edit { name } => edit_profile(&cfg.paths, &name),
            Self::Show { name } => show_profile(&cfg.paths, &name),
            Self::Delete { name } => delete_profile(&cfg.paths, &name),
            Self::List => list_profiles(&cfg.paths),
        }
    }
}

pub fn load_profile(path: ProfilePath) -> Result<UserProfile, Error> {
    let path = path.must_exist()?;
    yaml::from_reader(File::open(&path.path)?).map_err(|e| e.into())
}

fn create_profile(paths: &Paths, name: &str) -> Result<(), Error> {
    let path = ProfilePath::new(paths, name).must_not_exist()?;

    let profile = {
        let mut profile = UserProfile::new("anonymous");
        if let Ok(git_config) = git2::Config::open_default() {
            profile.name = git_config.get_string("user.name").ok();
            profile.email = git_config
                .get_string("user.email")
                .ok()
                .as_ref()
                .and_then(|addr| EmailAddr::parse(addr).ok());
        }

        profile
    };

    let _ = editor::edit_yaml(profile, Some(path.path))?;
    Ok(())
}

fn edit_profile(paths: &Paths, name: &str) -> Result<(), Error> {
    let path = ProfilePath::new(paths, name);
    let target = path.path.clone();
    let orig = load_profile(path)?;
    let _ = editor::edit_yaml(orig, Some(target))?;
    Ok(())
}

fn show_profile(paths: &Paths, name: &str) -> Result<(), Error> {
    let path = ProfilePath::new(paths, name).must_exist()?;
    let _ = io::copy(&mut File::open(&path.path)?, &mut io::stdout())?;
    Ok(())
}

fn delete_profile(paths: &Paths, name: &str) -> Result<(), Error> {
    let path = ProfilePath::new(paths, name).must_exist()?;
    remove_file(path.path).map_err(|e| e.into())
}

fn list_profiles(paths: &Paths) -> Result<(), Error> {
    for entry in paths.profiles_dir().read_dir()? {
        let entry = entry?;
        let path = entry.path();
        match path.extension() {
            Some(ext) if ext == "yaml" => println!(
                "{}",
                path.file_stem().unwrap().to_string_lossy().to_string()
            ),
            _ => {},
        }
    }
    Ok(())
}
