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

use std::{env, fs::File, io, path::Path, process::Command};

use serde::{de::DeserializeOwned, Serialize};
use serde_yaml as yaml;
use tempfile::{self, NamedTempFile};
use thiserror::Error;

const DEFAULT_EDITOR: &str = "nano";

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Yaml(#[from] yaml::Error),

    #[error(transparent)]
    Persist(#[from] tempfile::PersistError),
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
                // Try to create the temp file next to the target file, so we don't run into
                // issues if dest is on another device
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
        },
        Err(e) => {
            println!("{}", e);
            println!("Press ENTER to try again");
            let mut buf = String::with_capacity(1);
            io::stdin().read_line(&mut buf).unwrap();
            edit_yaml(data, store_at)
        },
    }
}
