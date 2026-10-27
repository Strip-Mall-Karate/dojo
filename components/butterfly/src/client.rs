// Copyright (c) 2016-2017 Chef Software Inc. and/or applicable contributors
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

//! The Butterfly client library.
//!
//! This connects to a given butterfly member's gossip port and injects a rumor.

use std::{io,
          net::{SocketAddr,
                TcpStream,
                ToSocketAddrs}};

use dojo_core::{crypto::SymKey,
                service::ServiceGroup};

use crate::{error::{Error,
                    Result},
            gossip,
            message,
            rumor::{departure::Departure,
                    service_config::ServiceConfig,
                    service_file::ServiceFile,
                    Rumor}};

/// Sends rumors to one member's gossip port, optionally encrypting them with a ring key. Each
/// send opens its own connection.
pub struct Client {
    addr:     SocketAddr,
    ring_key: Option<SymKey>,
}

impl Client {
    /// A client for the member at `addr`, encrypting the traffic if given a ring key.
    pub fn new(addr: &str, ring_key: Option<SymKey>) -> Result<Client> {
        let addr = addr.to_socket_addrs()
                       .map_err(Error::GossipConnectError)?
                       .next()
                       .ok_or_else(|| {
                           Error::GossipConnectError(io::Error::new(io::ErrorKind::NotFound,
                                                                    format!("{} names no \
                                                                             address",
                                                                            addr)))
                       })?;
        Ok(Client { addr, ring_key })
    }

    /// Create a departure notification and send it to the server.
    pub fn send_departure(&mut self, member_id: &str) -> Result<()> {
        let departure = Departure::new(member_id);
        self.send(&departure)
    }

    /// Create a service configuration and send it to the server.
    pub fn send_service_config(&mut self,
                               service_group: ServiceGroup,
                               incarnation: u64,
                               config: &[u8],
                               encrypted: bool)
                               -> Result<()> {
        let mut sc = ServiceConfig::new("butterflyclient", service_group, config.to_vec());
        sc.incarnation = incarnation;
        sc.encrypted = encrypted;
        self.send(&sc)
    }

    /// Create a service file and send it to the server.
    pub fn send_service_file<S>(&mut self,
                                service_group: ServiceGroup,
                                filename: S,
                                incarnation: u64,
                                body: &[u8],
                                encrypted: bool)
                                -> Result<()>
        where S: Into<String>
    {
        let mut sf = ServiceFile::new("butterflyclient", service_group, filename, body.to_vec());
        sf.incarnation = incarnation;
        sf.encrypted = encrypted;
        self.send(&sf)
    }

    /// Send any `Rumor` to the server.
    pub fn send<T>(&mut self, rumor: &T) -> Result<()>
        where T: Rumor
    {
        let bytes = rumor.write_to_bytes()?;
        let wire_msg = message::generate_wire(bytes, self.ring_key.as_ref())?;
        let mut stream = TcpStream::connect_timeout(&self.addr, gossip::SEND_TIMEOUT)
            .map_err(Error::GossipConnectError)?;
        stream.set_write_timeout(Some(gossip::SEND_TIMEOUT))
              .map_err(Error::GossipSendError)?;
        gossip::write_frame(&mut stream, &wire_msg).map_err(Error::GossipSendError)
    }
}
