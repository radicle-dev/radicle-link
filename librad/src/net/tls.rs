// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    sync::{Arc, RwLock},
};

use rustls::{
    sign::CertifiedKey,
    ClientCertVerified,
    ClientCertVerifier,
    ClientHello,
    DistinguishedNames,
    NoServerSessionStorage,
    ResolvesClientCert,
    ResolvesServerCert,
    RootCertStore,
    ServerCertVerified,
    ServerCertVerifier,
    SignatureScheme,
    TLSError,
};
use time::{Date, OffsetDateTime};

use crate::{
    net::x509,
    peer::PeerId,
    signer::{BoxedSignError, BoxedSigner, Signer, SomeSigner},
};

pub fn make_client_config<S>(signer: S) -> Result<rustls::ClientConfig, S::Error>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let peer_id = PeerId::from_signer(&signer);
    let cert = x509::Certificate::generate(&signer)?;

    let mut cfg = rustls::ClientConfig::new();
    cfg.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    cfg.client_auth_cert_resolver = Arc::new(CertResolver::new(signer, cert));
    cfg.dangerous()
        .set_certificate_verifier(Arc::new(RadServerCertVerifier::new(peer_id)));

    Ok(cfg)
}

pub fn make_server_config<S>(signer: S) -> Result<rustls::ServerConfig, S::Error>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let peer_id = PeerId::from_signer(&signer);
    let cert = x509::Certificate::generate(&signer)?;

    let mut cfg = rustls::ServerConfig::new(Arc::new(RadClientCertVerifier::new(peer_id)));
    cfg.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    cfg.cert_resolver = Arc::new(CertResolver::new(signer, cert));
    // FIXME: session resumption is broken in rustls < 0.19 -- we can't get at
    // the client certs when resuming. Disable until we can upgrade (depends on
    // https://github.com/quinn-rs/quinn/pull/873)
    cfg.session_storage = Arc::new(NoServerSessionStorage {});

    Ok(cfg)
}

struct Cert {
    expires: Date,
    cert: rustls::Certificate,
}

struct CertResolver {
    signer: BoxedSigner,
    cert: RwLock<Cert>,
}

impl CertResolver {
    fn new<S>(signer: S, cert: x509::Certificate) -> Self
    where
        S: Signer + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let signer = BoxedSigner::from(SomeSigner { signer });
        let expires = x509::validity_time_as_date(&cert.tbs_certificate.validity.not_after);
        let cert = rustls::Certificate(cert.to_der());
        Self {
            signer,
            cert: RwLock::new(Cert { expires, cert }),
        }
    }

    /// Get the certificate, or generate a fresh one if it has expired
    fn cert(&self) -> Result<rustls::Certificate, BoxedSignError> {
        let now = OffsetDateTime::now_utc().date();
        let read = self.cert.read().unwrap();
        if now >= read.expires {
            drop(read);
            let fresh = x509::Certificate::generate(&self.signer)?;
            let expires = x509::validity_time_as_date(&fresh.tbs_certificate.validity.not_after);
            let der = rustls::Certificate(fresh.to_der());
            {
                let mut cert = self.cert.write().unwrap();
                *cert = Cert {
                    expires,
                    cert: der.clone(),
                };
            }
            Ok(der)
        } else {
            Ok(read.cert.clone())
        }
    }

    fn certified_key(&self, schemes: &[SignatureScheme]) -> Option<CertifiedKey> {
        if schemes
            .iter()
            .any(|s| matches!(s, SignatureScheme::ED25519))
        {
            self.cert()
                .map(|cert| CertifiedKey {
                    cert: vec![cert],
                    key: Arc::new(Box::new(self.signer.clone())),
                    ocsp: None,
                    sct_list: None,
                })
                .map_err(|e| {
                    tracing::error!("could not obtain certificate: {}", e);
                    e
                })
                .ok()
        } else {
            tracing::warn!("ed25519 not in presented signature schemes");
            None
        }
    }
}

impl ResolvesClientCert for CertResolver {
    #[tracing::instrument(skip(self, _acceptable_issuers, sigschemes))]
    fn resolve(
        &self,
        _acceptable_issuers: &[&[u8]],
        sigschemes: &[SignatureScheme],
    ) -> Option<CertifiedKey> {
        self.certified_key(sigschemes)
    }

    fn has_certs(&self) -> bool {
        true
    }
}

impl ResolvesServerCert for CertResolver {
    #[tracing::instrument(skip(self, client_hello))]
    fn resolve(&self, client_hello: ClientHello) -> Option<CertifiedKey> {
        client_hello
            .server_name()
            .or_else(|| {
                tracing::warn!("client missing sni");
                None
            })
            .and_then(|sni| {
                let peer_id = PeerId::try_from(sni)
                    .map_err(|e| {
                        tracing::warn!(err = ?e, "invalid sni");
                        e
                    })
                    .ok()?;
                if peer_id == PeerId::from_signer(&self.signer) {
                    self.certified_key(client_hello.sigschemes())
                } else {
                    tracing::warn!("sni doesn't match local peer id");
                    None
                }
            })
    }
}

/// A certificte verifier for both server and client certificates which applies
/// our own validation logic.
///
/// From the standpoint of proper TLS, this is unutterably insecure.
struct AccursedUnutterableUnsafeInsecureCertificateVerifier {
    local_id: PeerId,
}

impl AccursedUnutterableUnsafeInsecureCertificateVerifier {
    fn new(local_id: PeerId) -> Self {
        AccursedUnutterableUnsafeInsecureCertificateVerifier { local_id }
    }
}

type RadServerCertVerifier = AccursedUnutterableUnsafeInsecureCertificateVerifier;
type RadClientCertVerifier = AccursedUnutterableUnsafeInsecureCertificateVerifier;

impl ServerCertVerifier for AccursedUnutterableUnsafeInsecureCertificateVerifier {
    fn verify_server_cert(
        &self,
        _roots: &RootCertStore,
        presented_certs: &[rustls::Certificate],
        dns_name: webpki::DNSNameRef,
        _ocsp_response: &[u8],
    ) -> Result<ServerCertVerified, TLSError> {
        let (cert, ca) = presented_cert(presented_certs)?;

        // Verify that we got a self-signed ed25519 certificate
        cert.verify_is_valid_tls_server_cert(
            &[&webpki::ED25519],
            &webpki::TLSServerTrustAnchors(&[ca]),
            &[],
            try_now()?,
        )
        .map_err(TLSError::WebPKIError)?;

        // Check that it is valid for the DNS name the other side sent
        cert.verify_is_valid_for_dns_name(dns_name)
            .map_err(TLSError::WebPKIError)?;

        // Verify that the DNS name is a radicle `PeerId`
        let peer_id_dns = PeerId::try_from(dns_name).map_err(|_| {
            TLSError::PeerIncompatibleError(format!(
                "Presented DNS name `{:?}` is not a radicle peer id",
                dns_name
            ))
        })?;

        // Verify that the certificate's public key is also a `PeerId`
        let cert = x509::Certificate::from_der(&presented_certs[0].0)
            .map_err(|e| TLSError::PeerIncompatibleError(e.to_string()))?;

        // Both must be equal
        if &peer_id_dns != cert.peer_id_ref() {
            return Err(TLSError::PeerIncompatibleError(
                "DNS name and subjectPublicKeyInfo must be equal".into(),
            ));
        }

        // We don't allow self-connections
        if cert.peer_id_ref() == &self.local_id {
            return Err(TLSError::PeerMisbehavedError(
                "Self-connections are not permitted".into(),
            ));
        }

        Ok(ServerCertVerified::assertion())
    }
}

impl ClientCertVerifier for AccursedUnutterableUnsafeInsecureCertificateVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self, _sni: Option<&webpki::DNSName>) -> Option<bool> {
        Some(true)
    }

    // hrm
    fn client_auth_root_subjects(
        &self,
        _sni: Option<&webpki::DNSName>,
    ) -> Option<DistinguishedNames> {
        Some(rustls::DistinguishedNames::new())
    }

    fn verify_client_cert(
        &self,
        presented_certs: &[rustls::Certificate],
        _sni: Option<&webpki::DNSName>,
    ) -> Result<ClientCertVerified, TLSError> {
        let (cert, ca) = presented_cert(presented_certs)?;
        // Verify that we've got a self-signed ed25519 cert
        cert.verify_is_valid_tls_client_cert(
            &[&webpki::ED25519],
            &webpki::TLSClientTrustAnchors(&[ca]),
            &[],
            try_now()?,
        )
        .map_err(TLSError::WebPKIError)?;

        // Verify the presented cert's public key is a `PeerId`
        let cert = x509::Certificate::from_der(&presented_certs[0].0)
            .map_err(|e| TLSError::PeerIncompatibleError(e.to_string()))?;

        // We don't allow self-connections
        if cert.peer_id_ref() == &self.local_id {
            return Err(TLSError::PeerMisbehavedError(
                "Self-connections are not permitted".into(),
            ));
        }

        Ok(ClientCertVerified::assertion())
    }
}

fn presented_cert(
    presented_certs: &[rustls::Certificate],
) -> Result<(webpki::EndEntityCert, webpki::TrustAnchor), TLSError> {
    if presented_certs.is_empty() {
        return Err(TLSError::NoCertificatesPresented);
    }

    // We expect only one certificate, which is the EE cert. The rest of the
    // presented certs can be ignored.
    let rustls::Certificate(der) = &presented_certs[0];

    let cert = webpki::EndEntityCert::from(der).map_err(TLSError::WebPKIError)?;
    // It's self-signed, so it's its own CA
    let ca =
        webpki::trust_anchor_util::cert_der_as_trust_anchor(der).map_err(TLSError::WebPKIError)?;

    Ok((cert, ca))
}

fn try_now() -> Result<webpki::Time, TLSError> {
    webpki::Time::try_from(std::time::SystemTime::now())
        .map_err(|_| TLSError::FailedToGetCurrentTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io;

    use crate::keys::SecretKey;
    use rustls::{ClientSession, ServerSession, Session};

    #[test]
    fn test_can_handshake() {
        let client_key = SecretKey::new();
        let server_key = SecretKey::new();

        let server_id = PeerId::from(&server_key).to_string();

        let client_config = Arc::new(make_client_config(client_key).unwrap());
        let sni = webpki::DNSNameRef::try_from_ascii_str(&server_id).unwrap();
        let mut client_session = ClientSession::new(&client_config, sni);

        let server_config = Arc::new(make_server_config(server_key).unwrap());
        let mut server_session = ServerSession::new(&server_config);

        do_handshake(&mut client_session, &mut server_session)
    }

    fn do_handshake(client: &mut ClientSession, server: &mut ServerSession) {
        while server.is_handshaking() || client.is_handshaking() {
            transfer(client, server);
            server.process_new_packets().unwrap();
            transfer(server, client);
            client.process_new_packets().unwrap();
        }
    }

    fn transfer(left: &mut dyn Session, right: &mut dyn Session) {
        let mut buf = [0u8; 262_144];

        while left.wants_write() {
            let sz = {
                let into_buf: &mut dyn io::Write = &mut &mut buf[..];
                left.write_tls(into_buf).unwrap()
            };

            if sz == 0 {
                break;
            }

            let mut offs = 0;
            loop {
                let from_buf: &mut dyn io::Read = &mut &buf[offs..sz];
                offs += right.read_tls(from_buf).unwrap();
                if sz == offs {
                    break;
                }
            }
        }
    }
}
