use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;

#[derive(Clone)]
pub struct Paths(ProjectDirs);

impl Paths {
    pub fn new() -> Result<Self, io::Error> {
        let proj = ProjectDirs::from("xyz", "radicle", "radicle")
            .expect("Unable to determine application directories");
        Paths(proj).init()
    }

    // Don't use system paths, but the supplied directory as a root.
    //
    // For testing, you know.
    pub fn from_root(root: &Path) -> Result<Self, io::Error> {
        Paths(ProjectDirs::from_path(root.to_path_buf()).unwrap()).init()
    }

    pub fn keys_dir(&self) -> PathBuf {
        self.0.config_dir().join("keys")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.0.data_dir().join("projects")
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.0.config_dir().join("profiles")
    }

    fn init(self) -> Result<Self, io::Error> {
        fs::create_dir_all(self.keys_dir())?;
        fs::create_dir_all(self.projects_dir())?;
        fs::create_dir_all(self.profiles_dir())?;
        Ok(self)
    }
}
