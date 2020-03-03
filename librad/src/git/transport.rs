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

static mut FACTORY: Vec<Box<dyn GitStreamFactory>> = Vec::new();

pub trait GitStreamFactory {
    fn open_stream(&self, to: &PeerId) -> Option<Stream>;
}

#[derive(Clone)]
pub struct Lock(Arc<RwLock<()>>);

pub unsafe fn register() -> Option<Lock> {
    static mut LOCK: Option<Lock> = None;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        git2::transport::register("rad", move |remote| {
            let lock = Lock(Arc::new(RwLock::new(())));
            LOCK = Some(lock.clone());
            Transport::smart(&remote, true, RadTransport::new(lock))
        })
        .unwrap();
    });

    LOCK.clone()
}

pub fn register_stream_factory(lock: Lock, fac: Box<dyn GitStreamFactory>) {
    let lock = lock.0.write().unwrap();
    unsafe { FACTORY.push(fac) };
    drop(lock);
}

fn open_stream(lock: Lock, to: &PeerId) -> Option<Stream> {
    let lock = lock.0.read().unwrap();
    let stream = unsafe {
        FACTORY
            .iter()
            .filter_map(|fac| fac.open_stream(to))
            .fuse()
            .next()
    };
    drop(lock);
    stream
}

struct RadTransport {
    lock: Lock,
}

impl RadTransport {
    fn new(lock: Lock) -> Self {
        Self { lock }
    }
}

impl SmartSubtransport for RadTransport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        let url = Url::parse(url).map_err(as_git2_error)?;

        let peer: PeerId = url
            .host_str()
            .ok_or_else(|| git2::Error::from_str("Missing host"))?
            .parse()
            .map_err(as_git2_error)?;
        let service_header = {
            let repo = url.path();
            match action {
                Service::UploadPackLs | Service::UploadPack => {
                    format!("git-upload-pack {}\0\n", repo)
                },
                Service::ReceivePackLs | Service::ReceivePack => {
                    format!("git-receive-pack {}\0\n", repo)
                },
            }
        };

        let mut stream = open_stream(self.lock.clone(), &peer)
            .ok_or_else(|| git2::Error::from_str(&format!("No connection to {}", peer)))?;
        block_on(stream.write_all(service_header.as_bytes())).map_err(as_git2_error)?;

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

fn as_git2_error<E: Display>(err: E) -> git2::Error {
    git2::Error::from_str(&err.to_string())
}
