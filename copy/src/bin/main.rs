// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom as _,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use argh::FromArgs;

use librad::{
    git::{
        identities::{self, local},
        storage::Storage,
        Urn,
    },
    git_ext::{OneLevel, RefLike},
    internal::canonical::Cstring,
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

const SECRET_KEY_FILE: &str = "librad.key";

/// Management of Radicle projects and their working-copies.
///
/// This tools allows you to create projects in your Radicle store and manage
/// the remotes for their working copies.
#[derive(Debug, FromArgs)]
pub struct Args {
    #[argh(subcommand)]
    command: Command,
}

#[derive(Debug, FromArgs)]
#[argh(subcommand)]
pub enum Command {
    Existing(Existing),
    New(New),
    Update(Update),
}

/// 🆙 Update the remotes that exist in the include file for the given project
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "include-update")]
pub struct Update {
    /// the project's URN we are interested in
    #[argh(option, from_str_fn(Urn::try_from))]
    urn: Urn,
}

/// 🆕 Creates a fresh, new Radicle project in the provided directory and using
/// the provided name. The final directory must not already exist, i.e.
/// <path>/<name> should not already exist.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "create-new")]
pub struct New {
    /// description of the project we are creating
    #[argh(option, from_str_fn(Cstring::from))]
    description: Option<Cstring>,
    /// the default branch name for the project
    #[argh(option, from_str_fn(Cstring::from))]
    default_branch: Cstring,
    /// the name of the project
    #[argh(option, from_str_fn(Cstring::from))]
    name: Cstring,
    /// the directory where we create the project
    #[argh(option)]
    path: PathBuf,
}

/// 🔄 Creates a new Radicle project using an existing git repository as the
/// working copy. The name of the project will be the last component of the
/// directory path, e.g. `~/Developer/radicle-link` will have the name
/// `radicle-link`. The git repository must already exist on your filesystem, if
/// it doesn't use the `new` command instead.
#[derive(Debug, FromArgs)]
#[argh(subcommand, name = "create-existing")]
pub struct Existing {
    /// description of the project we want to create
    #[argh(option, from_str_fn(Cstring::from))]
    description: Option<Cstring>,
    /// the default branch name for the project
    #[argh(option, from_str_fn(Cstring::from))]
    default_branch: Cstring,
    /// the directory of the existing git repository
    #[argh(option)]
    path: PathBuf,
}

fn main() -> anyhow::Result<()> {
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
            use radicle_copy::new::New;

            let default_branch = OneLevel::from(RefLike::try_from(default_branch.as_str())?);
            let raw = New::new(description, default_branch, name, path);
            let valid = New::validate(raw)?;
            let path = valid.path();
            let project = radicle_copy::init(paths, signer, &storage, whoami, valid)?;

            project_success(&project.urn(), path);
        },
        Command::Existing(Existing {
            description,
            default_branch,
            path,
            ..
        }) => {
            use radicle_copy::existing::Existing;

            let default_branch = OneLevel::from(RefLike::try_from(default_branch.as_str())?);
            let raw = Existing::new(description, default_branch, path.clone())?;
            let valid = Existing::validate(raw)?;
            let project = radicle_copy::init(paths, signer, &storage, whoami, valid)?;

            project_success(&project.urn(), path);
        },
        Command::Update(Update { urn }) => {
            let project = identities::project::get(&storage, &urn)?.ok_or_else(|| anyhow!(
                "the project URN `{}` does not exist, are you sure you passed in the right URN?", urn
            ))?;
            radicle_copy::include::update(&storage, &paths, &project)?;
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
