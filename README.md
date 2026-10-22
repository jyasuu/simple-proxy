# pingora reverse proxy

A minimal [Pingora](https://github.com/cloudflare/pingora) HTTP reverse proxy that
forwards all requests to a single configured upstream.

## Build

Requires Rust (tested with 1.94) and a C toolchain.

```sh
cargo build --release
```

Pingora is pinned to `0.8` because the `proxy` feature of `pingora 0.9`
references an unpublished crate.

The `openssl` feature is enabled so the proxy can reach `https://` upstreams.
It needs `libssl-dev` (`apt-get install libssl-dev`). Without it, TLS
upstreams fail with a `502`.

## Config

Run with a config path (defaults to `./conf.toml`):

```sh
./target/release/proxy [conf.toml]
```

Example `conf.toml`:

```toml
[server]
listen = "127.0.0.1:8080"

[upstream]
host = "origin.example.com"
port = 8080
tls = false
# Optional: forward the value of this client header to the upstream as
# the `Authorization` header. Leave unset to pass Authorization through as-is.
auth_header = "X-Upstream-Authorization"

[dump]
# Optional: append one JSONL record per request and per response.
file = "/tmp/proxy-dump.jsonl"
# Maximum body bytes captured per request and per response.
max_body_bytes = 1048576
```

| Key                 | Default             | Description                                            |
| ------------------- | ------------------- | ------------------------------------------------------ |
| `server.listen`     | `127.0.0.1:8080`    | Bind address for the proxy.                            |
| `upstream.host`     | (required)          | Upstream hostname or IP.                              |
| `upstream.port`     | `80`                | Upstream port.                                         |
| `upstream.tls`      | `false`             | Use TLS (`https`) to reach the upstream.               |
| `upstream.auth_header` | unset             | Client header whose value is relayed upstream as `Authorization`. |
| `dump.file`         | unset               | File to append JSONL request/response records to.      |
| `dump.max_body_bytes` | `1048576`          | Maximum body bytes captured per request/response.      |

The request path is forwarded unchanged, so a client asking for
`/zen/v1/chat/completions` against a proxy configured with
`host = "opencode.ai"` and `tls = true` reaches
`https://opencode.ai/zen/v1/chat/completions`.

No credentials are stored in the config. Authentication is delegated to the
client:

- If `auth_header` is set, its value is copied to the upstream `Authorization`
  header and the source header is stripped from the outgoing request.
- Otherwise the client's own `Authorization` header passes through unchanged.

## Dump

With `dump.file` set, the proxy appends one JSON object per line:

```jsonc
{
  "kind": "request",              // or "response"
  "ts": "2026-10-22T12:04:55.123Z",
  "method": "POST",               // request only
  "uri": "/zen/v1/chat/completions",
  "headers": { "content-type": "application/json" },
  "body": "{\"model\":\"big-pickle\"}",   // raw body
  "body_json": { "model": "big-pickle" }, // parsed body, null if not JSON
  "body_bytes": 2395,
  "body_truncated": false
}
```

Credential headers (`authorization`, `cookie`, `x-api-key`, ...) are replaced
with `<redacted>`. Bodies larger than `dump.max_body_bytes` are truncated and
flagged with `body_truncated`.

The dump contains full request bodies, which for an agent means the whole
prompt and conversation history. Treat the dump file as sensitive.

## Usage

```sh
curl http://127.0.0.1:8080/                              # no auth
curl -H 'Authorization: Basic <base64 user:pass>' http://127.0.0.1:8080/
curl -H 'X-Upstream-Authorization: Bearer <token>' http://127.0.0.1:8080/
```

## Lint / test

No automated tests. Formatting and linting (if desired):

```sh
cargo fmt
cargo clippy
```