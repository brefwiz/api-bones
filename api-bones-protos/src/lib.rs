// SPDX-License-Identifier: MIT
//! Embedded bytes of the api-bones canonical proto shapes.
//!
//! This crate ships the `bones/v1/*.proto` files via `include_bytes!`
//! and exposes a single accessor — [`files`] — that yields
//! `(relative_path, bytes)` pairs ready for staging onto a protoc
//! include path at consumer build time.
//!
//! **Zero runtime dependencies.** This crate exists purely as a
//! distribution mechanism for the bytes. Pair it with any staging
//! library (e.g. [`proto-build-kit`](https://crates.io/crates/proto-build-kit))
//! to assemble a tempdir for codegen tools (connectrpc-build,
//! tonic-prost-build, ...).
//!
//! # Example
//!
//! ```no_run
//! # // We can't actually run this in a doctest because it depends on
//! # // proto-build-kit, but it shows the shape.
//! # mod proto_build_kit {
//! #     pub struct Stager;
//! #     impl Stager {
//! #         pub fn new() -> Self { Self }
//! #         pub fn with<I>(self, _: I) -> Self { self }
//! #         pub fn stage(self) -> Result<tempfile::TempDir, std::io::Error> {
//! #             tempfile::tempdir()
//! #         }
//! #     }
//! # }
//! let staged = proto_build_kit::Stager::new()
//!     .with(api_bones_protos::files())
//!     .stage()
//!     .expect("stage");
//! // Add staged.path() to your protoc include path.
//! ```
//!
//! # What's included
//!
//! The shapes match the proto/bones/v1/ directory in the api-bones
//! repo. See the README for the canonical schema inventory.

const PAGINATION_PROTO: &[u8] = include_bytes!("../proto/bones/v1/pagination.proto");
const QUERIES_PROTO: &[u8] = include_bytes!("../proto/bones/v1/queries.proto");
const ERRORS_PROTO: &[u8] = include_bytes!("../proto/bones/v1/errors.proto");
const RATELIMIT_PROTO: &[u8] = include_bytes!("../proto/bones/v1/ratelimit.proto");
const ANNOTATIONS_PROTO: &[u8] = include_bytes!("../proto/bones/v1/annotations.proto");

/// Yield `(relative_path, bytes)` pairs for every `bones/v1/*.proto`
/// file shipped by this crate.
///
/// `relative_path` is the protoc-style import path consumers use
/// (`bones/v1/pagination.proto`, etc.). The order of the iterator is
/// stable but unspecified — consumers should not rely on it.
pub fn files() -> impl Iterator<Item = (&'static str, &'static [u8])> {
    [
        ("bones/v1/pagination.proto", PAGINATION_PROTO),
        ("bones/v1/queries.proto", QUERIES_PROTO),
        ("bones/v1/errors.proto", ERRORS_PROTO),
        ("bones/v1/ratelimit.proto", RATELIMIT_PROTO),
        ("bones/v1/annotations.proto", ANNOTATIONS_PROTO),
    ]
    .into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ships_five_files() {
        let v: Vec<_> = files().collect();
        assert_eq!(v.len(), 5);
    }

    #[test]
    fn relative_paths_are_unique() {
        let mut paths: Vec<_> = files().map(|(p, _)| p).collect();
        let n = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), n, "duplicate relative paths in files()");
    }

    #[test]
    fn every_file_has_non_empty_bytes() {
        for (path, bytes) in files() {
            assert!(!bytes.is_empty(), "{path} embeds empty bytes");
        }
    }

    #[test]
    fn pagination_proto_declares_expected_messages() {
        let body = std::str::from_utf8(PAGINATION_PROTO).expect("utf8");
        for msg in [
            "message PageRequest",
            "message PageResponse",
            "message OffsetPageRequest",
            "message OffsetPageResponse",
        ] {
            assert!(body.contains(msg), "pagination.proto missing `{msg}`");
        }
    }

    #[test]
    fn annotations_proto_declares_authz_option() {
        let body = std::str::from_utf8(ANNOTATIONS_PROTO).expect("utf8");
        assert!(
            body.contains("AuthzRule authz = 5102348;"),
            "annotations.proto missing the authz method option"
        );
        for value in [
            "AUTHZ_KIND_UNSPECIFIED",
            "AUTHZ_KIND_AUTHENTICATED",
            "AUTHZ_KIND_PUBLIC",
        ] {
            assert!(body.contains(value), "annotations.proto missing `{value}`");
        }
    }

    #[test]
    fn annotations_proto_declares_service_capability_option() {
        let body = std::str::from_utf8(ANNOTATIONS_PROTO).expect("utf8");
        assert!(
            body.contains("extend google.protobuf.ServiceOptions"),
            "annotations.proto missing the service-options extension"
        );
        assert!(
            body.contains("string capability = 5102349;"),
            "annotations.proto missing the provider capability option"
        );
    }

    /// Authority is capability, not principal class. A service-vs-user
    /// axis cannot describe a delegated token (user subject, service
    /// actor, simultaneously), so a class gate on one can only be made
    /// permissive rather than correct. The old names may still appear
    /// in `reserved` declarations (so their numbers can never be
    /// reused), but must never be declared as live enum values again.
    #[test]
    fn annotations_proto_carries_no_principal_class_axis() {
        let body = std::str::from_utf8(ANNOTATIONS_PROTO).expect("utf8");
        for banned in ["AUTHZ_KIND_SERVICE", "AUTHZ_KIND_USER"] {
            for live_declaration in [format!("{banned} = "), format!("{banned}=")] {
                assert!(
                    !body.contains(&live_declaration),
                    "annotations.proto reintroduces the principal-class gate `{banned}` as a \
                     live enum value; authority belongs in AuthzRule.requires"
                );
            }
        }
    }

    /// The numbers `AUTHZ_KIND_SERVICE` (1) and `AUTHZ_KIND_USER` (2)
    /// previously occupied must be `reserved`, not recycled. A vendored
    /// copy of this proto that still declares the old principal-class
    /// enum encodes `kind` as one of these two numbers on the wire; if
    /// a live value were ever assigned to either number, that stale
    /// vendor would silently resolve to it instead of failing closed.
    #[test]
    fn annotations_proto_reserves_the_recycled_principal_class_numbers() {
        let body = std::str::from_utf8(ANNOTATIONS_PROTO).expect("utf8");
        assert!(
            body.contains("reserved 1, 2;"),
            "annotations.proto must reserve the recycled AuthzKind numbers 1 and 2"
        );
        assert!(
            body.contains(r#"reserved "AUTHZ_KIND_SERVICE", "AUTHZ_KIND_USER";"#),
            "annotations.proto must reserve the recycled AuthzKind names"
        );
    }

    #[test]
    fn annotations_proto_declares_the_requires_capability_field() {
        let body = std::str::from_utf8(ANNOTATIONS_PROTO).expect("utf8");
        assert!(
            body.contains("repeated string requires = 3;"),
            "annotations.proto missing the requires capability field"
        );
    }

    #[test]
    fn queries_proto_declares_filter_op_enum() {
        let body = std::str::from_utf8(QUERIES_PROTO).expect("utf8");
        assert!(
            body.contains("enum FilterOp"),
            "queries.proto missing FilterOp enum"
        );
        assert!(
            body.contains("FILTER_OP_EQ"),
            "queries.proto missing FILTER_OP_EQ"
        );
    }
}
