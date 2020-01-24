use std::{collections::HashSet, io, iter};

use futures::{future, prelude::*};
use futures_cbor_codec::Codec;
use futures_codec::Framed;
use serde::{Deserialize, Serialize};

use libp2p::core::{InboundUpgrade, Negotiated, OutboundUpgrade, UpgradeInfo};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Rpc {
    GetCapabilities,
    Capabilities(HashSet<Capability>),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub enum Capability {
    GitDaemon { port: u16 },
}

pub struct Link;

impl UpgradeInfo for Link {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/rad/1.0.0")
    }
}

impl<S> InboundUpgrade<S> for Link
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Framed<Negotiated<S>, Codec<Rpc, Rpc>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, io::Error>>;

    fn upgrade_inbound(self, socket: Negotiated<S>, _: Self::Info) -> Self::Future {
        future::ok(Framed::new(socket, Codec::<Rpc, Rpc>::new()))
    }
}

impl<S> OutboundUpgrade<S> for Link
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Framed<Negotiated<S>, Codec<Rpc, Rpc>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, io::Error>>;

    fn upgrade_outbound(self, socket: Negotiated<S>, _: Self::Info) -> Self::Future {
        future::ok(Framed::new(socket, Codec::<Rpc, Rpc>::new()))
    }
}
