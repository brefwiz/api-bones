//! Integration tests that call `Display::fmt` for `ApiVersion` types from an
//! external-crate context, covering the second instantiation that unit tests
//! cannot reach.

use api_bones::version::{ApiVersion, ApiVersionParseError, SemverTriple};
use core::fmt::Write;

#[test]
fn semver_triple_display_from_external_crate() {
    let t = SemverTriple(1, 2, 3);
    let s = t.to_string();
    assert_eq!(s, "1.2.3");
    let mut buf = String::new();
    write!(buf, "{t}").unwrap();
    assert_eq!(buf, "1.2.3");
    // Call through dyn Display to hit the vtable dispatch path.
    let d: &dyn core::fmt::Display = &t;
    assert_eq!(d.to_string(), "1.2.3");
}

#[test]
fn api_version_display_from_external_crate() {
    let v = ApiVersion::Simple(5);
    let s = v.to_string();
    assert_eq!(s, "v5");
    let mut buf = String::new();
    write!(buf, "{v}").unwrap();
    assert_eq!(buf, "v5");
    let d: &dyn core::fmt::Display = &v;
    assert_eq!(d.to_string(), "v5");
}

#[test]
fn api_version_parse_error_display_from_external_crate() {
    let e = ApiVersionParseError("bad".into());
    let s = e.to_string();
    assert!(s.contains("invalid API version"));
    let mut buf = String::new();
    write!(buf, "{e}").unwrap();
    assert!(buf.contains("bad"));
    let d: &dyn core::fmt::Display = &e;
    assert!(d.to_string().contains("bad"));
}
