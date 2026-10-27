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

use std::{process::ExitStatus,
          result};

pub type Result<T> = result::Result<T, anyhow::Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Docker build failed with exit code: {0}")]
    BuildFailed(ExitStatus),
    #[error("Could not determine Docker image ID for image: {0}")]
    DockerImageIdNotFound(String),
    #[error("Invalid registry type: {0}")]
    InvalidRegistryType(String),
    #[error("Docker login failed with exit code: {0}")]
    LoginFailed(ExitStatus),
    #[error("Docker logout failed with exit code: {0}")]
    LogoutFailed(ExitStatus),
    #[error("No ECR Tokens returned")]
    NoECRTokensReturned,
    #[error("Could not fetch an ECR authorization token: {0}")]
    TokenFetchFailed(#[source] aws_sdk_ecr::Error),
    #[error("A primary service package could not be determined from: {0:?}. At least one \
             package with a run hook must be provided.")]
    PrimaryServicePackageNotFound(Vec<String>),
    #[error("Docker image push failed with exit code: {0}")]
    PushImageFailed(ExitStatus),
    #[error("Removing Docker local images failed with exit code: {0}")]
    RemoveImageFailed(ExitStatus),
}
