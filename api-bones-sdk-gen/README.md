# api-bones-sdk-gen

Native client declarations live in canonical protobuf descriptors through
`bones.v1.native_services`. They contain no RPC methods, so ordinary Connect
generation sees no native endpoint or handler.

Use `protoc-gen-bones-native` as a normal local buf plugin:

```yaml
plugins:
  - local: protoc-gen-bones-native
    out: sdk/rust/src/generated
    opt: target=rust
    strategy: all
  - local: protoc-gen-bones-native
    out: sdk/typescript/src/generated
    opt: target=typescript
    strategy: all
```

`strategy: all` is required. Plugin emits one deterministic `bones_native` file
per target for complete invocation, so buf must send selected files together.
This avoids same-name output collisions when native declarations span source
directories. Inputs containing no native declarations emit no files and remain
benign in mixed RPC generation.

Generated clients delegate to a provider-owned `NativeTransport`. Application
code supplies intent, stable message identity, entity identity, and opaque
bytes. Provider composition owns addressing, workload identity, authorization,
and serialization on its native wire.

`CallOptions` performs pre-dispatch cancellation and deadline checks. Transport
implementations must honor same signal/deadline for full in-flight operation.
Acknowledgement handles allow one active attempt. Failed attempts return handle
to retryable state; successful acknowledgement or negative acknowledgement
consumes it permanently.
