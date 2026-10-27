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

use futures_util::StreamExt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Welcome to the testing subscriber for the Automate / Dojo Event Streaming Prototype");
    println!("Press '^C' to end");
    let client = async_nats::connect("nats://127.0.0.1:4222").await?;
    let topic = "dojo";
    let mut messages = client.subscribe(topic).await?;
    println!("Subscribed to topic: '{}'", topic);
    println!("===================");
    while let Some(msg) = messages.next().await {
        println!("Message: {:#?}", String::from_utf8_lossy(&msg.payload));
    }
    Ok(())
}
