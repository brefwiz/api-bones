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

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
use core::fmt;
#[cfg(feature = "std")]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
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

impl fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
///
/// Requires `std` or `alloc` (fields contain `String`).
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
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

#[cfg(any(feature = "std", feature = "alloc"))]
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

    /// Return a typed builder for constructing a `HealthCheck`.
    ///
    /// Required fields (`component_type` and `status`) must be set before calling
    /// [`HealthCheckBuilder::build`]; the compiler enforces this via typestate.
    ///
    /// # Example
    /// ```rust
    /// use shared_types::health::{HealthCheck, HealthStatus};
    ///
    /// let check = HealthCheck::builder()
    ///     .component_type("datastore")
    ///     .status(HealthStatus::Pass)
    ///     .build();
    /// assert_eq!(check.status, HealthStatus::Pass);
    /// ```
    #[must_use]
    pub fn builder() -> HealthCheckBuilder<(), ()> {
        HealthCheckBuilder {
            component_type: (),
            status: (),
            output: None,
            time: None,
        }
    }
}

// ---------------------------------------------------------------------------
// HealthCheck builder — typestate
// ---------------------------------------------------------------------------

/// Typestate builder for [`HealthCheck`].
///
/// Type parameters track whether required fields have been set:
/// - `CT` — `String` once `.component_type()` is called, `()` otherwise
/// - `ST` — `HealthStatus` once `.status()` is called, `()` otherwise
///
/// [`HealthCheckBuilder::build`] is only available when both are set.
///
/// Requires `std` or `alloc`.
#[cfg(any(feature = "std", feature = "alloc"))]
pub struct HealthCheckBuilder<CT, ST> {
    component_type: CT,
    status: ST,
    output: Option<String>,
    time: Option<String>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<ST> HealthCheckBuilder<(), ST> {
    /// Set the component type, e.g. `"datastore"`, `"component"`, `"system"`.
    pub fn component_type(
        self,
        component_type: impl Into<String>,
    ) -> HealthCheckBuilder<String, ST> {
        HealthCheckBuilder {
            component_type: component_type.into(),
            status: self.status,
            output: self.output,
            time: self.time,
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<CT> HealthCheckBuilder<CT, ()> {
    /// Set the check result status.
    pub fn status(self, status: HealthStatus) -> HealthCheckBuilder<CT, HealthStatus> {
        HealthCheckBuilder {
            component_type: self.component_type,
            status,
            output: self.output,
            time: self.time,
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<CT, ST> HealthCheckBuilder<CT, ST> {
    /// Set a human-readable output or error message.
    #[must_use]
    pub fn output(mut self, output: impl Into<String>) -> Self {
        self.output = Some(output.into());
        self
    }

    /// Set an RFC 3339 timestamp of when this check was performed.
    #[must_use]
    pub fn time(mut self, time: impl Into<String>) -> Self {
        self.time = Some(time.into());
        self
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl HealthCheckBuilder<String, HealthStatus> {
    /// Build the [`HealthCheck`].
    ///
    /// Only available once both `component_type` and `status` have been set.
    #[must_use]
    pub fn build(self) -> HealthCheck {
        HealthCheck {
            component_type: self.component_type,
            status: self.status,
            output: self.output,
            time: self.time,
        }
    }
}

// ---------------------------------------------------------------------------
// LivenessResponse
// ---------------------------------------------------------------------------

/// Liveness probe response (`GET /health`) — RFC 8458.
///
/// Answers the question: "is this process alive?" No dependency checks.
/// Always returns HTTP `200 OK`.
///
/// Requires `std` or `alloc` (fields contain `String`).
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct LivenessResponse {
    /// Overall health status. Should always be [`HealthStatus::Pass`] for liveness.
    pub status: HealthStatus,
    /// Semantic version of the service, e.g. `"1.0.0"`.
    pub version: String,
    /// Unique identifier for this service instance, e.g. `"my-service"`.
    #[cfg_attr(feature = "serde", serde(rename = "serviceId"))]
    pub service_id: String,
}

#[cfg(any(feature = "std", feature = "alloc"))]
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
// ReadinessResponse (requires std for HashMap)
// ---------------------------------------------------------------------------

/// Readiness/startup probe response (`GET /health/ready`, `GET /health/startup`) — RFC 8458.
///
/// Answers the question: "is this service ready to handle traffic?"
/// Includes dependency checks (database, cache, upstream services).
///
/// HTTP status: `200 OK` on `pass`/`warn`, `503 Service Unavailable` on `fail`.
///
/// Requires the `std` feature (uses `HashMap` for dependency checks).
#[cfg(feature = "std")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
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

#[cfg(feature = "std")]
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

    /// Return a typed builder for constructing a `ReadinessResponse`.
    ///
    /// Required fields (`version` and `service_id`) must be set before calling
    /// [`ReadinessResponseBuilder::build`]; the compiler enforces this via typestate.
    /// Use [`ReadinessResponseBuilder::add_check`] to accumulate component checks.
    ///
    /// # Example
    /// ```rust
    /// use shared_types::health::{HealthCheck, ReadinessResponse};
    ///
    /// let resp = ReadinessResponse::builder()
    ///     .version("1.0.0")
    ///     .service_id("my-service")
    ///     .add_check("postgres:connection", HealthCheck::pass("datastore"))
    ///     .build();
    /// assert!(resp.checks.contains_key("postgres:connection"));
    /// ```
    #[must_use]
    pub fn builder() -> ReadinessResponseBuilder<(), ()> {
        ReadinessResponseBuilder {
            version: (),
            service_id: (),
            checks: HashMap::new(),
        }
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
// ReadinessResponse builder — typestate
// ---------------------------------------------------------------------------

/// Typestate builder for [`ReadinessResponse`].
///
/// Type parameters track whether required fields have been set:
/// - `V` — `String` once `.version()` is called, `()` otherwise
/// - `S` — `String` once `.service_id()` is called, `()` otherwise
///
/// [`ReadinessResponseBuilder::build`] is only available when both are set.
///
/// Requires the `std` feature (uses `HashMap` internally).
#[cfg(feature = "std")]
pub struct ReadinessResponseBuilder<V, S> {
    version: V,
    service_id: S,
    checks: HashMap<String, Vec<HealthCheck>>,
}

#[cfg(feature = "std")]
impl<S> ReadinessResponseBuilder<(), S> {
    /// Set the semantic version of the service, e.g. `"1.0.0"`.
    pub fn version(self, version: impl Into<String>) -> ReadinessResponseBuilder<String, S> {
        ReadinessResponseBuilder {
            version: version.into(),
            service_id: self.service_id,
            checks: self.checks,
        }
    }
}

#[cfg(feature = "std")]
impl<V> ReadinessResponseBuilder<V, ()> {
    /// Set the unique service instance identifier, e.g. `"my-service"`.
    pub fn service_id(self, service_id: impl Into<String>) -> ReadinessResponseBuilder<V, String> {
        ReadinessResponseBuilder {
            version: self.version,
            service_id: service_id.into(),
            checks: self.checks,
        }
    }
}

#[cfg(feature = "std")]
impl<V, S> ReadinessResponseBuilder<V, S> {
    /// Add a single component check under the given key.
    ///
    /// Key format: `"<component>:<measurement>"`, e.g. `"postgres:connection"`.
    /// Multiple checks under the same key are appended.
    #[must_use]
    pub fn add_check(mut self, key: impl Into<String>, check: HealthCheck) -> Self {
        self.checks.entry(key.into()).or_default().push(check);
        self
    }
}

#[cfg(feature = "std")]
impl ReadinessResponseBuilder<String, String> {
    /// Build the [`ReadinessResponse`], computing the aggregate status from all checks.
    ///
    /// Only available once both `version` and `service_id` have been set.
    #[must_use]
    pub fn build(self) -> ReadinessResponse {
        ReadinessResponse::new(self.version, self.service_id, self.checks)
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

    // -----------------------------------------------------------------------
    // HealthCheck builder tests
    // -----------------------------------------------------------------------

    #[test]
    fn health_check_builder_basic() {
        let check = HealthCheck::builder()
            .component_type("datastore")
            .status(HealthStatus::Pass)
            .build();
        assert_eq!(check.component_type, "datastore");
        assert_eq!(check.status, HealthStatus::Pass);
        assert!(check.output.is_none());
        assert!(check.time.is_none());
    }

    #[test]
    fn health_check_builder_equivalence_with_pass() {
        let via_factory = HealthCheck::pass("datastore");
        let via_builder = HealthCheck::builder()
            .component_type("datastore")
            .status(HealthStatus::Pass)
            .build();
        assert_eq!(via_factory.component_type, via_builder.component_type);
        assert_eq!(via_factory.status, via_builder.status);
        assert_eq!(via_factory.output, via_builder.output);
        assert_eq!(via_factory.time, via_builder.time);
    }

    #[test]
    fn health_check_builder_chaining_optionals() {
        let check = HealthCheck::builder()
            .component_type("system")
            .status(HealthStatus::Warn)
            .output("high latency")
            .time("2026-04-06T00:00:00Z")
            .build();
        assert_eq!(check.status, HealthStatus::Warn);
        assert_eq!(check.output.as_deref(), Some("high latency"));
        assert_eq!(check.time.as_deref(), Some("2026-04-06T00:00:00Z"));
    }

    #[test]
    fn health_check_builder_status_before_component_type() {
        // Typestate allows setting status before component_type
        let check = HealthCheck::builder()
            .status(HealthStatus::Fail)
            .component_type("component")
            .build();
        assert_eq!(check.status, HealthStatus::Fail);
        assert_eq!(check.component_type, "component");
    }

    // -----------------------------------------------------------------------
    // ReadinessResponse builder tests
    // -----------------------------------------------------------------------

    #[test]
    fn readiness_builder_empty_checks_is_pass() {
        let resp = ReadinessResponse::builder()
            .version("1.0.0")
            .service_id("my-service")
            .build();
        assert_eq!(resp.version, "1.0.0");
        assert_eq!(resp.service_id, "my-service");
        assert_eq!(resp.status, HealthStatus::Pass);
        assert!(resp.checks.is_empty());
    }

    #[test]
    fn readiness_builder_add_check() {
        let resp = ReadinessResponse::builder()
            .version("1.0.0")
            .service_id("svc")
            .add_check("postgres:connection", HealthCheck::pass("datastore"))
            .build();
        assert!(resp.checks.contains_key("postgres:connection"));
        assert_eq!(resp.status, HealthStatus::Pass);
    }

    #[test]
    fn readiness_builder_add_multiple_checks_same_key() {
        let resp = ReadinessResponse::builder()
            .version("1.0.0")
            .service_id("svc")
            .add_check("db:ping", HealthCheck::pass("datastore"))
            .add_check("db:ping", HealthCheck::warn("datastore", "slow"))
            .build();
        assert_eq!(resp.checks["db:ping"].len(), 2);
        assert_eq!(resp.status, HealthStatus::Warn);
    }

    #[test]
    fn readiness_builder_aggregate_fail() {
        let resp = ReadinessResponse::builder()
            .version("1.0.0")
            .service_id("svc")
            .add_check("redis:ping", HealthCheck::fail("datastore", "timeout"))
            .build();
        assert_eq!(resp.status, HealthStatus::Fail);
        assert_eq!(resp.http_status(), 503);
    }

    #[test]
    fn readiness_builder_equivalence_with_new() {
        let mut checks = HashMap::new();
        checks.insert(
            "postgres:connection".into(),
            vec![HealthCheck::pass("datastore")],
        );
        let via_new = ReadinessResponse::new("1.0.0", "svc", checks);
        let via_builder = ReadinessResponse::builder()
            .version("1.0.0")
            .service_id("svc")
            .add_check("postgres:connection", HealthCheck::pass("datastore"))
            .build();
        assert_eq!(via_new.status, via_builder.status);
        assert_eq!(via_new.version, via_builder.version);
        assert_eq!(via_new.service_id, via_builder.service_id);
        assert_eq!(via_new.checks.len(), via_builder.checks.len());
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn health_status_schema_is_valid() {
        let schema = schemars::schema_for!(HealthStatus);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object());
    }

    #[cfg(all(feature = "schemars", any(feature = "std", feature = "alloc")))]
    #[test]
    fn liveness_response_schema_is_valid() {
        let schema = schemars::schema_for!(LivenessResponse);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object());
    }
}
