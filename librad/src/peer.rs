use std::{fmt, str::FromStr};

use bs58;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};

use crate::keys::device;

pub const PEER_ID_PREFIX_ED25519: char = '0';

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct PeerId(device::PublicKey);

impl PeerId {
    pub fn device_key(&self) -> &device::PublicKey {
        &self.0
    }
}

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
                PeerId::from_str(&s).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(PeerIdVisitor)
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

impl FromStr for PeerId {
    type Err = conversion::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        let pre = chars.next();
        if pre == Some(PEER_ID_PREFIX_ED25519) {
            let suf: String = chars.collect();
            let bytes = bs58::decode(&suf)
                .with_alphabet(bs58::alphabet::BITCOIN)
                .with_check(None)
                .into_vec()?;
            device::PublicKey::from_slice(&bytes)
                .map(PeerId)
                .ok_or_else(|| Self::Err::InvalidPublicKey)
        } else {
            Err(Self::Err::UnknownPrefix(pre))
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
        let peer_id2 = PeerId::from_str(&peer_id1.to_string())?;
        if peer_id1 == peer_id2 {
            Ok(())
        } else {
            Err(conversion::Error::InvalidPublicKey)
        }
    }
}
