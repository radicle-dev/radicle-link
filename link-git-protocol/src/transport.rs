// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use bstr::BString;
use futures_lite::io::{AsyncRead, AsyncWrite};
use git_transport::{
    client::{
        self,
        git::{ConnectMode, Connection},
        SetServiceResponse,
        Transport,
        TransportWithoutIO,
    },
    Protocol,
    Service,
};

pub struct Stateless<R, W> {
    inner: Connection<R, W>,
}

impl<R, W> Stateless<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(repo: BString, recv: R, send: W) -> Self {
        let url = format!("rad://{}", repo);
        let inner = Connection::new(
            recv,
            send,
            Protocol::V2,
            repo,
            None::<(String, Option<u16>)>,
            ConnectMode::Daemon,
        )
        .custom_url(Some(url));

        Self { inner }
    }
}

impl<R, W> TransportWithoutIO for Stateless<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    fn request(
        &mut self,
        write_mode: client::WriteMode,
        on_into_read: client::MessageKind,
    ) -> Result<client::RequestWriter<'_>, client::Error> {
        self.inner.request(write_mode, on_into_read)
    }

    fn to_url(&self) -> String {
        self.inner.to_url()
    }

    fn supported_protocol_versions(&self) -> &[Protocol] {
        &[Protocol::V2]
    }

    fn connection_persists_across_multiple_requests(&self) -> bool {
        false
    }
}

#[async_trait(?Send)]
impl<R, W> Transport for Stateless<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    async fn handshake<'a>(
        &mut self,
        service: Service,
        extra_parameters: &'a [(&'a str, Option<&'a str>)],
    ) -> Result<SetServiceResponse<'_>, client::Error> {
        self.inner.handshake(service, extra_parameters).await
    }
}
