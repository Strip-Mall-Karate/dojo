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

use crate::{error::Result,
            sys::{service,
                  ShutdownMethod}};
use dojo_common::outputln;
use dojo_core::os::process::Pid;

static LOGKEY: &str = "ST"; // "Service Terminator"

/// Shut a service process down.
///
/// This blocks on the process, so it runs on the runtime's blocking threads in order to keep
/// from blocking the rest of the Supervisor.
pub async fn terminate_service(pid: Pid, service_group: String) -> Result<ShutdownMethod> {
    let shutdown = tokio::task::spawn_blocking(move || {
                       outputln!(preamble service_group, "Terminating service (PID: {})", pid);
                       let shutdown = service::kill(pid);
                       outputln!(preamble service_group, "{} (PID: {})", shutdown, pid);
                       shutdown
                   }).await?;
    Ok(shutdown)
}
