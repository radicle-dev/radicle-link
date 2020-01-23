use std::{
    io,
    net::{SocketAddr, TcpStream},
};

use failure::Fail;
use libp2p::multiaddr::{Multiaddr, Protocol};

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Missing /ip/<addr> in multiaddr")]
    MissingIp,

    #[fail(display = "Missing /tcp/<port> in multiaddr")]
    MissingTcp,

    #[fail(display = "Trailing protocol, expected /ip*/<addr>/tcp/<port> only")]
    TrailingProtocol,

    #[fail(display = "Invalid protocol, expected /ip*/<addr>/tcp/<port>")]
    InvalidProtocol,

    #[fail(display = "{}", 0)]
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

pub fn connect(maddr: &Multiaddr) -> Result<TcpStream, Error> {
    let addr = multiaddr_to_socketaddr(maddr)?;
    TcpStream::connect(addr).map_err(|e| e.into())
}

pub fn multiaddr_to_socketaddr(maddr: &Multiaddr) -> Result<SocketAddr, Error> {
    let mut iter = maddr.iter();
    let proto1 = iter.next().ok_or(Error::MissingIp)?;
    let proto2 = iter.next().ok_or(Error::MissingTcp)?;

    if iter.next().is_some() {
        return Err(Error::TrailingProtocol);
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(Error::InvalidProtocol),
    }
}
