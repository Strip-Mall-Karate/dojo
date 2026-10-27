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

//! Listening server for receiving client connections which speak SrvProtocol.
//!
//! The server runs on the Supervisor's runtime and dispatches operational commands to the
//! Manager over an unbounded mpsc channel, `MgrSender`, to `MgrReceiver`. These commands are
//! wrapped in a [`ctl_gateway.CtlRequest`] if they are transactional.
//!
//! Replies to transactional messages are sent back to the connection over an unbounded mpsc
//! channel, [`CtlSender`], to [`CtlReceiver`]. A new mpsc pair is created for each transactional
//! request where the sending half is given to a [`ctl_gateway.CtlRequest`].

use super::{CtlRequest,
            REQ_TIMEOUT};
use crate::manager::{commands,
                     ManagerState};
use dojo_core::crypto;
use dojo_sup_protocol::{self as protocol,
                        codec::*,
                        net::{self,
                              ErrCode,
                              NetErr,
                              NetResult}};
use futures_util::{SinkExt,
                   StreamExt};
use prometheus::{HistogramVec,
                 IntCounterVec};
use prost;
use std::{error,
          fmt,
          io,
          sync::Arc,
          time::Duration};
use tokio::{net::{TcpListener,
                  TcpStream},
            sync::mpsc,
            time};
use tokio_util::codec::Framed;

lazy_static! {
    static ref RPC_CALLS: IntCounterVec = register_int_counter_vec!("doj_sup_rpc_call_total",
                                                                    "Total number of RPC calls",
                                                                    &["name"]).unwrap();
    static ref RPC_CALL_DURATION: HistogramVec =
        register_histogram_vec!("doj_sup_rpc_call_request_duration_seconds",
                                "The latency for RPC calls",
                                &["name"]).unwrap();
}

/// A `TcpStream` framed with `SrvCodec`. This is the socket connection that the CtlGateway
/// server speaks.
type SrvStream = Framed<TcpStream, SrvCodec>;

/// Sending half of an mpsc unbounded channel used for sending replies for a transactional message
/// from the Manager back to the CtlGateway. This half is stored in a [`ctl_gateway.CtlRequest`].
pub type CtlSender = mpsc::UnboundedSender<SrvMessage>;
/// Receiving half of an mpsc unbounded channel used for sending replies for a transactional
/// message from the Manager back to the CtlGateway. The connection that made the request holds
/// it.
pub type CtlReceiver = mpsc::UnboundedReceiver<SrvMessage>;
/// Sender from the CtlGateway to the Manager to dispatch control commands for clients.
pub type MgrSender = mpsc::UnboundedSender<CtlCommand>;
/// Receiver on the Manager for the sender on the CtlGateway to receive control commands.
pub type MgrReceiver = mpsc::UnboundedReceiver<CtlCommand>;

#[derive(Debug)]
pub enum HandlerError {
    Decode(prost::DecodeError),
    Io(io::Error),
    NetErr(NetErr),
    SendError(mpsc::error::SendError<CtlCommand>),
}

impl error::Error for HandlerError {}

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let content = match *self {
            HandlerError::Decode(ref err) => format!("{}", err),
            HandlerError::Io(ref err) => format!("{}", err),
            HandlerError::NetErr(ref err) => format!("{}", err),
            HandlerError::SendError(ref err) => format!("{}", err),
        };
        write!(f, "{}", content)
    }
}

impl From<NetErr> for HandlerError {
    fn from(err: NetErr) -> Self { HandlerError::NetErr(err) }
}

impl From<io::Error> for HandlerError {
    fn from(err: io::Error) -> Self { HandlerError::Io(err) }
}

impl From<prost::DecodeError> for HandlerError {
    fn from(err: prost::DecodeError) -> Self { HandlerError::Decode(err) }
}

impl From<mpsc::error::SendError<CtlCommand>> for HandlerError {
    fn from(err: mpsc::error::SendError<CtlCommand>) -> Self { HandlerError::SendError(err) }
}

/// A wrapper around a [`ctl_gateway.CtlRequest`] and a closure for the Manager to execute.
pub struct CtlCommand {
    pub req: CtlRequest,
    fun:     Box<dyn Fn(&ManagerState, &mut CtlRequest) -> NetResult<()> + Send>,
}

impl CtlCommand {
    /// Create a new CtlCommand from the given CtlSender, transaction, and closure to execute.
    pub fn new<F>(tx: Option<CtlSender>, txn: Option<SrvTxn>, fun: F) -> Self
        where F: Fn(&ManagerState, &mut CtlRequest) -> NetResult<()> + Send + 'static
    {
        CtlCommand { fun: Box::new(fun),
                     req: CtlRequest::new(tx, txn), }
    }

    /// Run the contained closure with the given [`manager.ManagerState`].
    pub fn run(&mut self, state: &ManagerState) -> NetResult<()> {
        (self.fun)(state, &mut self.req)
    }
}

/// Accept connections on `listener` until the runtime shuts down.
///
/// New connections are authenticated using `secret_key`. Commands are sent to the Manager over
/// the channel `mgr_tx`.
pub async fn run(listener: TcpListener, secret_key: String, mgr_tx: MgrSender) {
    let secret_key = Arc::new(secret_key);
    loop {
        let (socket, addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(err) => {
                // Running out of file descriptors fails accept until some close.
                error!("ctl-gateway accept failed: {}", err);
                time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let secret_key = Arc::clone(&secret_key);
        let mgr_tx = mgr_tx.clone();
        tokio::spawn(async move {
            let res = serve(Framed::new(socket, SrvCodec::new()), &secret_key, mgr_tx).await;
            debug!("DISCONNECTED from {:?} with result {:?}", addr, res);
        });
    }
}

/// Serve one client: a handshake, then one request whose replies stream back until the last.
async fn serve(mut io: SrvStream, secret_key: &str, mgr_tx: MgrSender) -> Result<(), HandlerError> {
    match time::timeout(Duration::from_millis(REQ_TIMEOUT),
                        handshake(&mut io, secret_key)).await
    {
        Ok(result) => result?,
        Err(_) => {
            return Err(HandlerError::from(io::Error::new(io::ErrorKind::TimedOut,
                                                         "client timed out")))
        }
    }

    let msg = match io.next().await {
        Some(msg) => msg?,
        None => return Ok(()),
    };
    let label_values = &[msg.message_id()];
    RPC_CALLS.with_label_values(label_values).inc();
    let timer = RPC_CALL_DURATION.with_label_values(label_values)
                                 .start_timer();
    trace!("OnMessage, {}", msg.message_id());

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cmd = match command(&msg, &tx)? {
        Some(cmd) => cmd,
        None => {
            warn!("Unhandled message, {}", msg.message_id());
            return Ok(());
        }
    };
    // Only the command's request may reply, so the channel closes when the Manager is done
    // with it.
    drop(tx);
    if let Err(err) = mgr_tx.send(cmd) {
        // The Manager stopped receiving commands, most often because the Supervisor is in the
        // middle of an orderly shutdown.
        warn!("ManagerReceiver err: {}", err);
        return Err(HandlerError::from(err));
    }

    while let Some(reply) = rx.recv().await {
        trace!("MgrSender -> SrvHandler, {:?}", reply);
        let complete = reply.is_complete();
        io.send(reply).await?;
        if complete {
            break;
        }
    }
    timer.observe_duration();
    trace!("OnMessage complete");
    Ok(())
}

/// Authenticate a newly connected client before allowing requests. A failed handshake closes
/// the connection.
async fn handshake(io: &mut SrvStream, secret_key: &str) -> Result<(), HandlerError> {
    let aborted = || HandlerError::from(io::Error::from(io::ErrorKind::ConnectionAborted));
    let m = match io.next().await {
        Some(m) => m?,
        None => return Err(HandlerError::from(io::Error::from(io::ErrorKind::UnexpectedEof))),
    };
    if m.message_id() != "Handshake" {
        debug!("No handshake");
        return Err(aborted());
    }
    let txn = match m.transaction() {
        Some(txn) => txn,
        None => return Err(aborted()),
    };
    let success = match m.parse::<protocol::ctl::Handshake>() {
        Ok(decoded) => {
            trace!("Received handshake, {:?}", decoded);
            crypto::secure_eq(decoded.secret_key.unwrap_or_default(), secret_key)
        }
        Err(err) => {
            warn!("Handshake error, {:?}", err);
            return Err(aborted());
        }
    };
    let mut reply = if success {
        SrvMessage::from(net::ok())
    } else {
        SrvMessage::from(net::err(ErrCode::Unauthorized, "secret key mismatch"))
    };
    reply.reply_for(txn, true);
    io.send(reply).await?;
    if success {
        Ok(())
    } else {
        Err(HandlerError::from(io::Error::new(io::ErrorKind::ConnectionAborted,
                                              "handshake failed")))
    }
}

/// The command for a client's request, or `None` for a request the Supervisor doesn't handle.
fn command(msg: &SrvMessage, tx: &CtlSender) -> Result<Option<CtlCommand>, HandlerError> {
    let cmd = match msg.message_id() {
        "SvcGetDefaultCfg" => {
            let m = msg.parse::<protocol::ctl::SvcGetDefaultCfg>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_cfg(state, req, m.clone())
                 }))
        }
        "SvcFilePut" => {
            let m = msg.parse::<protocol::ctl::SvcFilePut>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_file_put(state, req, m.clone())
                 }))
        }
        "SvcSetCfg" => {
            let m = msg.parse::<protocol::ctl::SvcSetCfg>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_cfg_set(state, req, m.clone())
                 }))
        }
        "SvcValidateCfg" => {
            let m = msg.parse::<protocol::ctl::SvcValidateCfg>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_cfg_validate(state, req, m.clone())
                 }))
        }
        "SvcLoad" => {
            let m = msg.parse::<protocol::ctl::SvcLoad>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_load(state, req, &m)
                 }))
        }
        "SvcUnload" => {
            let m = msg.parse::<protocol::ctl::SvcUnload>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_unload(state, req, m.clone())
                 }))
        }
        "SvcStart" => {
            let m = msg.parse::<protocol::ctl::SvcStart>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_start(state, req, m.clone())
                 }))
        }
        "SvcStop" => {
            let m = msg.parse::<protocol::ctl::SvcStop>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_stop(state, req, m.clone())
                 }))
        }
        "SvcStatus" => {
            let m = msg.parse::<protocol::ctl::SvcStatus>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::service_status(state, req, m.clone())
                 }))
        }
        "SupDepart" => {
            let m = msg.parse::<protocol::ctl::SupDepart>()
                       .map_err(HandlerError::from)?;
            Some(CtlCommand::new(Some(tx.clone()), msg.transaction(), move |state, req| {
                     commands::supervisor_depart(state, req, m.clone())
                 }))
        }
        _ => None,
    };
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dojo_common::types::ListenCtlAddr;
    use dojo_sup_client::{SrvClient,
                          SrvClientError};
    use tokio::runtime::Runtime;

    const SECRET: &str = "the-secret";

    /// Start a gateway on a free port, with a stand-in Manager that answers each command with
    /// `replies` messages, the last complete. With no replies it drops the command unanswered.
    fn gateway(runtime: &Runtime, replies: usize) -> ListenCtlAddr {
        let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();
        let (mgr_tx, mut mgr_rx) = mpsc::unbounded_channel::<CtlCommand>();
        runtime.spawn(run(listener, SECRET.to_string(), mgr_tx));
        runtime.spawn(async move {
                   while let Some(mut cmd) = mgr_rx.recv().await {
                       for reply in 1..=replies {
                           if reply == replies {
                               cmd.req.reply_complete(net::ok());
                           } else {
                               cmd.req.reply_partial(net::ok());
                           }
                       }
                   }
               });
        ListenCtlAddr::from(addr)
    }

    #[test]
    fn a_call_streams_every_reply() {
        let runtime = Runtime::new().unwrap();
        let addr = gateway(&runtime, 3);

        let mut client = SrvClient::connect(&addr, SECRET).unwrap();
        let replies = client.call(protocol::ctl::SvcStatus::default())
                            .unwrap()
                            .collect::<Result<Vec<_>, _>>()
                            .unwrap();

        assert_eq!(replies.len(), 3);
        assert!(replies.iter().all(|r| r.message_id() == "NetOk"));
        assert!(replies[2].is_complete());
        assert!(!replies[1].is_complete());
    }

    #[test]
    fn a_wrong_secret_is_refused() {
        let runtime = Runtime::new().unwrap();
        let addr = gateway(&runtime, 1);

        match SrvClient::connect(&addr, "not-the-secret") {
            Err(SrvClientError::NetErr(err)) => {
                assert_eq!(err.code, ErrCode::Unauthorized as i32)
            }
            Err(err) => panic!("Expected Unauthorized, got {}", err),
            Ok(_) => panic!("Expected Unauthorized, but the handshake succeeded"),
        }
    }

    #[test]
    fn the_connection_closes_when_the_manager_drops_a_request() {
        let runtime = Runtime::new().unwrap();
        let addr = gateway(&runtime, 0);

        let mut client = SrvClient::connect(&addr, SECRET).unwrap();
        let replies = client.call(protocol::ctl::SvcStatus::default())
                            .unwrap()
                            .collect::<Vec<_>>();

        assert_eq!(replies.len(), 1);
        assert!(matches!(replies[0], Err(SrvClientError::ConnectionClosed)));
    }
}
