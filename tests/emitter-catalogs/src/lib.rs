// SPDX-License-Identifier: MIT
//! The embedded `(emitter, locale, json)` table of the fixture's catalogs.

/// Every catalog under both emitters, as the build helper wrote it.
pub const ERROR_CATALOGS: &[(&str, &str, &str)] = api_bones_i18n::emitter_error_catalogs!();
