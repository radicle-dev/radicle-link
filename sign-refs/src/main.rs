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

use std::{env, path::Path};

use radicle_keystore::{crypto, pinentry, FileStorage, Keystore};

use librad::{
    git::refs::Refs,
    keys::{PublicKey, SecretKey},
    paths::Paths,
    signer::{BoxedSigner, SomeSigner},
};

const SECRET_KEY_FILE: &str = "librad.key";

fn get_signer(keys_dir: &Path) -> anyhow::Result<BoxedSigner> {
    let file = keys_dir.join(SECRET_KEY_FILE);
    let prompt = crypto::Pwhash::new(
        pinentry::Prompt::new("Please enter your passphrase: "),
        *crypto::KDF_PARAMS_PROD,
    );
    let keystore = FileStorage::<_, PublicKey, _, _>::new(&file, prompt);
    let key: SecretKey = keystore.get_key().map(|keypair| keypair.secret_key)?;

    Ok(SomeSigner { signer: key }.into())
}

fn parse_args() -> anyhow::Result<Refs> {
    let args = env::args().skip(1).take(2).collect::<Vec<_>>();
    serde_json::from_str(&args[0]).map_err(|err| {
        anyhow::anyhow!(
            "invalid args: {:?}, expected a 'Refs' JSON\nError: {}",
            args,
            err
        )
    })
}

fn main() -> anyhow::Result<()> {
    let refs = parse_args()?;
    let paths = Paths::new()?;
    let signer = get_signer(paths.keys_dir())?;
    let signed = refs.sign(&signer)?;

    println!("{}", serde_json::to_string(&signed)?);
    Ok(())
}
