//! RFC 8458 health check response types.
//!
//! Implements the IETF Health Check Response Format for HTTP APIs
//! ([draft-inadarei-api-health-check](https://datatracker.ietf.org/doc/html/draft-inadarei-api-health-check)).
//!
//! ## Wire format
//!
//! Content-Type: `application/health+json`
//!
//! ### Liveness (`GET /health`)
//! ```json
//! {"status": "pass", "version": "1.0.0", "serviceId": "my-service"}
//! ```
//!
//! ### Readiness (`GET /health/ready`)
//! ```json
//! {
//!   "status": "pass",
//!   "version": "1.0.0",
//!   "serviceId": "my-service",
//!   "checks": {
//!     "postgres:connection": [{"componentType": "datastore", "status": "pass"}]
//!   }
//! }
//! ```
//!
//! ## HTTP status codes
//!
//! - `LivenessResponse` → always `200 OK`
//! - `ReadinessResponse` → `200 OK` on `pass`/`warn`, `503 Service Unavailable` on `fail`

use std::collections::HashMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// HealthStatus
// ---------------------------------------------------------------------------

/// RFC 8458 §3 health check status.
///
/// Serializes as lowercase `"pass"`, `"fail"`, or `"warn"`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum HealthStatus {
    /// The service is healthy and all checks pass.
    Pass,
    /// The service is unhealthy; callers should not route traffic here.
    Fail,
    /// The service is degraded but still operational.
    Warn,
}

impl HealthStatus {
    /// HTTP status code for a response carrying this health status.
    ///
    /// - `Pass` / `Warn` → `200 OK`
    /// - `Fail` → `503 Service Unavailable`
    #[must_use]
    pub const fn http_status(&self) -> u16 {
        match self {
            Self::Pass | Self::Warn => 200,
            Self::Fail => 503,
        }
    }

    /// Returns `true` if the status indicates healthy or degraded-but-operational.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Pass | Self::Warn)
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => f.write_str("pass"),
            Self::Fail => f.write_str("fail"),
            Self::Warn => f.write_str("warn"),
        }
    }
}

// ---------------------------------------------------------------------------
// HealthCheck
// ---------------------------------------------------------------------------

/// Individual component check result (RFC 8458 §4).
///
/// Used as values in [`ReadinessResponse::checks`].
/// Map key format: `"<component>:<measurement>"`, e.g. `"postgres:connection"`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct HealthCheck {
    /// Component type, e.g. `"datastore"`, `"component"`, `"system"`.
    #[cfg_attr(feature = "serde", serde(rename = "componentType"))]
    pub component_type: String,
    /// Check result status.
    pub status: HealthStatus,
    /// Human-readable output or error message. Omitted when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub output: Option<String>,
    /// RFC 3339 timestamp of when this check was last performed. Omitted when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub time: Option<String>,
}

impl HealthCheck {
    /// Create a passing component check.
    pub fn pass(component_type: impl Into<String>) -> Self {
        Self {
            component_type: component_type.into(),
            status: HealthStatus::Pass,
            output: None,
            time: None,
        }
    }

    /// Create a failing component check with an error message.
    pub fn fail(component_type: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            component_type: component_type.into(),
            status: HealthStatus::Fail,
            output: Some(output.into()),
            time: None,
        }
    }

    /// Create a warn-level component check with a message.
    pub fn warn(component_type: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            component_type: component_type.into(),
            status: HealthStatus::Warn,
            output: Some(output.into()),
            time: None,
        }
    }

    /// Attach an RFC 3339 timestamp to this check result.
    #[must_use]
    pub fn with_time(mut self, time: impl Into<String>) -> Self {
        self.time = Some(time.into());
        self
    }
}

// ---------------------------------------------------------------------------
// LivenessResponse
// ---------------------------------------------------------------------------

/// Liveness probe response (`GET /health`) — RFC 8458.
///
/// Answers the question: "is this process alive?" No dependency checks.
/// Always returns HTTP `200 OK`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct LivenessResponse {
    /// Overall health status. Should always be [`HealthStatus::Pass`] for liveness.
    pub status: HealthStatus,
    /// Semantic version of the service, e.g. `"1.0.0"`.
    pub version: String,
    /// Unique identifier for this service instance, e.g. `"my-service"`.
    #[cfg_attr(feature = "serde", serde(rename = "serviceId"))]
    pub service_id: String,
}

impl LivenessResponse {
    /// Create a passing liveness response.
    pub fn pass(version: impl Into<String>, service_id: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Pass,
            version: version.into(),
            service_id: service_id.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// ReadinessResponse
// ---------------------------------------------------------------------------

/// Readiness/startup probe response (`GET /health/ready`, `GET /health/startup`) — RFC 8458.
///
/// Answers the question: "is this service ready to handle traffic?"
/// Includes dependency checks (database, cache, upstream services).
///
/// HTTP status: `200 OK` on `pass`/`warn`, `503 Service Unavailable` on `fail`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ReadinessResponse {
    /// Overall health status, derived from the worst check result.
    pub status: HealthStatus,
    /// Semantic version of the service, e.g. `"1.0.0"`.
    pub version: String,
    /// Unique identifier for this service instance, e.g. `"my-service"`.
    #[cfg_attr(feature = "serde", serde(rename = "serviceId"))]
    pub service_id: String,
    /// Component check results. Key format: `"<component>:<measurement>"`.
    /// Example: `"postgres:connection"`, `"redis:latency"`.
    pub checks: HashMap<String, Vec<HealthCheck>>,
}

impl ReadinessResponse {
    /// Create a new readiness response, computing overall status from checks.
    ///
    /// Status is the worst of all check statuses: `fail` > `warn` > `pass`.
    pub fn new(
        version: impl Into<String>,
        service_id: impl Into<String>,
        checks: HashMap<String, Vec<HealthCheck>>,
    ) -> Self {
        let status = Self::aggregate_status(&checks);
        Self {
            status,
            version: version.into(),
            service_id: service_id.into(),
            checks,
        }
    }

    /// HTTP status code for this response.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        self.status.http_status()
    }

    /// Compute aggregate status from all checks: worst of pass/warn/fail wins.
    fn aggregate_status(checks: &HashMap<String, Vec<HealthCheck>>) -> HealthStatus {
        let mut has_warn = false;
        for check_list in checks.values() {
            for check in check_list {
                if check.status == HealthStatus::Fail {
                    return HealthStatus::Fail;
                }
                if check.status == HealthStatus::Warn {
                    has_warn = true;
                }
            }
        }
        if has_warn {
            HealthStatus::Warn
        } else {
            HealthStatus::Pass
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_http_codes() {
        assert_eq!(HealthStatus::Pass.http_status(), 200);
        assert_eq!(HealthStatus::Warn.http_status(), 200);
        assert_eq!(HealthStatus::Fail.http_status(), 503);
    }

    #[test]
    fn health_status_is_available() {
        assert!(HealthStatus::Pass.is_available());
        assert!(HealthStatus::Warn.is_available());
        assert!(!HealthStatus::Fail.is_available());
    }

    #[test]
    fn health_status_display() {
        assert_eq!(HealthStatus::Pass.to_string(), "pass");
        assert_eq!(HealthStatus::Fail.to_string(), "fail");
        assert_eq!(HealthStatus::Warn.to_string(), "warn");
    }

    #[test]
    fn readiness_aggregate_pass() {
        let mut checks = HashMap::new();
        checks.insert(
            "postgres:connection".into(),
            vec![HealthCheck::pass("datastore")],
        );
        let r = ReadinessResponse::new("1.0.0", "svc", checks);
        assert_eq!(r.status, HealthStatus::Pass);
        assert_eq!(r.http_status(), 200);
    }

    #[test]
    fn readiness_aggregate_fail_wins() {
        let mut checks = HashMap::new();
        checks.insert(
            "postgres:connection".into(),
            vec![HealthCheck::pass("datastore")],
        );
        checks.insert(
            "redis:ping".into(),
            vec![HealthCheck::fail("datastore", "timeout")],
        );
        let r = ReadinessResponse::new("1.0.0", "svc", checks);
        assert_eq!(r.status, HealthStatus::Fail);
        assert_eq!(r.http_status(), 503);
    }

    #[test]
    fn readiness_aggregate_warn() {
        let mut checks = HashMap::new();
        checks.insert(
            "postgres:connection".into(),
            vec![HealthCheck::pass("datastore")],
        );
        checks.insert(
            "redis:latency".into(),
            vec![HealthCheck::warn("datastore", "slow")],
        );
        let r = ReadinessResponse::new("1.0.0", "svc", checks);
        assert_eq!(r.status, HealthStatus::Warn);
        assert_eq!(r.http_status(), 200);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn health_status_serializes_lowercase() {
        assert_eq!(serde_json::to_value(HealthStatus::Pass).unwrap(), "pass");
        assert_eq!(serde_json::to_value(HealthStatus::Fail).unwrap(), "fail");
        assert_eq!(serde_json::to_value(HealthStatus::Warn).unwrap(), "warn");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn liveness_wire_format() {
        let r = LivenessResponse::pass("1.0.0", "my-service");
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["status"], "pass");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["serviceId"], "my-service");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn readiness_wire_format() {
        let mut checks = HashMap::new();
        checks.insert(
            "postgres:connection".into(),
            vec![HealthCheck::pass("datastore")],
        );
        let r = ReadinessResponse::new("1.0.0", "my-service", checks);
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["status"], "pass");
        assert_eq!(json["serviceId"], "my-service");
        assert!(json["checks"]["postgres:connection"].is_array());
        assert_eq!(
            json["checks"]["postgres:connection"][0]["componentType"],
            "datastore"
        );
        assert_eq!(json["checks"]["postgres:connection"][0]["status"], "pass");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn health_check_omits_optional_fields() {
        let c = HealthCheck::pass("datastore");
        let json = serde_json::to_value(&c).unwrap();
        assert!(json.get("output").is_none());
        assert!(json.get("time").is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn health_check_with_time() {
        let c = HealthCheck::pass("datastore").with_time("2026-03-09T21:00:00Z");
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["time"], "2026-03-09T21:00:00Z");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_liveness() {
        let r = LivenessResponse::pass("1.0.0", "my-service");
        let json = serde_json::to_value(&r).unwrap();
        let back: LivenessResponse = serde_json::from_value(json).unwrap();
        assert_eq!(back.status, HealthStatus::Pass);
        assert_eq!(back.version, "1.0.0");
        assert_eq!(back.service_id, "my-service");
    }
}
