# Connect credential scoping and replay checks

Generated clients can keep one process credential while safely binding a
different credential to one call. Store process identity in `CallCredential`,
implement `ScopeCall` for the generated client, then borrow it through
`ScopedClient`:

```rust,ignore
use api_bones::connect::{CallCredential, ScopeCall};

struct ReservationsClient {
    auth: CallCredential,
    // generated transport fields
}

impl ScopeCall for ReservationsClient {}

let client = ReservationsClient {
    auth: CallCredential::from_token_injector(load_workload_token),
};
let scoped = client.with_credential(user_token);
let response = scoped
    .client()
    .reserve_with_options(request, scoped.call_options())
    .await?;
```

The handle borrows the existing client, so it reuses TLS state and connection
pools. Its credential cannot leak into a concurrent call.

Retry decisions need both method idempotency and transport evidence. The
helpers classify only transport shape:

```rust,ignore
use api_bones::connect::{
    is_connection_write_failure, is_replayable_transport_failure,
    is_unprompted_retryable,
};

if method_is_idempotent && is_replayable_transport_failure(&error) {
    retry().await?;
}

assert_eq!(
    is_connection_write_failure(&error),
    error.code == connectrpc::ErrorCode::Internal,
);
assert!(is_unprompted_retryable(connectrpc::ErrorCode::Unavailable));
```

The TypeScript transport adds one exemption to the idempotency gate. When the
Node transport's own connection facts prove a request never left the client (the
socket never connected -- refused, unresolved, timed out or unroutable -- the TLS
handshake had not completed, or a plaintext socket had written no byte; on
HTTP/2, no stream was opened, the peer refused the stream with `REFUSED_STREAM`,
or it sent a GOAWAY naming a last processed stream below this one), it
marks the failure as not delivered, and the retry interceptor replays it for any
method, declared idempotent or not: no server saw the call. The replay uses the
same bounded attempt count, backoff and throttle as every other retry. Every
failure that may have reached the server, such as a reset after the request was
sent, keeps the idempotency gate. Custom transports that can prove the same
mark a failure with `markNotDelivered`.

HTTP/2 sessions are kept alive: an idle session is pinged every 30 seconds and
one that does not answer within 15 seconds is dropped, so the next call opens a
fresh connection instead of writing onto one the peer already abandoned. This
mirrors the interval, timeout and while-idle behaviour of the Rust client.

Never use these predicates alone to replay a non-idempotent RPC. They answer
whether transport failure permits replay, not whether operation semantics do.

## Retrying a throttle and an unavailable peer

`fetch_with_retry(fetch, retries, deadline)` is the one Rust retry loop; the
page and batch crates run every fetch through it, so a generated client needs
no consumer code and no knob. It retries two kinds of failure on one attempt
count:

- A throttle, a `ResourceExhausted` that carries a `retry-after` (seconds or an
  HTTP-date, read from the response headers or the trailers). A limiter refuses
  before the handler runs, so the call is replayed whatever the method's
  idempotency, while the attempt count is below three. `retries_for` returning
  zero for an unspecified method does not hold a throttle back. A
  `ResourceExhausted` without a usable `retry-after` names no moment the server
  will be ready and is returned at once.
- An `Unavailable`, while the attempt count is below `retries`, which
  `retries_for` derives from the method's declared idempotency.

Because the count is shared, a throttle after blind retries has fewer repeats
left, and the other way round. A request the transport replays on its own
because it provably never ran is one `fetch` call and is not counted.

Each wait is the server's retry-after (none for `Unavailable`) plus
`RetryPolicy::next_delay_jittered`: a 50 ms exponential backoff with jitter,
never shorter than the backoff and at most twice it, slept with
`api_bones_rt::sleep`. Before sleeping:

1. A wait that reaches the time left until `deadline` ends the call as
   `DeadlineExceeded` without sleeping. This holds for every wait.
2. A retry-after longer than five seconds returns the `ResourceExhausted`
   unchanged, so the caller decides whether to wait that long.

`deadline` is an `api_bones_rt::Instant`. The batch crate derives it from the
call's timeout; the page crate passes none until the transport's per-call
deadline reaches it. Dropping the call's future during a wait abandons the
call, which a batch reports as cancelled.
