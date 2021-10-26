// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom as _, ops::Deref, time::Duration};

use futures::executor::block_on;
use picky_asn1::{
    bit_string::BitString,
    date::{GeneralizedTime, UTCTime},
    restricted_string::IA5String,
    wrapper::IntegerAsn1,
};
use picky_asn1_der::Asn1DerError;
use picky_asn1_x509::{
    self as x509,
    oids,
    validity,
    AlgorithmIdentifier,
    DirectoryName,
    ExtendedKeyUsage,
    Extension,
    Extensions,
    GeneralName,
    PublicKey,
    SubjectPublicKeyInfo,
    TbsCertificate,
    Validity,
    Version,
};
use thiserror::Error;
use time::{Date, OffsetDateTime};

use crate::{keystore::sign::Signer, PeerId};

#[derive(Debug, Error)]
pub enum FromDerError {
    #[error("the subject common name must be a valid PeerId")]
    SubjectNotPeerId(#[source] crypto::peer::conversion::Error),

    #[error("invalid subject public key")]
    InvalidPublicKey,

    #[error("unsupported algorithm for subject public key")]
    UnsupportedAlgorithm,

    #[error("missing subject common name")]
    MissingSubjectCN,

    #[error("subject public key and common name must be the same")]
    KeyMismatch,

    #[error(transparent)]
    Asn1(#[from] Asn1DerError),
}

/// Self-signed X509 certificate.
#[derive(Debug, PartialEq)]
pub struct Certificate {
    peer_id: PeerId,
    cert: x509::Certificate,
}

impl Certificate {
    /// Generate a new self-signed [`Certificate`].
    pub fn generate<S>(signer: &S) -> Result<Self, S::Error>
    where
        S: Signer,
        S::Error: std::error::Error,
    {
        let peer_id = PeerId::from_signer(signer);

        let algorithm = AlgorithmIdentifier::new_ed25519();
        let serial_number = IntegerAsn1::from_bytes_be_unsigned(vec![1]);
        let issuer = DirectoryName::new_common_name(PeerId::from_signer(signer).default_encoding());
        let subject = issuer.clone(); // self-signed
        let subject_public_key_info = SubjectPublicKeyInfo {
            algorithm: algorithm.clone(),
            subject_public_key: PublicKey::Ed(
                BitString::with_bytes(signer.public_key().as_ref()).into(),
            ),
        };
        let validity = valid_until(Duration::from_secs(7889400)); // 3 months
        let extensions = Extensions(vec![
            Extension::new_subject_alt_name(vec![GeneralName::DnsName(
                peer_id.to_string().parse::<IA5String>().unwrap().into(),
            )]),
            Extension::new_extended_key_usage(ExtendedKeyUsage::new(vec![
                oids::kp_server_auth(),
                oids::kp_client_auth(),
            ])),
        ])
        .into();

        let tbs_certificate = TbsCertificate {
            version: Version::V3.into(),
            serial_number,
            signature: algorithm.clone(),
            issuer,
            validity,
            subject,
            subject_public_key_info,
            extensions,
        };

        let signature_value = {
            let tbs_der = picky_asn1_der::to_vec(&tbs_certificate).unwrap();
            let signature = block_on(signer.sign(&tbs_der))?;
            BitString::with_bytes(signature.as_ref()).into()
        };

        let cert = x509::Certificate {
            tbs_certificate,
            signature_algorithm: algorithm,
            signature_value,
        };

        Ok(Self { peer_id, cert })
    }

    /// Serialise in DER format.
    pub fn to_der(&self) -> Vec<u8> {
        picky_asn1_der::to_vec(&self.cert).unwrap()
    }

    /// Attempt to deserialise from DER format.
    ///
    /// Also validates that the subject public key is equal to the subject, and
    /// both parse as the same [`PeerId`].
    pub fn from_der(der: &[u8]) -> Result<Self, FromDerError> {
        let cert: x509::Certificate = picky_asn1_der::from_bytes(der)?;
        let peer_id = {
            let spk = match &cert
                .tbs_certificate
                .subject_public_key_info
                .subject_public_key
            {
                PublicKey::Ed(point) => crate::PublicKey::from_slice(point.payload_view())
                    .ok_or(FromDerError::InvalidPublicKey)
                    .map(PeerId::from),
                _ => Err(FromDerError::UnsupportedAlgorithm),
            }?;
            let subj = {
                let cn = cert
                    .tbs_certificate
                    .subject
                    .find_common_name()
                    .ok_or(FromDerError::MissingSubjectCN)?;

                PeerId::from_default_encoding(&cn.to_utf8_lossy())
                    .map_err(FromDerError::SubjectNotPeerId)
            }?;

            if spk != subj {
                Err(FromDerError::KeyMismatch)
            } else {
                Ok(spk)
            }
        }?;

        Ok(Self { peer_id, cert })
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub fn peer_id_ref(&self) -> &PeerId {
        self.as_ref()
    }
}

impl Deref for Certificate {
    type Target = x509::Certificate;

    fn deref(&self) -> &Self::Target {
        &self.cert
    }
}

impl AsRef<PeerId> for Certificate {
    fn as_ref(&self) -> &PeerId {
        &self.peer_id
    }
}

fn valid_until(d: Duration) -> Validity {
    let now = OffsetDateTime::now_utc();
    let until = now + d;
    Validity {
        not_before: into_validity_time(now),
        not_after: into_validity_time(until),
    }
}

fn into_validity_time(t: OffsetDateTime) -> validity::Time {
    let (y, m, d) = t.date().to_calendar_date();
    // Safety: `OffsetDateTime` cannot yield out-of-range values, and we're checking
    // for UTC vs. Generalized
    if y > 2049 {
        unsafe { GeneralizedTime::new_unchecked(y as u16, m.into(), d, 0, 0, 0).into() }
    } else {
        unsafe { UTCTime::new_unchecked(y as u16, m.into(), d, 0, 0, 0).into() }
    }
}

pub(crate) fn validity_time_as_date(t: &validity::Time) -> Date {
    use validity::Time::*;

    let (y, m, d) = match t {
        Utc(utc) => (utc.year().into(), utc.month(), utc.day()),
        Generalized(gen) => (gen.year().into(), gen.month(), gen.day()),
    };

    // Safety: we construct validity time from `OffsetDateTime`
    Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap()
}
