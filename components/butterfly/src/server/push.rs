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

//! Distributing rumors.
//!
//! Every gossip round sends each member's hot rumors to it, `FANOUT` members at a time and no more
//! often than `Timing::GOSSIP_PERIOD_DEFAULT_MS`.

use std::{sync::LazyLock,
          time::Duration};

use bytes::Bytes;
use dojo_core::util::ToI64;
use futures_util::SinkExt;
use prometheus::{IntCounterVec,
                 IntGaugeVec};
use tokio::{net::TcpStream,
            task::JoinSet,
            time};
use tokio_util::codec::FramedWrite;

use crate::{gossip,
            member::{Member,
                     Membership},
            rumor::{RumorEnvelope,
                    RumorKey,
                    RumorKind,
                    RumorType},
            server::{timing::Timing,
                     Server},
            trace::TraceKind};

const FANOUT: usize = 5;

static GOSSIP_MESSAGES_SENT: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!("doj_butterfly_gossip_messages_sent_total",
                              "Total number of gossip messages sent",
                              &["type", "mode"]).unwrap()
});
static GOSSIP_BYTES_SENT: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!("doj_butterfly_gossip_sent_bytes",
                            "Gossip message size sent in bytes",
                            &["type", "mode"]).unwrap()
});

/// The Push server
/// Sends hot rumors to every member each gossip round, until the runtime shuts down. Gets a list of
/// members to talk to that are not Confirmed, then sends to them in `FANOUT` sized chunks. If a
/// chunk finishes faster than `Timing::GOSSIP_PERIOD_DEFAULT_MS`, waits out the rest of it.
pub async fn run(server: Server, timing: Timing) {
    loop {
        if server.paused() {
            time::sleep(Duration::from_millis(100)).await;
            continue;
        }

        server.update_gossip_round();

        let mut check_list = server.member_list.check_list(server.member_id());
        let long_wait = timing.gossip_timeout();

        while !check_list.is_empty() {
            let mut sends = JoinSet::new();
            let drain_length = check_list.len().min(FANOUT);
            let next_gossip = timing.gossip_timeout();
            for member in check_list.drain(0..drain_length) {
                if server.is_member_blocked(&member.id) {
                    debug!("Not sending rumors to {} - it is blocked", member.id);
                    continue;
                }
                // Unlike the SWIM mechanism, we don't actually want to send gossip traffic to
                // persistent members that are confirmed dead. When the failure detector thread
                // finds them alive again, we'll go ahead and get back to the business at hand.
                if server.member_list.pingable(&member)
                   && !server.member_list.persistent_and_confirmed(&member)
                {
                    let rumors = server.rumor_heat.currently_hot_rumors(&member.id);
                    if !rumors.is_empty() {
                        sends.spawn(PushWorker::new(server.clone()).send_rumors(member, rumors));
                    }
                }
            }
            while let Some(sent) = sends.join_next().await {
                if let Err(e) = sent {
                    error!("Push worker died: {}", e);
                }
            }
            time::sleep_until(next_gossip.into()).await;
        }
        time::sleep_until(long_wait.into()).await;
    }
}

/// Sends rumors to one member
struct PushWorker {
    pub server: Server,
}

impl PushWorker {
    /// Create a new PushWorker.
    pub fn new(server: Server) -> PushWorker { PushWorker { server } }

    /// Sends the list of rumors to a given member over one connection, then closes it.
    async fn send_rumors(self, member: Member, rumors: Vec<RumorKey>) {
        let frames = self.frames(&member, &rumors);
        let to_addr = format!("{}:{}", member.address, member.gossip_port);
        let stream = match time::timeout(gossip::SEND_TIMEOUT, TcpStream::connect(&to_addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => return Self::connect_failed(&member, &e),
            Err(e) => return Self::connect_failed(&member, &e),
        };
        debug!("Connected to {:?} to send rumors", member);
        let mut sink = FramedWrite::new(stream, gossip::codec());
        for (kind, payload) in frames {
            let len = payload.len().to_i64();
            match time::timeout(gossip::SEND_TIMEOUT, sink.send(Bytes::from(payload))).await {
                Ok(Ok(())) => {
                    GOSSIP_MESSAGES_SENT.with_label_values(&[&kind.to_string(), "success"])
                                        .inc();
                    GOSSIP_BYTES_SENT.with_label_values(&[&kind.to_string(), "success"])
                                     .set(len);
                    debug!("Sent a {} rumor to {:?}", kind, member);
                }
                Ok(Err(e)) => {
                    warn!("Could not send rumor to {:?} @ {:?}: {}",
                          member.id, to_addr, e);
                    break;
                }
                Err(e) => {
                    warn!("Could not send rumor to {:?} @ {:?}: {}",
                          member.id, to_addr, e);
                    break;
                }
            }
        }
        self.server.rumor_heat.cool_rumors(&member.id, &rumors);
    }

    fn connect_failed(member: &Member, e: &dyn std::fmt::Display) {
        error!("Cannot connect to {:?} to send rumors: {}", member, e);
        let label_values = &["socket_connect", "failure"];
        GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
        GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
    }

    /// Encodes and wraps each rumor for sending, skipping any that cannot be.
    fn frames(&self, member: &Member, rumors: &[RumorKey]) -> Vec<(RumorType, Vec<u8>)> {
        let mut frames = Vec::with_capacity(rumors.len());
        'rumorlist: for rumor_key in rumors.iter() {
            let rumor_as_bytes = match rumor_key.kind {
                RumorType::Member => {
                    let send_rumor = match self.create_member_rumor(&rumor_key) {
                        Some(rumor) => rumor,
                        None => continue 'rumorlist,
                    };
                    trace_it!(GOSSIP: &self.server,
                              TraceKind::SendRumor,
                              &member.id,
                              &send_rumor);
                    match send_rumor.encode() {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Could not write our own rumor to bytes; abandoning sending \
                                    rumor: {:?}",
                                   e);
                            let label_values = &["member_rumor_encode", "failure"];
                            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                            continue 'rumorlist;
                        }
                    }
                }
                RumorType::Service => {
                    // trace_it!(GOSSIP: &self.server,
                    //           TraceKind::SendRumor,
                    //           &member.id,
                    //           &send_rumor);
                    match self.server
                              .service_store
                              .encode(&rumor_key.key, &rumor_key.id)
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Could not write our own rumor to bytes; abandoning sending \
                                    rumor: {:?}",
                                   e);
                            let label_values = &["service_rumor_encode", "failure"];
                            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                            continue 'rumorlist;
                        }
                    }
                }
                RumorType::ServiceConfig => {
                    // trace_it!(GOSSIP: &self.server,
                    //           TraceKind::SendRumor,
                    //           &member.id,
                    //           &send_rumor);
                    match self.server
                              .service_config_store
                              .encode(&rumor_key.key, &rumor_key.id)
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Could not write our own rumor to bytes; abandoning sending \
                                    rumor: {:?}",
                                   e);
                            let label_values = &["service_config_rumor_encode", "failure"];
                            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                            continue 'rumorlist;
                        }
                    }
                }
                RumorType::ServiceFile => {
                    // trace_it!(GOSSIP: &self.server,
                    //           TraceKind::SendRumor,
                    //           &member.id,
                    //           &send_rumor);
                    match self.server
                              .service_file_store
                              .encode(&rumor_key.key, &rumor_key.id)
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Could not write our own rumor to bytes; abandoning sending \
                                    rumor: {:?}",
                                   e);
                            let label_values = &["service_file_rumor_encode", "failure"];
                            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                            continue 'rumorlist;
                        }
                    }
                }
                RumorType::Departure => {
                    match self.server
                              .departure_store
                              .encode(&rumor_key.key, &rumor_key.id)
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Could not write our own rumor to bytes; abandoning sending \
                                    rumor: {:?}",
                                   e);
                            let label_values = &["departure_rumor_encode", "failure"];
                            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                            continue 'rumorlist;
                        }
                    }
                }
                RumorType::Election => {
                    // trace_it!(GOSSIP: &self.server,
                    //           TraceKind::SendRumor,
                    //           &member.id,
                    //           &send_rumor);
                    match self.server
                              .election_store
                              .encode(&rumor_key.key, &rumor_key.id)
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Could not write our own rumor to bytes; abandoning sending \
                                    rumor: {:?}",
                                   e);
                            let label_values = &["election_rumor_encode", "failure"];
                            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                            continue 'rumorlist;
                        }
                    }
                }
                RumorType::ElectionUpdate => {
                    match self.server
                              .update_store
                              .encode(&rumor_key.key, &rumor_key.id)
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Could not write our own rumor to bytes; abandoning sending \
                                    rumor: {:?}",
                                   e);
                            let label_values = &["election_update_rumor_encode", "failure"];
                            GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                            GOSSIP_BYTES_SENT.with_label_values(label_values).set(0);
                            continue 'rumorlist;
                        }
                    }
                }
                RumorType::Fake | RumorType::Fake2 => {
                    debug!("You have fake rumors; how odd!");
                    continue 'rumorlist;
                }
            };
            let rumor_len = rumor_as_bytes.len().to_i64();
            let payload = match self.server.generate_wire(rumor_as_bytes) {
                Ok(payload) => payload,
                Err(e) => {
                    error!("Generating protobuf failed: {}", e);
                    let label_values = &["generate_wire", "failure"];
                    GOSSIP_MESSAGES_SENT.with_label_values(label_values).inc();
                    GOSSIP_BYTES_SENT.with_label_values(label_values)
                                     .set(rumor_len);
                    continue 'rumorlist;
                }
            };
            frames.push((rumor_key.kind, payload));
        }
        frames
    }

    /// Given a rumorkey, creates a protobuf rumor for sharing.
    fn create_member_rumor(&self, rumor_key: &RumorKey) -> Option<RumorEnvelope> {
        let member = self.server.member_list.get_cloned(&rumor_key.key())?;
        let payload = Membership { member,
                                   health: self.server
                                               .member_list
                                               .health_of_by_id(&rumor_key.key())
                                               .unwrap() };
        let rumor = RumorEnvelope { r#type:  RumorType::Member,
                                    from_id: self.server.member_id().to_string(),
                                    kind:    RumorKind::Membership(payload), };
        Some(rumor)
    }
}
