use std::{
    fmt::Display,
    io::{self, Read, Write},
    sync::{Arc, Once, RwLock},
};

use futures::{
    executor::block_on,
    io::{AsyncReadExt, AsyncWriteExt},
};
use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use url::Url;

use crate::{net::connection::Stream, peer::PeerId};

pub type Factories = Arc<RwLock<Vec<Box<dyn GitStreamFactory>>>>;

lazy_static! {
    static ref FACTORIES: Factories = Arc::new(RwLock::new(Vec::new()));
}

pub trait GitStreamFactory: Sync + Send {
    fn open_stream(&self, to: &PeerId) -> Option<Stream>;
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

    pub fn register_stream_factory(&self, fac: Box<dyn GitStreamFactory>) {
        self.fac.write().unwrap().push(fac)
    }

    fn open_stream(&self, to: &PeerId) -> Option<Stream> {
        self.fac
            .read()
            .unwrap()
            .iter()
            .filter_map(|fac| fac.open_stream(to))
            .next()
    }
}

impl SmartSubtransport for RadTransport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        let url = Url::parse(url).map_err(git_error)?;

        let peer: PeerId = url
            .host_str()
            .ok_or_else(|| git_error("Missing host"))?
            .parse()
            .map_err(git_error)?;

        let service_header = {
            let repo = url.path();
            match action {
                Service::UploadPackLs | Service::UploadPack => {
                    format!("git-upload-pack {}\0host={}\0\n", repo, peer)
                },
                Service::ReceivePackLs | Service::ReceivePack => {
                    format!("git-receive-pack {}\0host={}\0\n", repo, peer)
                },
            }
        };

        let mut stream = self
            .open_stream(&peer)
            .ok_or_else(|| git_error(format!("No connection to {}", peer)))?;

        block_on(stream.write_all(service_header.as_bytes())).map_err(git_error)?;

        Ok(Box::new(RadSubTransport { stream }))
    }

    fn close(&self) -> Result<(), git2::Error> {
        Ok(())
    }
}

struct RadSubTransport {
    stream: Stream,
}

impl Read for RadSubTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        block_on(self.stream.read(buf))
    }
}

impl Write for RadSubTransport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        block_on(self.stream.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        block_on(self.stream.flush())
    }
}

fn git_error<E: Display>(err: E) -> git2::Error {
    git2::Error::from_str(&err.to_string())
}
