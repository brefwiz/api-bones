// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
use api_bones_fields::{DefinitionChange, FieldDefinition, FieldViolation};
#[derive(Default, cucumber::World)]
pub struct FieldsWorld {
    pub definition: FieldDefinition,
    pub violations: Option<Vec<FieldViolation>>,
    pub change: Option<DefinitionChange>,
}

impl std::fmt::Debug for FieldsWorld {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FieldsWorld")
            .field("violations", &self.violations)
            .field("change", &self.change)
            .finish_non_exhaustive()
    }
}
