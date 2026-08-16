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

Never use these predicates alone to replay a non-idempotent RPC. They answer
whether transport failure permits replay, not whether operation semantics do.
