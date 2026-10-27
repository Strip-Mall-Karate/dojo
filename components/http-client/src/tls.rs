// Copyright (c) 2026 Strip Mall Karate Company
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

//! The TLS configuration: rustls with the ring provider, trusting the CA
//! certificates Dojo has always looked for, in this order:
//!
//! 1. `SSL_CERT_FILE`, or every certificate file in `SSL_CERT_DIR`.
//! 2. The installed `karate/cacerts` package's `ssl/cert.pem`.
//! 3. Mozilla's root certificates, built in.
//!
//! `DOJ_SSL_CERT_VERIFY_NONE` turns certificate verification off.

use std::{env,
          fs,
          path::{Path,
                 PathBuf},
          str::FromStr,
          sync::Arc};

use dojo_core::package::{PackageIdent,
                         PackageInstall};
use log::debug;
use rustls::{ClientConfig,
             DigitallySignedStruct,
             RootCertStore,
             SignatureScheme,
             client::danger::{HandshakeSignatureValid,
                              ServerCertVerified,
                              ServerCertVerifier},
             crypto::{CryptoProvider,
                      ring}};
use rustls_pki_types::{CertificateDer,
                       ServerName,
                       UnixTime,
                       pem::PemObject};

use crate::error::{Error,
                   Result};

const CACERTS_PKG_IDENT: &str = "karate/cacerts";

pub fn config(fs_root_path: Option<&Path>) -> Result<ClientConfig> {
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions()?;
    let config = if env::var_os("DOJ_SSL_CERT_VERIFY_NONE").is_some() {
        builder.dangerous()
               .with_custom_certificate_verifier(Arc::new(NoVerification(provider)))
               .with_no_client_auth()
    } else {
        builder.with_root_certificates(roots(fs_root_path)?)
               .with_no_client_auth()
    };
    Ok(config)
}

fn roots(fs_root_path: Option<&Path>) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    let files = ca_files(fs_root_path)?;
    if files.is_empty() {
        debug!("Trusting Mozilla's root certificates");
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        return Ok(roots);
    }
    for file in files {
        debug!("Trusting the CA certificates in {}", file.display());
        let certs = CertificateDer::pem_file_iter(&file).map_err(|e| {
                                                            Error::CaCertificates(file.clone(),
                                                                                  e.to_string())
                                                        })?;
        for cert in certs {
            let cert = cert.map_err(|e| Error::CaCertificates(file.clone(), e.to_string()))?;
            roots.add(cert)?;
        }
    }
    Ok(roots)
}

/// The PEM files to trust, or none to trust the built-in roots.
fn ca_files(fs_root_path: Option<&Path>) -> Result<Vec<PathBuf>> {
    if let Some(file) = env::var_os("SSL_CERT_FILE") {
        return Ok(vec![PathBuf::from(file)]);
    }
    if let Some(dir) = env::var_os("SSL_CERT_DIR") {
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)?.filter_map(|e| e.ok())
                                                         .map(|e| e.path())
                                                         .filter(|p| p.is_file())
                                                         .collect();
        files.sort();
        return Ok(files);
    }
    let ident = PackageIdent::from_str(CACERTS_PKG_IDENT)?;
    match PackageInstall::load(&ident, fs_root_path) {
        Ok(install) => Ok(vec![install.installed_path().join("ssl/cert.pem")]),
        Err(_) => Ok(Vec::new()),
    }
}

/// Accepts every certificate, for `DOJ_SSL_CERT_VERIFY_NONE`.
#[derive(Debug)]
struct NoVerification(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerification {
    fn verify_server_cert(&self,
                          _: &CertificateDer<'_>,
                          _: &[CertificateDer<'_>],
                          _: &ServerName<'_>,
                          _: &[u8],
                          _: UnixTime)
                          -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self,
                              message: &[u8],
                              cert: &CertificateDer<'_>,
                              dss: &DigitallySignedStruct)
                              -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message,
                                               cert,
                                               dss,
                                               &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self,
                              message: &[u8],
                              cert: &CertificateDer<'_>,
                              dss: &DigitallySignedStruct)
                              -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message,
                                               cert,
                                               dss,
                                               &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
