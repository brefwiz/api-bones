// SPDX-License-Identifier: MIT
//! Renders a refused call's typed error identity in the viewer's locale from
//! the owner's catalog.

mod args;
pub mod build;
mod catalog;
mod format;
mod locale;
mod localize;

pub use args::{
    ArgError, ArgRule, ArgValue, Args, MAX_ARGS, MAX_KEY_LEN, MAX_STRING_BYTES, error_args,
};
pub use catalog::register_catalog;
pub use format::{FormatError, Message, ParseError, ParseErrorKind};
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
