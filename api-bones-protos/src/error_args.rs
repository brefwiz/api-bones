// SPDX-License-Identifier: MIT
//! The typed arguments an error code declares.
//!
//! An error code is a value of the emitter's error enum, named by
//! `bones.v1.errors`. That value carries `bones.v1.error_args`, one
//! `bones.v1.ErrorArgDeclaration` per argument its message takes.
//! [`declared_error_args`] reads them back from a compiled descriptor, so a
//! build or a check holds the code and its arguments as one declaration.

use std::fmt;

use prost_reflect::{EnumValueDescriptor, Value};

const ERROR_ARGS: &str = "bones.v1.error_args";

/// `bones.v1.ErrorArgKind`, without its unspecified zero value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorArgKind {
    /// `ERROR_ARG_KIND_STRING`.
    String,
    /// `ERROR_ARG_KIND_INT64`.
    Int64,
    /// `ERROR_ARG_KIND_DOUBLE`.
    Double,
    /// `ERROR_ARG_KIND_TIMESTAMP`.
    Timestamp,
}

impl ErrorArgKind {
    fn from_number(number: i32) -> Option<Self> {
        match number {
            1 => Some(Self::String),
            2 => Some(Self::Int64),
            3 => Some(Self::Double),
            4 => Some(Self::Timestamp),
            _ => None,
        }
    }
}

/// One declared argument: the placeholder name and the kind of value supplied.
///
/// The name is read as written. Whether it follows the argument-name grammar
/// is checked where the declaration meets a catalog string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorArgDecl {
    /// The placeholder name.
    pub name: String,
    /// The kind of value the emitter supplies.
    pub kind: ErrorArgKind,
}

/// A declaration that names no usable kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationError {
    /// The enum value carrying the declaration, as `<enum full name>.<value name>`.
    pub value: String,
    /// The declared argument name.
    pub name: String,
    /// The `ErrorArgKind` number found: zero is `ERROR_ARG_KIND_UNSPECIFIED`.
    pub kind: i32,
}

impl fmt::Display for DeclarationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: argument `{}` has no valid kind (ErrorArgKind {})",
            self.value, self.name, self.kind
        )
    }
}

impl core::error::Error for DeclarationError {}

/// The arguments `value` declares with `bones.v1.error_args`, in declaration
/// order. A value that carries none, or a pool that does not import
/// `bones/v1/annotations.proto`, declares none.
///
/// # Errors
///
/// [`DeclarationError`] for the first declaration whose kind is unspecified or
/// not an `ErrorArgKind` value.
pub fn declared_error_args(
    value: &EnumValueDescriptor,
) -> Result<Vec<ErrorArgDecl>, DeclarationError> {
    let Some(extension) = value.parent_pool().get_extension_by_name(ERROR_ARGS) else {
        return Ok(Vec::new());
    };
    let options = value.options();
    if !options.has_extension(&extension) {
        return Ok(Vec::new());
    }
    let Value::List(entries) = options.get_extension(&extension).into_owned() else {
        return Ok(Vec::new());
    };
    entries
        .iter()
        .filter_map(Value::as_message)
        .map(|message| {
            let name = message
                .get_field_by_name("name")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let number = message
                .get_field_by_name("kind")
                .and_then(|v| v.as_enum_number())
                .unwrap_or_default();
            ErrorArgKind::from_number(number)
                .map(|kind| ErrorArgDecl {
                    name: name.clone(),
                    kind,
                })
                .ok_or_else(|| DeclarationError {
                    value: format!("{}.{}", value.parent_enum().full_name(), value.name()),
                    name,
                    kind: number,
                })
        })
        .collect()
}
