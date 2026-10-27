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
          num,
          path::PathBuf,
          result};

use crate::{doj_core,
            doj_http};

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", api_error(.0, .1))]
    APIError(reqwest::StatusCode, String),
    #[error("Failed to read response body, {0}")]
    BadResponseBody(io::Error),
    #[error("Failed to write contents of builder response, {}, {}", .0.display(), .1)]
    DownloadWrite(PathBuf, io::Error),
    #[error("{0}")]
    DojoCore(#[from] doj_core::Error),
    #[error("{0}")]
    DojoHttpClient(#[from] doj_http::Error),
    #[error("{0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    IO(#[from] io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("Failed to read origin key, {}, {}", .0.display(), .1)]
    KeyReadError(PathBuf, io::Error),
    #[error("Builder's response named no file to write it to")]
    NoFilePart,
    #[error("Failed to read package artifact, {}, {}", .0.display(), .1)]
    PackageReadError(PathBuf, io::Error),
    #[error("{0}")]
    ParseIntError(#[from] num::ParseIntError),
    #[error("Cannot perform the specified operation. Specify a fully qualifed package \
             identifier (ex: karate/busybox-static/1.42.2/20170513215502)")]
    IdentNotFullyQualified,
    #[error("Upload failed: {0}")]
    UploadFailed(String),
    #[error("{0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("Could not write to destination; perhaps the disk is full?")]
    WriteSyncFailed,
}

/// `[404 Not Found] message`, or just the status when there is no message.
fn api_error(status: &reqwest::StatusCode, message: &str) -> String {
    if message.is_empty() {
        format!("[{}]", status)
    } else {
        format!("[{}] {}", status, message)
    }
}
