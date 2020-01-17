use std::{env, fs::File, io, path::Path, process::Command};

use failure::Fail;
use serde::{de::DeserializeOwned, Serialize};
use serde_yaml as yaml;
use tempfile::{self, NamedTempFile};

const DEFAULT_EDITOR: &str = "nano";

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Editor error: {}", 0)]
    Io(io::Error),

    #[fail(display = "Editor error: {}", 0)]
    Yaml(yaml::Error),

    #[fail(display = "Editor error: {:?}", 0)]
    Persist(tempfile::PersistError),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<yaml::Error> for Error {
    fn from(err: yaml::Error) -> Self {
        Error::Yaml(err)
    }
}

impl From<tempfile::PersistError> for Error {
    fn from(err: tempfile::PersistError) -> Self {
        Error::Persist(err)
    }
}

pub fn edit_yaml<T, P>(data: T, store_at: Option<P>) -> Result<T, Error>
where
    T: Serialize + DeserializeOwned,
    P: AsRef<Path>,
{
    let result = {
        let tmp = NamedTempFile::new()?;
        yaml::to_writer(&tmp, &data)?;
        let tmp = tmp.into_temp_path();

        let editor = env::var_os("EDITOR").unwrap_or_else(|| DEFAULT_EDITOR.into());
        let mut proc = Command::new(editor).arg(&tmp).spawn()?;
        proc.wait()?;

        yaml::from_reader(File::open(tmp)?)
    };

    match result {
        Ok(updated) => {
            if let Some(dest) = store_at {
                // Try to create the temp file next to the target file, so we don't run into issues
                // if dest is on another device
                let tmpdir = dest
                    .as_ref()
                    .parent()
                    .map(|dir| dir.to_path_buf())
                    .unwrap_or_else(env::temp_dir);
                let tmp = NamedTempFile::new_in(&tmpdir)?;
                yaml::to_writer(&tmp, &updated)?;
                tmp.persist(dest)?;
            }
            Ok(updated)
        }
        Err(e) => {
            println!("{}", e);
            println!("Press ENTER to try again");
            let mut buf = String::with_capacity(1);
            io::stdin().read_line(&mut buf).unwrap();
            edit_yaml(data, store_at)
        }
    }
}
