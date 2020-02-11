use std::{
    fmt::{self, Display},
    io::{self, prelude::*},
    mem,
    net::ToSocketAddrs,
    sync::Arc,
};

use bytes::Bytes;
use futures::{executor::block_on, io::AsyncReadExt};
use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use http::{request::Builder as RequestBuilder, Method, Request, Response, Uri};
use log::debug;
use quinn_h3::{
    body::BodyReader,
    client::{Client, Connection},
};

/// A custom HTTP/3 transport for libgit.
///
/// This is currently for demonstration purposes only. Specifically, we expect
/// the call to [`SmartSubtransport::action`] to pass a network address in the
/// `url` parameter -- the consequence being that we need to establish a fresh
/// connection every time.
///
/// The real implementation would instead ask some peer-to-peer membership state
/// to hand us back only a QUIC _stream_ on an already existing connection,
/// which the algorithm has determined to provide the repo we're interested in.
struct Http3Transport {
    client: Arc<Client>,
}

struct Http3Subtransport {
    connection: Connection,
    request: Req,
    response: Option<BodyReader>,
}

// FIXME(kim): clippy laments that the variants differ in size too much. Perhaps
// we should also support streaming the request body.
enum Req {
    BodyExpected(RequestBuilder),
    Ready(Request<Bytes>),
    Sent,
}

impl Req {
    fn is_ready(&self) -> bool {
        match self {
            Self::Ready(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
enum RequestError {
    BodyExpected,
    AlreadySent,
    ErrorResponse(Response<()>),
    H3(quinn_h3::Error),
}

impl std::error::Error for RequestError {}

impl Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::BodyExpected => f.write_str("Request error: body expected"),
            Self::AlreadySent => f.write_str("Request error: already sent"),
            Self::ErrorResponse(resp) => write!(f, "Error response received: {:?}", resp),
            Self::H3(e) => write!(f, "HTTP error: {}", e),
        }
    }
}

impl Http3Subtransport {
    fn send_request(&mut self, data: &[u8]) -> Result<(), RequestError> {
        debug!("subtransport send_request");
        let old = mem::replace(&mut self.request, Req::Sent);
        let req = match old {
            Req::Sent => {
                debug!("request already sent!");
                Err(RequestError::AlreadySent)
            },
            Req::BodyExpected(builder) => Ok(builder.body(Bytes::copy_from_slice(data)).unwrap()),
            Req::Ready(req) => Ok(req),
        }?;

        debug!("sending request");
        let recv_body = block_on(async {
            let (recv_response, _) = self
                .connection
                .send_request(req)
                .await
                .map_err(RequestError::H3)?;

            let (resp, recv_body) = recv_response.await.map_err(RequestError::H3)?;
            debug!("received response {:?}", resp);

            if resp.status().is_success() {
                Ok(recv_body)
            } else {
                Err(RequestError::ErrorResponse(resp))
            }
        })?;

        self.response = Some(recv_body);

        Ok(())
    }
}

/// Register the HTTP/3 transport with libgit.
///
/// # Safety
///
/// This is unsafe for the same reasons `git::transport::register` is unsafe:
/// the call must be externally synchronised with all calls to `libgit`.
/// Repeatedly calling this function will leak the `Client`, as the rust
/// bindings don't expose a way to unregister a previously-registered
/// transport with the same scheme.
pub unsafe fn register_h3_transport(client: Client) {
    let client = Arc::new(client);
    git2::transport::register("rad", move |remote| {
        Transport::smart(
            &remote,
            true,
            Http3Transport {
                client: client.clone(),
            },
        )
    })
    .unwrap()
}

impl SmartSubtransport for Http3Transport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        let uri = url.parse::<Uri>().map_err(as_git2_error)?;

        let request = match action {
            Service::UploadPackLs => Req::Ready(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("{}/info/refs?service=git-upload-pack", url))
                    .body(Default::default())
                    .unwrap(),
            ),
            Service::UploadPack => Req::BodyExpected(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("{}/git-upload-pack", url)),
            ),
            Service::ReceivePackLs => Req::Ready(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("{}/info/refs?service=git-receive-pack", url))
                    .body(Default::default())
                    .unwrap(),
            ),
            Service::ReceivePack => Req::BodyExpected(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("{}/git-receive-pack", url)),
            ),
        };

        let addr = (
            uri.host().unwrap_or("localhost"),
            uri.port_u16().unwrap_or(4433),
        )
            .to_socket_addrs()
            .map_err(as_git2_error)?
            .next()
            .ok_or_else(|| git2::Error::from_str("Couldn't resolve address"))?;

        debug!("connecting to {:?}", addr);

        // Fake the SNI by using the userinfo
        let server_name: String = uri
            .authority()
            .map(|auth| auth.to_string().chars().take_while(|c| *c != '@').collect())
            .expect("no userinfo in uri");

        let connection = block_on(async {
            self.client
                .connect(&addr, &server_name)
                .map_err(|e| {
                    debug!("connect: {}", e);
                    as_git2_error(e)
                })?
                .await
                .map_err(|e| {
                    debug!("await connect: {}", e);
                    as_git2_error(e)
                })
        })?;

        let should_send = request.is_ready();
        let mut subtrans = Http3Subtransport {
            connection,
            request,
            response: None,
        };
        debug!("http3 transport created");

        if should_send {
            debug!("sending request immediately");
            subtrans.send_request(&[]).map_err(as_git2_error)?;
        }
        Ok(Box::new(subtrans))
    }

    fn close(&self) -> Result<(), git2::Error> {
        Ok(()) // ...
    }
}

impl Read for Http3Subtransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(reader) = self.response.as_mut() {
            block_on(reader.read(buf))
        } else {
            debug!("transport read: no data available");
            Ok(0)
        }
    }
}

impl Write for Http3Subtransport {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        debug!("transport write");
        self.send_request(data)
            .map(|()| data.len())
            .map_err(as_io_error)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn as_io_error<E>(err: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::Other, err)
}

fn as_git2_error<E: Display>(err: E) -> git2::Error {
    git2::Error::from_str(&err.to_string())
}
