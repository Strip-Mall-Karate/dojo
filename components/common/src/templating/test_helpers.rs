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

use std::{fs::File,
          io::{Read,
               Write},
          path::{Path,
                 PathBuf}};

use serde_json;

pub fn create_with_content<P>(path: P, content: &str)
    where P: AsRef<Path>
{
    let mut file = File::create(path).expect("Cannot create file");
    file.write_all(content.as_bytes())
        .expect("Cannot write to file");
}

pub fn file_content<P>(path: P) -> String
    where P: AsRef<Path>
{
    let mut file = File::open(path).expect("Could not open file!");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read file!");
    contents
}

/// Asserts that `json_string` is valid according to the specified JSON schema.
///
/// In the case of invalid input, all validation errors are neatly
/// rendered, along with a pretty-printed formatting of the
/// offending JSON.
pub fn assert_valid(json_string: &str, schema: &str) {
    let errors = validate_string(json_string, schema);
    if !errors.is_empty() {
        let input: serde_json::Value =
            serde_json::from_str(json_string).expect("JSON should parse if we get this far");
        let pretty_json = serde_json::to_string_pretty(&input).expect("JSON should serialize");
        panic!(
               r#"
JSON does not validate!
Errors:
{}

JSON:
{}
"#,
               errors.join("\n"),
               pretty_json
        );
    }
}

/// Compares the incoming JSON string against the JSON schema and returns every validation
/// error, rendered with the path to the offending value. An empty list means the input is
/// valid.
///
/// In general, you should prefer using `assert_valid` directly.
pub fn validate_string(input: &str, schema: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
                                                        .join("fixtures")
                                                        .join(schema);
    let mut schema_file = File::open(path).expect("could not open schema file");
    let mut raw_schema = String::new();
    schema_file.read_to_string(&mut raw_schema)
               .expect("could not read schema file");
    let parsed_schema: serde_json::Value =
        serde_json::from_str(&raw_schema).expect("Could not parse schema as JSON");
    // Unknown keywords are ignored, which allows the `$comment` keyword as well as our own
    // `$deprecated` and `$since` keywords.
    let validator = jsonschema::validator_for(&parsed_schema).expect("Could not compile the \
                                                                      schema");

    let input_json: serde_json::Value =
        serde_json::from_str(input).expect("Could not parse input as JSON");
    validator.iter_errors(&input_json)
             .map(|e| format!("  {}: {}", e.instance_path(), e))
             .collect()
}
