use std::io;
use std::io::Write;

use failure;
use pgp::armor;
use pgp::constants::{Features, HashAlgorithm, KeyFlags, SignatureType};
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
use secstr::SecVec;
use time;

pub use pgp::packet::UserID;

pub struct Key(TPK);

pub struct Signature(Vec<u8>);

#[derive(Debug)]
pub enum Error {
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
    pub fn from_scalar<U: Into<packet::UserID>>(
        ed25519_scalar: SecVec<u8>,
        uid: U,
        creation_time: time::Tm,
    ) -> Result<Key, Error> {
        // Force primary key from our scalar
        let key4 = Key4::import_secret_ed25519(ed25519_scalar.unsecure(), creation_time)?;
        let key = packet::Key::from(key4);

        // Self-sign
        let sig = signature::Builder::new(SignatureType::DirectKey)
            .set_features(&Features::sequoia())?
            .set_key_flags(&KeyFlags::default().set_certify(true).set_sign(true))?
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

    pub fn export_public(&self, out: &mut dyn io::Write) -> Result<(), Error> {
        self.0.armored().export(out).map_err(|e| e.into())
    }

    pub fn export_private(&self, mut out: &mut dyn io::Write) -> Result<(), Error> {
        let mut armor = armor::Writer::new(&mut out, armor::Kind::SecretKey, &[])?;
        self.0.as_tsk().export(&mut armor).map_err(|e| e.into())
    }

    /// Import an ASCII-armored OpenPGP key.
    ///
    /// Note that this will import both public and private keys. Obviously, `sign` will fail if no
    /// private key material is known. Also note that we don't currently handle encrypted key
    /// material.
    pub fn import<D: AsRef<[u8]> + ?Sized>(key: &D) -> Result<Self, Error> {
        let tpk = pgp::TPK::from_bytes(key)?;
        Ok(Self(tpk))
    }

    pub fn fingerprint(&self) -> pgp::Fingerprint {
        self.0.fingerprint()
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
    fn test_export_import() {
        let device_key = device::Key::new();
        let pgp_key = device_key
            .as_pgp("leboeuf")
            .expect("Failed to obtain PGP key");

        let mut buf = Vec::new();
        pgp_key.export_public(&mut buf).expect("Export failed");
        let import = super::Key::import(&buf).expect("Import failed");
        assert_eq!(pgp_key.fingerprint(), import.fingerprint())
    }

    #[test]
    fn test_export_import_private() {
        let device_key = device::Key::new();
        let pgp_key = device_key
            .as_pgp("leboeuf")
            .expect("Failed to obtain PGP key");

        let mut buf = Vec::new();
        pgp_key.export_private(&mut buf).expect("Export failed");
        let import = super::Key::import(&buf).expect("Import failed");
        assert_eq!(pgp_key.fingerprint(), import.fingerprint())
    }

    #[test]
    fn test_export_public() {
        let device_key = device::Key::from_seed(&SEED, time::at(CREATED_AT));
        let pgp_key = device_key
            .as_pgp("leboeuf")
            .expect("Failed to obtain PGP key");

        let mut buf = Vec::new();
        pgp_key.export_public(&mut buf).expect("Export failed");

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
}
