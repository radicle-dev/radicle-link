use std::{
    collections::HashMap,
    fmt::Display,
    io::{self, Read, Write},
    sync::{Arc, Once, RwLock},
};

use async_trait::async_trait;
use futures::{
    executor::block_on,
    io::{AsyncReadExt, AsyncWriteExt},
};
use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use log::{error, trace};
use url::Url;

use crate::{net::connection::Stream, peer::PeerId};

pub type Factories = Arc<RwLock<HashMap<PeerId, Box<dyn GitStreamFactory>>>>;

lazy_static! {
    static ref FACTORIES: Factories = Arc::new(RwLock::new(HashMap::with_capacity(1)));
}

#[async_trait]
pub trait GitStreamFactory: Sync + Send {
    async fn open_stream(&self, to: &PeerId) -> Option<Stream>;
}

pub fn register() -> RadTransport {
    static INIT: Once = Once::new();

    unsafe {
        INIT.call_once(|| {
            git2::transport::register("rad", move |remote| {
                Transport::smart(&remote, true, RadTransport::new())
            })
            .unwrap();
        })
    }

    RadTransport::new()
}

#[derive(Clone)]
pub struct RadTransport {
    fac: Factories,
}

impl RadTransport {
    fn new() -> Self {
        Self {
            fac: FACTORIES.clone(),
        }
    }

    pub fn register_stream_factory(&self, peer_id: &PeerId, fac: Box<dyn GitStreamFactory>) {
        self.fac.write().unwrap().insert(peer_id.clone(), fac);
    }

    fn open_stream(&self, from: &PeerId, to: &PeerId) -> Option<Stream> {
        self.fac
            .read()
            .unwrap()
            .get(from)
            .and_then(|fac| block_on(fac.open_stream(to)))
    }
}

impl SmartSubtransport for RadTransport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        trace!("SmartSubtransport::action: {}", url);
        let url = Url::parse(url).map_err(git_error)?;

        let local_peer: PeerId = url.username().parse().map_err(git_error)?;
        let remote_peer: PeerId = url
            .host_str()
            .ok_or_else(|| git_error("Missing host"))?
            .parse()
            .map_err(git_error)?;

        let stream = self
            .open_stream(&local_peer, &remote_peer)
            .ok_or_else(|| git_error(format!("No connection to {}", remote_peer)))?;

        Ok(Box::new(RadSubTransport {
            header_sent: false,
            remote_peer,
            remote_repo: url.path().to_string(),
            service: action,
            stream,
        }))
    }

    fn close(&self) -> Result<(), git2::Error> {
        trace!("SmartSubtransport::close()");
        Ok(())
    }
}

struct RadSubTransport {
    header_sent: bool,
    remote_peer: PeerId,
    remote_repo: String,
    service: Service,
    stream: Stream,
}

impl RadSubTransport {
    async fn ensure_header_sent(&mut self) -> io::Result<()> {
        if !self.header_sent {
            self.header_sent = true;
            trace!("Writing service header");
            self.stream
                .write_all(self.service_header().as_bytes())
                .await
        } else {
            Ok(())
        }
    }

    fn service_header(&self) -> String {
        match self.service {
            Service::UploadPackLs => format!(
                "git-upload-pack {}\0advertise\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
            Service::UploadPack => format!(
                "git-upload-pack {}\0\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
            Service::ReceivePackLs => format!(
                "git-receive-pack {}\0advertise\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
            Service::ReceivePack => format!(
                "git-receive-pack {}\0\0host={}\0\n",
                self.remote_repo, self.remote_peer
            ),
        }
    }
}

impl Read for RadSubTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        trace!("RadSubTransport::read");
        block_on(async {
            self.ensure_header_sent().await?;
            self.stream.read(buf).await.map_err(io_error)
        })
    }
}

impl Write for RadSubTransport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        trace!("RadSubTransport::write");
        block_on(async {
            self.ensure_header_sent().await?;
            //trace!("write buf: {:?}", std::str::from_utf8(buf));
            self.stream.write(buf).await.map_err(io_error)
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        trace!("RadSubTransport::flush");
        block_on(async {
            self.ensure_header_sent().await?;
            self.stream.flush().await.map_err(io_error)
        })
    }
}

fn git_error<E: Display>(err: E) -> git2::Error {
    // libgit will always tell us "an unknown error occurred", so log them out
    // here
    error!("git transport error: {}", err);
    git2::Error::from_str(&err.to_string())
}

fn io_error<E: Display>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}
