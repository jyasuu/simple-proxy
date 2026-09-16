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
```

| Key                 | Default             | Description                                            |
| ------------------- | ------------------- | ------------------------------------------------------ |
| `server.listen`     | `127.0.0.1:8080`    | Bind address for the proxy.                            |
| `upstream.host`     | (required)          | Upstream hostname or IP.                              |
| `upstream.port`     | `80`                | Upstream port.                                         |
| `upstream.tls`      | `false`             | Use TLS (`https`) to reach the upstream.               |
| `upstream.auth_header` | unset             | Client header whose value is relayed upstream as `Authorization`. |

No credentials are stored in the config. Authentication is delegated to the
client:

- If `auth_header` is set, its value is copied to the upstream `Authorization`
  header and the source header is stripped from the outgoing request.
- Otherwise the client's own `Authorization` header passes through unchanged.

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