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

#![feature(str_strip)]

use std::{
    env,
    io,
    path::{Path, PathBuf},
};

use librad::{
    git::local::{
        transport::{LocalTransport, Localio, Mode::Stateful, Settings},
        url::LocalUrl,
    },
    keys::{PublicKey, SecretKey},
    paths::Paths,
};
use radicle_git_helpers::credential;
use radicle_keystore::{crypto::Pwhash, FileStorage, Keystore};

// FIXME: this should be defined elsewhere to be consistent between applications
const SECRET_KEY_FILE: &str = "librad.key";

fn main() -> anyhow::Result<()> {
    let url = {
        let args = env::args().skip(1).take(2).collect::<Vec<_>>();
        args[0]
            .parse()
            .or_else(|_| args[1].parse())
            .or_else(|_| Err(anyhow::anyhow!("invalid args: {:?}", args)))
    }?;

    let git_dir = env::var("GIT_DIR").map(PathBuf::from)?;

    let mut transport = {
        let paths = Paths::from_env()?;
        let key = get_signer(&git_dir, paths.keys_dir(), &url)?;
        LocalTransport::new(Settings { paths, signer: key })
    }?;

    loop {
        let mut buf = String::with_capacity(32);
        io::stdin().read_line(&mut buf)?;
        let line = buf.trim();

        if line == "capabilities" {
            println!("connect\n\n");
            continue;
        }

        if let Some(service) = line.strip_prefix("connect ") {
            let service = match service {
                "git-upload-pack" => Ok(git2::transport::Service::UploadPack),
                "git-receive-pack" => Ok(git2::transport::Service::ReceivePack),
                unknown => Err(anyhow::anyhow!("unknown service: {}", unknown)),
            }?;

            println!();

            transport
                .connect(url, service, Stateful, unsafe { Localio::native() })?
                .wait()?;

            break;
        }

        return Err(anyhow::anyhow!("unexpected command: {}", line));
    }

    Ok(())
}

fn get_signer(git_dir: &Path, keys_dir: &Path, url: &LocalUrl) -> anyhow::Result<SecretKey> {
    let pass = credential::Git::new(git_dir).get(url)?;
    let file = keys_dir.join(SECRET_KEY_FILE);
    let keystore = FileStorage::<_, PublicKey, _, _>::new(&file, Pwhash::new(pass));
    keystore
        .get_key()
        .map(|keypair| keypair.secret_key)
        .map_err(|e| e.into())
}
