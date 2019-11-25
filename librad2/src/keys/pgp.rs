use std::io;
use std::io::Write;

use failure;
use pgp::armor;
use pgp::constants::{Features, HashAlgorithm, KeyFlags, SignatureType};
use pgp::conversions::Time;
use pgp::packet;
use pgp::packet::key::Key4;
use pgp::packet::signature;
use pgp::parse::stream::{
    DetachedVerifier, MessageLayer, MessageStructure, VerificationHelper, VerificationResult,
};
use pgp::parse::Parse;
use pgp::serialize::stream;
use pgp::serialize::Serialize;
use pgp::tpk::TPK;
use sodiumoxide::crypto::sign::ed25519 as sodium;
use time;

pub use pgp::packet::UserID;

pub struct Key(TPK);

pub struct Signature(Vec<u8>);

#[derive(Debug)]
pub enum Error {
    NotATSK,
    PGPError(failure::Error),
    IoError(io::Error),
}

impl From<failure::Error> for Error {
    fn from(fail: failure::Error) -> Self {
        Error::PGPError(fail)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::IoError(err)
    }
}

impl Key {
    pub fn from_sodium<U: Into<packet::UserID>>(
        sodium: &sodium::SecretKey,
        uid: U,
        creation_time: time::Tm,
    ) -> Result<Key, Error> {
        let key = {
            // ACHTUNG: NaCl stores the public part in the second half of the secret key, so
            // we just extract the first 32 bytes. Relying on this is obviously leaky, but the best
            // we can do for now is to `use` the ed25519 module, so the chances are slimmer for
            // this to break in case libsodium decides to make some other signature scheme the
            // default.
            let scalar = &sodium.as_ref()[..32];
            let key4 = Key4::import_secret_ed25519(&scalar, creation_time)?;
            packet::Key::from(key4)
        };

        // Self-sign
        let sig = signature::Builder::new(SignatureType::DirectKey)
            .set_features(&Features::sequoia())?
            .set_key_flags(&KeyFlags::default().set_sign(true))?
            .set_signature_creation_time(creation_time)?
            .set_key_expiration_time(None)?
            .set_issuer_fingerprint(key.fingerprint())?
            .set_issuer(key.keyid())?
            .set_preferred_hash_algorithms(vec![HashAlgorithm::SHA512])?;

        let mut signer = key.clone().into_keypair()?;
        let sig = sig.sign_primary_key_binding(&mut signer, HashAlgorithm::SHA512)?;

        // Assemble TPK
        let mut packets = Vec::<pgp::Packet>::with_capacity(3);
        packets.push(pgp::Packet::PublicKey(key.mark_parts_public()));
        packets.push(sig.clone().into());

        let mut tpk = TPK::from_packet_pile(pgp::PacketPile::from(packets))?;

        // Sign user id
        let the_uid: packet::UserID = uid.into();
        let uid_sig_builder = signature::Builder::from(sig)
            .set_type(SignatureType::PositiveCertificate)
            .set_signature_creation_time(creation_time)?;
        let uid_sig = the_uid.bind(&mut signer, &tpk, uid_sig_builder, None, None)?;
        tpk = tpk.merge_packets(vec![the_uid.into(), uid_sig.into()])?;

        Ok(Key(tpk))
    }

    pub fn sign(&mut self, data: &[u8]) -> Result<Signature, Error> {
        // Set up armor writer
        let mut buf = Vec::new();
        let armor = armor::Writer::new(&mut buf, armor::Kind::Signature, &[])?;

        // Pull out signing keypair from TSK
        let mut keypair = self
            .0
            .primary()
            .clone()
            .mark_parts_secret()
            .into_keypair()?;

        let msg = stream::Message::new(armor);
        let mut signer = stream::Signer::detached(msg, vec![&mut keypair], None)?;
        signer.write_all(data)?;
        signer.finalize()?;

        Ok(Signature(buf))
    }

    pub fn verify(&self, sig: &Signature, data: &[u8]) -> Result<(), Error> {
        let helper = Helper(self);
        let mut verifier = DetachedVerifier::from_bytes(&sig.0, data, helper, None)?;
        io::copy(&mut verifier, &mut io::sink())?;
        Ok(())
    }

    pub fn export(&self, out: &mut dyn io::Write) -> Result<(), Error> {
        self.0.armored().export(out).map_err(|e| e.into())
    }

    /// Certify this key using the TSK read from the supplied `io::Read`.
    ///
    /// We don't want device keys to be stored elsewhere, yet want to enable PGP users to certify
    /// them. That is, make the device key a "subkey" of their primary identity key published to
    /// key servers _without_ actually storing the device key in the GPG keyring.
    ///
    /// To achieve this, we read the certifying _secret_ key (as obtained by `gpg --export-secret-keys
    /// --armor`), add this key as a subkey, and write a TPK which should be sent directly to
    /// keyservers.
    pub fn certify_with<R: io::Read, W: io::Write>(
        &self,
        tsk_reader: &mut R,
        tpk_writer: &mut W,
    ) -> Result<(), Error> {
        let mut tpk = pgp::TPK::from_reader(tsk_reader)?;
        if !tpk.is_tsk() {
            return Err(Error::NotATSK);
        }

        // Their primary key
        let primary = tpk.primary();
        let mut primary_signer = primary.clone().mark_parts_secret().into_keypair().unwrap();

        // Our key, to be used as a subkey
        let subkey = self
            .0
            .primary()
            .clone()
            .mark_parts_secret()
            .mark_role_secondary()
            .into_keypair()
            .unwrap();
        let mut subkey_signer = subkey.clone();

        let mut sig = signature::Builder::new(SignatureType::SubkeyBinding)
            .set_features(&Features::sequoia())?
            .set_key_flags(&KeyFlags::default().set_sign(true))?
            .set_key_expiration_time(None)?
            .set_preferred_hash_algorithms(vec![HashAlgorithm::SHA512])?;

        let backsig = signature::Builder::new(SignatureType::PrimaryKeyBinding)
            .set_signature_creation_time(time::now().canonicalize())?
            .set_issuer_fingerprint(self.fingerprint())?
            .set_issuer(self.keyid())?
            .sign_subkey_binding(
                &mut subkey_signer,
                &primary,
                &subkey.public(),
                HashAlgorithm::SHA512,
            )?;

        sig = sig.set_embedded_signature(backsig)?;

        let signature = subkey.clone().public().bind(
            &mut primary_signer,
            &tpk,
            sig,
            HashAlgorithm::SHA512,
            None,
        )?;

        tpk = tpk.merge_packets(vec![
            pgp::Packet::SecretSubkey(subkey.public().clone().mark_parts_secret()),
            signature.into(),
        ])?;

        tpk.armored().export(tpk_writer)?;
        Ok(())
    }

    pub fn fingerprint(&self) -> pgp::Fingerprint {
        self.0.fingerprint()
    }

    pub fn keyid(&self) -> pgp::KeyID {
        self.0.keyid()
    }
}

struct Helper<'a>(&'a Key);

impl<'a> VerificationHelper for Helper<'a> {
    fn get_public_keys(&mut self, _ids: &[pgp::KeyID]) -> pgp::Result<Vec<TPK>> {
        Ok(vec![(self.0).0.clone()])
    }

    fn check(&mut self, structure: &MessageStructure) -> pgp::Result<()> {
        // Copy & Pasta from
        // https://gitlab.com/sequoia-pgp/sequoia/blob/master/openpgp/examples/generate-sign-verify.rs

        let mut good = false;
        for (i, layer) in structure.iter().enumerate() {
            match (i, layer) {
                // First, we are interested in signatures over the
                // data, i.e. level 0 signatures.
                (0, MessageLayer::SignatureGroup { ref results }) => {
                    // Finally, given a VerificationResult, which only says
                    // whether the signature checks out mathematically, we apply
                    // our policy.
                    match results.get(0) {
                        Some(VerificationResult::GoodChecksum(..)) => good = true,
                        /*
                        Some(VerificationResult::NotAlive(..)) => {
                            return Err(failure::err_msg("Signature good, but not alive"))
                        }
                        */
                        Some(VerificationResult::MissingKey(_)) => {
                            return Err(failure::err_msg("Missing key to verify signature"))
                        }
                        Some(VerificationResult::BadChecksum(_)) => {
                            return Err(failure::err_msg("Bad signature"))
                        }
                        None => return Err(failure::err_msg("No signature")),
                    }
                }
                _ => return Err(failure::err_msg("Unexpected message structure")),
            }
        }

        if good {
            Ok(()) // Good signature.
        } else {
            Err(failure::err_msg("Signature verification failed"))
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::keys::device;

    use pgp::tpk;
    use sodiumoxide::crypto::sign::Seed;

    const SEED: Seed = Seed([
        20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81,
        181, 134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
    ]);

    const CREATED_AT: time::Timespec = time::Timespec {
        sec: 8734710,
        nsec: 0,
    };

    const DATA_TO_SIGN: &[u8] = b"ceci n'est pas un pipe";

    #[test]
    fn test_idempotency() {
        let device_key = device::Key::new();
        let pgp_one = device_key
            .as_pgp("leboeuf")
            .expect("Failed to obtain PGP key");
        let pgp_two = device_key
            .as_pgp("leboeuf")
            .expect("Failed to obtain PGP key");
        assert_eq!(pgp_one.fingerprint(), pgp_two.fingerprint())
    }

    #[test]
    fn test_sign_verify() -> Result<(), Error> {
        let device_key = device::Key::new();
        let mut pgp_key = device_key.as_pgp("leboeuf")?;
        let sig = pgp_key.sign(&DATA_TO_SIGN)?;
        pgp_key.verify(&sig, &DATA_TO_SIGN)
    }

    #[test]
    fn test_export() {
        let device_key = device::Key::from_seed(&SEED, time::at(CREATED_AT));
        let pgp_key = device_key
            .as_pgp("leboeuf")
            .expect("Failed to obtain PGP key");

        let mut buf = Vec::new();
        pgp_key.export(&mut buf).expect("Export failed");

        // Read armor
        let mut cursor = io::Cursor::new(&buf);
        let mut reader = armor::Reader::new(
            &mut cursor,
            armor::ReaderMode::Tolerant(Some(armor::Kind::PublicKey)),
        );

        // Extract the headers.
        let mut headers: Vec<&str> = reader
            .headers()
            .unwrap()
            .into_iter()
            .map(|header| {
                assert_eq!(&header.0[..], "Comment");
                &header.1[..]
            })
            .collect();
        headers.sort();

        let mut expected_headers = [
            "leboeuf@Gbsp8juYVbEWvvdFSreVLC98nS5JRXcVfkpZaiQYu9tW",
            "D97A F228 9757 4999 80E6  D4EA AAFE AD11 A3D5 43E4",
        ];
        expected_headers.sort();

        assert_eq!(&expected_headers[..], &headers[..]);
    }

    #[test]
    fn test_certify() -> Result<(), Error> {
        let device_key = device::Key::new();
        let pgp_key = device_key.as_pgp("leboeuf")?;
        let (certifier, _) = tpk::TPKBuilder::general_purpose(
            tpk::CipherSuite::Cv25519,
            UserID::from("leboeuf@acme.org").into(),
        )
        .generate()?;

        let mut cert_buf = Vec::new();
        certifier.as_tsk().export(&mut armor::Writer::new(
            &mut cert_buf,
            armor::Kind::SecretKey,
            &[],
        )?)?;

        let mut out = Vec::new();
        pgp_key.certify_with(&mut io::Cursor::new(&cert_buf), &mut out)?;

        let tpk = tpk::TPK::from_bytes(&out)?;
        tpk.keys_valid()
            .signing_capable()
            .map(|(_, _, key)| key)
            .filter(|key| key.fingerprint() == pgp_key.fingerprint())
            .nth(0)
            .ok_or(failure::err_msg("Key not certified").into())
            .map(|_| ())
    }
}
