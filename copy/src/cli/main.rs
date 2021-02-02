// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom as _,
    path::{Path, PathBuf},
};

use anyhow::anyhow;

use librad::{
    git::{
        identities::{self, local},
        storage::Storage,
        Urn,
    },
    git_ext::{OneLevel, RefLike},
    keys::{PublicKey, SecretKey},
    paths::Paths,
    signer::{BoxedSigner, SomeSigner},
};
use radicle_keystore::{
    crypto::{self, Pwhash},
    pinentry::Prompt,
    FileStorage,
    Keystore,
};

use super::args::*;
use crate::{fork, init, include};

const SECRET_KEY_FILE: &str = "librad.key";

pub fn main() -> anyhow::Result<()> {
    let args: Args = argh::from_env();
    let paths = Paths::from_env()?;
    let signer = get_signer(paths.keys_dir())?;
    let storage = Storage::open(&paths, signer.clone())?;
    let whoami = local::default(&storage)?
        .ok_or_else(|| anyhow!("the default identity is not set for your Radicle store"))?;
    match args.command {
        Command::New(New {
            description,
            default_branch,
            name,
            path,
        }) => {
            use crate::new::New;

            let default_branch = OneLevel::from(RefLike::try_from(default_branch.as_str())?);
            let raw = New::new(description, default_branch, name, path);
            let valid = New::validate(raw)?;
            let path = valid.path();
            let project = init(paths, signer, &storage, whoami, valid)?;

            project_success(&project.urn(), path);
        },
        Command::Existing(Existing {
            description,
            default_branch,
            path,
            ..
        }) => {
            use crate::existing::Existing;

            let default_branch = OneLevel::from(RefLike::try_from(default_branch.as_str())?);
            let raw = Existing::new(description, default_branch, path.clone())?;
            let valid = Existing::validate(raw)?;
            let project = init(paths, signer, &storage, whoami, valid)?;

            project_success(&project.urn(), path);
        },
        Command::Fork(Fork { peer, urn, path }) => {
            fork(paths, signer, &storage, peer, path.clone(), &urn)?;
            println!("Your fork was created 🎉");
            println!("The working copy exists at `{}`", path.display());
        },
        Command::Update(Update { urn }) => {
            let project = identities::project::get(&storage, &urn)?.ok_or_else(|| anyhow!(
                "the project URN `{}` does not exist, are you sure you passed in the right URN?", urn
            ))?;
            include::update(&storage, &paths, &project)?;
        },
    };

    Ok(())
}

fn project_success(urn: &Urn, path: PathBuf) {
    println!("Your project was created 🎉");
    println!("The project's URN is `{}`", urn);
    println!("The working copy exists at `{}`", path.display());
}

fn get_signer(keys_dir: &Path) -> anyhow::Result<BoxedSigner> {
    let file = keys_dir.join(SECRET_KEY_FILE);
    let keystore = FileStorage::<_, PublicKey, _, _>::new(
        &file,
        Pwhash::new(
            Prompt::new("please enter your Radicle password: "),
            *crypto::KDF_PARAMS_PROD,
        ),
    );
    let key: SecretKey = keystore.get_key().map(|keypair| keypair.secret_key)?;

    Ok(SomeSigner { signer: key }.into())
}
