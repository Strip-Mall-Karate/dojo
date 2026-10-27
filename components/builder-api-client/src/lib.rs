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

use dojo_core as doj_core;
use dojo_http_client as doj_http;

pub mod error;
pub use crate::error::{Error,
                       Result};
pub use reqwest::StatusCode;

use std::{fmt,
          fs::{self,
               File},
          io::{self,
               Read,
               Write},
          path::{Path,
                 PathBuf},
          string::ToString};

use crate::{doj_core::{ChannelIdent,
                       crypto::keys::box_key_pair::WrappedSealedBox,
                       fs::AtomicWriter,
                       package::{Identifiable,
                                 PackageArchive,
                                 PackageIdent,
                                 PackageTarget}},
            doj_http::{ApiClient,
                       util::decoded_response}};
use chrono::DateTime;
use log::debug;
use percent_encoding::{AsciiSet,
                       CONTROLS,
                       utf8_percent_encode};
use reqwest::{IntoUrl,
              blocking::{Body,
                         RequestBuilder,
                         Response},
              header::{ACCEPT,
                       CONTENT_TYPE}};
use serde::{Deserialize,
            Serialize};
use serde_json::json;
use url::Url;

/// The characters a URL path segment percent-encodes: the WHATWG URL
/// standard's path percent-encode set, plus `/` and `%`.
const PATH_SEGMENT: &AsciiSet = &CONTROLS.add(b' ')
                                         .add(b'"')
                                         .add(b'#')
                                         .add(b'<')
                                         .add(b'>')
                                         .add(b'?')
                                         .add(b'`')
                                         .add(b'{')
                                         .add(b'}')
                                         .add(b'/')
                                         .add(b'%');

const JSON: &str = "application/json";

const DEFAULT_API_PATH: &str = "/v1";

#[derive(Clone, Deserialize)]
#[serde(rename = "error")]
pub struct NetError {
    pub code: i32,
    pub msg:  String,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[err: {:?}, msg: {}]", self.code, self.msg)
    }
}

#[derive(Clone, Deserialize)]
pub struct OriginKeyIdent {
    pub origin:   String,
    pub revision: String,
    pub location: String,
}

#[derive(Clone, Default, Deserialize)]
pub struct Project {
    pub name:   String,
    pub ident:  String,
    pub state:  String,
    pub job_id: String,
    pub target: String,
}

#[derive(Clone, Deserialize)]
pub struct OriginSecret {
    pub id:        String,
    pub origin_id: String,
    pub name:      String,
    pub value:     String,
}

impl fmt::Display for Project {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = format!("{:50} {}", self.ident, self.state);

        if let Ok(j) = self.job_id.parse::<i64>() {
            if j > 0 {
                let job_ids = format!(" (Job ID {})", self.job_id);
                s = s + &job_ids;
            }
        }

        write!(f, "{}", s)
    }
}

#[derive(Default, Deserialize)]
pub struct SchedulerResponse {
    pub id:           String,
    pub state:        String,
    pub projects:     Vec<Project>,
    pub created_at:   String,
    pub project_name: String,
    pub target:       String,
}

impl fmt::Display for SchedulerResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut output = Vec::new();
        output.push(format!("Status for Job Group {} ({}): {}",
                            self.id, self.project_name, self.state));

        if let Ok(c) = DateTime::parse_from_rfc3339(&self.created_at) {
            output.push(format!("Created at: {}", c.to_string()));
        }

        if !self.projects.is_empty() {
            output.push("".to_string());
            output.push("Reverse dependencies:".to_string());
            let mut projects = self.projects.clone();
            projects.sort_by(|a, b| a.ident.cmp(&b.ident));

            for project in projects.iter() {
                output.push(project.to_string())
            }
        }

        write!(f, "{}", output.join("\n"))
    }
}

#[derive(Deserialize)]
pub struct ReverseDependencies {
    pub origin: String,
    pub name:   String,
    pub rdeps:  Vec<String>,
}

#[derive(Default, Deserialize)]
pub struct JobGroupPromoteResponse {
    pub group_id:     String,
    pub not_promoted: Vec<PackageIdent>,
}

/// Custom conversion logic to allow `serde` to successfully
/// round-trip `u64` datatypes through JSON serialization.
///
/// To use it, add `#[serde(with = "json_u64")]` to any `u64`-typed struct
/// fields.
mod json_u64 {
    use serde::{self,
                Deserialize,
                Deserializer,
                Serializer};

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S>(num: &u64, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        let s = format!("{}", num);
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
        where D: Deserializer<'de>
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<u64>().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct OriginPrivateSigningKey {
    #[serde(with = "json_u64")]
    pub id:        u64,
    #[serde(with = "json_u64")]
    pub origin_id: u64,
    pub name:      String,
    pub revision:  String,
    pub body:      Vec<u8>,
    #[serde(with = "json_u64")]
    pub owner_id:  u64,
}

mod json {
    use serde::Deserialize;

    /// The part of Builder's package JSON Dojo reads.
    #[derive(Clone, Deserialize)]
    pub struct Package {
        pub ident: PackageIdent,
    }

    #[derive(Clone, Deserialize)]
    pub struct PackageIdent {
        pub origin:  String,
        pub name:    String,
        pub version: String,
        pub release: String,
    }

    impl From<PackageIdent> for super::PackageIdent {
        fn from(ident: PackageIdent) -> Self {
            super::PackageIdent { origin:  ident.origin,
                                  name:    ident.name,
                                  version: Some(ident.version),
                                  release: Some(ident.release), }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PackageResults<T> {
    pub range_start: isize,
    pub range_end:   isize,
    pub total_count: isize,
    pub data:        Vec<T>,
}

#[derive(Clone, Deserialize)]
pub struct OriginChannelIdent {
    pub name: String,
}

pub trait DisplayProgress: Write {
    fn size(&mut self, size: u64);
    fn finish(&mut self);
}

pub struct Client(ApiClient);

impl Client {
    pub fn new<U>(endpoint: U,
                  product: &str,
                  version: &str,
                  fs_root_path: Option<&Path>)
                  -> Result<Self>
        where U: IntoUrl
    {
        let mut endpoint = endpoint.into_url().map_err(Error::Http)?;
        if !endpoint.cannot_be_a_base() && endpoint.path() == "/" {
            endpoint.set_path(DEFAULT_API_PATH);
        }
        Ok(Client(ApiClient::new(endpoint,
                                 product,
                                 version,
                                 fs_root_path)?))
    }

    /// Retrieves the status of every group job in an origin
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    pub fn get_origin_schedule(&self,
                               origin: &str,
                               limit: usize)
                               -> Result<Vec<SchedulerResponse>> {
        debug!("Retrieving status for job groups in the {} origin", origin);

        let path = format!("depot/pkgs/schedule/{}/status", origin);

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("limit", &limit.to_string());
        };

        let res = self.0.get_with_custom_url(&path, custom).send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        }

        let sr: Vec<SchedulerResponse> = decoded_response(res)?;
        Ok(sr)
    }

    /// Retrieves the status of a group job
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    pub fn get_schedule(&self, group_id: i64, include_projects: bool) -> Result<SchedulerResponse> {
        debug!("Retrieving schedule for job group {}", group_id);

        let path = format!("depot/pkgs/schedule/{}", group_id);

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("include_projects", &include_projects.to_string());
        };

        let res = self.0.get_with_custom_url(&path, custom).send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        }

        let sr: SchedulerResponse = decoded_response(res)?;
        Ok(sr)
    }

    /// Schedules a job for a package ident
    ///
    /// # Failures
    ///
    /// * Key cannot be found
    /// * Remote Builder is not available
    pub fn schedule_job(&self,
                        (ident, target): (&PackageIdent, PackageTarget),
                        package_only: bool,
                        token: &str)
                        -> Result<String> {
        let path = format!("depot/pkgs/schedule/{}/{}", ident.origin(), ident.name());

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("package_only", &package_only.to_string())
               .append_pair("target", &target.to_string());
        };

        match self.add_authz(self.0.post_with_custom_url(&path, custom), token)
                  .send()
        {
            Ok(response) => {
                if response.status() == StatusCode::CREATED || response.status() == StatusCode::OK {
                    let sr: SchedulerResponse = decoded_response(response)?;
                    Ok(sr.id)
                } else {
                    Err(err_from_response(response))
                }
            }
            Err(e) => Err(Error::from(e)),
        }
    }

    /// Fetch the reverse dependencies for a package
    ///
    /// # Failures
    ///
    /// * Remote API Server is not available
    pub fn fetch_rdeps(&self,
                       (ident, target): (&PackageIdent, PackageTarget))
                       -> Result<Vec<String>> {
        debug!("Fetching the reverse dependencies for {}", ident);

        let url = format!("rdeps/{}", ident);

        let mut res = self.0
                          .get_with_custom_url(&url, |u| {
                              u.set_query(Some(&format!("target={}", &target.to_string())))
                          })
                          .send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        }

        let mut encoded = String::new();
        res.read_to_string(&mut encoded).map_err(Error::IO)?;
        debug!("Body: {:?}", encoded);
        let rd: ReverseDependencies = serde_json::from_str(&encoded).map_err(Error::Json)?;
        Ok(rd.rdeps.to_vec())
    }

    /// Promote/Demote a job group to/from a channel
    ///
    /// # Failures
    ///
    /// * Remote API Server is not available
    pub fn job_group_promote_or_demote<T: AsRef<str> + serde::Serialize>(&self,
                                                                         group_id: u64,
                                                                         idents: &[T],
                                                                         channel: &ChannelIdent,
                                                                         token: &str,
                                                                         promote: bool)
                                                                         -> Result<()> {
        let json_idents = json!(idents);
        let body = json!({ "idents": json_idents });
        let sbody = serde_json::to_string(&body).unwrap();
        let url = format!("jobs/group/{}/{}/{}",
                          group_id,
                          if promote { "promote" } else { "demote" },
                          channel);
        let res = self.add_authz(self.0.post(&url), token)
                      .body(sbody)
                      .header(ACCEPT, JSON)
                      .header(CONTENT_TYPE, JSON)
                      .send()?;

        if res.status() != StatusCode::NO_CONTENT {
            debug!("Failed to {} group, status: {:?}",
                   if promote { "promote" } else { "demote" },
                   res.status());
            return Err(err_from_response(res));
        }

        Ok(())
    }

    /// Cancel a job group
    ///
    /// # Failures
    ///
    /// * Remote API Server is not available
    pub fn job_group_cancel(&self, group_id: u64, token: &str) -> Result<()> {
        let url = format!("jobs/group/{}/cancel", group_id);
        let res = self.add_authz(self.0.post(&url), token).send()?;

        if res.status() != StatusCode::NO_CONTENT {
            debug!("Failed to cancel group, status: {:?}", res.status());
            return Err(err_from_response(res));
        }

        Ok(())
    }

    /// Download a public encryption key from a remote Builder to the given filepath.
    ///
    /// # Failures
    ///
    /// * Key cannot be found
    /// * Remote Builder is not available
    /// * File cannot be created and written to
    pub fn fetch_origin_public_encryption_key<D, P: ?Sized>(&self,
                                                            origin: &str,
                                                            token: &str,
                                                            dst_path: &P,
                                                            progress: Option<D>)
                                                            -> Result<PathBuf>
        where P: AsRef<Path>,
              D: DisplayProgress + Sized
    {
        self.download(self.0
                          .get(&format!("depot/origins/{}/encryption_key", origin)),
                      dst_path.as_ref(),
                      Some(token),
                      progress)
    }

    /// Create secret for an origin
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    pub fn create_origin_secret(&self,
                                origin: &str,
                                token: &str,
                                key: &str,
                                secret: &WrappedSealedBox)
                                -> Result<()> {
        let path = format!("depot/origins/{}/secret", origin);
        let body = json!({
            "name": key,
            "value": secret
        });

        let sbody = serde_json::to_string(&body)?;
        let res = self.add_authz(self.0.post(&path), token)
                      .body(sbody)
                      .header(ACCEPT, JSON)
                      .header(CONTENT_TYPE, JSON)
                      .send()?;

        if res.status() != StatusCode::CREATED {
            return Err(err_from_response(res));
        }

        Ok(())
    }

    /// Delete a secret for an origin
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    pub fn delete_origin_secret(&self, origin: &str, token: &str, key: &str) -> Result<()> {
        let path = format!("depot/origins/{}/secret/{}", origin, key);

        let res = self.add_authz(self.0.delete(&path), token).send()?;

        // We expect NoContent, because the origin must be empty to delete
        if res.status() == StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err(err_from_response(res))
        }
    }

    /// Delete an origin
    ///
    ///  # Failures
    ///
    ///  * Remote builder is not available
    ///  * Origin is populated with > 0 packages
    ///  * Submitted Origin is not found
    pub fn delete_origin(&self, origin: &str, token: &str) -> Result<()> {
        let path = format!("depot/origins/{}", origin);

        let res = self.add_authz(self.0.delete(&path), token).send()?;

        if res.status() != StatusCode::NO_CONTENT {
            return Err(err_from_response(res));
        }

        Ok(())
    }

    /// List all secrets keys for an origin
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    pub fn list_origin_secrets(&self, origin: &str, token: &str) -> Result<Vec<String>> {
        let path = format!("depot/origins/{}/secret", origin);

        let mut res = self.add_authz(self.0.get(&path), token).send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        }

        let mut encoded = String::new();
        res.read_to_string(&mut encoded)
           .map_err(Error::BadResponseBody)?;
        debug!("Response body: {:?}", encoded);
        let secret_keys: Vec<String> =
            serde_json::from_str::<Vec<OriginSecret>>(&encoded)?.into_iter()
                                                                .map(|s| s.name)
                                                                .collect();
        Ok(secret_keys)
    }

    /// Download a public key from a remote Builder to the given filepath.
    ///
    /// # Failures
    ///
    /// * Key cannot be found
    /// * Remote Builder is not available
    /// * File cannot be created and written to
    pub fn fetch_origin_key<D, P: ?Sized>(&self,
                                          origin: &str,
                                          revision: &str,
                                          dst_path: &P,
                                          progress: Option<D>)
                                          -> Result<PathBuf>
        where P: AsRef<Path>,
              D: DisplayProgress + Sized
    {
        self.download(self.0
                          .get(&format!("depot/origins/{}/keys/{}", origin, revision)),
                      dst_path.as_ref(),
                      None,
                      progress)
    }

    /// Download a secret key from a remote Builder to the given filepath.
    ///
    /// # Failures
    ///
    /// * Key cannot be found
    /// * Remote Builder is not available
    /// * File cannot be created and written to
    pub fn fetch_secret_origin_key<D, P: ?Sized>(&self,
                                                 origin: &str,
                                                 token: &str,
                                                 dst_path: &P,
                                                 progress: Option<D>)
                                                 -> Result<PathBuf>
        where P: AsRef<Path>,
              D: DisplayProgress + Sized
    {
        self.download(self.0
                          .get(&format!("depot/origins/{}/secret_keys/latest", origin)),
                      dst_path.as_ref(),
                      Some(token),
                      progress)
    }

    pub fn show_origin_keys(&self, origin: &str) -> Result<Vec<OriginKeyIdent>> {
        let mut res = self.0.get(&origin_keys_path(origin)).send()?;
        debug!("Response: {:?}", res);

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        };

        let mut encoded = String::new();
        res.read_to_string(&mut encoded)
           .map_err(Error::BadResponseBody)?;
        debug!("Response body: {:?}", encoded);
        let revisions: Vec<OriginKeyIdent> = serde_json::from_str::<Vec<OriginKeyIdent>>(&encoded)?;
        Ok(revisions)
    }

    /// Return a list of channels for a given package
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    /// * Package does not exist
    pub fn package_channels(&self,
                            (ident, target): (&PackageIdent, PackageTarget),
                            token: Option<&str>)
                            -> Result<Vec<String>> {
        if !ident.fully_qualified() {
            return Err(Error::IdentNotFullyQualified);
        }

        let path = package_channels_path(ident);
        debug!("Retrieving channels for {}, target {}", ident, target);

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("target", &target.to_string());
        };

        let mut res = self.maybe_add_authz(self.0.get_with_custom_url(&path, custom), token)
                          .send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        }

        let mut encoded = String::new();
        res.read_to_string(&mut encoded)
           .map_err(Error::BadResponseBody)?;
        debug!("Response body: {:?}", encoded);
        let channels: Vec<String> = serde_json::from_str::<Vec<String>>(&encoded)?.into_iter()
                                                                                  .collect();
        Ok(channels)
    }

    /// Upload a public origin key to a remote Builder.
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    /// * File cannot be read
    ///
    /// # Panics
    ///
    /// * Authorization token was not set on client
    pub fn put_origin_key<D>(&self,
                             origin: &str,
                             revision: &str,
                             src_path: &Path,
                             token: &str,
                             progress: Option<D>)
                             -> Result<()>
        where D: DisplayProgress + Sized + Send + 'static
    {
        let path = format!("depot/origins/{}/keys/{}", &origin, &revision);
        let file =
            File::open(src_path).map_err(|e| Error::KeyReadError(src_path.to_path_buf(), e))?;
        let file_size = file.metadata()
                            .map_err(|e| Error::KeyReadError(src_path.to_path_buf(), e))?
                            .len();

        created(self.add_authz(self.0.post(&path), token)
                    .body(upload_body(file, file_size, progress))
                    .send())
    }

    /// Download a secret key from a remote Builder to the given filepath.
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    /// * File cannot be read
    ///
    /// # Panics
    ///
    /// * Authorization token was not set on client
    pub fn fetch_origin_secret_key<P>(&self,
                                      origin: &str,
                                      token: &str,
                                      dst_path: P)
                                      -> Result<PathBuf>
        where P: AsRef<Path>
    {
        self.x_download(&origin_secret_keys_latest(origin), dst_path.as_ref(), token)
    }

    /// Upload a secret origin key to a remote Builder.
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    /// * File cannot be read
    ///
    /// # Panics
    ///
    /// * Authorization token was not set on client
    pub fn put_origin_secret_key<D>(&self,
                                    origin: &str,
                                    revision: &str,
                                    src_path: &Path,
                                    token: &str,
                                    progress: Option<D>)
                                    -> Result<()>
        where D: DisplayProgress + Sized + Send + 'static
    {
        let path = format!("depot/origins/{}/secret_keys/{}", &origin, &revision);
        let file =
            File::open(src_path).map_err(|e| Error::KeyReadError(src_path.to_path_buf(), e))?;
        let file_size = file.metadata()
                            .map_err(|e| Error::KeyReadError(src_path.to_path_buf(), e))?
                            .len();

        created(self.add_authz(self.0.post(&path), token)
                    .body(upload_body(file, file_size, progress))
                    .send())
    }

    /// Download the latest release of a package.
    ///
    /// By the time this function is called, the ident must be fully qualified. The download URL in
    /// the depot requires a fully qualified ident to work. If you want the latest version of
    /// a package, e.g. karate/redis, you can display package details for that via a different URL,
    /// e.g. /pkgs/karate/redis/latest but that only _shows_ you the details - it doesn't download
    /// the package.
    ///
    /// # Failures
    ///
    /// * Package cannot be found
    /// * Remote Builder is not available
    /// * File cannot be created and written to
    pub fn fetch_package<D, P>(&self,
                               (ident, target): (&PackageIdent, PackageTarget),
                               token: Option<&str>,
                               dst_path: &P,
                               progress: Option<D>)
                               -> Result<PackageArchive>
        where P: AsRef<Path> + ?Sized,
              D: DisplayProgress + Sized
    {
        // Ensure ident is fully qualified.
        //
        // TODO fn: this will be removed when we can describe a fully qualified ident by type as a
        // param to this function
        if !ident.fully_qualified() {
            return Err(Error::IdentNotFullyQualified);
        }

        let req_builder = self.0.get_with_custom_url(&package_download(ident), |u| {
                                    u.set_query(Some(&format!("target={}", target)))
                                });

        match self.download(req_builder, dst_path.as_ref(), token, progress) {
            Ok(file) => Ok(PackageArchive::new(file)),
            Err(e) => Err(e),
        }
    }

    /// Returns a package struct for the latest package.
    ///
    /// An optional version can be specified which will scope the release returned to the latest
    /// release of that package.
    ///
    /// # Failures
    ///
    /// * Package cannot be found
    /// * Remote Builder is not available
    pub fn show_package(&self,
                        (package, target): (&PackageIdent, PackageTarget),
                        channel: &ChannelIdent,
                        token: Option<&str>)
                        -> Result<PackageIdent> {
        let mut url = channel_package_path(channel, package);

        if !package.fully_qualified() {
            url.push_str("/latest");
        }

        let mut res = self.maybe_add_authz(self.0
                                               .get_with_custom_url(&url, |u| {
                                                   u.set_query(Some(&format!("target={}", target)))
                                               }),
                                           token)
                          .send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        }

        let mut encoded = String::new();
        res.read_to_string(&mut encoded)
           .map_err(Error::BadResponseBody)?;
        debug!("Body: {:?}", encoded);
        let package: json::Package = serde_json::from_str::<json::Package>(&encoded)?;
        Ok(package.ident.into())
    }

    /// Upload a package to a remote Builder.
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    /// * File cannot be read
    ///
    /// # Panics
    ///
    /// * Authorization token was not set on client
    pub fn put_package<D>(&self,
                          pa: &mut PackageArchive,
                          token: &str,
                          force_upload: bool,
                          progress: Option<D>)
                          -> Result<()>
        where D: DisplayProgress + Sized + Send + 'static
    {
        let checksum = pa.checksum()?;
        let ident = pa.ident()?;
        let target = pa.target()?;

        let file = File::open(&pa.path).map_err(|e| Error::PackageReadError(pa.path.clone(), e))?;
        let file_size = file.metadata()
                            .map_err(|e| Error::PackageReadError(pa.path.clone(), e))?
                            .len();

        let path = package_path(&ident);

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("checksum", &checksum)
               .append_pair("target", &target.to_string())
               .append_pair("forced", &force_upload.to_string());
        };
        debug!("Reading from {}", &pa.path.display());

        created(self.add_authz(self.0.post_with_custom_url(&path, custom), token)
                    .body(upload_body(file, file_size, progress))
                    .send())
    }

    pub fn x_put_package(&self, pa: &mut PackageArchive, token: &str) -> Result<()> {
        let checksum = pa.checksum()?;
        let ident = pa.ident()?;
        let target = pa.target()?;

        let file = File::open(&pa.path).map_err(|e| Error::PackageReadError(pa.path.clone(), e))?;
        let file_size = file.metadata()
                            .map_err(|e| Error::PackageReadError(pa.path.clone(), e))?
                            .len();
        let path = package_path(&ident);
        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("checksum", &checksum)
               .append_pair("target", &target.to_string())
               .append_pair("builder", "");
        };
        debug!("Reading from {}", &pa.path.display());

        created(self.add_authz(self.0.post_with_custom_url(&path, custom), token)
                    .body(Body::sized(file, file_size))
                    .send())
    }

    /// Delete a package from Builder
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    /// * If package does not exist in Builder
    /// * If the package does not qualify for deletion
    /// * Authorization token was not set on client
    pub fn delete_package(&self,
                          (ident, target): (&PackageIdent, PackageTarget),
                          token: &str)
                          -> Result<()> {
        let path = package_path(ident);
        debug!("Deleting package {}, target {}", ident, target);

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("target", &target.to_string());
        };

        let res = self.add_authz(self.0.delete_with_custom_url(&path, custom), token)
                      .send()?;

        if res.status() != StatusCode::NO_CONTENT {
            return Err(err_from_response(res));
        };

        Ok(())
    }

    /// Promote a package to a given channel
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    ///
    /// # Panics
    ///
    /// * If package does not exist in Builder
    /// * Authorization token was not set on client
    pub fn promote_package(&self,
                           (ident, target): (&PackageIdent, PackageTarget),
                           channel: &ChannelIdent,
                           token: &str)
                           -> Result<()> {
        if !ident.fully_qualified() {
            return Err(Error::IdentNotFullyQualified);
        }
        let path = channel_package_promote(channel, ident);
        debug!("Promoting package {}, target {}", ident, target);

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("target", &target.to_string());
        };

        let res = self.add_authz(self.0.put_with_custom_url(&path, custom), token)
                      .send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        };

        Ok(())
    }

    /// Demote a package from a given channel
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    ///
    /// # Panics
    ///
    /// * If package does not exist in Builder
    /// * Authorization token was not set on client
    pub fn demote_package(&self,
                          (ident, target): (&PackageIdent, PackageTarget),
                          channel: &ChannelIdent,
                          token: &str)
                          -> Result<()> {
        if !ident.fully_qualified() {
            return Err(Error::IdentNotFullyQualified);
        }
        let path = channel_package_demote(channel, ident);
        debug!("Demoting package {}, target {}", ident, target);

        let custom = |url: &mut Url| {
            url.query_pairs_mut()
               .append_pair("target", &target.to_string());
        };

        let res = self.add_authz(self.0.put_with_custom_url(&path, custom), token)
                      .send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        };

        Ok(())
    }

    /// Create a custom channel
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    pub fn create_channel(&self, origin: &str, channel: &ChannelIdent, token: &str) -> Result<()> {
        let path = format!("depot/channels/{}/{}", origin, channel);
        debug!("Creating channel, path: {:?}", path);

        let res = self.add_authz(self.0.post(&path), token).send()?;

        if res.status() != StatusCode::CREATED {
            return Err(err_from_response(res));
        };

        Ok(())
    }

    /// Delete a custom channel
    ///
    /// # Failures
    ///
    /// * Remote Builder is not available
    pub fn delete_channel(&self, origin: &str, channel: &ChannelIdent, token: &str) -> Result<()> {
        let path = format!("depot/channels/{}/{}", origin, channel);
        debug!("Deleting channel, path: {:?}", path);

        let res = self.add_authz(self.0.delete(&path), token).send()?;

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        };

        Ok(())
    }

    /// Return a list of channels for a given origin
    ///
    /// # Failures
    /// * Remote Builder is not available
    /// * Authorization token was not set on client
    pub fn list_channels(&self,
                         origin: &str,
                         include_sandbox_channels: bool)
                         -> Result<Vec<String>> {
        let path = format!("depot/channels/{}", origin);
        let mut res = if include_sandbox_channels {
            self.0
                .get_with_custom_url(&path, |url| url.set_query(Some("sandbox=true")))
                .send()?
        } else {
            self.0.get(&path).send()?
        };

        match res.status() {
            StatusCode::OK | StatusCode::PARTIAL_CONTENT => {
                let mut encoded = String::new();
                res.read_to_string(&mut encoded)
                   .map_err(Error::BadResponseBody)?;
                let results: Vec<OriginChannelIdent> = serde_json::from_str(&encoded)?;
                let channels = results.into_iter().map(|o| o.name).collect();
                Ok(channels)
            }
            _ => Err(err_from_response(res)),
        }
    }

    /// Returns a vector of PackageIdent structs
    ///
    /// # Failures
    ///
    /// * Remote depot unavailable
    pub fn search_package(&self,
                          search_term: &str,
                          token: Option<&str>)
                          -> Result<(Vec<PackageIdent>, bool)> {
        let mut res = self.maybe_add_authz(self.0.get(&package_search(search_term)), token)
                          .send()?;
        match res.status() {
            StatusCode::OK | StatusCode::PARTIAL_CONTENT => {
                let mut encoded = String::new();
                res.read_to_string(&mut encoded)
                   .map_err(Error::BadResponseBody)?;
                let package_results: PackageResults<PackageIdent> = serde_json::from_str(&encoded)?;
                let packages: Vec<PackageIdent> = package_results.data;
                Ok((packages, res.status() == StatusCode::PARTIAL_CONTENT))
            }
            _ => Err(err_from_response(res)),
        }
    }

    fn maybe_add_authz(&self, rb: RequestBuilder, token: Option<&str>) -> RequestBuilder {
        if token.is_some() {
            rb.bearer_auth(token.unwrap())
        } else {
            rb
        }
    }

    fn add_authz(&self, rb: RequestBuilder, token: &str) -> RequestBuilder { rb.bearer_auth(token) }

    fn download<D>(&self,
                   rb: RequestBuilder,
                   dst_path: &Path,
                   token: Option<&str>,
                   progress: Option<D>)
                   -> Result<PathBuf>
        where D: DisplayProgress + Sized
    {
        let mut res = self.maybe_add_authz(rb, token).send()?;
        debug!("Response: {:?}", res);

        if res.status() != StatusCode::OK {
            return Err(err_from_response(res));
        }

        fs::create_dir_all(&dst_path)?;
        let dst_file_path = dst_path.join(file_name(&res)?);
        let size = res.content_length().unwrap_or(0);
        let w = AtomicWriter::new(&dst_file_path)?;
        w.with_writer(|f| {
             match progress {
                 Some(mut progress) => {
                     progress.size(size);
                     io::copy(&mut res, &mut Both(f, progress))
                 }
                 None => io::copy(&mut res, f),
             }
         })
         .map_err(Error::BadResponseBody)?;
        Ok(dst_file_path)
    }

    fn x_download(&self, path: &str, dst_path: &Path, token: &str) -> Result<PathBuf> {
        self.download::<NoProgress>(self.0.get(path), dst_path, Some(token), None)
    }
}

/// The file name Builder gives a download in its `X-Filename` header.
fn file_name(res: &Response) -> Result<String> {
    res.headers()
       .get("X-Filename")
       .and_then(|v| v.to_str().ok())
       .filter(|name| !name.is_empty() && !name.contains('/') && *name != "..")
       .map(String::from)
       .ok_or(Error::NoFilePart)
}

/// A `CREATED` response, or the error the response describes.
fn created(result: reqwest::Result<Response>) -> Result<()> {
    let response = result?;
    if response.status() == StatusCode::CREATED {
        Ok(())
    } else {
        Err(err_from_response(response))
    }
}

/// An upload of `file`, which is `size` bytes, reported to `progress`.
fn upload_body<D>(file: File, size: u64, progress: Option<D>) -> Body
    where D: DisplayProgress + Send + 'static
{
    match progress {
        Some(mut progress) => {
            progress.size(size);
            Body::sized(Reported(file, progress), size)
        }
        None => Body::sized(file, size),
    }
}

/// Reads from a reader and reports what it read to a progress display.
struct Reported<R, D>(R, D);

impl<R: Read, D: DisplayProgress> Read for Reported<R, D> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.0.read(buf)?;
        self.1.write_all(&buf[..n])?;
        Ok(n)
    }
}

/// Writes to a file and reports what it wrote to a progress display.
struct Both<'a, D>(&'a mut File, D);

impl<D: DisplayProgress> Write for Both<'_, D> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.0.write(buf)?;
        self.1.write_all(&buf[..n])?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()?;
        self.1.flush()
    }
}

/// No progress display, for downloads nobody watches.
struct NoProgress;

impl Write for NoProgress {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> { Ok(buf.len()) }

    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

impl DisplayProgress for NoProgress {
    fn size(&mut self, _: u64) {}

    fn finish(&mut self) {}
}

fn err_from_response(mut response: Response) -> Error {
    if response.status() == StatusCode::UNAUTHORIZED {
        return Error::APIError(response.status(),
                               "Please check that you have specified a valid Personal Access \
                                Token."
                                       .to_string());
    }

    let mut buff = String::new();
    match response.read_to_string(&mut buff) {
        Ok(_) => {
            match serde_json::from_str::<NetError>(&buff) {
                Ok(err) => Error::APIError(response.status(), err.to_string()),
                Err(_) => Error::APIError(response.status(), buff),
            }
        }
        Err(_) => {
            buff.truncate(0);
            Error::APIError(response.status(), buff)
        }
    }
}

fn origin_keys_path(origin: &str) -> String { format!("depot/origins/{}/keys", origin) }

fn origin_secret_keys_latest(origin: &str) -> String {
    format!("depot/origins/{}/secret_keys/latest", origin)
}

fn package_download(package: &PackageIdent) -> String {
    format!("{}/download", package_path(package))
}

fn package_path(package: &PackageIdent) -> String { format!("depot/pkgs/{}", package) }

fn package_search(term: &str) -> String {
    let encoded_term = utf8_percent_encode(term, PATH_SEGMENT);
    format!("depot/pkgs/search/{}", encoded_term)
}

fn channel_package_path(channel: &ChannelIdent, package: &PackageIdent) -> String {
    let mut path = format!("depot/channels/{}/{}/pkgs/{}",
                           package.origin(),
                           channel,
                           package.name());
    if let Some(version) = package.version() {
        path.push_str("/");
        path.push_str(version);
        if let Some(release) = package.release() {
            path.push_str("/");
            path.push_str(release);
        }
    }
    path
}

fn package_channels_path(package: &PackageIdent) -> String {
    format!("depot/pkgs/{}/{}/{}/{}/channels",
            package.origin(),
            package.name(),
            package.version().unwrap(),
            package.release().unwrap())
}

fn channel_package_promote(channel: &ChannelIdent, package: &PackageIdent) -> String {
    format!("depot/channels/{}/{}/pkgs/{}/{}/{}/promote",
            package.origin(),
            channel,
            package.name(),
            package.version().unwrap(),
            package.release().unwrap())
}

fn channel_package_demote(channel: &ChannelIdent, package: &PackageIdent) -> String {
    format!("depot/channels/{}/{}/pkgs/{}/{}/{}/demote",
            package.origin(),
            channel,
            package.name(),
            package.version().unwrap(),
            package.release().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;
    use std::{net::TcpListener,
              sync::mpsc,
              thread};

    /// Serves one request on localhost with `response`, and returns the
    /// client's URL and what the request was: its head and body.
    fn serve_once(response: &'static str) -> (String, mpsc::Receiver<(String, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                head.push(byte[0]);
            }
            let head = String::from_utf8(head).unwrap();
            let length = head.lines()
                             .find_map(|l| {
                                 l.to_lowercase()
                                  .strip_prefix("content-length: ")
                                  .map(String::from)
                             })
                             .map_or(0, |n| n.trim().parse::<usize>().unwrap());
            let mut body = vec![0u8; length];
            stream.read_exact(&mut body).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            tx.send((head, body)).unwrap();
        });
        (url, rx)
    }

    fn client(url: &str) -> Client { Client::new(url, "dojo-test", "0.0.0", None).unwrap() }

    #[test]
    fn a_download_is_written_under_the_name_builder_gives() {
        let (url, received) = serve_once("HTTP/1.1 200 OK\r\nX-Filename: karate-20261027.pub\r\n\
                                          Content-Length: 5\r\n\r\nhello");
        let dir = tempfile::tempdir().unwrap();
        let path =
            client(&url).fetch_origin_key::<NoProgress, _>("karate", "20261027", dir.path(), None)
                        .unwrap();
        assert_eq!(path, dir.path().join("karate-20261027.pub"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        let (head, _) = received.recv().unwrap();
        assert!(head.starts_with("GET /v1/depot/origins/karate/keys/20261027 HTTP/1.1\r\n"),
                "{}",
                head);
    }

    #[test]
    fn a_download_without_a_file_name_is_refused() {
        let (url, _) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello");
        let dir = tempfile::tempdir().unwrap();
        let result =
            client(&url).fetch_origin_key::<NoProgress, _>("karate", "20261027", dir.path(), None);
        assert!(matches!(result, Err(Error::NoFilePart)));
    }

    #[test]
    fn an_upload_sends_the_file_with_the_token() {
        let (url, received) = serve_once("HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("karate-20261027.pub");
        fs::write(&key, "public key").unwrap();
        client(&url).put_origin_key::<NoProgress>("karate", "20261027", &key, "t0ken", None)
                    .unwrap();
        let (head, body) = received.recv().unwrap();
        assert!(head.starts_with("POST /v1/depot/origins/karate/keys/20261027 HTTP/1.1\r\n"));
        assert!(head.to_lowercase()
                    .contains("authorization: bearer t0ken\r\n"),
                "{}",
                head);
        assert_eq!(body, b"public key");
    }

    #[test]
    fn an_error_response_carries_its_status_and_message() {
        let (url, _) = serve_once("HTTP/1.1 404 Not Found\r\nContent-Length: 38\r\n\r\n\
                                   {\"code\": 404, \"msg\": \"no such origin\"}");
        match client(&url).show_origin_keys("karate") {
            Err(Error::APIError(status, message)) => {
                assert_eq!(status, StatusCode::NOT_FOUND);
                assert_eq!(message, "[err: 404, msg: no such origin]");
            }
            Err(e) => panic!("wrong error: {}", e),
            Ok(_) => panic!("succeeded on a 404"),
        }
    }

    #[test]
    fn json_round_trip_u64_fields() {
        let pre = OriginPrivateSigningKey { id:        705_705_315_793_903_646,
                                            origin_id: 705_705_305_031_319_582,
                                            name:      "karate".to_string(),
                                            revision:  "20160810182414".to_string(),
                                            body:      vec![1, 2, 3],
                                            owner_id:  0, };

        let as_json = serde_json::to_value(&pre).unwrap();
        let expected = json!({
            "id": "705705315793903646",
            "origin_id": "705705305031319582",
            "name": "karate",
            "revision": "20160810182414",
            "body": [
                1,
                2,
                3
            ],
            "owner_id": "0"
        });
        assert_eq!(as_json, expected);

        let post: OriginPrivateSigningKey = serde_json::from_value(as_json).unwrap();
        assert_eq!(pre.id, post.id);
        assert_eq!(pre.origin_id, post.origin_id);
        assert_eq!(pre.owner_id, post.owner_id);
    }
}
