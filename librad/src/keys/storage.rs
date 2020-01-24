use std::{
    convert::Infallible,
    fmt::Debug,
    fs::File,
    io,
    path::PathBuf,
    time::{Duration, SystemTime, SystemTimeError},
};

use failure::Fail;
use secstr::SecUtf8;
use serde::{Deserialize, Serialize};
use sodiumoxide::crypto::{pwhash, secretbox, sign};

use crate::{keys::device, paths::Paths};

#[derive(Debug, Fail)]
pub enum Error<P: Fail> {
    #[fail(display = "The key already exists")]
    KeyExists,

    #[fail(display = "Key not found")]
    NoSuchKey,

    #[fail(display = "Unable to retrieve key: Invalid salt")]
    InvalidSalt,

    #[fail(display = "Unable to retrieve key: Invalid nonce")]
    InvalidNonce,

    #[fail(display = "Unable to retrieve key: Invalid key")]
    InvalidKey,

    #[fail(display = "Unable to retrieve key: Invalid passphrase")]
    InvalidPassphrase,

    #[fail(display = "Unable to retrieve key: Invalid creation timestamp")]
    InvalidCreationTimestamp,

    #[fail(display = "Refusing to store key: creation timestamp is before UNIX epoch")]
    BackwardsTime(#[fail(cause)] SystemTimeError),

    #[fail(display = "{}", 0)]
    IoError(io::Error),

    #[fail(display = "{}", 0)]
    SerdeError(serde_cbor::error::Error),

    #[fail(display = "{}", 0)]
    PinentryError(P),
}

impl<T: Fail> From<io::Error> for Error<T> {
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}

impl<T: Fail> From<serde_cbor::error::Error> for Error<T> {
    fn from(err: serde_cbor::error::Error) -> Self {
        Self::SerdeError(err)
    }
}

impl<T: Fail> From<SystemTimeError> for Error<T> {
    fn from(err: SystemTimeError) -> Self {
        Self::BackwardsTime(err)
    }
}

pub trait Pinentry {
    type Error;

    fn get_passphrase(&self) -> Result<SecUtf8, Self::Error>;
}

impl Pinentry for SecUtf8 {
    type Error = Infallible;

    fn get_passphrase(&self) -> Result<SecUtf8, Infallible> {
        Ok(self.clone())
    }
}

pub trait Storage {
    fn put_device_key<F>(&mut self, k: &device::Key, pinentry: F) -> Result<(), Error<F::Error>>
    where
        F: Pinentry,
        F::Error: Fail;

    fn get_device_key<F>(&self, pinentry: F) -> Result<device::Key, Error<F::Error>>
    where
        F: Pinentry,
        F::Error: Fail;
}

#[derive(Default)]
pub struct MemoryStorage {
    device_key: Option<(device::Key, pwhash::HashedPassword)>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        MemoryStorage { device_key: None }
    }
}

impl Storage for MemoryStorage {
    fn put_device_key<F>(&mut self, k: &device::Key, pinentry: F) -> Result<(), Error<F::Error>>
    where
        F: Pinentry,
        F::Error: Fail,
    {
        match self.device_key {
            Some(_) => Err(Error::KeyExists),
            None => {
                let pass = pinentry.get_passphrase().map_err(Error::PinentryError)?;
                let pwhash = pwhash::pwhash(
                    pass.unsecure().as_bytes(),
                    pwhash::OPSLIMIT_INTERACTIVE,
                    pwhash::MEMLIMIT_INTERACTIVE,
                )
                .unwrap();
                self.device_key = Some((k.clone(), pwhash));
                Ok(())
            },
        }
    }

    fn get_device_key<F>(&self, pinentry: F) -> Result<device::Key, Error<F::Error>>
    where
        F: Pinentry,
        F::Error: Fail,
    {
        self.device_key
            .as_ref()
            .map_or(Err(Error::NoSuchKey), |(k, pwhash)| {
                let pass = pinentry.get_passphrase().map_err(Error::PinentryError)?;
                if pwhash::pwhash_verify(&pwhash, pass.unsecure().as_bytes()) {
                    Ok(k.clone())
                } else {
                    Err(Error::InvalidPassphrase)
                }
            })
    }
}

pub struct FileStorage {
    paths: Paths,
}

impl FileStorage {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    pub fn key_file_path(&self) -> PathBuf {
        self.paths.keys_dir().join("device.key")
    }
}

#[derive(Serialize, Deserialize)]
struct StorableKey {
    nonce: secretbox::Nonce,
    salt: pwhash::Salt,
    created_at: u64,
    sealed_key: Vec<u8>,
}

impl Storage for FileStorage {
    fn put_device_key<F>(&mut self, k: &device::Key, pinentry: F) -> Result<(), Error<F::Error>>
    where
        F: Pinentry,
        F::Error: Fail,
    {
        let file_path = self.key_file_path();

        if file_path.exists() {
            Err(Error::KeyExists)
        } else {
            let salt = pwhash::gen_salt();
            let nonce = secretbox::gen_nonce();
            let pass = pinentry.get_passphrase().map_err(Error::PinentryError)?;

            let deriv = derive_key(&salt, &pass);
            let sealed_key = secretbox::seal(k.as_ref(), &nonce, &deriv);

            let created_at = k
                .created_at()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs();

            let storable = StorableKey {
                nonce,
                salt,
                created_at,
                sealed_key,
            };

            let key_file = File::create(file_path)?;
            serde_cbor::to_writer(&key_file, &storable)?;
            key_file.sync_all()?;

            Ok(())
        }
    }

    fn get_device_key<F>(&self, pinentry: F) -> Result<device::Key, Error<F::Error>>
    where
        F: Pinentry,
        F::Error: Fail,
    {
        let file_path = self.key_file_path();

        if !file_path.exists() {
            Err(Error::NoSuchKey)
        } else {
            let key_file = File::open(file_path)?;
            let storable: StorableKey = serde_cbor::from_reader(key_file)?;
            let pass = pinentry.get_passphrase().map_err(Error::PinentryError)?;

            // Unseal key
            let deriv = derive_key(&storable.salt, &pass);
            let key_plain = secretbox::open(&storable.sealed_key, &storable.nonce, &deriv)
                .or(Err(Error::InvalidPassphrase))?;
            let key = sign::SecretKey::from_slice(&key_plain).ok_or(Error::InvalidKey)?;

            let created_at = SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(storable.created_at))
                .ok_or(Error::InvalidCreationTimestamp)?;

            Ok(device::Key::from_secret(key, created_at))
        }
    }
}

fn derive_key(salt: &pwhash::Salt, passphrase: &SecUtf8) -> secretbox::Key {
    let mut k = secretbox::Key([0; secretbox::KEYBYTES]);
    let secretbox::Key(ref mut kb) = k;
    pwhash::derive_key_interactive(kb, passphrase.unsecure().as_bytes(), salt)
        .expect("Key derivation failed");
    k
}

// ----------------------------------------------------------------------------

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::keys::device;
    use tempfile::tempdir;

    fn with_mem_store<F>(f: F)
    where
        F: FnOnce(MemoryStorage) -> (),
    {
        f(MemoryStorage::new())
    }

    fn with_fs_store<F>(f: F)
    where
        F: FnOnce(FileStorage) -> (),
    {
        let tmp = tempdir().expect("Can't get tempdir");
        let paths = Paths::from_root(tmp.path()).expect("Can't get paths");
        f(FileStorage::new(paths))
    }

    #[test]
    fn mem_get_after_put() {
        with_mem_store(get_after_put)
    }

    #[test]
    fn mem_put_twice() {
        with_mem_store(put_twice)
    }

    #[test]
    fn mem_get_empty() {
        with_mem_store(get_empty)
    }

    #[test]
    fn mem_passphrase_mismatch() {
        with_mem_store(passphrase_mismatch)
    }

    #[test]
    fn fs_get_after_put() {
        with_fs_store(get_after_put)
    }

    #[test]
    fn fs_put_twice() {
        with_fs_store(put_twice)
    }

    #[test]
    fn fs_get_empty() {
        with_fs_store(get_empty)
    }

    #[test]
    fn fs_passphrase_mismatch() {
        with_fs_store(passphrase_mismatch)
    }

    fn get_after_put<S: Storage>(mut store: S) {
        let key = device::Key::new();
        let pass = SecUtf8::from("asd");

        store
            .put_device_key(&key, pass.clone())
            .expect("Put failed");
        let res = store.get_device_key(pass).expect("Get failed");

        assert!(key == res, "Keys don't match")
    }

    fn put_twice<S: Storage>(mut store: S) {
        let key = device::Key::new();
        let pass = SecUtf8::from("asd");

        store
            .put_device_key(&key, pass.clone())
            .expect("Put failed");

        match store.put_device_key(&key, pass) {
            Err(Error::KeyExists) => (),
            Err(e) => panic!("Unexpected error: {:?}", e),
            _ => panic!("Second put should fail"),
        }
    }

    fn get_empty<S: Storage>(store: S) {
        match store.get_device_key(SecUtf8::from("asdf")) {
            Err(Error::NoSuchKey) => (),
            Err(e) => panic!("Unexpected error: {:?}", e),
            _ => panic!("Get on empty key store should fail"),
        }
    }

    fn passphrase_mismatch<S: Storage>(mut store: S) {
        let key = device::Key::new();

        store
            .put_device_key(&key, SecUtf8::from("right"))
            .expect("Put failed");

        match store.get_device_key(SecUtf8::from("wrong")) {
            Err(Error::InvalidPassphrase) => (),
            Err(e) => panic!("Unexpected error: {:?}", e),
            _ => panic!("Mismatched passphrase should fail"),
        }
    }
}
