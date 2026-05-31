// SPDX-License-Identifier: MIT
//! Effective client IP resolver — rightmost-untrusted XFF algorithm (RFC 7239 §5.2).
//!
//! # Algorithm
//!
//! 1. Read the peer socket address from the [`axum::extract::ConnectInfo<SocketAddr>`]
//!    request extension. If absent (e.g. test transports without `ConnectInfo`),
//!    return `None`.
//! 2. If the peer IP is **not** inside any `trusted_cidrs`, return it immediately —
//!    XFF is ignored to prevent spoofing.
//! 3. If the peer IP **is** trusted, walk `X-Forwarded-For` right → left, skipping
//!    every IP that is also trusted; return the first untrusted IP found.
//! 4. Fall back to the peer IP when XFF is absent, all-trusted, or malformed.
//!
//! Pass an empty `trusted_cidrs` slice to disable XFF inspection entirely (safe
//! default — equivalent to returning the bare peer IP).
//!
//! # Example
//!
//! ```rust
//! use api_bones::net::client_ip;
//! use ipnet::IpNet;
//! use std::net::{IpAddr, Ipv4Addr, SocketAddr};
//!
//! let trusted: Vec<IpNet> = vec!["127.0.0.1/8".parse().unwrap()];
//!
//! // Build a minimal request with ConnectInfo + XFF.
//! let peer: SocketAddr = "127.0.0.1:4000".parse().unwrap();
//! let mut req = http::Request::new(());
//! req.extensions_mut()
//!     .insert(axum::extract::ConnectInfo(peer));
//! req.headers_mut().insert(
//!     "x-forwarded-for",
//!     "203.0.113.1".parse().unwrap(),
//! );
//!
//! let ip = client_ip::resolve(&req, &trusted);
//! assert_eq!(ip, Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1))));
//! ```

use axum::extract::ConnectInfo;
use ipnet::IpNet;
use std::net::{IpAddr, SocketAddr};

/// Resolve the effective client IP for `req` using the rightmost-untrusted
/// XFF algorithm (RFC 7239 §5.2).
///
/// See the [module documentation](self) for the full algorithm description.
#[must_use]
pub fn resolve<B>(req: &http::Request<B>, trusted_cidrs: &[IpNet]) -> Option<IpAddr> {
    let peer_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0)?;

    let peer_ip = peer_addr.ip();

    if !is_trusted(peer_ip, trusted_cidrs) {
        return Some(peer_ip);
    }

    // Peer is trusted — inspect XFF.
    let xff_value = req.headers().get("x-forwarded-for")?;
    let xff_str = std::str::from_utf8(xff_value.as_bytes()).ok()?;

    // Walk right → left; first untrusted IP wins.
    for part in xff_str.rsplit(',') {
        let trimmed = part.trim();
        if let Ok(ip) = trimmed.parse::<IpAddr>() {
            if !is_trusted(ip, trusted_cidrs) {
                return Some(ip);
            }
        }
        // Malformed token — skip (treat as trusted to be conservative, keep walking).
    }

    // All XFF entries were trusted (or XFF was empty / all-malformed) → fall back to peer.
    Some(peer_ip)
}

fn is_trusted(ip: IpAddr, cidrs: &[IpNet]) -> bool {
    cidrs.iter().any(|net| net.contains(&ip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn make_req(peer: &str, xff: Option<&str>) -> http::Request<()> {
        let peer_addr: SocketAddr = peer.parse().unwrap();
        let mut req = http::Request::new(());
        req.extensions_mut()
            .insert(ConnectInfo::<SocketAddr>(peer_addr));
        if let Some(xff_val) = xff {
            req.headers_mut()
                .insert("x-forwarded-for", xff_val.parse().unwrap());
        }
        req
    }

    fn trusted() -> Vec<IpNet> {
        vec!["10.0.0.0/8".parse().unwrap(), "192.168.0.0/16".parse().unwrap()]
    }

    /// ConnectInfo absent → None.
    #[test]
    fn no_connect_info_returns_none() {
        let req = http::Request::new(());
        assert_eq!(resolve(&req, &trusted()), None);
    }

    /// Peer not in trusted CIDR → peer IP returned (XFF ignored).
    #[test]
    fn peer_not_trusted_returns_peer() {
        let req = make_req("203.0.113.5:1234", Some("1.2.3.4"));
        let ip = resolve(&req, &trusted()).unwrap();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)));
    }

    /// Peer trusted + XFF has untrusted entry → rightmost untrusted wins.
    #[test]
    fn peer_trusted_xff_rightmost_untrusted() {
        // XFF: "1.2.3.4, 10.0.0.2" — rightmost is trusted, next is untrusted.
        let req = make_req("192.168.1.1:4000", Some("203.0.113.1, 10.0.0.2"));
        let ip = resolve(&req, &trusted()).unwrap();
        // Walking right → left: 10.0.0.2 is trusted (skip), 203.0.113.1 is untrusted (win).
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)));
    }

    /// Peer trusted + XFF all trusted → fallback to peer.
    #[test]
    fn peer_trusted_xff_all_trusted_returns_peer() {
        let req = make_req("10.0.0.1:80", Some("10.0.0.2, 10.0.0.3"));
        let ip = resolve(&req, &trusted()).unwrap();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::from([10, 0, 0, 1])));
    }

    /// Malformed XFF (non-IP tokens) → fallback to peer.
    #[test]
    fn malformed_xff_returns_peer() {
        let req = make_req("192.168.1.5:9000", Some("not-an-ip, also-bad"));
        let ip = resolve(&req, &trusted()).unwrap();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)));
    }
}
