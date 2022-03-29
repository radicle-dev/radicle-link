// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryInto as _;

use async_compat::{Compat, CompatExt};
use async_trait::async_trait;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{
    messages,
    wire_types::{self, Message},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unexpected EOF")]
    UnexpectedEof,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("error decoding")]
    DecodeFailed,
}

pub struct MessageReader<R> {
    reader: R,
    buffer: Vec<u8>,
    state: ReadState,
}

enum ReadState {
    /// Reading length prefix.
    ReadLen([u8; 4], u8),
    /// Reading CBOR item bytes.
    ReadVal(usize),
}

impl ReadState {
    fn new() -> ReadState {
        ReadState::ReadLen([0; 4], 0)
    }
}

impl<R> MessageReader<R> {
    pub fn new(r: R) -> MessageReader<R> {
        MessageReader {
            state: ReadState::new(),
            reader: r,
            buffer: Vec::with_capacity(10),
        }
    }

    fn parse<'b, E: minicbor::Decode<'b>>(&'b self) -> Result<Message<E>, minicbor::decode::Error> {
        let mut decoder = minicbor::Decoder::new(&self.buffer);
        let headers: E = decoder.decode()?;
        let payload = if decoder.position() < self.buffer.len() {
            Some(self.buffer[decoder.position()..].to_vec())
        } else {
            None
        };
        Ok(Message { headers, payload })
    }
}

impl<R: AsyncRead + Unpin> MessageReader<R> {
    /// Read a length prefixed message
    ///
    /// # Cancellation
    ///
    /// This future is cancel safe. Dropping the future half way through
    /// decoding will save progress in the `MessageReader` so that calling
    /// this method again will restart correctly.
    pub(crate) async fn read_message<'b, E: minicbor::Decode<'b>>(
        &'b mut self,
    ) -> Result<Option<Message<E>>, Error> {
        loop {
            match self.state {
                ReadState::ReadLen(buf, 4) => {
                    let len = u32::from_be_bytes(buf) as usize;
                    self.buffer.clear();
                    self.buffer.resize(len, 0u8);
                    self.state = ReadState::ReadVal(0)
                },
                ReadState::ReadLen(ref mut buf, ref mut o) => {
                    let n = self.reader.read(&mut buf[usize::from(*o)..]).await?;
                    if n == 0 {
                        return Ok(None);
                    }
                    *o += n as u8
                },
                ReadState::ReadVal(o) if o >= self.buffer.len() => {
                    self.state = ReadState::new();
                    return self.parse().map_err(|_| Error::DecodeFailed).map(Some);
                },
                ReadState::ReadVal(ref mut o) => {
                    let n = self.reader.read(&mut self.buffer[*o..]).await?;
                    if n == 0 {
                        return Err(Error::Io(std::io::ErrorKind::UnexpectedEof.into()));
                    }
                    *o += n
                },
            }
        }
    }
}

pub struct MessageWriter<W> {
    buffer: Vec<u8>,
    writer: W,
}

impl<W> MessageWriter<W> {
    pub fn new(w: W) -> MessageWriter<W> {
        MessageWriter {
            writer: w,
            buffer: Vec::new(),
        }
    }

    fn serialize<E: minicbor::Encode>(&mut self, msg: &Message<E>) {
        self.buffer.resize(4, 0u8);
        // SAFETY: We are writing to an in memory buffer and the only thing that can go
        // wrong is the minicbor::Encode impl for the headers being broken, in
        // which case _shrug_
        minicbor::encode(&msg.headers, &mut self.buffer).unwrap();
        if let Some(payload) = &msg.payload {
            self.buffer.extend(payload);
        }
        let prefix = (self.buffer.len() as u32 - 4).to_be_bytes();
        self.buffer[..4].copy_from_slice(&prefix);
    }
}

impl<W: AsyncWrite + Unpin> MessageWriter<W> {
    /// # Cancellation
    ///
    /// This is not cancel safe. Dropping this future will leave incomplete
    /// messages in the buffer.
    pub(crate) async fn write_message<E: minicbor::Encode>(
        &mut self,
        msg: &Message<E>,
    ) -> Result<(), std::io::Error> {
        self.serialize(msg);
        let mut offset_written = 0;
        while offset_written < self.buffer.len() {
            let n = self.writer.write(&self.buffer[offset_written..]).await?;
            if n == 0 {
                return Err(std::io::ErrorKind::WriteZero.into());
            }
            offset_written += n;
        }
        Ok(())
    }
}

#[async_trait]
pub trait Transport {
    type Error;

    /// Send a request to the remote
    ///
    /// # Cancellation
    ///
    /// This method may not be cancel safe
    async fn send_request(&mut self, request: messages::Request) -> Result<(), Self::Error>;

    /// Receive a request messages. A `None` return indicates that the
    /// connection has closed
    ///
    /// # Cancellation
    ///
    /// This method must be cancel safe
    async fn recv_request(&mut self) -> Result<Option<messages::Request>, Self::Error>;

    /// Send a response message to the remote
    ///
    /// # Cancellation
    ///
    /// This method may not be cancel safe
    async fn send_response<P>(
        &mut self,
        response: messages::Response<P>,
    ) -> Result<(), Self::Error>
    where
        P: messages::SendPayload;

    /// Receive a message from the remote. A return value of `None` indicates
    /// that the connection has closed
    ///
    /// # Cancellation
    ///
    /// This method must be cancel safe
    async fn recv_response<P>(&mut self) -> Result<Option<messages::Response<P>>, Self::Error>
    where
        P: messages::RecvPayload;
}

pub struct SocketTransport {
    reader: MessageReader<Compat<tokio::net::unix::OwnedReadHalf>>,
    writer: MessageWriter<Compat<tokio::net::unix::OwnedWriteHalf>>,
}

impl From<tokio::net::UnixStream> for SocketTransport {
    fn from(s: tokio::net::UnixStream) -> Self {
        let (rx, sx) = s.into_split();
        Self {
            reader: MessageReader::new(rx.compat()),
            writer: MessageWriter::new(sx.compat()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SocketTransportError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unable to decode message")]
    DecodeFailed,
}

impl SocketTransport {
    fn process_recv_response<R>(
        &mut self,
        msg_result: Result<Option<wire_types::Response>, Error>,
    ) -> Result<Option<messages::Response<R>>, SocketTransportError>
    where
        R: messages::RecvPayload,
    {
        let wire_message: Option<wire_types::Response> = msg_result.map_err(|e| {
            tracing::error!(err=?e, "failed to decode wire type");
            SocketTransportError::DecodeFailed
        })?;
        if let Some(wire_message) = wire_message {
            let message = wire_message.try_into().map_err(|e| {
                tracing::error!(err=?e, "failed to decode response from wire type");
                SocketTransportError::DecodeFailed
            })?;
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    fn process_recv_request(
        &mut self,
        msg_result: Result<Option<wire_types::Request>, Error>,
    ) -> Result<Option<messages::Request>, SocketTransportError> {
        let wire_message: Option<wire_types::Request> = msg_result.map_err(|e| {
            tracing::error!(err=?e, "failed to decode wire type");
            SocketTransportError::DecodeFailed
        })?;
        if let Some(wire_message) = wire_message {
            let message: messages::Request = wire_message.try_into().map_err(|e| {
                tracing::error!(err=?e, "failed to decode response from wire type");
                SocketTransportError::DecodeFailed
            })?;
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl Transport for SocketTransport {
    type Error = SocketTransportError;

    async fn send_request(&mut self, request: messages::Request) -> Result<(), Self::Error> {
        let wire_message: wire_types::Request = request.into();
        self.writer.write_message(&wire_message).await?;
        Ok(())
    }

    async fn recv_request(&mut self) -> Result<Option<messages::Request>, Self::Error> {
        let wire_message = self.reader.read_message().await;
        self.process_recv_request(wire_message)
    }

    async fn send_response<P>(&mut self, response: messages::Response<P>) -> Result<(), Self::Error>
    where
        P: messages::SendPayload,
    {
        let wire_message: wire_types::Response = response.into();
        self.writer.write_message(&wire_message).await?;
        Ok(())
    }

    async fn recv_response<P>(&mut self) -> Result<Option<messages::Response<P>>, Self::Error>
    where
        P: messages::RecvPayload,
    {
        let wire_message = self.reader.read_message().await;
        self.process_recv_response(wire_message)
    }
}
