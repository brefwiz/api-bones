// SPDX-License-Identifier: MIT
//! The transport of a client: TLS to a public server against the bundled
//! roots, cleartext only to the loopback.

use api_bones::connect::client_tls_config;
use api_bones_sdk::Error;
use api_bones_transport::HttpTransport;
use http::Uri;

/// Whether the host of a cleartext URL is a loopback literal.
fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Checks `base_url` before anything else runs, so a refused URL never reaches
/// a credential read.
///
/// `https` is served to any host. `http` is served to a loopback host only:
/// a bearer must not leave the device in the clear. A URL carrying userinfo is
/// refused.
///
/// # Errors
/// `invalid_argument` when `base_url` is not a URI, names another scheme,
/// carries userinfo, or is `http` to a host that is not loopback.
pub fn check_base_url(base_url: &str) -> Result<Uri, Error> {
    let uri: Uri = base_url
        .parse()
        .map_err(|_| Error::invalid_argument("base_url is not a URI"))?;
    let authority = uri
        .authority()
        .ok_or_else(|| Error::invalid_argument("base_url names no host"))?;
    if authority.as_str().contains('@') {
        return Err(Error::invalid_argument("base_url must not carry userinfo"));
    }
    match uri.scheme_str() {
        Some("https") => Ok(uri),
        Some("http") if is_loopback(authority.host()) => Ok(uri),
        Some("http") => Err(Error::invalid_argument(
            "base_url must be https unless it names the loopback",
        )),
        _ => Err(Error::invalid_argument("base_url must be http or https")),
    }
}

/// The transport that serves `base_url`: TLS over the bundled roots for
/// `https`, prior-knowledge cleartext HTTP/2 for a loopback `http`. The URL is
/// checked first.
///
/// # Errors
/// As [`check_base_url`].
pub fn transport(base_url: &str) -> Result<HttpTransport, Error> {
    let uri = check_base_url(base_url)?;
    Ok(if uri.scheme_str() == Some("https") {
        HttpTransport::tls(client_tls_config())
    } else {
        HttpTransport::h2c()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(url: &str) -> String {
        check_base_url(url).expect_err(url).message
    }

    #[test]
    fn https_is_served_to_any_host() {
        assert!(check_base_url("https://api.example.com").is_ok());
        assert!(check_base_url("https://api.example.com:8443/base").is_ok());
    }

    #[test]
    fn cleartext_is_served_to_the_loopback_only() {
        for url in [
            "http://127.0.0.1:8080",
            "http://localhost:8080",
            "http://[::1]:8080",
        ] {
            assert!(check_base_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://api.example.com",
            "http://10.0.0.1:8080",
            "http://localhost.example.com",
        ] {
            assert!(refused(url).contains("loopback"), "{url}");
        }
    }

    #[test]
    fn userinfo_a_foreign_scheme_and_a_non_uri_are_refused() {
        assert!(refused("https://user:pw@api.example.com").contains("userinfo"));
        assert!(refused("ftp://api.example.com").contains("http or https"));
        assert!(refused("not a uri").contains("not a URI"));
        assert!(refused("/relative").contains("no host"));
    }

    #[test]
    fn the_transport_follows_the_scheme_and_a_refused_url_builds_none() {
        let _ = connectrpc::rustls::crypto::aws_lc_rs::default_provider().install_default();
        assert!(format!("{:?}", transport("https://a.example").expect("tls")).contains("\"tls\""));
        assert!(format!("{:?}", transport("http://127.0.0.1:1").expect("h2c")).contains("\"h2c\""));
        assert!(transport("http://a.example").is_err());
    }
}
