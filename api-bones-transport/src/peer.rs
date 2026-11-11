// SPDX-License-Identifier: MIT
//! How an event names the peer a request goes to.

use http::Uri;

/// The peer `uri` names, as `host:port` (`host` when the URI has no port,
/// empty when it has no authority). The authority's userinfo is never part of
/// it, so credentials embedded in a URI stay out of the logs. An IPv6 literal
/// keeps its brackets, as [`Uri::host`] returns it.
pub(crate) fn name(uri: &Uri) -> String {
    match (uri.host(), uri.port_u16()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_owned(),
        (None, _) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(uri: &str) -> String {
        name(&uri.parse().expect("valid test uri"))
    }

    #[test]
    fn userinfo_is_not_part_of_the_peer() {
        assert_eq!(
            named("https://user:pass@example.test:8443/x"),
            "example.test:8443"
        );
    }

    #[test]
    fn a_uri_without_a_port_names_the_host_alone() {
        assert_eq!(named("http://example.test/x"), "example.test");
    }

    #[test]
    fn an_ipv6_literal_keeps_its_brackets() {
        assert_eq!(named("https://[::1]:8443/"), "[::1]:8443");
        assert_eq!(named("https://u:p@[::1]/"), "[::1]");
    }

    #[test]
    fn a_uri_without_an_authority_names_no_peer() {
        assert_eq!(named("/svc/Method"), "");
    }
}
