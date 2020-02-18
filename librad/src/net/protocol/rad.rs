use std::{collections::HashSet, io};

use futures::{sink::SinkExt, stream::TryStreamExt, AsyncRead, AsyncWrite};
use futures_codec::{CborCodec, CborCodecError, FramedRead, FramedWrite};
use serde::{Deserialize, Serialize};

use crate::{
    paths::Paths,
    peer::PeerId,
    project::{Project, ProjectId},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    GetPeerInfo,
    GetProjects,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    PeerInfo(PeerInfo),
    Projects(Vec<ProjectId>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    peer_id: PeerId,
    listen_port: u16,
    capabilities: HashSet<Capability>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Reserved = 0,
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Invalid payload")]
    InvalidPayload(#[fail(cause)] serde_cbor::Error),

    #[fail(display = "{}", 0)]
    Io(#[fail(cause)] io::Error),
}

impl From<CborCodecError> for Error {
    fn from(err: CborCodecError) -> Self {
        match err {
            CborCodecError::Cbor(e) => Self::InvalidPayload(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}

pub struct Protocol {
    self_info: PeerInfo,
    paths: Paths,
}

impl Protocol {
    pub async fn handle_incoming<R, W>(&self, recv: R, send: W) -> Result<(), Error>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let codec = CborCodec::<Response, Request>::new();
        let mut framed_recv = FramedRead::new(recv, codec.clone());
        let mut framed_send = FramedWrite::new(send, codec);

        loop {
            match framed_recv.try_next().await {
                Ok(Some(req)) => {
                    let resp = match req {
                        Request::GetPeerInfo => Response::PeerInfo(self.self_info.clone()),
                        Request::GetProjects => {
                            Response::Projects(Project::list(&self.paths).collect())
                        },
                    };

                    match framed_send.send(resp).await {
                        Ok(()) => {},
                        Err(CborCodecError::Io(e)) => return Err(Error::Io(e)),
                        Err(CborCodecError::Cbor(_)) => unreachable!(),
                    }
                },
                Ok(None) => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        }
    }
}
