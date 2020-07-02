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
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};

use librad::git::local::{self, url::LocalUrl};
use radicle_keystore::pinentry::SecUtf8;

pub fn git_credential(git_dir: &Path, url: &LocalUrl) -> io::Result<SecUtf8> {
    let mut child = Command::new("git")
        .envs(::std::env::vars().filter(|(key, _)| key.starts_with("GIT_TRACE")))
        .current_dir(git_dir)
        .args(&["credential", "fill"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    {
        let stdin = child.stdin.as_mut().expect("could not obtain stdin");
        stdin.write_all(
            format!(
                "protocol={}\nhost={}\nusername=radicle\n\n",
                local::URL_SCHEME,
                url.repo()
            )
            .as_bytes(),
        )?;
    }

    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let passphrase = stdout
        .lines()
        .find_map(|line| line.strip_prefix("password=").map(SecUtf8::from));

    passphrase.ok_or_else(no_pass_err)
}

// FIXME: this should be equivalent to `git_credential`, but without shelling
// out. Doesn't seem to work, tho.
pub fn credential_helper(config: &git2::Config, url: &LocalUrl) -> io::Result<SecUtf8> {
    let mut helper = git2::CredentialHelper::new(&url.to_string());
    helper.username(Some("radicle"));
    helper.config(config);

    helper
        .execute()
        .ok_or_else(no_pass_err)
        .map(|(_, pass)| SecUtf8::from(pass))
}

fn no_pass_err() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "couldn't obtain passphrase")
}
