// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! This module is the client interface to the p2p node RPC API. The APIs here
//! are designed to work in both an asynchronous and synchronous context. To
//! start you'll need to create a [`Connection`] by calling either
//! [`Connection::connect`]. Once you have a connection you then create a
//! command using the `commands::*` functions. A command then has various
//! methods on it which determine exactly how the command should be executed.
//!
//! See the documentation of [`Command`] for more information.

use std::{marker::PhantomData, net::SocketAddr};

use git_ext::Oid;
use radicle_git_ext as git_ext;
use tokio::net::UnixStream;

use librad::{git::Urn, PeerId};

use super::{announce, io, messages, request_pull};

pub struct Connection<T> {
    socket: T,
    user_agent: messages::UserAgent,
}

impl Connection<io::SocketTransport> {
    /// Asynchronously connect to the domain socket given by `socket_path`. The
    /// `user_agent` will be used to identify this client in log messages so
    /// it's best to choose something unique. This method will block until a
    /// connection is made.
    ///
    /// # Panics
    ///
    /// This function panics if no tokio runtime is available
    pub async fn connect<U: ToString, P: AsRef<std::path::Path>>(
        user_agent: U,
        socket_path: P,
    ) -> Result<Self, std::io::Error> {
        let stream = UnixStream::connect(socket_path).await?;
        Ok(Self {
            socket: io::SocketTransport::from(stream),
            user_agent: user_agent.to_string().into(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReplyError<T> {
    #[error(transparent)]
    Transport(T),
    #[error("no reply when one was expected")]
    MissingReply,
    #[error("unexpected ack response")]
    UnexpectedAck,
    #[error("no ack received")]
    MissingAck,
}

#[derive(Debug, thiserror::Error)]
pub enum ExecuteError<T> {
    #[error(transparent)]
    Transport(T),
    #[error("no ack response")]
    MissingAck,
}

pub struct Replies<T, Response> {
    request_id: messages::RequestId,
    conn: Connection<T>,
    _marker: PhantomData<Response>,
}

impl<T, R> Replies<T, R> {
    fn process_recv<E>(
        self,
        msg: Option<messages::Response<R>>,
    ) -> Result<Reply<T, R>, (Connection<T>, ReplyError<E>)> {
        match msg {
            None => Err((self.conn, ReplyError::MissingReply)),
            Some(msg) => {
                debug_assert!(msg.request_id == self.request_id);
                match msg.payload {
                    messages::ResponsePayload::Ack => Err((self.conn, ReplyError::UnexpectedAck)),
                    messages::ResponsePayload::Progress(s) => Ok(Reply::Progress {
                        replies: self,
                        msg: s,
                    }),
                    messages::ResponsePayload::Error(s) => Ok(Reply::Error {
                        conn: self.conn,
                        msg: s,
                    }),
                    messages::ResponsePayload::Success(payload) => Ok(Reply::Success {
                        conn: self.conn,
                        payload,
                    }),
                }
            },
        }
    }
}

impl<T, R> Replies<T, R>
where
    T: io::Transport,
    R: messages::RecvPayload,
{
    /// Asynchronously wait for a message from the server which we expect in
    /// response to a message. A value of `Ok(Reply<T>)` indicates that we
    /// received a message and you should match on the `Reply` to decide
    /// what to do next. A return value of `(Connection, ReplyError)` indicates
    /// that there was some kind of transport error.
    pub async fn next(mut self) -> Result<Reply<T, R>, (Connection<T>, ReplyError<T::Error>)> {
        match self.conn.socket.recv_response().await {
            Err(e) => Err((self.conn, ReplyError::Transport(e))),
            Ok(msg) => self.process_recv(msg),
        }
    }
}

/// State of an in progress request which we expect to return a response
pub enum Reply<T, Response> {
    /// The server returned a "progress" message
    Progress {
        replies: Replies<T, Response>,
        msg: String,
    },
    /// The server indicated an error, no further messages will be sent
    Error { conn: Connection<T>, msg: String },
    /// The server indiciated that the call was successful, no further messages
    /// will be sent
    Success {
        conn: Connection<T>,
        payload: Response,
    },
}

pub struct Command<Request, Response> {
    payload: Request,
    _marker: PhantomData<Response>,
}

impl<Rq, Rs> Command<Rq, Rs>
where
    Rq: Into<messages::RequestPayload>,
    Rs: messages::RecvPayload,
{
    /// Asynchronously execute this command and set the request mode to "fire
    /// and forget". This means that the server will not send a response so
    /// you do not need to block and read the response.
    ///
    /// # Cancellation
    ///
    /// Cancelling may leave unfinished messages on the socket, this future is
    /// therefore not cancel safe.
    pub async fn execute<T>(self, conn: &mut Connection<T>) -> Result<(), ExecuteError<T::Error>>
    where
        T: io::Transport,
    {
        let req = self.request(&conn.user_agent, messages::RequestMode::FireAndForget);
        conn.socket
            .send_request(req)
            .await
            .map_err(ExecuteError::Transport)?;
        match conn
            .socket
            .recv_response::<Rs>()
            .await
            .map_err(ExecuteError::Transport)?
        {
            Some(resp) if matches!(resp.payload, messages::ResponsePayload::Ack) => Ok(()),
            _ => Err(ExecuteError::MissingAck),
        }
    }

    /// Asynchronously execute this command and wait for a response. Note that
    /// this consumes `conn`. This is deliberate. A successful request will
    /// return a [`Replies`], which exposes further methods to read
    /// responses from the server. For example:
    ///
    /// ```no_run
    /// # async fn dothings() {
    /// use node_lib::api::{io::SocketTransport, client::{Connection, Command, Reply}};
    ///
    /// let conn: Connection<SocketTransport> = Connection::connect("some user agent".to_string(), "<somepath>").await.unwrap();
    /// let command: Command = panic!("somehow create a command");
    /// let mut replies = command.execute_with_reply(conn).await.unwrap();
    /// let next_conn = loop {
    ///     match replies.next().await {
    ///         Ok(Reply::Progress{replies: next_replies, msg}) => {
    ///             println!("{}\n", msg);
    ///             replies = next_replies;
    ///         },
    ///         Ok(Reply::Error{conn, msg}) => {
    ///             println!("some error: {}\n", msg);
    ///             break conn;
    ///         },
    ///         Ok(Reply::Success{conn}) => break conn,
    ///         Err((conn, err)) => {
    ///             println!("transport error: {}\n", err);
    ///             break conn;
    ///         }
    ///     }
    /// };
    /// // Do more things with connection
    /// # }
    /// ```
    ///
    /// # Cancellation
    ///
    /// This method is not cancel safe
    pub async fn execute_with_reply<T>(
        self,
        mut conn: Connection<T>,
    ) -> Result<Replies<T, Rs>, ReplyError<T::Error>>
    where
        T: io::Transport,
    {
        let req = Self::request(
            self,
            &conn.user_agent,
            messages::RequestMode::ReportProgress,
        );
        conn.socket
            .send_request(req)
            .await
            .map_err(ReplyError::Transport)?;
        match conn
            .socket
            .recv_response::<Rs>()
            .await
            .map_err(ReplyError::Transport)?
        {
            Some(resp) if matches!(resp.payload, messages::ResponsePayload::Ack) => Ok(Replies {
                conn,
                request_id: resp.request_id,
                _marker: PhantomData,
            }),
            _ => Err(ReplyError::MissingAck),
        }
    }

    fn request(
        self,
        user_agent: &messages::UserAgent,
        mode: messages::RequestMode,
    ) -> messages::Request {
        messages::Request {
            user_agent: user_agent.clone(),
            mode,
            payload: self.payload.into(),
        }
    }
}

impl Command<announce::Request, announce::Response> {
    pub fn announce(urn: Urn, rev: Oid) -> Self {
        Self {
            payload: announce::Request { urn, rev },
            _marker: PhantomData,
        }
    }
}

impl Command<request_pull::Request, request_pull::Response> {
    pub fn request_pull(urn: Urn, peer: PeerId, addrs: Vec<SocketAddr>) -> Self {
        Self {
            payload: request_pull::Request { urn, peer, addrs },
            _marker: PhantomData,
        }
    }
}
