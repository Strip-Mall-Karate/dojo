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

use handlebars::{Context,
                 Handlebars,
                 Helper,
                 HelperDef,
                 HelperResult,
                 Output,
                 RenderContext};

use super::render_error;

#[derive(Clone, Copy)]
pub struct StrReplaceHelper;

impl HelperDef for StrReplaceHelper {
    fn call<'reg: 'rc, 'rc>(&self,
                            h: &Helper<'rc>,
                            _: &'reg Handlebars<'reg>,
                            _: &'rc Context,
                            _: &mut RenderContext<'reg, 'rc>,
                            out: &mut dyn Output)
                            -> HelperResult {
        let param =
            h.param(0)
             .and_then(|v| v.value().as_str())
             .ok_or_else(|| render_error("Expected 3 string parameters for \"strReplace\""))?;
        let old = h.param(1)
                   .and_then(|v| v.value().as_str())
                   .ok_or_else(|| render_error("Expected 3 string parameters for \"strReplace\""))?;
        let new = h.param(2)
                   .and_then(|v| v.value().as_str())
                   .ok_or_else(|| render_error("Expected 3 string parameters for \"strReplace\""))?;
        out.write(&param.replace(old, new))?;
        Ok(())
    }
}

pub static STR_REPLACE: StrReplaceHelper = StrReplaceHelper;
