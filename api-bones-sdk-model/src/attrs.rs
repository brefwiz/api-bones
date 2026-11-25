// SPDX-License-Identifier: MIT
//! The arguments of the SDK attributes, read from the attribute's tokens.
//!
//! A macro receives the arguments as a token stream; the generator finds the
//! same attributes on parsed source and reads the tokens of the attribute. Both
//! reach the same `parse` here, so an attribute means one thing.

use proc_macro2::TokenStream;
use syn::meta::ParseNestedMeta;
use syn::parse::Parser;
use syn::{Attribute, LitStr, Meta};

/// The arguments of `#[client(surface = "...")]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientArgs {
    /// The `sdk_surfaces` id every export of the impl belongs to.
    pub surface: String,
}

/// The arguments of `#[export]`, `#[export(sync)]` and
/// `#[export(surface = "...")]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportArgs {
    /// The export is synchronous: no endpoint, no handle.
    pub sync: bool,
    /// Overrides the surface of the `#[client]` impl for this method.
    pub surface: Option<String>,
}

/// The arguments of `#[error(emitter = "...")]` on an error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorArgs {
    /// The `emitter` field of the error JSON.
    pub emitter: String,
}

/// The arguments of `#[error(code = "...", platform = "...")]` on a variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorVariantArgs {
    /// The `code` field of the error JSON.
    pub code: String,
    /// The `platform` field of the error JSON, an `ErrorCode` name.
    pub platform: String,
}

/// The attribute of the given name, spelled bare or behind a path such as
/// `api_bones_sdk::client`.
#[must_use]
pub fn find<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a Attribute> {
    attrs.iter().find(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == name)
    })
}

/// The argument tokens of an attribute: empty for a bare path.
///
/// # Errors
///
/// A [`syn::Error`] when the attribute is written `#[name = value]`.
pub fn tokens(attr: &Attribute) -> syn::Result<TokenStream> {
    match &attr.meta {
        Meta::Path(_) => Ok(TokenStream::new()),
        Meta::List(list) => Ok(list.tokens.clone()),
        Meta::NameValue(_) => Err(syn::Error::new_spanned(
            attr,
            "write the arguments in parentheses, not after `=`",
        )),
    }
}

/// Refuses any argument: for `#[record]` and `#[enumeration]`.
///
/// # Errors
///
/// A [`syn::Error`] naming the attribute when it carries arguments.
pub fn no_args(name: &str, args: TokenStream) -> syn::Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            args,
            format!("`#[{name}]` takes no arguments"),
        ))
    }
}

/// Stores the string value of `meta` into `slot`, refusing a repeat.
fn string_arg(meta: &ParseNestedMeta<'_>, slot: &mut Option<String>) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error("this argument is given twice"));
    }
    *slot = Some(meta.value()?.parse::<LitStr>()?.value());
    Ok(())
}

fn require(slot: Option<String>, attr: &str, arg: &str) -> syn::Result<String> {
    slot.ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("`#[{attr}]` needs `{arg} = \"...\"`"),
        )
    })
}

impl ClientArgs {
    /// Reads `surface = "..."`.
    ///
    /// # Errors
    ///
    /// A [`syn::Error`] for an unknown, repeated or missing argument.
    pub fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut surface = None;
        syn::meta::parser(|meta| {
            if meta.path.is_ident("surface") {
                string_arg(&meta, &mut surface)
            } else {
                Err(meta.error("unknown argument, expected `surface = \"...\"`"))
            }
        })
        .parse2(args)?;
        Ok(Self {
            surface: require(surface, "client", "surface")?,
        })
    }
}

impl ExportArgs {
    /// Reads `sync` and `surface = "..."`, both optional.
    ///
    /// # Errors
    ///
    /// A [`syn::Error`] for an unknown or repeated argument.
    pub fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut parsed = Self::default();
        let mut surface = None;
        syn::meta::parser(|meta| {
            if meta.path.is_ident("sync") {
                if parsed.sync {
                    return Err(meta.error("this argument is given twice"));
                }
                parsed.sync = true;
                Ok(())
            } else if meta.path.is_ident("surface") {
                string_arg(&meta, &mut surface)
            } else {
                Err(meta.error("unknown argument, expected `sync` or `surface = \"...\"`"))
            }
        })
        .parse2(args)?;
        parsed.surface = surface;
        Ok(parsed)
    }
}

impl ErrorArgs {
    /// Reads `emitter = "..."`.
    ///
    /// # Errors
    ///
    /// A [`syn::Error`] for an unknown, repeated or missing argument.
    pub fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut emitter = None;
        syn::meta::parser(|meta| {
            if meta.path.is_ident("emitter") {
                string_arg(&meta, &mut emitter)
            } else {
                Err(meta.error("unknown argument, expected `emitter = \"...\"`"))
            }
        })
        .parse2(args)?;
        Ok(Self {
            emitter: require(emitter, "error", "emitter")?,
        })
    }
}

impl ErrorVariantArgs {
    /// Reads `code = "..."` and `platform = "..."`.
    ///
    /// # Errors
    ///
    /// A [`syn::Error`] for an unknown, repeated or missing argument.
    pub fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut code = None;
        let mut platform = None;
        syn::meta::parser(|meta| {
            if meta.path.is_ident("code") {
                string_arg(&meta, &mut code)
            } else if meta.path.is_ident("platform") {
                string_arg(&meta, &mut platform)
            } else {
                Err(meta
                    .error("unknown argument, expected `code = \"...\"` or `platform = \"...\"`"))
            }
        })
        .parse2(args)?;
        Ok(Self {
            code: require(code, "error", "code")?,
            platform: require(platform, "error", "platform")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(text: &str) -> TokenStream {
        text.parse().expect("tokens")
    }

    fn attr(text: &str) -> Attribute {
        let item: syn::ItemFn = syn::parse_str(&format!("{text} fn f() {{}}")).expect("an item");
        item.attrs.into_iter().next().expect("an attribute")
    }

    #[test]
    fn client_reads_its_surface() {
        let parsed = ClientArgs::parse(stream("surface = \"connect-pagination\"")).expect("ok");
        assert_eq!(parsed.surface, "connect-pagination");
    }

    #[test]
    fn client_refuses_missing_unknown_and_repeated_arguments() {
        let missing = ClientArgs::parse(stream("")).expect_err("missing");
        assert!(missing.to_string().contains("needs `surface"), "{missing}");
        let unknown = ClientArgs::parse(stream("surfaces = \"x\"")).expect_err("unknown");
        assert!(
            unknown.to_string().contains("unknown argument"),
            "{unknown}"
        );
        let twice =
            ClientArgs::parse(stream("surface = \"a\", surface = \"b\"")).expect_err("twice");
        assert!(twice.to_string().contains("given twice"), "{twice}");
        let not_a_string = ClientArgs::parse(stream("surface = 3")).expect_err("literal");
        assert_ne!(not_a_string.to_string(), "");
    }

    #[test]
    fn export_reads_sync_and_surface_in_any_order() {
        assert_eq!(
            ExportArgs::parse(stream("")).expect("bare"),
            ExportArgs::default()
        );
        let both = ExportArgs::parse(stream("surface = \"s\", sync")).expect("both");
        assert!(both.sync);
        assert_eq!(both.surface.as_deref(), Some("s"));
        let sync = ExportArgs::parse(stream("sync")).expect("sync");
        assert!(sync.sync);
        assert_eq!(sync.surface, None);
    }

    #[test]
    fn export_refuses_unknown_and_repeated_arguments() {
        let unknown = ExportArgs::parse(stream("blocking")).expect_err("unknown");
        assert!(unknown.to_string().contains("expected `sync`"), "{unknown}");
        let twice_sync = ExportArgs::parse(stream("sync, sync")).expect_err("twice");
        assert!(
            twice_sync.to_string().contains("given twice"),
            "{twice_sync}"
        );
        let twice_surface =
            ExportArgs::parse(stream("surface = \"a\", surface = \"b\"")).expect_err("twice");
        assert!(twice_surface.to_string().contains("given twice"));
    }

    #[test]
    fn error_type_reads_its_emitter() {
        let parsed = ErrorArgs::parse(stream("emitter = \"widgets\"")).expect("ok");
        assert_eq!(parsed.emitter, "widgets");
        let missing = ErrorArgs::parse(stream("")).expect_err("missing");
        assert!(missing.to_string().contains("needs `emitter"), "{missing}");
        let unknown = ErrorArgs::parse(stream("code = \"x\"")).expect_err("unknown");
        assert!(
            unknown.to_string().contains("expected `emitter"),
            "{unknown}"
        );
    }

    #[test]
    fn error_variant_reads_code_and_platform() {
        let parsed =
            ErrorVariantArgs::parse(stream("code = \"WIDGET_GONE\", platform = \"NOT_FOUND\""))
                .expect("ok");
        assert_eq!(parsed.code, "WIDGET_GONE");
        assert_eq!(parsed.platform, "NOT_FOUND");
        let no_platform = ErrorVariantArgs::parse(stream("code = \"X\"")).expect_err("platform");
        assert!(no_platform.to_string().contains("needs `platform"));
        let no_code = ErrorVariantArgs::parse(stream("platform = \"X\"")).expect_err("code");
        assert!(no_code.to_string().contains("needs `code"));
        let unknown = ErrorVariantArgs::parse(stream("emitter = \"X\"")).expect_err("unknown");
        assert!(unknown.to_string().contains("expected `code"), "{unknown}");
    }

    #[test]
    fn find_matches_the_last_path_segment() {
        let bare = attr("#[client(surface = \"bare\")]");
        let pathed = attr("#[api_bones_sdk::client(surface = \"pathed\")]");
        let other = attr("#[inline]");
        let attrs = [other, pathed];
        let found = find(&attrs, "client").expect("pathed");
        assert_eq!(
            tokens(found).expect("tokens").to_string(),
            "surface = \"pathed\""
        );
        let attrs = [bare];
        let found = find(&attrs, "client").expect("bare");
        assert_eq!(
            tokens(found).expect("tokens").to_string(),
            "surface = \"bare\""
        );
        assert!(find(&[], "client").is_none());
        assert!(find(&[attr("#[inline]")], "client").is_none());
    }

    #[test]
    fn tokens_of_a_bare_a_listed_and_a_named_attribute() {
        assert!(tokens(&attr("#[export]")).expect("bare").is_empty());
        let listed = tokens(&attr("#[export(sync)]")).expect("listed");
        assert_eq!(listed.to_string(), "sync");
        let named = tokens(&attr("#[export = \"x\"]")).expect_err("named");
        assert!(named.to_string().contains("in parentheses"), "{named}");
    }

    #[test]
    fn a_marker_attribute_takes_no_arguments() {
        no_args("record", TokenStream::new()).expect("none");
        let refused = no_args("record", stream("camel")).expect_err("args");
        assert_eq!(refused.to_string(), "`#[record]` takes no arguments");
    }
}
