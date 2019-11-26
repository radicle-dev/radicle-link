use std::convert::TryFrom;
use std::fmt;

use bs58;
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::keys::device;

pub const PEER_ID_PREFIX_ED25519: char = '0';

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerId(device::PublicKey);

impl From<device::PublicKey> for PeerId {
    fn from(pk: device::PublicKey) -> Self {
        Self(pk)
    }
}

impl From<device::Key> for PeerId {
    fn from(k: device::Key) -> Self {
        Self(k.public())
    }
}

impl Serialize for PeerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PeerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PeerIdVisitor;

        impl<'de> Visitor<'de> for PeerIdVisitor {
            type Value = PeerId;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "A PeerId")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_string(s.to_string())
            }

            fn visit_string<E>(self, s: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                PeerId::try_from(s).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_string(PeerIdVisitor)
    }
}

mod conversion {
    #[derive(Debug, Fail)]
    pub enum Error {
        #[fail(display = "Empty input")]
        EmptyInput,
        #[fail(display = "Unknown prefix {:?}", 0)]
        UnknownPrefix(Option<char>),
        #[fail(display = "Invalid public key")]
        InvalidPublicKey,
        #[fail(display = "Decode error: {}", 0)]
        DecodeError(bs58::decode::Error),
    }

    impl From<bs58::decode::Error> for Error {
        fn from(e: bs58::decode::Error) -> Self {
            Self::DecodeError(e)
        }
    }
}

impl TryFrom<String> for PeerId {
    type Error = conversion::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut chars = value.chars();
        let pre = chars.nth(0);
        if pre == Some(PEER_ID_PREFIX_ED25519) {
            let suf: String = chars.collect();
            let bytes = bs58::decode(&suf)
                .with_alphabet(bs58::alphabet::BITCOIN)
                .with_check(None)
                .into_vec()?;
            device::PublicKey::from_slice(&bytes)
                .map(PeerId)
                .ok_or_else(|| Self::Error::InvalidPublicKey)
        } else {
            Err(Self::Error::UnknownPrefix(pre))
        }
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            PEER_ID_PREFIX_ED25519,
            bs58::encode(self.0.as_ref())
                .with_alphabet(bs58::alphabet::BITCOIN)
                .with_check()
                .into_string()
        )
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_rountrip() -> Result<(), conversion::Error> {
        let peer_id1 = PeerId::from(device::Key::new().public());
        let peer_id2 = PeerId::try_from(peer_id1.to_string())?;
        if peer_id1 == peer_id2 {
            Ok(())
        } else {
            Err(conversion::Error::InvalidPublicKey)
        }
    }
}
