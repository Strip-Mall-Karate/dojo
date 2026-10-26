// Copyright (c) 2026 Strip Mall Karate Company
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

//! Compiles a plan's Fennel to Lua.
//!
//! Fennel runs in a Lua state of its own, thrown away after one compile.
//! Macros run in the plan's sandbox environment instead of Fennel's default
//! compiler sandbox, which can read files. Fennel's module and macro search is
//! switched off, so `include` and `import-macros` cannot pull in files a
//! plan does not declare.

use mlua::{Function,
           Lua,
           Table};

use crate::sandbox;

const FENNEL: &str = include_str!("../vendor/fennel.lua");

const COMPILE: &str = r#"
local fennel, env, globals, source, name = ...
fennel.path = ""
fennel["macro-path"] = ""
local searchers = fennel["macro-searchers"]
for i = #searchers, 1, -1 do searchers[i] = nil end
return fennel.compileString(source, {
  filename = name,
  correlate = true,
  allowedGlobals = globals,
  compilerEnv = env,
  ["error-pinpoint"] = false,
})
"#;

/// Compiles `source` to Lua whose line numbers match the Fennel. `name` is
/// the file name compile errors report.
pub fn compile(source: &str, name: &str) -> mlua::Result<String> {
    let lua = Lua::new();
    let fennel: Table = lua.load(FENNEL).set_name("=fennel").eval()?;
    let env = sandbox::environment(&lua)?;
    let globals = lua.create_sequence_from(sandbox::GLOBALS.iter().copied())?;
    let compile: Function = lua.load(COMPILE).set_name("=compile").into_function()?;
    sandbox::limit(&lua,
                   sandbox::COMPILE_MEMORY,
                   sandbox::COMPILE_INSTRUCTIONS,
                   "compiling the plan")?;
    compile.call((fennel, env, globals, source, name))
}
