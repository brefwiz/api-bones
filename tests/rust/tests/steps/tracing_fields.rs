// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Reading what a tracing span or event records, shared by the steps that
//! capture telemetry.

use std::collections::BTreeMap;

/// The fields of one tracing span or event, by name.
pub type TelemetryFields = BTreeMap<String, serde_json::Value>;

/// Reads the fields a span or event records into [`TelemetryFields`].
pub struct Fields(pub TelemetryFields);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().into(), format!("{value:?}").into());
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().into(), value.into());
    }
}
