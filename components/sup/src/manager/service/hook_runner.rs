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

//! Runs a service lifecycle hook on the runtime's blocking threads, and wraps the whole
//! execution in a future.

use super::{hook_timer,
            Pkg};
use crate::error::Result;
use dojo_common::templating::hooks::Hook;
use dojo_core::service::ServiceGroup;
use std::sync::Arc;

pub struct HookRunner<H: Hook + Sync> {
    hook:          Arc<H>,
    service_group: ServiceGroup,
    pkg:           Pkg,
}

impl<H> HookRunner<H>
    where H: Hook + Sync + 'static,
          H::ExitValue: Send + 'static
{
    pub fn new(hook: Arc<H>, service_group: ServiceGroup, pkg: Pkg) -> HookRunner<H> {
        HookRunner { hook,
                     service_group,
                     pkg }
    }

    // TODO (CM): May want to consider adding a configurable
    // timeout to how long this hook is allowed to run.
    pub async fn run(self) -> Result<H::ExitValue> {
        let exit_value = tokio::task::spawn_blocking(move || {
                             let _timer = hook_timer(H::file_name());
                             self.hook.run(&self.service_group, &self.pkg)
                         }).await?;
        Ok(exit_value)
    }
}
