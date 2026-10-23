# api-bones-protos

[![crates.io](https://img.shields.io/crates/v/api-bones-protos.svg)](https://crates.io/crates/api-bones-protos)
[![docs.rs](https://img.shields.io/docsrs/api-bones-protos)](https://docs.rs/api-bones-protos)

Embedded bytes of the canonical proto shapes shipped by the
[`api-bones`](https://crates.io/crates/api-bones) crate ecosystem
(`bones/v1/*.proto`) and `google/api/field_behavior.proto`, vendored byte-identical
from googleapis commit `525d4c82dc1d`. The vendored file is Apache-2.0 licensed by its
own header; the rest of the crate is MIT.

**Zero runtime dependencies.** This crate exists purely as a
distribution mechanism for the proto bytes. Pair it with a staging
library (e.g.
[`proto-build-kit`](https://crates.io/crates/proto-build-kit)) to
write the bytes to a tempdir at protoc-relative paths at consumer
build time:

```rust,no_run
use proto_build_kit::Stager;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let staged = Stager::new()
        .with(api_bones_protos::files())
        .stage()?;

    // Add staged.path() to your codegen include path:
    connectrpc_build::Config::new()
        .files(&["proto/my/v1/svc.proto"])
        .includes(&["proto/", staged.path()])
        .include_file("_connectrpc.rs")
        .compile()?;
    Ok(())
}
```

## What's included

| File                              | Messages |
|-----------------------------------|----------|
| `bones/v1/pagination.proto`       | `PageRequest`, `PageResponse`, `OffsetPageRequest`, `OffsetPageResponse` |
| `bones/v1/queries.proto`          | `SortDirection`, `SortField`, `SortParams`, `FilterOp`, `FilterEntry`, `FilterParams`, `SearchParams` |
| `bones/v1/errors.proto`           | `ValidationFailure` |
| `bones/v1/ratelimit.proto`        | `RateLimitInfo` |
| `bones/v1/annotations.proto`      | method options `errors`, `authz`, `mcp`, `call_shape`, `batch_of`, `automation_action`; message options `resource`, `automation_trigger`; field options `attr`, `classification`; service options `capability`, `required_capabilities`, `provided_capabilities`, `capability_labels`, `permission_labels`, `provided_broker_scopes`, `capability_scopes`, `attributes`; `AuthzKind`, `AuthzRule`, `RequiredCapability`, `Label`, `CapabilityLabel`, `PermissionLabel`, `BrokerScope`, `CapabilityScopes`, `AttributeDimension`, `McpProjection`, `ResourceOption`, `ResourceMethods`, `CallShape`, `AutomationTrigger`, `AutomationAction`, `EntityResolver`, `ActionEffect`, `IdempotencyMode`, `DataClassification` and their enums |
| `bones/v1/automation_manifest.proto` | service option `automation_templates`; `AutomationTemplate`, `AutomationDocument`, `TriggerRef`, `ActionRef`, `Step`, `ActionStep`, `InputBinding`, `Parameter`, `Value`, `Argument`, `AutomationManifest`, `ManifestTrigger`, `ManifestAction`, `SignedAutomationManifest` |

See `proto/README.md` in the api-bones repo for the canonical
schema inventory and the per-message Rust counterparts in the
`api-bones` crate.

## License

MIT.
