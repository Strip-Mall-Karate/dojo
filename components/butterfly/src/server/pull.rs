// Copyright (c) 2017 Chef Software Inc. and/or applicable contributors
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

//! Receiving rumors.
//!
//! Every member listens on its gossip port and reads the rumors other members
//! push to it, one task per connection.

use std::{sync::LazyLock,
          time::Duration};

use dojo_core::util::ToI64;
use futures_util::StreamExt;
use prometheus::{IntCounterVec,
                 IntGaugeVec};
use tokio::{net::{TcpListener,
                  TcpStream},
            time};
use tokio_util::codec::FramedRead;

use crate::{gossip,
            rumor::{RumorEnvelope,
                    RumorKind},
            server::Server,
            trace::TraceKind};

static GOSSIP_MESSAGES_RECEIVED: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!("doj_butterfly_gossip_messages_received_total",
                              "Total number of gossip messages received",
                              &["type", "mode", "blocked"]).unwrap()
});
static GOSSIP_BYTES_RECEIVED: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!("doj_butterfly_gossip_received_bytes",
                            "Gossip message size received in bytes",
                            &["type", "mode", "blocked"]).unwrap()
});

/// Accepts gossip connections on `listener` and reads the rumors each one
/// carries, until the runtime shuts down.
pub async fn run(server: Server, listener: TcpListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(read_rumors(server.clone(), stream));
            }
            Err(e) => error!("Cannot accept a gossip connection: {}", e),
        }
    }
}

/// Reads frames from one sender until it closes the connection, sends a bad
/// frame, or goes quiet for longer than `gossip::RECV_TIMEOUT`. A paused
/// server holds each frame until it resumes, as a partitioned member would
/// receive it late.
async fn read_rumors(server: Server, stream: TcpStream) {
    let peer = stream.peer_addr().ok();
    let mut frames = FramedRead::new(stream, gossip::codec());
    loop {
        let frame = match time::timeout(gossip::RECV_TIMEOUT, frames.next()).await {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(e))) => {
                error!("Cannot read a rumor from {:?}: {}", peer, e);
                return;
            }
            Ok(None) => return,
            Err(_) => {
                debug!("Closing an idle gossip connection from {:?}", peer);
                return;
            }
        };
        while server.paused() {
            time::sleep(Duration::from_millis(100)).await;
        }
        receive(&server, &frame);
    }
}

/// Unwraps, decodes and records one rumor.
fn receive(server: &Server, frame: &[u8]) {
    let payload = match server.unwrap_wire(frame) {
        Ok(payload) => payload,
        Err(e) => {
            // NOTE: In the future, we might want to block people who send us
            // garbage all the time.
            error!("Error parsing protocol message: {:?}", e);
            let label_values = &["unwrap_wire", "failure", "unknown"];
            GOSSIP_BYTES_RECEIVED.with_label_values(label_values)
                                 .set(frame.len().to_i64());
            GOSSIP_MESSAGES_RECEIVED.with_label_values(label_values)
                                    .inc();
            return;
        }
    };

    let proto = match RumorEnvelope::decode(&payload) {
        Ok(proto) => proto,
        Err(e) => {
            error!("Error parsing protocol message: {:?}", e);
            let label_values = &["undecodable", "failure", "unknown"];
            GOSSIP_BYTES_RECEIVED.with_label_values(label_values)
                                 .set(payload.len().to_i64());
            GOSSIP_MESSAGES_RECEIVED.with_label_values(label_values)
                                    .inc();
            return;
        }
    };

    let blocked = server.is_member_blocked(&proto.from_id);
    let blocked_label = if blocked { "true" } else { "false" };
    let label_values = &[&proto.r#type.to_string(), "success", blocked_label];

    GOSSIP_MESSAGES_RECEIVED.with_label_values(label_values)
                            .inc();
    GOSSIP_BYTES_RECEIVED.with_label_values(label_values)
                         .set(payload.len().to_i64());

    if blocked {
        warn!("Not processing message from {} - it is blocked",
              proto.from_id);
        return;
    }

    trace_it!(GOSSIP: server, TraceKind::RecvRumor, &proto.from_id, &proto);
    match proto.kind {
        RumorKind::Membership(membership) => {
            server.insert_member_from_rumor(membership.member, membership.health);
        }
        RumorKind::Service(service) => server.insert_service(*service),
        RumorKind::ServiceConfig(service_config) => {
            server.insert_service_config(service_config);
        }
        RumorKind::ServiceFile(service_file) => {
            server.insert_service_file(service_file);
        }
        RumorKind::Election(election) => {
            server.insert_election(election);
        }
        RumorKind::ElectionUpdate(election) => {
            server.insert_update_election(election);
        }
        RumorKind::Departure(departure) => {
            server.insert_departure(departure);
        }
    }
}
