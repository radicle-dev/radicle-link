// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;

use crate::{
    identities::Xor,
    net::{
        protocol::{interrogation, io, PeerAdvertisement},
        quic,
    },
    PeerId,
};

use super::error;

pub struct Interrogation {
    pub(super) peer: PeerId,
    pub(super) conn: quic::Connection,
}

impl Interrogation {
    /// Ask the interrogated peer to send its [`PeerAdvertisement`].
    pub async fn peer_advertisement(
        &self,
    ) -> Result<PeerAdvertisement<SocketAddr>, error::Interrogation> {
        use interrogation::{Request, Response};

        self.request(Request::GetAdvertisement)
            .await
            .and_then(|resp| match resp {
                Response::Advertisement(ad) => Ok(ad),
                Response::Error(e) => Err(error::Interrogation::ErrorResponse(e)),
                _ => Err(error::Interrogation::InvalidResponse),
            })
    }

    /// Ask the interrogated peer to send back the [`SocketAddr`] the local peer
    /// appears to have.
    pub async fn echo_addr(&self) -> Result<SocketAddr, error::Interrogation> {
        use interrogation::{Request, Response};

        self.request(Request::EchoAddr)
            .await
            .and_then(|resp| match resp {
                Response::YourAddr(ad) => Ok(ad),
                Response::Error(e) => Err(error::Interrogation::ErrorResponse(e)),
                _ => Err(error::Interrogation::InvalidResponse),
            })
    }

    /// Ask the interrogated peer to send the complete list of URNs it has.
    ///
    /// The response is compactly encoded as an [`Xor`] filter, with a very
    /// small false positive probability.
    pub async fn urns(&self) -> Result<Xor, error::Interrogation> {
        use interrogation::{Request, Response};

        self.request(Request::GetUrns)
            .await
            .and_then(|resp| match resp {
                Response::Urns(urns) => Ok(urns.into_owned()),
                Response::Error(e) => Err(error::Interrogation::ErrorResponse(e)),
                _ => Err(error::Interrogation::InvalidResponse),
            })
    }

    async fn request(
        &self,
        request: interrogation::Request,
    ) -> Result<interrogation::Response<'static, SocketAddr>, error::Interrogation> {
        match io::send::single_response(&self.conn, request, interrogation::FRAMED_BUFSIZ).await {
            Err(e) => Err(e.into()),
            Ok(resp) => resp.ok_or(error::Interrogation::NoResponse(self.peer)),
        }
    }
}
