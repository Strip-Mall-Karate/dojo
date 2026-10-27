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

use std::{io,
          path::PathBuf,
          result};

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    DojoCore(#[from] dojo_core::Error),
    #[error("{0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    IO(#[from] io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("cannot read CA certificates from {0}: {1}")]
    CaCertificates(PathBuf, String),
    #[error("{0}")]
    Tls(#[from] rustls::Error),
    #[error("{0}")]
    UrlParseError(#[from] url::ParseError),
}
