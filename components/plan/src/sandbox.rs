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

//! The environment a plan and its macros run in, and the limits on them.
//!
//! A plan gets pure functions and nothing that reaches the machine: no `io`,
//! `os`, `require`, `load` or `debug`. `pcall` is left out too, because a
//! plan that could catch errors could catch the one that ends a runaway loop.
//! `pairs` walks keys in sorted order, so the same plan always does the same
//! thing.

use std::cell::Cell;

use mlua::{HookTriggers,
           Lua,
           Table,
           VmState};

/// Memory a plan may use while it loads.
pub const PLAN_MEMORY: usize = 64 << 20;
/// Lua instructions a plan may run while it loads.
pub const PLAN_INSTRUCTIONS: u64 = 20_000_000;
/// Memory Fennel may use compiling a plan, its own code included.
pub const COMPILE_MEMORY: usize = 256 << 20;
/// Lua instructions Fennel may run compiling a plan, macros included.
pub const COMPILE_INSTRUCTIONS: u64 = 500_000_000;

const HOOK_EVERY: u32 = 1_000;

/// The globals of a plan, and of the macros it defines. Fennel refuses any
/// other name at compile time.
pub const GLOBALS: &[&str] = &["assert",
                               "error",
                               "ipairs",
                               "math",
                               "pairs",
                               "select",
                               "setmetatable",
                               "string",
                               "table",
                               "tonumber",
                               "tostring",
                               "type",
                               "utf8"];

const ENVIRONMENT: &str = r#"
local function order(a, b)
  local ta, tb = type(a), type(b)
  if ta ~= tb then return ta < tb end
  if ta == "number" or ta == "string" then return a < b end
  return tostring(a) < tostring(b)
end

local function sorted_pairs(t)
  local keys = {}
  for k in next, t do keys[#keys + 1] = k end
  table.sort(keys, order)
  local i = 0
  return function()
    i = i + 1
    local k = keys[i]
    if k ~= nil then return k, t[k] end
  end, t, nil
end

local function copy(source, without)
  local out = {}
  for k, v in next, source do
    if not without[k] then out[k] = v end
  end
  return out
end

return {
  assert = assert,
  error = error,
  ipairs = ipairs,
  math = copy(math, { random = true, randomseed = true }),
  pairs = sorted_pairs,
  select = select,
  setmetatable = setmetatable,
  string = copy(string, { dump = true }),
  table = copy(table, {}),
  tonumber = tonumber,
  tostring = tostring,
  type = type,
  utf8 = copy(utf8, {}),
}
"#;

/// Builds a fresh table of the sandbox globals in `lua`.
pub fn environment(lua: &Lua) -> mlua::Result<Table> {
    lua.load(ENVIRONMENT).set_name("=sandbox").eval()
}

/// Caps the memory and instructions `lua` may use from now on. Past the
/// instruction budget every further check raises an error again, so a caught
/// error cannot buy more time.
pub fn limit(lua: &Lua, memory: usize, instructions: u64, what: &'static str) -> mlua::Result<()> {
    lua.set_memory_limit(memory)?;
    let spent = Cell::new(0u64);
    lua.set_hook(HookTriggers::new().every_nth_instruction(HOOK_EVERY),
                 move |_, _| {
                     spent.set(spent.get() + u64::from(HOOK_EVERY));
                     if spent.get() > instructions {
                         Err(mlua::Error::runtime(format!("{what} ran past its budget \
                                                           of {instructions} \
                                                           instructions")))
                     } else {
                         Ok(VmState::Continue)
                     }
                 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globals_match_the_environment() {
        let lua = Lua::new();
        let mut names: Vec<String> = environment(&lua).unwrap()
                                                      .pairs::<String, mlua::Value>()
                                                      .map(|pair| pair.unwrap().0)
                                                      .collect();
        names.sort();
        assert_eq!(names, GLOBALS);
    }

    #[test]
    fn pairs_walks_keys_in_order() {
        let lua = Lua::new();
        let env = environment(&lua).unwrap();
        let keys: String = lua
            .load(r#"local s = "" for k in pairs({c = 1, a = 2, b = 3, [2] = 4, [1] = 5}) do s = s .. tostring(k) end return s"#)
            .set_environment(env)
            .eval()
            .unwrap();
        assert_eq!(keys, "12abc");
    }
}
