use async_trait::async_trait;
use pingora::http::RequestHeader;
use pingora::lb::selection::RoundRobin;
use pingora::lb::LoadBalancer;
use pingora::proxy::{http_proxy_service, ProxyHttp, Session};
use pingora::server::Server;
use pingora::upstreams::peer::HttpPeer;
use pingora::{Error, ErrorType, Result};
use serde::Deserialize;
use std::env;
use std::fs;

#[derive(Deserialize)]
struct Conf {
    #[serde(default)]
    server: ServerConf,
    upstream: UpstreamConf,
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

pub struct Proxy {
    lb: LoadBalancer<RoundRobin>,
    upstream: UpstreamConf,
    auth_header: Option<String>,
}

#[async_trait]
impl ProxyHttp for Proxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {
        ()
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
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let conf_path = args.get(1).map(String::as_str).unwrap_or("conf.toml");
    let raw = fs::read_to_string(conf_path)
        .unwrap_or_else(|e| panic!("failed to read config {conf_path}: {e}"));
    let conf: Conf = toml::from_str(&raw).unwrap_or_else(|e| panic!("invalid config: {e}"));

    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    let upstreams = LoadBalancer::try_from_iter([conf.upstream.addr()]).unwrap();

    let mut lb = http_proxy_service(
        &server.configuration,
        Proxy {
            lb: upstreams,
            auth_header: conf.upstream.auth_header.clone(),
            upstream: conf.upstream,
        },
    );
    lb.add_tcp(&conf.server.listen);

    server.add_service(lb);
    server.run_forever();
}