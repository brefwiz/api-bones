// SPDX-License-Identifier: MIT
//! The spelling of an export, of the features that gate it, and of the
//! TypeScript method and JSON keys derived from a Rust name.

use std::fmt;

use heck::{ToLowerCamelCase, ToSnakeCase};

/// A part of an export name that cannot be spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameError {
    part: &'static str,
    value: String,
}

impl NameError {
    /// Which part of the name: `service`, `type` or `method`.
    #[must_use]
    pub fn part(&self) -> &'static str {
        self.part
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the {} `{}` contains `__`, which separates the parts of an export name",
            self.part, self.value
        )
    }
}

impl std::error::Error for NameError {}

fn checked(part: &'static str, value: String) -> Result<String, NameError> {
    if value.contains("__") {
        Err(NameError { part, value })
    } else {
        Ok(value)
    }
}

/// The wasm export of one method: `<svc>__<Type>__<method>`.
///
/// `svc` is the SPEC `service` with `-` replaced by `_`; `method` is the Rust
/// function name.
///
/// # Errors
///
/// A [`NameError`] when a part contains `__`.
pub fn export_name(service: &str, ty: &str, method: &str) -> Result<String, NameError> {
    let service = checked("service", service.replace('-', "_"))?;
    let ty = checked("type", ty.to_owned())?;
    let method = checked("method", method.to_owned())?;
    Ok(format!("{service}__{ty}__{method}"))
}

/// The three features that gate an export, in the order `sdk-all`,
/// `sdk-<surface>`, `sdk-m-<method>`.
#[must_use]
pub fn gate_features(surface: &str, method: &str) -> [String; 3] {
    [
        "sdk-all".to_owned(),
        format!("sdk-{surface}"),
        format!("sdk-m-{method}"),
    ]
}

/// The TypeScript method for a Rust method name: lower camel case.
#[must_use]
pub fn typescript_name(method: &str) -> String {
    method.to_lower_camel_case()
}

/// The key of a parameter or record field in the JSON a call carries: lower
/// camel case.
#[must_use]
pub fn json_name(field: &str) -> String {
    field.to_lower_camel_case()
}

/// The string an enumeration value crosses as: snake case.
#[must_use]
pub fn enum_value(variant: &str) -> String {
    variant.to_snake_case()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_joins_service_type_and_method() {
        assert_eq!(
            export_name("api-bones-test", "NumbersClient", "list").expect("ok"),
            "api_bones_test__NumbersClient__list"
        );
    }

    #[test]
    fn a_part_with_a_double_underscore_is_refused_and_named() {
        for (service, ty, method, part) in [
            ("a__b", "T", "m", "service"),
            ("a-_b", "T", "m", "service"),
            ("a", "T__x", "m", "type"),
            ("a", "T", "m__x", "method"),
        ] {
            let error = export_name(service, ty, method).expect_err("refused");
            assert_eq!(error.part(), part);
            assert!(error.to_string().contains("`__`"), "{error}");
        }
    }

    #[test]
    fn the_three_gate_features_are_named_in_order() {
        assert_eq!(
            gate_features("connect-pagination", "list"),
            ["sdk-all", "sdk-connect-pagination", "sdk-m-list"]
        );
    }

    #[test]
    fn rust_names_become_typescript_and_json_names() {
        assert_eq!(typescript_name("batch_get_widgets"), "batchGetWidgets");
        assert_eq!(typescript_name("list"), "list");
        assert_eq!(json_name("page_size"), "pageSize");
        assert_eq!(enum_value("CursorExpired"), "cursor_expired");
    }
}
