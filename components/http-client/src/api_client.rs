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

use std::{path::Path,
          time::Duration};

use dojo_core::{package::PackageTarget,
                util::sys};
use log::debug;
use reqwest::{IntoUrl,
              Method,
              blocking::{Client,
                         RequestBuilder}};
use url::Url;

use crate::{error::Result,
            tls};

/// How long a request may take, connecting, sending and receiving.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(300);

/// An HTTP client for one API endpoint, sending Dojo's User-Agent.
#[derive(Debug)]
pub struct ApiClient {
    /// The base URL every path is joined to.
    endpoint: Url,
    inner:    Client,
}

impl ApiClient {
    /// Creates a client for `endpoint` that names `product` and `version`
    /// in its User-Agent.
    ///
    /// # Errors
    ///
    /// * If `endpoint` is not a URL
    /// * If the TLS configuration or its CA certificates cannot be loaded
    /// * If system information for the User-Agent cannot be read
    pub fn new<T>(endpoint: T,
                  product: &str,
                  version: &str,
                  fs_root_path: Option<&Path>)
                  -> Result<Self>
        where T: IntoUrl
    {
        let endpoint = endpoint.into_url()?;
        let inner = Client::builder().user_agent(user_agent(product, version)?)
                                     .timeout(CLIENT_TIMEOUT)
                                     .tls_backend_preconfigured(tls::config(fs_root_path)?)
                                     .build()?;
        Ok(ApiClient { endpoint, inner })
    }

    pub fn get(&self, path: &str) -> RequestBuilder { self.get_with_custom_url(path, |_| {}) }

    pub fn get_with_custom_url<F>(&self, path: &str, customize_url: F) -> RequestBuilder
        where F: FnMut(&mut Url)
    {
        self.request(Method::GET, path, customize_url)
    }

    pub fn head(&self, path: &str) -> RequestBuilder { self.head_with_custom_url(path, |_| {}) }

    pub fn head_with_custom_url<F>(&self, path: &str, customize_url: F) -> RequestBuilder
        where F: FnMut(&mut Url)
    {
        self.request(Method::HEAD, path, customize_url)
    }

    pub fn patch(&self, path: &str) -> RequestBuilder { self.patch_with_custom_url(path, |_| {}) }

    pub fn patch_with_custom_url<F>(&self, path: &str, customize_url: F) -> RequestBuilder
        where F: FnMut(&mut Url)
    {
        self.request(Method::PATCH, path, customize_url)
    }

    pub fn post(&self, path: &str) -> RequestBuilder { self.post_with_custom_url(path, |_| {}) }

    pub fn post_with_custom_url<F>(&self, path: &str, customize_url: F) -> RequestBuilder
        where F: FnMut(&mut Url)
    {
        self.request(Method::POST, path, customize_url)
    }

    pub fn put(&self, path: &str) -> RequestBuilder { self.put_with_custom_url(path, |_| {}) }

    pub fn put_with_custom_url<F>(&self, path: &str, customize_url: F) -> RequestBuilder
        where F: FnMut(&mut Url)
    {
        self.request(Method::PUT, path, customize_url)
    }

    pub fn delete(&self, path: &str) -> RequestBuilder { self.delete_with_custom_url(path, |_| {}) }

    pub fn delete_with_custom_url<F>(&self, path: &str, customize_url: F) -> RequestBuilder
        where F: FnMut(&mut Url)
    {
        self.request(Method::DELETE, path, customize_url)
    }

    fn request<F>(&self, method: Method, path: &str, mut customize_url: F) -> RequestBuilder
        where F: FnMut(&mut Url)
    {
        let mut url = self.url_for(path);
        customize_url(&mut url);
        debug!("{} {}", method, url);
        self.inner.request(method, url)
    }

    fn url_for(&self, path: &str) -> Url {
        let mut url = self.endpoint.clone();

        if path.is_empty() {
            return url;
        }

        if url.path().ends_with('/') || path.starts_with('/') {
            url.set_path(&format!("{}{}", self.endpoint.path(), path));
        } else {
            url.set_path(&format!("{}/{}", self.endpoint.path(), path));
        }

        url
    }
}

/// `product/version (target; kernel release)`.
///
/// # Errors
///
/// * If system information cannot be obtained via `uname`
fn user_agent(product: &str, version: &str) -> Result<String> {
    let uname = sys::uname()?;
    let ua = format!("{}/{} ({}; {})",
                     product.trim(),
                     version.trim(),
                     PackageTarget::active_target(),
                     uname.release.trim().to_lowercase());
    debug!("User-Agent: {}", &ua);
    Ok(ua)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(endpoint: &str) -> ApiClient {
        ApiClient::new(endpoint, "dojo-test", "0.0.0", None).unwrap()
    }

    #[test]
    fn paths_join_the_endpoint() {
        assert_eq!(client("https://bldr.karate.company/v1").url_for("depot/pkgs")
                                                           .as_str(),
                   "https://bldr.karate.company/v1/depot/pkgs");
        assert_eq!(client("https://bldr.karate.company/v1/").url_for("depot/pkgs")
                                                            .as_str(),
                   "https://bldr.karate.company/v1/depot/pkgs");
        assert_eq!(client("https://bldr.karate.company/v1").url_for("")
                                                           .as_str(),
                   "https://bldr.karate.company/v1");
    }

    /// Serves one request on localhost and returns its head, as received.
    fn serve_once() -> (String, std::sync::mpsc::Receiver<String>) {
        use std::{io::{Read,
                       Write},
                  net::TcpListener,
                  sync::mpsc,
                  thread};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                head.push(byte[0]);
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                  .unwrap();
            tx.send(String::from_utf8(head).unwrap()).unwrap();
        });
        (url, rx)
    }

    #[test]
    fn requests_carry_the_user_agent_and_path() {
        let (url, received) = serve_once();
        let response = client(&url).get("depot/pkgs").send().unwrap();
        assert_eq!(response.status(), 200);
        let head = received.recv().unwrap().to_lowercase();
        assert!(head.starts_with("get /v1/depot/pkgs http/1.1\r\n"),
                "{}",
                head);
        assert!(head.contains("\r\nuser-agent: dojo-test/0.0.0 ("),
                "{}",
                head);
    }
}
