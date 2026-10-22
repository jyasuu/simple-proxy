use async_trait::async_trait;
use bytes::Bytes;
use chrono::{SecondsFormat, Utc};
use pingora::http::{HMap, RequestHeader, ResponseHeader};
use pingora::lb::selection::RoundRobin;
use pingora::lb::LoadBalancer;
use pingora::proxy::{http_proxy_service, ProxyHttp, Session};
use pingora::server::Server;
use pingora::upstreams::peer::HttpPeer;
use pingora::{Error, ErrorType, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::env;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::time::Duration;

const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
struct Conf {
    #[serde(default)]
    server: ServerConf,
    upstream: UpstreamConf,
    #[serde(default)]
    dump: DumpConf,
}

#[derive(Deserialize, Default)]
struct ServerConf {
    #[serde(default = "default_listen")]
    listen: String,
}

fn default_listen() -> String {
    String::from("127.0.0.1:8080")
}

#[derive(Deserialize)]
struct UpstreamConf {
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default)]
    tls: bool,
    #[serde(default)]
    auth_header: Option<String>,
}

fn default_port() -> u16 {
    80
}

impl UpstreamConf {
    fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn host_header(&self) -> String {
        let default = if self.tls { 443 } else { 80 };
        if self.port == default {
            self.host.clone()
        } else {
            self.addr()
        }
    }
}

/// Optional request/response capture.
#[derive(Deserialize, Default)]
struct DumpConf {
    /// Append JSONL records to this file. Capturing is disabled when unset.
    #[serde(default)]
    file: Option<String>,
    /// Maximum number of body bytes captured per request and per response.
    #[serde(default = "default_max_body_bytes")]
    max_body_bytes: usize,
}

fn default_max_body_bytes() -> usize {
    DEFAULT_MAX_BODY_BYTES
}

/// Per request state shared between the proxy filters.
#[derive(Default)]
pub struct ProxyCtx {
    method: String,
    uri: String,
    req_headers: Vec<(String, String)>,
    req_body: Vec<u8>,
    req_truncated: bool,
    resp_headers: Vec<(String, String)>,
    resp_body: Vec<u8>,
    resp_truncated: bool,
    status: Option<u16>,
    dumped_request: bool,
    dumped_response: bool,
}

pub struct Proxy {
    lb: LoadBalancer<RoundRobin>,
    upstream: UpstreamConf,
    auth_header: Option<String>,
    dump_file: Option<Mutex<File>>,
    max_body_bytes: usize,
}

impl Proxy {
    /// Append one JSONL record to the dump file.
    fn write_dump(&self, record: Value) {
        let Some(file) = &self.dump_file else {
            return;
        };
        let mut file = match file.lock() {
            Ok(f) => f,
            Err(poisoned) => poisoned.into_inner(),
        };
        let line = serde_json::to_string(&record).unwrap_or_else(|_| "{}".to_string());
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }

    fn dump_request(&self, ctx: &mut ProxyCtx) {
        if ctx.dumped_request {
            return;
        }
        ctx.dumped_request = true;

        let body = String::from_utf8_lossy(&ctx.req_body).to_string();
        let body_json = serde_json::from_str::<Value>(&body).ok();

        let record = json!({
            "kind": "request",
            "ts": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            "method": ctx.method,
            "uri": ctx.uri,
            "headers": header_map(&ctx.req_headers),
            "body_bytes": ctx.req_body.len(),
            "body_truncated": ctx.req_truncated,
            "body": body,
            "body_json": body_json,
        });
        self.write_dump(record);
    }

    fn dump_response(&self, ctx: &mut ProxyCtx) {
        if ctx.dumped_response {
            return;
        }
        ctx.dumped_response = true;

        let body = String::from_utf8_lossy(&ctx.resp_body).to_string();
        let body_json = serde_json::from_str::<Value>(&body).ok();

        let record = json!({
            "kind": "response",
            "ts": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            "uri": ctx.uri,
            "status": ctx.status,
            "headers": header_map(&ctx.resp_headers),
            "body_bytes": ctx.resp_body.len(),
            "body_truncated": ctx.resp_truncated,
            "body": body,
            "body_json": body_json,
        });
        self.write_dump(record);
    }
}

/// Render header pairs as a JSON object, redacting credential headers.
fn header_map(headers: &[(String, String)]) -> Value {
    let mut map = Map::new();
    for (name, value) in headers {
        let value = if is_sensitive(name) {
            "<redacted>".to_string()
        } else {
            value.clone()
        };
        map.insert(name.clone(), Value::String(value));
    }
    Value::Object(map)
}

fn is_sensitive(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "api-key"
            | "cookie"
            | "set-cookie"
            | "x-opencode-ticket"
    )
}

fn collect_headers(headers: &HMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or("<binary>").to_string(),
            )
        })
        .collect()
}

#[async_trait]
impl ProxyHttp for Proxy {
    type CTX = ProxyCtx;

    fn new_ctx(&self) -> Self::CTX {
        ProxyCtx::default()
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let upstream = self
            .lb
            .select(b"", 256)
            .ok_or_else(|| Error::explain(ErrorType::InternalError, "no upstream"))?;
        let peer = Box::new(HttpPeer::new(
            upstream,
            self.upstream.tls,
            self.upstream.host.clone(),
        ));
        Ok(peer)
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let req = session.req_header();
        ctx.method = req.method.to_string();
        ctx.uri = req.uri.to_string();
        ctx.req_headers = collect_headers(&req.headers);
        Ok(false)
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(chunk) = body {
            let remaining = self.max_body_bytes.saturating_sub(ctx.req_body.len());
            let take = chunk.len().min(remaining);
            ctx.req_body.extend_from_slice(&chunk[..take]);
            if take < chunk.len() {
                ctx.req_truncated = true;
            }
        }
        if end_of_stream {
            self.dump_request(ctx);
        }
        Ok(())
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        upstream_request
            .insert_header("Host", self.upstream.host_header())
            .unwrap();

        if let Some(header_name) = &self.auth_header {
            if let Some(value) = session.req_header().headers.get(header_name.as_str()) {
                upstream_request
                    .insert_header("Authorization", value.clone())
                    .unwrap();
                upstream_request
                    .remove_header(header_name.as_str())
                    .unwrap();
            }
        }
        Ok(())
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        ctx.status = Some(upstream_response.status.as_u16());
        ctx.resp_headers = collect_headers(&upstream_response.headers);
        Ok(())
    }

    fn response_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<Option<Duration>> {
        if let Some(chunk) = body {
            let remaining = self.max_body_bytes.saturating_sub(ctx.resp_body.len());
            let take = chunk.len().min(remaining);
            ctx.resp_body.extend_from_slice(&chunk[..take]);
            if take < chunk.len() {
                ctx.resp_truncated = true;
            }
        }
        if end_of_stream {
            self.dump_response(ctx);
        }
        Ok(None)
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let conf_path = args.get(1).map(String::as_str).unwrap_or("conf.toml");
    let raw = fs::read_to_string(conf_path)
        .unwrap_or_else(|e| panic!("failed to read config {conf_path}: {e}"));
    let conf: Conf = toml::from_str(&raw).unwrap_or_else(|e| panic!("invalid config: {e}"));

    let dump_file = match &conf.dump.file {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                let _ = fs::create_dir_all(parent);
            }
            Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap_or_else(|e| panic!("failed to open dump file {path}: {e}")),
            )
        }
        None => None,
    };

    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    let upstreams = LoadBalancer::try_from_iter([conf.upstream.addr()]).unwrap();

    let mut lb = http_proxy_service(
        &server.configuration,
        Proxy {
            lb: upstreams,
            auth_header: conf.upstream.auth_header.clone(),
            upstream: conf.upstream,
            dump_file: dump_file.map(Mutex::new),
            max_body_bytes: conf.dump.max_body_bytes,
        },
    );
    lb.add_tcp(&conf.server.listen);

    server.add_service(lb);
    server.run_forever();
}
