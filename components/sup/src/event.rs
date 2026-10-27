// Copyright (c) 2019 Chef Software Inc. and/or applicable contributors
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

//! Main interface for a stream of events the Supervisor can send out
//! in the course of its operations.
//!
//! Currently, the Supervisor is able to send events to a [NATS][1]
//! server. The `init_stream` function must be called before sending
//! events to start the publishing task on the Supervisor's runtime.
//! Thereafter, you can pass "event" structs to the `event` function,
//! which will publish the event to the stream.
//!
//! All events are published under the "dojo" subject. A JetStream
//! stream on that subject keeps them, if the server has one.
//!
//! [1]:https://nats.io

mod types;

use crate::error::Result;
use state::Container;
use std::{fmt::Debug,
          sync::Once};
use tokio::{runtime::Handle,
            sync::mpsc::{self,
                         UnboundedSender}};
pub use types::*;

static INIT: Once = Once::new();
lazy_static! {
    // TODO (CM): When const fn support lands in stable, we can ditch
    // this lazy_static call.

    /// Reference to the event stream.
    static ref EVENT_STREAM: Container = Container::new();
    /// Core information that is shared between all events.
    static ref EVENT_CORE: Container = Container::new();
}

/// Connects to a NATS server and starts a task on `runtime` for
/// sending it events. Stashes the handle to the stream, as well as the
/// core event information that will be a part of all events, in a
/// global static reference for access later.
pub fn init_stream(conn_info: EventConnectionInfo, event_core: EventCore, runtime: &Handle) {
    INIT.call_once(|| {
            let event_stream =
                init_nats_stream(conn_info, runtime).expect("Could not connect to NATS");
            EVENT_STREAM.set(event_stream);
            EVENT_CORE.set(event_core);
        });
}

/// Publish an event. This is the main interface that client code will
/// use.
///
/// If `init_stream` has not been called already, this function will
/// be a no-op.
// NOTE: we can take advantage of this to "disable" the event
// subsystem if users don't wish to send events out; just don't call
// `init_stream` if they don't want it.
pub fn publish(event: &impl Event) {
    // TODO: incorporate the current timestamp into the rendered event
    // (which will require tweaks to the rendering logic, but we know
    // that'll need to be updated anyway).

    // We render the event to bytes here, rather than over in the
    // publication thread, because it allows our Event types to deal
    // with references, which means less allocations and unnecessary
    // copying (otherwise, we'd have to have copies of everything to
    // send it over to the other thread, or much more complicated
    // thread-safe types!). It also makes the publication thread logic
    // a bit simpler; it just takes bytes and sends them out.
    if let Some(e) = EVENT_STREAM.try_get::<EventStream>() {
        e.send(event.render(EVENT_CORE.get::<EventCore>()));
    }
}

/// A collection of data that will be present in all events. Rather
/// than baking this into the structure of each event, we represent it
/// once and merge the information into the final rendered form of the
/// event.
///
/// This prevents us from having to thread information throughout the
/// system, just to get it to the places where the events are
/// generated (e.g., not all code has direct access to the
/// Supervisor's ID).
#[derive(Clone, Debug)]
pub struct EventCore {
    /// The unique identifier of the Supervisor sending the event.
    pub supervisor_id: String,
}

/// A lightweight handle for the event stream. All events get to the
/// event stream through this.
pub(self) struct EventStream(UnboundedSender<Vec<u8>>);

impl EventStream {
    /// Queues an event to be sent out.
    fn send(&self, event: Vec<u8>) {
        trace!("About to queue an event: {:?}", event);
        if let Err(e) = self.0.send(event) {
            error!("Failed to queue event: {:?}", e);
        }
    }
}

/// Defines the logic for transforming concrete event into a
/// byte representation to publish to the event stream.
// TODO: The ultimate format we need is not well defined yet, so we're
// using the absolute simplest thing that could possibly "work". Maybe
// it'll be JSON, maybe it'll be protobuf, maybe it'll be something
// else. Whatever it ultimately becomes, this is where the
// transformation logic will live.
pub trait Event: Debug {
    fn render(&self, core: &EventCore) -> Vec<u8> {
        format!("{:?} - {:?}", core, self).into_bytes()
    }
}

////////////////////////////////////////////////////////////////////////

/// All messages are published under this subject.
const DOJO_SUBJECT: &str = "dojo";

/// All the information needed to establish a connection to a NATS
/// server.
// TODO: This will change as we firm up what the interaction between
// Dojo and A2 looks like.
pub struct EventConnectionInfo {
    pub name:   String,
    pub server: String,
}

/// Defines default connection information for a NATS server running
/// on localhost.
// TODO: As we become clear on the interaction between Dojo and A2,
// this implementation *may* disappear. It's useful for testing and
// prototyping, though.
impl Default for EventConnectionInfo {
    fn default() -> Self {
        EventConnectionInfo { name:   String::from("dojo"),
                              server: String::from("nats://127.0.0.1:4222"), }
    }
}

fn init_nats_stream(conn_info: EventConnectionInfo, runtime: &Handle) -> Result<EventStream> {
    // TODO (CM): Investigate back-pressure scenarios
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let EventConnectionInfo { name, server } = conn_info;
    let client = runtime.block_on(async_nats::ConnectOptions::new().name(name).connect(server))?;

    runtime.spawn(async move {
               while let Some(event) = event_rx.recv().await {
                   if let Err(e) = client.publish(DOJO_SUBJECT, event.into()).await {
                       error!("Error publishing event: {:?}", e);
                   }
               }
           });
    Ok(EventStream(event_tx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{BufRead,
                   BufReader,
                   Write},
              net::TcpListener,
              sync::mpsc as std_mpsc,
              thread,
              time::Duration};

    /// Speaks just enough of the NATS protocol to accept one client, and hands back each message
    /// it publishes.
    fn fake_nats() -> (String, std_mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std_mpsc::channel();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(b"INFO {\"server_id\":\"fake\",\"server_name\":\"fake\",\
                               \"version\":\"2.10.0\",\"go\":\"go1.22\",\"host\":\"127.0.0.1\",\
                               \"port\":4222,\"headers\":true,\"max_payload\":1048576,\
                               \"proto\":1}\r\n")
                  .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                if line.starts_with("PING") {
                    socket.write_all(b"PONG\r\n").unwrap();
                } else if line.starts_with("PUB ") {
                    let mut payload = String::new();
                    reader.read_line(&mut payload).unwrap();
                    tx.send(format!("{}{}", line, payload)).unwrap();
                }
                line.clear();
            }
        });
        (format!("nats://{}", addr), rx)
    }

    #[test]
    fn events_are_published_on_the_dojo_subject() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (server, published) = fake_nats();
        let stream = init_nats_stream(EventConnectionInfo { name: String::from("test"),
                                                            server },
                                      runtime.handle()).unwrap();

        stream.send(b"hello".to_vec());

        assert_eq!(published.recv_timeout(Duration::from_secs(10)).unwrap(),
                   "PUB dojo 5\r\nhello\r\n");
    }
}
