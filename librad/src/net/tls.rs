use std::{convert::TryFrom, sync::Arc};

use rcgen::{self, CertificateParams, DnType, ExtendedKeyUsagePurpose, PKCS_ED25519};
use rustls::{
    ClientCertVerified,
    ClientCertVerifier,
    DistinguishedNames,
    RootCertStore,
    ServerCertVerified,
    ServerCertVerifier,
    TLSError,
};

use crate::keys::device;

/// Generate a TLS certificate.
///
/// The certificate is self-signed by the given [`device::Key`], and advertises
/// a subject alt name of "<base58btc-encoded-public-key>.radicle".
fn gen_cert(key: &device::Key) -> rcgen::Certificate {
    let params = {
        let mut params = CertificateParams::new(vec![format!("{}.radicle", key)]);

        params.alg = &PKCS_ED25519;
        params.distinguished_name = {
            let mut distinguished_name = rcgen::DistinguishedName::new();
            distinguished_name.push(DnType::CommonName, "radicle-link self-signed");
            distinguished_name
        };
        params.is_ca = rcgen::IsCa::SelfSignedOnly;
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        params.custom_extensions = vec![];
        params.key_pair = {
            let key_pair = rcgen::KeyPair::try_from(key.as_pkcs8().as_slice())
                .expect("A valid PKCS#8 document is valid. qed");

            Some(key_pair)
        };

        // TODO: should we set sane `not_before` / `not_after` values?

        params
    };

    rcgen::Certificate::from_params(params)
        .expect("A certificate with valid parameters is valid. qed")
}

pub fn make_client_config(key: &device::Key) -> rustls::ClientConfig {
    let cert = gen_cert(key);

    let mut cfg = rustls::ClientConfig::new();
    cfg.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    cfg.set_single_client_cert(
        vec![rustls::Certificate(cert.serialize_der().unwrap())],
        rustls::PrivateKey(cert.serialize_private_key_der()),
    );
    cfg.dangerous()
        .set_certificate_verifier(Arc::new(RadServerCertVerifier::new()));

    cfg
}

pub fn make_server_config(key: &device::Key) -> rustls::ServerConfig {
    let cert = gen_cert(key);

    let mut cfg = rustls::ServerConfig::new(Arc::new(RadClientCertVerifier::new()));
    cfg.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    cfg.set_single_cert(
        vec![rustls::Certificate(cert.serialize_der().unwrap())],
        rustls::PrivateKey(cert.serialize_private_key_der()),
    )
    .unwrap();

    cfg
}

/// A certificte verifier for both server and client certificates which applies
/// our own validation logic.
///
/// From the standpoint of proper TLS, this is unutterably insecure.
struct AccursedUnutterableUnsafeInsecureCertificateVerifier;

impl AccursedUnutterableUnsafeInsecureCertificateVerifier {
    fn new() -> Self {
        AccursedUnutterableUnsafeInsecureCertificateVerifier
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

        // let's see if this works
        cert.verify_is_valid_tls_server_cert(
            &[&webpki::ED25519],
            &webpki::TLSServerTrustAnchors(&[ca]),
            &[],
            try_now()?,
        )
        .map_err(TLSError::WebPKIError)?;

        cert.verify_is_valid_for_dns_name(dns_name)
            .map_err(TLSError::WebPKIError)?;

        // TODO(kim): Should we check that the SNI is a valid radicle peer id?
        // ie. it matches the cert's pub key
        // TODO(kim): Verify that the TLS handshake obtains a proof of ownership
        // of the cert's private key

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
        cert.verify_is_valid_tls_client_cert(
            &[&webpki::ED25519],
            &webpki::TLSClientTrustAnchors(&[ca]),
            &[],
            try_now()?,
        )
        .map_err(TLSError::WebPKIError)?;

        // TODO(kim): same as for server cert

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
    let cert = webpki::EndEntityCert::from(&presented_certs[0].0).map_err(TLSError::WebPKIError)?;

    // It's self-signed, so it's its own CA
    let ca = webpki::trust_anchor_util::cert_der_as_trust_anchor(&presented_certs[0].0)
        .map_err(TLSError::WebPKIError)?;

    Ok((cert, ca))
}

fn try_now() -> Result<webpki::Time, TLSError> {
    webpki::Time::try_from(std::time::SystemTime::now())
        .map_err(|_| TLSError::FailedToGetCurrentTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    use rustls::{ClientSession, ServerSession, Session};

    #[test]
    fn test_pkcs8_is_sane() {
        let key = device::Key::new();
        let cert = gen_cert(&key);
        assert_eq!(cert.serialize_private_key_der(), key.as_pkcs8())
    }

    #[test]
    fn test_can_handshake() {
        let client_key = device::Key::new();
        let server_key = device::Key::new();

        let client_config = Arc::new(make_client_config(&client_key));
        let mut client_session = ClientSession::new(
            &client_config,
            webpki::DNSNameRef::try_from_ascii(format!("{}.radicle", server_key).as_bytes())
                .unwrap(),
        );

        let server_config = Arc::new(make_server_config(&server_key));
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
            let sz = left.write_tls(&mut buf.as_mut()).unwrap();

            if sz == 0 {
                break;
            }

            let mut offs = 0;
            loop {
                offs += right.read_tls(&mut buf[offs..sz].as_ref()).unwrap();
                if sz == offs {
                    break;
                }
            }
        }
    }
}
