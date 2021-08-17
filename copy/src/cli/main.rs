// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::anyhow;

use librad::{
    crypto::{BoxedSigner, SomeSigner},
    git::{
        identities::{self, local},
        storage::Storage,
        Urn,
    },
    git_ext::{OneLevel, RefLike},
    profile::Profile,
    PublicKey,
    SecretKey,
};
use radicle_keystore::{
    crypto::{self, Pwhash},
    pinentry::Prompt,
    FileStorage,
    Keystore,
};

use super::args::{community, garden, Args, Command, Community, Garden};
use crate::{
    garden::{graft, plant, repot},
    include,
};

pub fn main() -> anyhow::Result<()> {
    let args: Args = argh::from_env();
    let profile = Profile::load()?;
    let paths = profile.paths();
    let signer = get_signer(paths.keys_dir(), args.key)?;
    let storage = Storage::open(paths, signer.clone())?;
    let whoami = local::default(&storage)?
        .ok_or_else(|| anyhow!("the default identity is not set for your Radicle store"))?;
    match args.command {
        Command::Garden(Garden { garden }) => match garden {
            garden::Options::Plant(garden::Plant {
                description,
                default_branch,
                name,
                path,
            }) => {
                use crate::garden::plant::Plant;

                let default_branch = OneLevel::from(RefLike::try_from(default_branch.as_str())?);
                let raw = Plant::new(description, default_branch, name, path);
                let valid = Plant::validate(raw)?;
                let path = valid.path();
                let project = plant(paths.clone(), signer, &storage, whoami, valid)?;

                project_success(&project.urn(), path);
            },
            garden::Options::Repot(garden::Repot {
                description,
                default_branch,
                path,
                ..
            }) => {
                use crate::garden::repot::Repot;

                let default_branch = OneLevel::from(RefLike::try_from(default_branch.as_str())?);
                let raw = Repot::new(description, default_branch, path.clone())?;
                let valid = Repot::validate(raw)?;
                let project = repot(paths.clone(), signer, &storage, whoami, valid)?;

                project_success(&project.urn(), path);
            },
            garden::Options::Graft(garden::Graft { peer, urn, path }) => {
                graft(paths.clone(), signer, &storage, peer, path.clone(), &urn)?;
                println!("Your working copy was created 🎉");
                println!("It exists at `{}`", path.display());
            },
        },
        Command::Community(Community { community }) => match community {
            community::Options::Update(community::Update { urn }) => {
                let project = identities::project::get(&storage, &urn)?.ok_or_else(|| anyhow!(
                "the project URN `{}` does not exist, are you sure you passed in the right URN?", urn
            ))?;
                include::update(&storage, paths, &project)?;
            },
        },
    };

    Ok(())
}

fn project_success(urn: &Urn, path: PathBuf) {
    println!("Your project was created 🎉");
    println!("The project's URN is `{}`", urn);
    println!("The working copy exists at `{}`", path.display());
}

fn get_signer<K>(keys_dir: &Path, key_file: Option<K>) -> anyhow::Result<BoxedSigner>
where
    K: AsRef<Path>,
{
    let file = match key_file {
        Some(file) => keys_dir.join(file),
        None => default_singer_file(keys_dir)?,
    };
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

fn default_singer_file(keys_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut keys = fs::read_dir(keys_dir)?;
    match keys.next() {
        None => Err(anyhow!(
            "No key was found in `{}`, have you initialised your key yet?",
            keys_dir.display()
        )),
        Some(key) => {
            if keys.next().is_some() {
                Err(anyhow!("Multiple keys were found in `{}`, you will have to specify which key you are using", keys_dir.display()))
            } else {
                Ok(key?.path())
            }
        },
    }
}
