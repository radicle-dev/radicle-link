use std::{convert::TryFrom, fmt, str::FromStr};

use log::trace;
use multibase::Base::Base32Z;
use serde::{de::Visitor, Deserialize, Deserializer, Serialize, Serializer};

use crate::keys::device;

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct PeerId(device::PublicKey);

impl PeerId {
    pub fn device_key(&self) -> &device::PublicKey {
        &self.0
    }

    /// Canonical representation of a `PeerId`
    ///
    /// This is the `multibase` encoding, using the `z-base32` alphabet, of the
    /// public key bytes prepended by a one byte reserved value.
    pub fn default_encoding(&self) -> String {
        let mut buf = [0u8; device::PUBLICKEYBYTES + 1];
        buf[1..].copy_from_slice(&self.0.as_ref()[0..]);

        multibase::encode(Base32Z, &buf[0..])
    }

    const ENCODED_LEN: usize = 1 + (((device::PUBLICKEYBYTES + 1) * 8 + 4) / 5); // 54

    /// Attempt to deserialise from the canonical representation
    pub fn from_default_encoding(s: &str) -> Result<Self, conversion::Error> {
        use conversion::Error::*;

        if s.len() != Self::ENCODED_LEN {
            return Err(UnexpectedInputLength(s.len()));
        }

        let (_, bytes) = multibase::decode(s)?;
        let (version, key) = bytes
            .split_first()
            .expect("We check the input length, therefore there must be data. qed");

        if *version != 0 {
            return Err(UnknownVersion(*version));
        }

        device::PublicKey::from_slice(&key)
            .map(PeerId)
            .ok_or_else(|| InvalidPublicKey)
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
        #[fail(display = "Unexpected input length: {}", 0)]
        UnexpectedInputLength(usize),

        #[fail(display = "Unknown version: {}", 0)]
        UnknownVersion(u8),

        #[fail(display = "Invalid public key")]
        InvalidPublicKey,

        #[fail(display = "Decode error: {}", 0)]
        DecodeError(multibase::Error),
    }

    impl From<multibase::Error> for Error {
        fn from(e: multibase::Error) -> Self {
            Self::DecodeError(e)
        }
    }
}

impl FromStr for PeerId {
    type Err = conversion::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_default_encoding(s)
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.default_encoding())
    }
}

impl<'a> TryFrom<webpki::DNSNameRef<'a>> for PeerId {
    type Error = webpki::Error;

    fn try_from(dns_name: webpki::DNSNameRef) -> Result<Self, Self::Error> {
        let dns_name: &str = dns_name.into();
        PeerId::from_str(dns_name).map_err(|e| {
            trace!("PeerId::from_str({}) failed: {}", dns_name, e);
            webpki::Error::NameConstraintViolation
        })
    }
}

impl Into<webpki::DNSName> for PeerId {
    fn into(self) -> webpki::DNSName {
        webpki::DNSNameRef::try_from_ascii_str(&self.to_string())
            .unwrap()
            .to_owned()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_default_encoding_roundtrip() {
        let peer_id1 = PeerId::from(device::Key::new().public());
        let peer_id2 = PeerId::from_default_encoding(&peer_id1.default_encoding()).unwrap();

        assert_eq!(peer_id1, peer_id2)
    }

    #[test]
    fn test_default_encoding_empty_input() {
        assert!(matches!(
            PeerId::from_default_encoding(""),
            Err(conversion::Error::UnexpectedInputLength(0))
        ))
    }

    #[test]
    fn test_str_roundtrip() {
        let peer_id1 = PeerId::from(device::Key::new().public());
        let peer_id2 = PeerId::from_str(&peer_id1.to_string()).unwrap();

        assert_eq!(peer_id1, peer_id2)
    }

    #[test]
    fn test_dns_name_roundtrip() {
        let peer_id1 = PeerId::from(device::Key::new());
        let dns_name: webpki::DNSName = peer_id1.clone().into();
        let peer_id2 = PeerId::try_from(dns_name.as_ref()).unwrap();

        assert_eq!(peer_id1, peer_id2)
    }
}
