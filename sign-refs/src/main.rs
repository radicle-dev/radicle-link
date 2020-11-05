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

use std::{env, io, path::Path};

use radicle_keystore::{crypto, pinentry, FileStorage, Keystore};

use librad::{
    git::refs::Refs,
    keys::{self, PublicKey, SecretKey},
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

fn parse_args() -> anyhow::Result<(bool, Refs)> {
    let args = env::args().skip(1).take(2).collect::<Vec<_>>();
    let stdin = if args[0] == "stdin" { true } else { false };
    serde_json::from_str(&args[1])
        .map_err(|err| {
            anyhow::anyhow!(
                "invalid args: {:?}, expected a 'Refs' JSON\nError: {}",
                args,
                err
            )
        })
        .map(|refs| (stdin, refs))
}

pub fn from_std_in<R: io::Read>(mut r: R) -> anyhow::Result<BoxedSigner> {
    use radicle_keystore::SecretKeyExt;

    let mut bytes = Vec::new();
    r.read_to_end(&mut bytes)?;

    let sbytes: keys::SecStr = bytes.into();
    match keys::SecretKey::from_bytes_and_meta(sbytes, &()) {
        Ok(key) => Ok(SomeSigner { signer: key }.into()),
        Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err).into()),
    }
}

fn main() -> anyhow::Result<()> {
    let (stdin, refs) = parse_args()?;

    let signer = if stdin {
        from_std_in(io::stdin())?
    } else {
        let paths = Paths::from_env()?;
        get_signer(paths.keys_dir())?
    };
    let signed = refs.sign(&signer)?;

    println!("{}", serde_json::to_string(&signed)?);
    Ok(())
}
