# api-bones-transport

The one Connect client transport of the brefwiz fleet. Every Rust Connect
client is built from `HttpTransport`, a `connectrpc::client::ClientTransport`
with three constructors and no options:

| Constructor | For | Establishment bound |
|---|---|---|
| `HttpTransport::fleet(tls)` | mutual TLS to a fleet peer | `FLEET_CONNECT_BOUND` (250 ms) for TCP and for DNS + TCP + TLS |
| `HttpTransport::tls(tls)` | TLS to a public endpoint | connectrpc's defaults: 5 s TCP, 20 s overall |
| `HttpTransport::h2c()` | cleartext HTTP/2, loopback and contract tests | connectrpc's defaults |

```rust,no_run
use api_bones_transport::HttpTransport;
use connectrpc::client::ClientConfig;

let transport = HttpTransport::h2c();
let config = ClientConfig::new("http://127.0.0.1:8080".parse().unwrap());
```

## What every constructor does

- **HTTP/2 only, one connection per peer.** ALPN offers `h2` alone, so a burst
  of calls to one peer waits on one connect and multiplexes as streams.
- **A request that never ran is replayed once.** The connection refused it
  before opening a stream, a GOAWAY with `NO_ERROR` excluded its stream, or
  the peer refused the stream with `REFUSED_STREAM` (RFC 9113 section 8.7).
  The verdict is taken where the failure arose: a GOAWAY counts only when it
  reached the request's own stream. The replay goes to a fresh
  connection, inside the caller's deadline, with `connect-timeout-ms` or
  `grpc-timeout` rewritten to the time remaining. An error-coded GOAWAY, an
  I/O error, a reset of any other kind or a streaming request body is never
  replayed. The replay sits below the call-level retry policy and is not one
  of its attempts.
- **Connections run on one process runtime.** Started on first construction
  and never dropped, it runs connection tasks, the connector (DNS, TCP, TLS)
  and the HTTP/2 timers, so a connection outlives the runtime of the call
  that opened it.
- **Built on `h2::client`.** One stream is opened at a time per connection, so
  a request the connection never opened can move to a fresh one without being
  a replay. The response body is `ResponseBody`.
- **Fixed timings.** Pool idle timeout 90 s; HTTP/2 keepalive PING every 30 s,
  idle included, closing the connection after 10 s unanswered.

## Events

Under the `api_bones_transport` target: `connection_opened`,
`connection_closed`, `request_failed` (with the `never_ran` classification and
what the transport did) and `retry`.

## Helpers

- `never_ran(&error)`: whether an error proves the peer never ran the request.
- `failed_to_connect(&error)`: whether an error is a failure to establish the
  connection rather than one after the request was on the wire.

The crate depends on no api-bones major, so any consumer can use it.

License: MIT
