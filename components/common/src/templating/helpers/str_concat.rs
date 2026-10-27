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

#[derive(Clone, Copy)]
pub struct StrConcatHelper;

impl HelperDef for StrConcatHelper {
    fn call<'reg: 'rc, 'rc>(&self,
                            h: &Helper<'rc>,
                            _: &'reg Handlebars<'reg>,
                            _: &'rc Context,
                            _: &mut RenderContext<'reg, 'rc>,
                            out: &mut dyn Output)
                            -> HelperResult {
        let list: Vec<String> = h.params()
                                 .iter()
                                 .map(|v| v.value())
                                 .filter(|v| !v.is_object())
                                 .map(|v| v.to_string().replace("\"", ""))
                                 .collect();
        out.write(&list.concat())?;
        Ok(())
    }
}

pub static STR_CONCAT: StrConcatHelper = StrConcatHelper;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_concat_helper() {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("strConcat", Box::new(STR_CONCAT));
        let expected = "foobarbaz";
        assert_eq!(expected,
                   handlebars.render_template("{{strConcat \"foo\" \"bar\" \"baz\"}}", &json!({}))
                             .unwrap());
    }
}
