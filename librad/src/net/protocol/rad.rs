use std::{collections::HashSet, io};

use futures::{sink::SinkExt, stream::TryStreamExt, AsyncRead, AsyncWrite};
use futures_codec::{CborCodec, CborCodecError, Framed};
use serde::{Deserialize, Serialize};

use crate::{
    paths::Paths,
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
    // TODO(kim): Due to its object model, `serde` has no obvious way to support
    // indefinite-length arrays (as defined in CBOR). We need to either trick it
    // into it (like, some kind of `Deserialize` for an iterator type), or
    // implement pagination for this.
    Projects(Vec<ProjectId>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
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

#[derive(Clone)]
pub struct Protocol {
    my_info: PeerInfo,
    paths: Paths,
}

impl Protocol {
    pub fn new(my_info: PeerInfo, paths: &Paths) -> Self {
        Self {
            my_info,
            paths: paths.clone(),
        }
    }

    pub async fn outgoing<S>(&self, stream: S) -> Result<(), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut stream = Framed::new(stream, CborCodec::<Request, Response>::new());
        unimplemented!()
    }

    pub async fn incoming<S>(&self, stream: S) -> Result<(), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut stream = Framed::new(stream, CborCodec::<Response, Request>::new());
        while let Some(req) = stream.try_next().await? {
            let resp = match req {
                Request::GetPeerInfo => Response::PeerInfo(self.my_info.clone()),
                Request::GetProjects => Response::Projects(Project::list(&self.paths).collect()),
            };

            stream.send(resp).await?
        }

        Ok(())
    }
}
