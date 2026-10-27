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

use crate::{error::{Result,
                    SupError},
            manager::{self,
                      service::{HealthCheck,
                                HealthCheckHook}}};
use axum::{extract::{Path,
                     Request,
                     State},
           http::{header,
                  StatusCode},
           middleware::{self,
                        Next},
           response::{Html,
                      IntoResponse,
                      Response},
           routing::get,
           Json,
           Router};
use dojo_common::{cli::{LISTEN_HTTP_ADDRESS_ENVVAR,
                        LISTEN_HTTP_DEFAULT_IP,
                        LISTEN_HTTP_DEFAULT_PORT},
                  templating::hooks,
                  FeatureFlag};
use dojo_core::{crypto,
                env::Config as EnvConfig,
                service::ServiceGroup};
use hyper_util::{rt::TokioIo,
                 service::TowerToHyperService};
use prometheus::{self,
                 CounterVec,
                 Encoder,
                 HistogramVec,
                 TextEncoder};
use rustls::ServerConfig;
use serde_json::{self,
                 Value};
use std::{fmt,
          fs::File,
          io::{self,
               Read},
          net::{IpAddr,
                SocketAddr,
                SocketAddrV4,
                ToSocketAddrs},
          ops::{Deref,
                DerefMut},
          option,
          result,
          str::FromStr,
          sync::{Arc,
                 RwLock},
          time::Duration};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

const APIDOCS: &str = include_str!(concat!(env!("OUT_DIR"), "/api.html"));

/// Default listening port for the HTTPGateway listener.
pub const DEFAULT_PORT: u16 = 9631;

lazy_static! {
    static ref HTTP_GATEWAY_REQUESTS: CounterVec =
        register_counter_vec!("doj_sup_http_gateway_requests_total",
                              "Total number of HTTP gateway requests",
                              &["path"]).unwrap();
    static ref HTTP_GATEWAY_REQUEST_DURATION: HistogramVec =
        register_histogram_vec!("doj_sup_http_gateway_request_duration_seconds",
                                "The latency for HTTP gateway requests",
                                &["path"]).unwrap();
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct ListenAddr(SocketAddr);

impl ListenAddr {
    pub fn new(ip: IpAddr, port: u16) -> ListenAddr { ListenAddr(SocketAddr::new(ip, port)) }
}

impl Default for ListenAddr {
    fn default() -> ListenAddr {
        ListenAddr(SocketAddr::V4(SocketAddrV4::new(
            LISTEN_HTTP_DEFAULT_IP
                .parse()
                .expect("LISTEN_HTTP_DEFAULT_IP can not be parsed."),
            LISTEN_HTTP_DEFAULT_PORT,
        )))
    }
}

impl EnvConfig for ListenAddr {
    const ENVVAR: &'static str = LISTEN_HTTP_ADDRESS_ENVVAR;
}

impl Deref for ListenAddr {
    type Target = SocketAddr;

    fn deref(&self) -> &SocketAddr { &self.0 }
}

impl DerefMut for ListenAddr {
    fn deref_mut(&mut self) -> &mut SocketAddr { &mut self.0 }
}

impl FromStr for ListenAddr {
    type Err = SupError;

    fn from_str(val: &str) -> Result<Self> { Ok(ListenAddr(SocketAddr::from_str(val)?)) }
}

impl ToSocketAddrs for ListenAddr {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> io::Result<Self::Iter> { self.0.to_socket_addrs() }
}

impl fmt::Display for ListenAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> result::Result<(), fmt::Error> {
        write!(f, "{}", self.0)
    }
}

#[derive(Default, Serialize)]
struct HealthCheckBody {
    status: String,
    stdout: String,
    stderr: String,
}

impl From<HealthCheck> for StatusCode {
    fn from(health_check: HealthCheck) -> Self {
        match health_check {
            HealthCheck::Ok | HealthCheck::Warning => StatusCode::OK,
            HealthCheck::Critical => StatusCode::SERVICE_UNAVAILABLE,
            HealthCheck::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Clone)]
struct AppState {
    gateway_state: Arc<RwLock<manager::GatewayState>>,
}

// Begin middleware
async fn authentication(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let authorized = {
        let gateway_state = state.gateway_state
                                 .read()
                                 .expect("GatewayState lock is poisoned");
        match gateway_state.auth_token.as_ref() {
            // If there's no auth token in the state, everything will continue to function
            // unauthenticated.
            None => {
                debug!("No auth token present. HTTP gateway starting in unauthenticated mode.");
                true
            }
            // With an auth token in the state, anything short of a fully formed Authorization
            // header containing a Bearer token that matches it results in an Unauthorized
            // response.
            Some(current_token) => {
                match req.headers()
                         .get(header::AUTHORIZATION)
                         .ok_or("header missing")
                         .and_then(|hv| hv.to_str().or(Err("can't convert to str")))
                {
                    Ok(hdr) => {
                        let hdr_components: Vec<&str> = hdr.split_whitespace().collect();
                        match hdr_components.as_slice() {
                            ["Bearer", incoming_token] => {
                                crypto::secure_eq(current_token, incoming_token)
                            }
                            _ => false,
                        }
                    }
                    Err(e) => {
                        debug!("Error reading required Authorization header: {:?}.", e);
                        false
                    }
                }
            }
        }
    };

    if authorized {
        next.run(req).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

async fn metrics_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let label_values = &[path.as_str()];

    HTTP_GATEWAY_REQUESTS.with_label_values(label_values).inc();
    let timer = HTTP_GATEWAY_REQUEST_DURATION.with_label_values(label_values)
                                             .start_timer();
    let response = next.run(req).await;
    timer.observe_duration();
    response
}
// End middleware

/// Serve the HTTP gateway on `listener` until the runtime shuts down.
pub async fn run(listener: TcpListener,
                 tls_config: Option<ServerConfig>,
                 gateway_state: Arc<RwLock<manager::GatewayState>>,
                 feature_flags: FeatureFlag) {
    let app = routes(AppState { gateway_state }, feature_flags);
    match tls_config {
        Some(c) => serve_tls(listener, TlsAcceptor::from(Arc::new(c)), app).await,
        None => {
            if let Err(e) = axum::serve(listener, app).await {
                error!("HTTP gateway stopped: {}", e);
            }
        }
    }
}

async fn serve_tls(listener: TcpListener, acceptor: TlsAcceptor, app: Router) {
    loop {
        let (socket, addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                // Running out of file descriptors fails accept until some close.
                error!("HTTP gateway accept failed: {}", e);
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move {
            let stream = match acceptor.accept(socket).await {
                Ok(stream) => stream,
                Err(e) => {
                    debug!("TLS handshake with {} failed: {}", addr, e);
                    return;
                }
            };
            if let Err(e) =
                hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream),
                                                                            TowerToHyperService::new(app))
                                                          .await
            {
                debug!("HTTP gateway connection from {} ended: {}", addr, e);
            }
        });
    }
}

fn routes(state: AppState, feature_flags: FeatureFlag) -> Router {
    let mut app = Router::new().route("/", get(doc))
                               .route("/services", get(services))
                               .route("/services/{svc}/{group}", get(service_without_org))
                               .route("/services/{svc}/{group}/config", get(config_without_org))
                               .route("/services/{svc}/{group}/health", get(health_without_org))
                               .route("/services/{svc}/{group}/{org}", get(service_with_org))
                               .route("/services/{svc}/{group}/{org}/config", get(config_with_org))
                               .route("/services/{svc}/{group}/{org}/health", get(health_with_org))
                               .route("/metrics", get(metrics));
    if !feature_flags.contains(FeatureFlag::REDACT_HTTP) {
        app = app.route("/butterfly", get(butterfly))
                 .route("/census", get(census));
    }
    // The last layer added runs first: authentication, then metrics.
    app.layer(middleware::from_fn(metrics_middleware))
       .layer(middleware::from_fn_with_state(state.clone(), authentication))
       .with_state(state)
}

fn json_response(data: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], data).into_response()
}

// Begin route handlers
async fn butterfly(State(state): State<AppState>) -> Response {
    let data = state.gateway_state
                    .read()
                    .expect("GatewayState lock is poisoned")
                    .butterfly_data
                    .clone();
    json_response(data)
}

async fn census(State(state): State<AppState>) -> Response {
    let data = state.gateway_state
                    .read()
                    .expect("GatewayState lock is poisoned")
                    .census_data
                    .clone();
    json_response(data)
}

async fn services(State(state): State<AppState>) -> Response {
    let data = state.gateway_state
                    .read()
                    .expect("GatewayState lock is poisoned")
                    .services_data
                    .clone();
    json_response(data)
}

async fn config_with_org(State(state): State<AppState>,
                         Path((svc, group, org)): Path<(String, String, String)>)
                         -> Response {
    config(&state, svc, group, Some(&org))
}

async fn config_without_org(State(state): State<AppState>,
                            Path((svc, group)): Path<(String, String)>)
                            -> Response {
    config(&state, svc, group, None)
}

fn config(state: &AppState, svc: String, group: String, org: Option<&str>) -> Response {
    let service_group = match ServiceGroup::new(None, svc, group, org) {
        Ok(sg) => sg,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let data = &state.gateway_state
                     .read()
                     .expect("GatewayState lock is poisoned")
                     .services_data;

    match service_from_services(&service_group, &data) {
        Some(mut s) => Json(s["cfg"].take()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn health_with_org(State(state): State<AppState>,
                         Path((svc, group, org)): Path<(String, String, String)>)
                         -> Response {
    health(&state, svc, group, Some(&org))
}

async fn health_without_org(State(state): State<AppState>,
                            Path((svc, group)): Path<(String, String)>)
                            -> Response {
    health(&state, svc, group, None)
}

fn health(state: &AppState, svc: String, group: String, org: Option<&str>) -> Response {
    let service_group = match ServiceGroup::new(None, svc, group, org) {
        Ok(sg) => sg,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let health_check = state.gateway_state
                            .read()
                            .expect("GatewayState lock is poisoned")
                            .health_check_data
                            .get(&service_group)
                            .cloned();

    match health_check {
        Some(health_check) => {
            let mut body = HealthCheckBody::default();
            let stdout_path = hooks::stdout_log_path::<HealthCheckHook>(&service_group);
            let stderr_path = hooks::stderr_log_path::<HealthCheckHook>(&service_group);

            body.status = health_check.to_string();
            if let Ok(mut file) = File::open(&stdout_path) {
                let _ = file.read_to_string(&mut body.stdout);
            }
            if let Ok(mut file) = File::open(&stderr_path) {
                let _ = file.read_to_string(&mut body.stderr);
            }

            (StatusCode::from(health_check), Json(body)).into_response()
        }
        None => {
            debug!("Didn't find any health data for service group {:?}",
                   &service_group);
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

async fn service_with_org(State(state): State<AppState>,
                          Path((svc, group, org)): Path<(String, String, String)>)
                          -> Response {
    service(&state, svc, group, Some(&org))
}

async fn service_without_org(State(state): State<AppState>,
                             Path((svc, group)): Path<(String, String)>)
                             -> Response {
    service(&state, svc, group, None)
}

fn service(state: &AppState, svc: String, group: String, org: Option<&str>) -> Response {
    let service_group = match ServiceGroup::new(None, svc, group, org) {
        Ok(sg) => sg,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let data = &state.gateway_state
                     .read()
                     .expect("GatewayState lock is poisoned")
                     .services_data;

    match service_from_services(&service_group, &data) {
        Some(s) => Json(s).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn metrics() -> Response {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];

    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        error!("Error encoding metrics: {:?}", e);
    }

    let resp = match String::from_utf8(buffer) {
        Ok(s) => s,
        Err(e) => {
            error!("Error constructing string from metrics buffer: {:?}", e);
            String::from("")
        }
    };

    ([(header::CONTENT_TYPE, encoder.format_type().to_string())], resp).into_response()
}

async fn doc() -> Html<&'static str> { Html(APIDOCS) }
// End route handlers

fn service_from_services(service_group: &ServiceGroup, services_json: &str) -> Option<Value> {
    match serde_json::from_str(services_json) {
        Ok(Value::Array(services)) => {
            services.into_iter()
                    .find(|s| s["service_group"] == service_group.as_ref())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::test_helpers::*;
    use dojo_butterfly::{member::Member,
                         server::{Server,
                                  ServerProxy,
                                  Suitability},
                         trace::Trace};
    use serde_json;
    use std::{fs::File,
              io::Read,
              net::{IpAddr,
                    Ipv4Addr,
                    SocketAddr},
              path::PathBuf,
              sync::Mutex};

    fn validate_sample_file_against_schema(name: &str, schema: &str) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
                                                            .join("fixtures")
                                                            .join("http-gateway")
                                                            .join(name);

        let mut f = File::open(path).unwrap_or_else(|_| panic!("could not open {}", &name));
        let mut json = String::new();
        f.read_to_string(&mut json)
         .unwrap_or_else(|_| panic!("could not read {}", &name));

        assert_valid(&json, schema);
    }

    #[test]
    fn sample_census_file_is_valid() {
        validate_sample_file_against_schema("sample-census-output.json",
                                            "http_gateway_census_schema.json");
    }

    #[test]
    fn trivial_census_failure() {
        let failure = validate_string(r#"{"census_groups": {}, "changed": false, "last_election_counter": "narf"}"#,
                                      "http_gateway_census_schema.json");
        assert!(!failure.is_valid(),
                "Expected schema validation to fail, but it succeeded");
    }

    #[test]
    fn sample_butterfly_file_is_valid() {
        validate_sample_file_against_schema("sample-butterfly-output.json",
                                            "http_gateway_butterfly_schema.json");
    }

    #[test]
    fn trivial_butterfly_failure() {
        let failure = validate_string(r#"{"departure": {}, "election": {}, "member": {}, "service": false, "service_file": []}"#,
                                      "http_gateway_butterfly_schema.json");
        assert!(!failure.is_valid(),
                "Expected schema validation to fail, but it succeeded");
    }

    #[test]
    fn butterfly_server_proxy_is_valid() {
        lazy_static! {
            static ref SWIM_PORT: Mutex<u16> = Mutex::new(6666);
            static ref GOSSIP_PORT: Mutex<u16> = Mutex::new(7777);
        }

        #[derive(Debug)]
        struct ZeroSuitability;
        impl Suitability for ZeroSuitability {
            fn get(&self, _service_group: &str) -> u64 { 0 }
        }

        fn start_server() -> Server {
            let swim_port;
            {
                let mut swim_port_guard = SWIM_PORT.lock().expect("SWIM_PORT poisoned");
                swim_port = *swim_port_guard;
                *swim_port_guard += 1;
            }
            let swim_listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), swim_port);
            let gossip_port;
            {
                let mut gossip_port_guard = GOSSIP_PORT.lock().expect("GOSSIP_PORT poisoned");
                gossip_port = *gossip_port_guard;
                *gossip_port_guard += 1;
            }
            let gossip_listen =
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), gossip_port);
            let mut member = Member::default();
            member.swim_port = swim_port;
            member.gossip_port = gossip_port;
            Server::new(swim_listen,
                        gossip_listen,
                        member,
                        Trace::default(),
                        None,
                        None,
                        None,
                        Box::new(ZeroSuitability)).unwrap()
        }

        let server = start_server();
        let proxy = ServerProxy::new(&server);
        let json = serde_json::to_string(&proxy).unwrap();
        assert_valid(&json, "http_gateway_butterfly_schema.json");
    }

    #[test]
    fn sample_services_with_cfg_file_is_valid() {
        validate_sample_file_against_schema("sample-services-with-cfg-output.json",
                                            "http_gateway_services_schema.json");
    }

    #[test]
    fn sample_services_without_cfg_file_is_valid() {
        validate_sample_file_against_schema("sample-services-without-cfg-output.json",
                                            "http_gateway_services_schema.json");
    }

    #[test]
    fn trivial_services_failure() {
        let failure = validate_string(r#"[{"lulz": true}]"#, "http_gateway_services_schema.json");
        assert!(!failure.is_valid(),
                "Expected schema validation to fail, but it succeeded");
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use axum::body::{self,
                     Body};
    use tower::ServiceExt;

    const TOKEN: &str = "the-token";

    fn app(auth_token: Option<&str>, feature_flags: FeatureFlag) -> Router {
        let mut gateway_state = manager::GatewayState::default();
        gateway_state.auth_token = auth_token.map(String::from);
        gateway_state.butterfly_data = r#"{"member": {}}"#.to_string();
        gateway_state.services_data =
            r#"[{"service_group": "redis.default", "cfg": {"port": 6379}}]"#.to_string();
        routes(AppState { gateway_state: Arc::new(RwLock::new(gateway_state)), },
               feature_flags)
    }

    async fn get(app: Router, uri: &str, bearer: Option<&str>) -> (StatusCode, String) {
        let mut req = axum::http::Request::get(uri);
        if let Some(token) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {}", token));
        }
        let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_configured_token_is_required() {
        let flags = FeatureFlag::empty();
        assert_eq!(get(app(Some(TOKEN), flags), "/services", None).await.0,
                   StatusCode::UNAUTHORIZED);
        assert_eq!(get(app(Some(TOKEN), flags), "/services", Some("wrong")).await
                                                                           .0,
                   StatusCode::UNAUTHORIZED);
        assert_eq!(get(app(Some(TOKEN), flags), "/services", Some(TOKEN)).await
                                                                         .0,
                   StatusCode::OK);
    }

    #[tokio::test]
    async fn without_a_token_every_request_is_served() {
        assert_eq!(get(app(None, FeatureFlag::empty()), "/services", None).await
                                                                          .0,
                   StatusCode::OK);
    }

    #[tokio::test]
    async fn redaction_hides_butterfly_and_census() {
        let (status, body) = get(app(None, FeatureFlag::empty()), "/butterfly", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"member": {}}"#);

        for path in &["/butterfly", "/census"] {
            assert_eq!(get(app(None, FeatureFlag::REDACT_HTTP), path, None).await.0,
                       StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn a_service_and_its_config_are_found_by_group() {
        let (status, body) = get(app(None, FeatureFlag::empty()),
                                 "/services/redis/default/config",
                                 None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"port":6379}"#);

        let (status, body) = get(app(None, FeatureFlag::empty()),
                                 "/services/redis/default",
                                 None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#""service_group":"redis.default""#),
                "{}",
                body);

        assert_eq!(get(app(None, FeatureFlag::empty()),
                       "/services/memcached/default",
                       None).await
                            .0,
                   StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn metrics_are_prometheus_text() {
        let res = app(None, FeatureFlag::empty()).oneshot(axum::http::Request::get("/metrics").body(Body::empty())
                                                                                    .unwrap())
                                                 .await
                                                 .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers()[header::CONTENT_TYPE].to_str()
                                                   .unwrap()
                                                   .starts_with("text/plain"));
    }
}
