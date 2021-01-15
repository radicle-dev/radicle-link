// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    env,
    io,
    path::{Path, PathBuf},
};

use crate::credential;
use librad::{
    git::local::{
        transport::{CanOpenStorage, LocalTransport, Localio, Mode::Stateful, Settings},
        url::LocalUrl,
    },
    keys::{PublicKey, SecretKey},
    paths::Paths,
    signer::{BoxedSigner, SomeSigner},
};
use radicle_keystore::{
    crypto::{self, Pwhash},
    FileStorage,
    Keystore,
};

// FIXME: this should be defined elsewhere to be consistent between applications
const SECRET_KEY_FILE: &str = "librad.key";

pub fn run() -> anyhow::Result<()> {
    let url = {
        let args = env::args().skip(1).take(2).collect::<Vec<_>>();
        args[0]
            .parse()
            .or_else(|_| args[1].parse())
            .map_err(|_| anyhow::anyhow!("invalid args: {:?}", args))
    }?;

    let git_dir = env::var("GIT_DIR").map(PathBuf::from)?;

    let mut transport = {
        let paths = Paths::from_env()?;
        let signer = get_signer(&git_dir, paths.keys_dir(), &url)?;
        let settings: Box<dyn CanOpenStorage> = Box::new(Settings { paths, signer });
        Ok::<_, anyhow::Error>(LocalTransport::from(settings))
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
                .connect(url, service, Stateful, Localio::inherit())?
                .wait()?;

            break;
        }

        return Err(anyhow::anyhow!("unexpected command: {}", line));
    }

    Ok(())
}

fn get_signer(git_dir: &Path, keys_dir: &Path, url: &LocalUrl) -> anyhow::Result<BoxedSigner> {
    let mut cred = credential::Git::new(git_dir);
    let pass = cred.get(url)?;
    let file = keys_dir.join(SECRET_KEY_FILE);
    let keystore = FileStorage::<_, PublicKey, _, _>::new(
        &file,
        Pwhash::new(pass.clone(), *crypto::KDF_PARAMS_PROD),
    );
    let key: SecretKey = keystore.get_key().map(|keypair| keypair.secret_key)?;
    cred.put(url, pass)?;

    Ok(SomeSigner { signer: key }.into())
}
