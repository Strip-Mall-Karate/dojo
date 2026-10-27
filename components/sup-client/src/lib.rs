// Copyright (c) 2018 Chef Software Inc. and/or applicable contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Client for connecting and communicating with a server listener which speaks SrvProtocol.
//!
//! All calls block until the remote answers. The Supervisor serves one request per connection,
//! so a client connects, makes one call and drops the connection.
//!
//! # RPC Call Example
//!
//! ```ignore
//! let mut conn = SrvClient::connect(&listen_addr, secret_key)?;
//! let msg = protocols::ctl::SvcGetDefaultCfg::default();
//! for reply in conn.call(msg)? {
//!     let reply = reply?;
//!     match reply.message_id() {
//!         "ServiceCfg" => {
//!             let m = reply.parse::<protocols::types::ServiceCfg>()?;
//!             println!("{:?}", m.default);
//!         }
//!         "NetErr" => {
//!             let m = reply.parse::<protocols::net::NetErr>()?;
//!             println!("{}", m);
//!         }
//!         _ => (),
//!     }
//! }
//! ```

use dojo_sup_protocol as protocol;
#[macro_use]
extern crate log;
use dojo_common as common;

use std::{error,
          fmt,
          io::{self,
               Read,
               Write},
          net::TcpStream,
          path::PathBuf};

use crate::protocol::{codec::*,
                      net::NetErr};
use bytes::BytesMut;
use tokio_util::codec::{Decoder,
                        Encoder};

use crate::common::types::ListenCtlAddr;

/// Bytes read from the socket at a time.
const READ_CHUNK: usize = 8 * 1024;

/// Error types returned by a [`SrvClient`].
#[derive(Debug)]
pub enum SrvClientError {
    /// The remote server unexpectedly closed the connection.
    ConnectionClosed,
    /// Unable to locate a secret key on disk.
    CtlSecretNotFound(PathBuf),
    /// Decoding a message from the remote failed.
    Decode(prost::DecodeError),
    /// An Os level IO error occurred.
    Io(io::Error),
    /// An RPC call to the remote was received but failed.
    NetErr(NetErr),
    /// A parse error from an Invalid Color string
    ParseColor(termcolor::ParseColorError),
}

impl error::Error for SrvClientError {}

impl fmt::Display for SrvClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let content = match *self {
            SrvClientError::ConnectionClosed => {
                "The connection was unexpectedly closed.\n\nThis may be because the given \
                 Supervisor is in the middle of an orderly shutdown,\nand is no longer processing \
                 command requests."
                                   .to_string()
            }
            SrvClientError::CtlSecretNotFound(ref path) => {
                format!("No Supervisor CtlGateway secret set in `cli.toml` or found at {}. Run \
                         `doj setup` or run the Supervisor for the first time before attempting \
                         to command the Supervisor.",
                        path.display())
            }
            SrvClientError::Decode(ref err) => format!("{}", err),
            SrvClientError::Io(ref err) => {
                format!("Unable to contact the Supervisor.\n\nIf the Supervisor you are \
                         contacting is local, this probably means it is not running. You can run \
                         a Supervisor in the foreground with:\n\ndoj sup run\n\nOr try restarting \
                         the Supervisor through your operating system's init process.\n\nOriginal \
                         error is:\n\n{}",
                        err)
            }
            SrvClientError::NetErr(ref err) => format!("{}", err),
            SrvClientError::ParseColor(ref err) => format!("{}", err),
        };
        write!(f, "{}", content)
    }
}

impl From<NetErr> for SrvClientError {
    fn from(err: NetErr) -> Self { SrvClientError::NetErr(err) }
}

impl From<io::Error> for SrvClientError {
    fn from(err: io::Error) -> Self { SrvClientError::Io(err) }
}

impl From<prost::DecodeError> for SrvClientError {
    fn from(err: prost::DecodeError) -> Self { SrvClientError::Decode(err) }
}

impl From<termcolor::ParseColorError> for SrvClientError {
    fn from(err: termcolor::ParseColorError) -> Self { SrvClientError::ParseColor(err) }
}

/// Client for connecting and communicating with a server listener which speaks SrvProtocol.
///
/// See module doc for usage.
pub struct SrvClient {
    /// Socket connected to a remote server.
    socket:      TcpStream,
    /// Bytes read from the socket but not yet decoded into a message.
    read_buf:    BytesMut,
    /// Transaction ID counter.
    ///
    /// Useful for determining the next transaction ID in sequence embed in
    /// the next transactional message sent.
    current_txn: SrvTxn,
}

impl SrvClient {
    /// Connect to the given remote server and authenticate with the given secret_key.
    pub fn connect(addr: &ListenCtlAddr, secret_key: &str) -> Result<SrvClient, SrvClientError> {
        let socket = TcpStream::connect(addr.as_ref())?;
        let mut client = SrvClient { socket,
                                     read_buf: BytesMut::new(),
                                     current_txn: SrvTxn::default() };
        let mut request = protocol::ctl::Handshake::default();
        request.secret_key = Some(secret_key.to_string());
        let reply = client.call(request)?
                          .next()
                          .unwrap_or(Err(SrvClientError::ConnectionClosed))?;
        reply.try_ok()?;
        Ok(client)
    }

    pub fn read_secret_key() -> Result<String, SrvClientError> {
        let mut buf = String::new();
        protocol::read_secret_key(protocol::sup_root(None), &mut buf)
            .map_err(SrvClientError::from)?;
        Ok(buf)
    }

    /// Send a transactional request to the connected server. The returned `SrvReply` yields one
    /// or more `SrvMessage` responses for the given request.
    pub fn call<T>(&mut self, request: T) -> Result<SrvReply<'_>, SrvClientError>
        where T: Into<SrvMessage> + fmt::Debug
    {
        self.current_txn.increment();
        let mut msg: SrvMessage = request.into();
        msg.set_transaction(self.current_txn);
        self.send(msg)?;
        Ok(SrvReply { client:   self,
                      complete: false, })
    }

    /// Send a non-transactional request to the connected server.
    pub fn cast<T>(&mut self, request: T) -> Result<(), SrvClientError>
        where T: Into<SrvMessage> + fmt::Debug
    {
        self.send(request.into())
    }

    fn send(&mut self, msg: SrvMessage) -> Result<(), SrvClientError> {
        trace!("Sending SrvMessage -> {:?}", msg);
        let mut buf = BytesMut::with_capacity(msg.size());
        SrvCodec::new().encode(msg, &mut buf)?;
        self.socket.write_all(&buf)?;
        Ok(())
    }

    /// Block until the next whole message arrives.
    fn receive(&mut self) -> Result<SrvMessage, SrvClientError> {
        let mut chunk = [0; READ_CHUNK];
        loop {
            if let Some(msg) = SrvCodec::new().decode(&mut self.read_buf)? {
                return Ok(msg);
            }
            match self.socket.read(&mut chunk)? {
                0 => return Err(SrvClientError::ConnectionClosed),
                n => self.read_buf.extend_from_slice(&chunk[..n]),
            }
        }
    }
}

/// The replies to one transactional request, ending after the message marked complete.
pub struct SrvReply<'a> {
    client:   &'a mut SrvClient,
    complete: bool,
}

impl Iterator for SrvReply<'_> {
    type Item = Result<SrvMessage, SrvClientError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.complete {
            return None;
        }
        let reply = self.client.receive();
        self.complete = reply.as_ref().map_or(true, SrvMessage::is_complete);
        Some(reply)
    }
}
