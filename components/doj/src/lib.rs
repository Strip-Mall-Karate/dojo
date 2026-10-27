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

#![recursion_limit = "128"]

use dojo_api_client as api_client;
use dojo_common as common;
use dojo_core as hcore;
use dojo_http_client as http_client;
use dojo_sup_client as sup_client;
use dojo_sup_protocol as protocol;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

extern crate serde_json;

pub mod cli;
pub mod command;
pub mod config;
pub mod error;
mod exec;
pub mod scaffolding;

pub const PRODUCT: &str = "doj";
pub const VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/VERSION"));
pub const CTL_SECRET_ENVVAR: &str = "DOJ_CTL_SECRET";
pub const ORIGIN_ENVVAR: &str = "DOJ_ORIGIN";
pub const BLDR_URL_ENVVAR: &str = "DOJ_BLDR_URL";

pub use crate::hcore::AUTH_TOKEN_ENVVAR;
