// SPDX-License-Identifier: MIT
//! Renders a refused call's typed error identity in the viewer's locale from
//! the owner's catalog.

mod args;
pub mod build;
mod catalog;
mod check;
#[cfg(feature = "descriptor")]
pub mod emitters;
mod format;
mod locale;
mod localize;
#[cfg(test)]
mod test_support;

pub use args::{
    ArgError, ArgRule, ArgValue, Args, MAX_ARGS, MAX_KEY_LEN, MAX_STRING_BYTES, error_args,
};
pub use catalog::register_catalog;
pub use check::{ArgDeclaration, ArgKind, Issue, Problem, check_catalog, check_error_catalogs};
pub use format::{Expected, FormatError, Message, ParseError, ParseErrorKind};
pub use locale::{ALIASES, DEFAULT_LOCALE, PLATFORM_LOCALES, match_locale, negotiate_locale};
pub use localize::{
    ErrorIdentity, Localizable, LocalizeError, LocalizeOptions, Localized, PLATFORM_CODES,
    Violation, localize,
};

#[doc(hidden)]
pub mod __private {
    pub use crate::catalog::Registration;
    pub use inventory;
}

/// Embed the catalogs [`build::emit_error_catalogs`] found and register them
/// for `$emitter`: call it once in the emitter's library.
///
/// ```ignore
/// api_bones_i18n::error_catalogs!("payments");
/// ```
#[macro_export]
macro_rules! error_catalogs {
    ($emitter:literal) => {
        $crate::__private::inventory::submit! {
            $crate::__private::Registration {
                emitter: $emitter,
                catalogs: include!(concat!(env!("OUT_DIR"), "/error_catalogs.rs")),
            }
        }
    };
}

/// The `(emitter, locale, json)` table `emitters::embed_and_check_error_catalogs`
/// (feature `descriptor`) wrote, as an expression of type `&[(&str, &str, &str)]`:
///
/// ```ignore
/// pub const ERROR_CATALOGS: &[(&str, &str, &str)] =
///     api_bones_i18n::emitter_error_catalogs!();
/// ```
#[macro_export]
macro_rules! emitter_error_catalogs {
    () => {
        include!(concat!(
            env!("OUT_DIR"),
            "/",
            $crate::__emitter_table_file!()
        ))
    };
}

/// The file under `OUT_DIR` that holds the table: written by
/// `emitters::embed_and_check_error_catalogs`, read by
/// [`emitter_error_catalogs!`]. A macro, so `concat!` can take it.
#[doc(hidden)]
#[macro_export]
macro_rules! __emitter_table_file {
    () => {
        "emitter_error_catalogs.rs"
    };
}
