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

use handlebars::{to_json,
                 BlockContext,
                 BlockParams,
                 Context,
                 Handlebars,
                 Helper,
                 HelperDef,
                 HelperResult,
                 JsonTruthy,
                 Output,
                 RenderContext,
                 Renderable};
use serde_json::Value as Json;

use super::render_error;

/// Iterates like `each`, but only over the members whose `alive` field is true. Given an
/// object, it iterates over the object's fields if the object itself is alive.
#[derive(Clone, Copy)]
pub struct EachAliveHelper;

fn is_alive(member: &Json) -> bool { member.get("alive").and_then(Json::as_bool).unwrap_or(false) }

impl HelperDef for EachAliveHelper {
    fn call<'reg: 'rc, 'rc>(&self,
                            h: &Helper<'rc>,
                            r: &'reg Handlebars<'reg>,
                            ctx: &'rc Context,
                            rc: &mut RenderContext<'reg, 'rc>,
                            out: &mut dyn Output)
                            -> HelperResult {
        let value = h.param(0)
                     .ok_or_else(|| render_error("Param not found for helper \"eachAlive\""))?;
        let Some(template) = h.template() else {
            return Ok(());
        };
        match (value.value().is_truthy(false), value.value()) {
            (true, Json::Array(list)) => {
                let alive_members: Vec<&Json> = list.iter().filter(|m| is_alive(m)).collect();
                let len = alive_members.len();
                rc.push_block(BlockContext::new());
                for (i, member) in alive_members.into_iter().enumerate() {
                    if let Some(block) = rc.block_mut() {
                        block.set_local_var("first", to_json(i == 0));
                        block.set_local_var("last", to_json(i == len - 1));
                        block.set_local_var("index", to_json(i));
                        block.set_base_value(member.clone());
                        if let Some(name) = h.block_param() {
                            let mut params = BlockParams::new();
                            params.add_value(name, member.clone())?;
                            block.set_block_params(params);
                        }
                    }
                    template.render(r, ctx, rc, out)?;
                }
                rc.pop_block();
                Ok(())
            }
            (true, Json::Object(obj)) => {
                if !is_alive(value.value()) {
                    return Ok(());
                }
                let obj_path = value.context_path();
                let len = obj.len();
                rc.push_block(BlockContext::new());
                for (i, (k, v)) in obj.iter().enumerate() {
                    if let Some(block) = rc.block_mut() {
                        block.set_local_var("first", to_json(i == 0));
                        block.set_local_var("last", to_json(i == len - 1));
                        block.set_local_var("key", to_json(k));
                        block.set_local_var("index", to_json(i));
                        match obj_path {
                            Some(path) => {
                                let mut path = path.clone();
                                path.push(k.clone());
                                *block.base_path_mut() = path;
                            }
                            None => block.set_base_value(v.clone()),
                        }
                        let mut params = BlockParams::new();
                        if let Some((value_name, key_name)) = h.block_param_pair() {
                            params.add_value(value_name, v.clone())?;
                            params.add_value(key_name, to_json(k))?;
                        } else if let Some(name) = h.block_param() {
                            params.add_value(name, v.clone())?;
                        }
                        block.set_block_params(params);
                    }
                    template.render(r, ctx, rc, out)?;
                }
                rc.pop_block();
                Ok(())
            }
            (false, _) => {
                match h.inverse() {
                    Some(else_template) => else_template.render(r, ctx, rc, out),
                    None => Ok(()),
                }
            }
            (true, other) => Err(render_error(format!("Param type is not iterable: {:?}", other))),
        }
    }
}

pub static EACH_ALIVE: EachAliveHelper = EachAliveHelper;
