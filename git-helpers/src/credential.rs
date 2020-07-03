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

use std::{
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use keystore::pinentry::SecUtf8;
use librad::git::local::url::LocalUrl;

pub type Passphrase = SecUtf8;

pub trait Credential {
    fn get(&self, url: &LocalUrl) -> io::Result<Passphrase>;
    fn put(&mut self, url: &LocalUrl, passphrase: Passphrase) -> io::Result<()>;
}

pub struct Git {
    git_dir: PathBuf,
}

impl Git {
    pub fn new(git_dir: &Path) -> Self {
        Self {
            git_dir: git_dir.to_path_buf(),
        }
    }

    pub fn get(&self, url: &LocalUrl) -> io::Result<Passphrase> {
        let mut child = Command::new("git")
            .env("GIT_DIR", &self.git_dir)
            .envs(env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
            .args(&["credential", "fill"])
            .current_dir(&self.git_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        {
            let stdin = child.stdin.as_mut().expect("could not obtain stdin");
            stdin.write_all(format!("url={}\nusername=radicle\n\n", url).as_bytes())?;
        }

        let output = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let passphrase = stdout
            .lines()
            .find_map(|line| line.strip_prefix("password=").map(Passphrase::from));

        passphrase
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "couldn't obtain passphrase"))
    }

    pub fn put(&mut self, url: &LocalUrl, passphrase: Passphrase) -> io::Result<()> {
        let mut child = Command::new("git")
            .env("GIT_DIR", &self.git_dir)
            .envs(env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
            .args(&["credential", "approve"])
            .current_dir(&self.git_dir)
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.as_mut().expect("could not obtain stdin");
        stdin.write_all(
            format!(
                "url={}\nusername=radicle\npassword={}",
                url,
                passphrase.unsecure()
            )
            .as_bytes(),
        )?;

        Ok(())
    }
}

impl Credential for Git {
    fn get(&self, url: &LocalUrl) -> io::Result<Passphrase> {
        self.get(url)
    }

    fn put(&mut self, url: &LocalUrl, passphrase: Passphrase) -> io::Result<()> {
        self.put(url, passphrase)
    }
}
