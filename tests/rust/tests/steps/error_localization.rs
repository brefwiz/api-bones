// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../features/error_localization.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows. Connect errors render through `api_bones::i18n::localize`, which
//! widens the rendering crate's trait to the foreign Connect error; every
//! other input renders through `api_bones_i18n::localize`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::world::{LocalizationSubject, LocalizationWorld};
use api_bones::ProblemJson;
use api_bones::connect::{ErrorInfo, ValidationFailureDetail, try_with_error_info};
use api_bones::error::{ErrorArg, ErrorInfoMember};
use api_bones_i18n::{ArgValue, LocalizeOptions};
use connectrpc::{ConnectError, ErrorCode, ErrorDetail};
use cucumber::{given, then, when};

const OPERATOR_MESSAGE: &str = "seat limit reached";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../features/fixtures/error_localization")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Expand `repeat(<text>,<n>)` and `\uXXXX` the way the feature defines them.
fn expand(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        if let Some(call) = rest.strip_prefix("repeat(") {
            let close = call.find(')').expect("repeat(<text>,<n>) closes");
            let (unit, count) = call[..close].rsplit_once(',').expect("repeat(<text>,<n>)");
            out.push_str(&unit.repeat(count.parse().expect("repeat count")));
            rest = &call[close + 1..];
        } else if let Some(hex) = rest
            .strip_prefix("\\u")
            .and_then(|tail| tail.get(..4))
            .filter(|hex| hex.chars().all(|c| c.is_ascii_hexdigit()))
        {
            let code = u32::from_str_radix(hex, 16).expect("hex digits");
            out.push(char::from_u32(code).expect("a scalar value"));
            rest = &rest[6..];
        } else {
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

fn parse_value(kind: &str, value: &str) -> ArgValue {
    match kind {
        "string" => ArgValue::String(value.to_owned()),
        "int64" => ArgValue::Int64(value.parse().expect("an int64")),
        "double" => ArgValue::Double(value.parse().expect("a double")),
        "timestamp" => {
            let at: jiff::Timestamp = value.parse().expect("an RFC 3339 timestamp");
            ArgValue::Timestamp {
                seconds: at.as_second(),
                nanos: at.subsec_nanosecond(),
            }
        }
        "timestamp_raw" => {
            let (seconds, nanos) = value.split_once(':').expect("seconds:nanos");
            ArgValue::Timestamp {
                seconds: seconds.parse().expect("seconds"),
                nanos: nanos.parse().expect("nanos"),
            }
        }
        other => panic!("the contract names an argument type this step cannot build: {other}"),
    }
}

/// `key=type:value` items joined by `; `, `(none)` for none, `args(<n>)` for
/// `n` numbered int64 arguments.
fn parse_args(spec: &str) -> Vec<(String, ArgValue)> {
    if spec == "(none)" {
        return Vec::new();
    }
    let mut args = Vec::new();
    for item in spec.split("; ") {
        if let Some(count) = item
            .strip_prefix("args(")
            .and_then(|tail| tail.strip_suffix(')'))
        {
            let count: i64 = count.parse().expect("args(<n>)");
            args.extend((1..=count).map(|n| (format!("k{n}"), ArgValue::Int64(n))));
            continue;
        }
        let item = expand(item);
        let (key, typed) = item.split_once('=').expect("key=type:value");
        let (kind, value) = typed.split_once(':').expect("type:value");
        args.push((key.to_owned(), parse_value(kind, value)));
    }
    args
}

fn wire_arg(value: ArgValue) -> ErrorArg {
    match value {
        ArgValue::String(value) => ErrorArg::String(value),
        ArgValue::Int64(value) => ErrorArg::Int64(value),
        ArgValue::Double(value) => ErrorArg::Double(value),
        ArgValue::Timestamp { seconds, nanos } => ErrorArg::Timestamp { seconds, nanos },
    }
}

fn wire_args(spec: &str) -> BTreeMap<String, ErrorArg> {
    parse_args(spec)
        .into_iter()
        .map(|(key, value)| (key, wire_arg(value)))
        .collect()
}

fn failure_with(connect_code: &str, detail: Option<ErrorDetail>) -> ConnectError {
    let code: ErrorCode = connect_code.parse().unwrap_or_else(|()| {
        panic!("the contract names a Connect code this step cannot build: {connect_code}")
    });
    let error = ConnectError::new(code, OPERATOR_MESSAGE);
    match detail {
        Some(detail) => error.with_detail(detail),
        None => error,
    }
}

#[given(expr = "the platform error catalogs are loaded")]
fn given_platform_catalogs(_world: &mut LocalizationWorld) {
    let dir = repo_root().join("api-bones-i18n/catalogs/errors");
    for locale in api_bones_i18n::PLATFORM_LOCALES {
        let body = std::fs::read_to_string(dir.join(format!("{locale}.json")))
            .unwrap_or_else(|error| panic!("the {locale} catalog is unreadable: {error}"));
        let entries: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&body).expect("a catalog object");
        let title = entries
            .get("bones.errors.INTERNAL_SERVER_ERROR.title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("the {locale} catalog has no INTERNAL_SERVER_ERROR title"));
        let info = ErrorInfo {
            code: "INTERNAL_SERVER_ERROR".to_owned(),
            emitter: String::new(),
            args: BTreeMap::new(),
        };
        let error = try_with_error_info(failure_with("internal", None), &info)
            .expect("no arguments are within bounds");
        let rendered = api_bones::i18n::localize(&error, locale, &LocalizeOptions::default());
        assert_eq!(
            rendered.text, title,
            "the {locale} platform catalog is not the one the crate renders from"
        );
    }
}

#[given(expr = "the error catalogs of emitter {string} are registered from fixture {string}")]
#[given(expr = "the error catalogs of emitter {string} are registered again from fixture {string}")]
fn given_catalogs(_world: &mut LocalizationWorld, emitter: String, fixture: String) {
    let dir = fixtures().join(fixture);
    for entry in std::fs::read_dir(&dir).expect("fixture directory") {
        let path = entry.expect("directory entry").path();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a file name");
        if path.extension().is_none_or(|ext| ext != "json") || stem.contains('.') {
            continue;
        }
        let json = std::fs::read_to_string(&path).expect("catalog file");
        api_bones_i18n::register_catalog(&emitter, stem, &json);
    }
}

#[given("the catalogs the build helper embeds are registered")]
fn given_embedded_catalogs(_world: &mut LocalizationWorld) {
    for (emitter, locale, json) in api_bones_emitter_catalogs_fixture::ERROR_CATALOGS {
        api_bones_i18n::register_catalog(emitter, locale, json);
    }
}

#[given(
    expr = "a Connect error {string} whose ErrorInfo has code {string}, emitter {string} and args {string}"
)]
fn given_error_info(
    world: &mut LocalizationWorld,
    connect_code: String,
    code: String,
    emitter: String,
    args: String,
) {
    let info = ErrorInfo {
        code,
        emitter,
        args: wire_args(&args),
    };
    let error = try_with_error_info(failure_with(&connect_code, None), &info)
        .expect("the arguments are within bounds");
    world.subject = Some(LocalizationSubject::Connect(error));
}

#[given(
    expr = "a Connect error {string} whose unchecked ErrorInfo has code {string}, emitter {string} and args {string}"
)]
fn given_unchecked_error_info(
    world: &mut LocalizationWorld,
    connect_code: String,
    code: String,
    emitter: String,
    args: String,
) {
    let info = ErrorInfo {
        code,
        emitter,
        args: wire_args(&args),
    };
    let error = failure_with(&connect_code, Some(info.to_detail()));
    world.subject = Some(LocalizationSubject::Connect(error));
}

#[given(expr = "a Connect error {string} with no ErrorInfo")]
fn given_bare(world: &mut LocalizationWorld, connect_code: String) {
    world.subject = Some(LocalizationSubject::Connect(failure_with(
        &connect_code,
        None,
    )));
}

#[given(expr = "a Connect error {string} whose ErrorInfo detail is {string}")]
fn given_error_info_wire(world: &mut LocalizationWorld, connect_code: String, wire: String) {
    let detail = ErrorDetail {
        type_url: "bones.v1.ErrorInfo".into(),
        value: Some(wire),
        debug: None,
    };
    world.subject = Some(LocalizationSubject::Connect(failure_with(
        &connect_code,
        Some(detail),
    )));
}

#[given(expr = "a Connect error {string} whose ValidationFailure detail is {string}")]
fn given_failure_wire(world: &mut LocalizationWorld, connect_code: String, wire: String) {
    let detail = ErrorDetail {
        type_url: "bones.v1.ValidationFailure".into(),
        value: Some(wire),
        debug: None,
    };
    world.subject = Some(LocalizationSubject::Connect(failure_with(
        &connect_code,
        Some(detail),
    )));
}

/// The first `ValidationFailure` the wire value decodes to.
#[given(expr = "a ValidationFailure whose wire value is {string}")]
fn given_bare_failure(world: &mut LocalizationWorld, wire: String) {
    let carrier = failure_with(
        "invalid_argument",
        Some(ErrorDetail {
            type_url: "bones.v1.ValidationFailure".into(),
            value: Some(wire),
            debug: None,
        }),
    );
    let failure = api_bones::connect::violations_of(&carrier)
        .into_iter()
        .next()
        .expect("the wire value decodes to a ValidationFailure");
    world.subject = Some(LocalizationSubject::Failure(failure));
}

#[given(
    expr = "a ValidationFailure with field {string}, rule {string} and unchecked args {string}"
)]
fn given_unchecked_failure(
    world: &mut LocalizationWorld,
    field: String,
    rule: String,
    args: String,
) {
    world.subject = Some(LocalizationSubject::Failure(ValidationFailureDetail {
        field,
        rule,
        message: OPERATOR_MESSAGE.into(),
        args: wire_args(&args),
    }));
}

#[given(expr = "the problem+json body from fixture {string}")]
fn given_problem(world: &mut LocalizationWorld, fixture: String) {
    let body = std::fs::read_to_string(fixtures().join(&fixture)).expect("problem fixture");
    let problem: ProblemJson = serde_json::from_str(&body).expect("a problem+json body");
    world.subject = Some(LocalizationSubject::Problem(problem));
}

#[when(expr = "the error is localized for {string}")]
fn when_localized(world: &mut LocalizationWorld, locale: String) {
    let options = LocalizeOptions::default();
    let localized = match world.subject.as_ref().expect("no error given") {
        LocalizationSubject::Connect(error) => api_bones::i18n::localize(error, &locale, &options),
        LocalizationSubject::Problem(problem) => {
            api_bones_i18n::localize(problem, &locale, &options)
        }
        LocalizationSubject::Failure(failure) => {
            api_bones_i18n::localize(failure, &locale, &options)
        }
    };
    world.localized = Some(localized);
}

#[when(expr = "the error is localized for {string} in time zone {string}")]
fn when_localized_in_zone(world: &mut LocalizationWorld, locale: String, zone: String) {
    let options = if zone == "(none)" {
        Ok(LocalizeOptions::default())
    } else {
        LocalizeOptions::default().time_zone(&zone)
    };
    match options {
        Ok(options) => {
            let localized = match world.subject.as_ref().expect("no error given") {
                LocalizationSubject::Connect(error) => {
                    api_bones::i18n::localize(error, &locale, &options)
                }
                LocalizationSubject::Problem(problem) => {
                    api_bones_i18n::localize(problem, &locale, &options)
                }
                LocalizationSubject::Failure(failure) => {
                    api_bones_i18n::localize(failure, &locale, &options)
                }
            };
            world.localized = Some(localized);
        }
        Err(refusal) => world.refused = Some(refusal.rule.to_owned()),
    }
}

#[when(expr = "the violations of the error are read")]
fn when_violations(world: &mut LocalizationWorld) {
    world.violations = match world.subject.as_ref().expect("no error given") {
        LocalizationSubject::Connect(error) => api_bones::i18n::violations_of(error),
        LocalizationSubject::Problem(problem) => api_bones::i18n::violations_of(problem),
        LocalizationSubject::Failure(_) => panic!("a bare ValidationFailure carries no violations"),
    };
}

#[when(expr = "violation {int} is localized for {string}")]
fn when_violation_localized(world: &mut LocalizationWorld, index: usize, locale: String) {
    let violation = world
        .violations
        .get(index - 1)
        .expect("the violation exists");
    world.localized = Some(api_bones_i18n::localize(
        violation,
        &locale,
        &LocalizeOptions::default(),
    ));
}

#[when(expr = "the emitter builds ErrorInfo code {string} with args {string}")]
fn when_built(world: &mut LocalizationWorld, code: String, args: String) {
    let entries = parse_args(&args);
    let by_args = api_bones_i18n::error_args(
        entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone())),
    )
    .map(drop)
    .map_err(|refusal| refusal.rule.as_str().to_owned());
    let mut member = ErrorInfoMember::new(code, "acme");
    let mut by_with_arg = Ok(());
    for (key, value) in entries {
        if by_with_arg.is_ok() {
            by_with_arg = member
                .clone()
                .with_arg(key, wire_arg(value))
                .map(|next| member = next)
                .map_err(|refusal| refusal.rule().to_owned());
        }
    }
    assert_eq!(
        by_args, by_with_arg,
        "Args::insert and with_arg name the same first failing rule"
    );
    world.built = Some(by_args);
}

#[then(expr = "the localized text is {string}")]
fn then_text(world: &mut LocalizationWorld, expected: String) {
    let localized = world
        .localized
        .as_ref()
        .expect("the error was not localized");
    assert_eq!(
        white_space_folded(&localized.text),
        white_space_folded(&expected)
    );
}

#[then(expr = "the localized hint is {string}")]
fn then_hint(world: &mut LocalizationWorld, expected: String) {
    let localized = world
        .localized
        .as_ref()
        .expect("the error was not localized");
    let hint = localized.hint.as_deref().map(white_space_folded);
    let expected = (expected != "none").then(|| white_space_folded(&expected));
    assert_eq!(hint, expected);
}

#[then(expr = "the localized text does not contain {string}")]
fn then_text_excludes(world: &mut LocalizationWorld, absent: String) {
    let localized = world
        .localized
        .as_ref()
        .expect("the error was not localized");
    assert!(
        !localized.text.contains(&absent),
        "text: {}",
        localized.text
    );
}

/// Every Unicode white-space character becomes one U+0020.
fn white_space_folded(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect()
}

#[then(expr = "the localization is refused with rule {string}")]
fn then_refused(world: &mut LocalizationWorld, rule: String) {
    assert_eq!(world.refused.as_deref(), Some(rule.as_str()));
}

#[then(expr = "the construction succeeds")]
fn then_built(world: &mut LocalizationWorld) {
    assert_eq!(world.built, Some(Ok(())));
}

#[then(expr = "the construction is refused with rule {string}")]
fn then_built_refused(world: &mut LocalizationWorld, rule: String) {
    assert_eq!(world.built, Some(Err(rule)));
}

#[then(expr = "there is/are {int} violation(s)")]
fn then_violation_count(world: &mut LocalizationWorld, count: usize) {
    assert_eq!(world.violations.len(), count);
}

#[then(expr = "violation {int} has field {string} and rule {string}")]
fn then_violation_at(world: &mut LocalizationWorld, index: usize, field: String, rule: String) {
    let actual = world
        .violations
        .get(index - 1)
        .map(|violation| (violation.field.clone(), violation.rule.clone()));
    assert_eq!(actual, Some((field, Some(rule))));
}

#[given(expr = "the platform locale registry from fixture {string}")]
fn given_registry(world: &mut LocalizationWorld, fixture: String) {
    let body = std::fs::read_to_string(fixtures().join(fixture)).expect("registry fixture");
    world.registry = Some(serde_json::from_str(&body).expect("registry JSON"));
}

#[when(expr = "each of its locales and aliases is negotiated")]
fn when_negotiated(world: &mut LocalizationWorld) {
    let registry = world.registry.as_ref().expect("no registry given");
    let mut requests: Vec<(String, String)> = registry["locales"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|code| {
            let code = code.as_str().expect("a string");
            (code.to_owned(), code.to_owned())
        })
        .collect();
    requests.extend(
        registry["aliases"]
            .as_object()
            .expect("an object")
            .iter()
            .map(|(alias, code)| (alias.clone(), code.as_str().expect("a string").to_owned())),
    );
    world.negotiated_empty = Some(api_bones_i18n::negotiate_locale("").to_owned());
    world.negotiated.clear();
    for (request, expected) in requests {
        let got = api_bones_i18n::negotiate_locale(&request).to_owned();
        world.negotiated.push((request, got, expected));
    }
}

#[then(expr = "each negotiates to its registry locale and the empty request to the default locale")]
fn then_negotiated(world: &mut LocalizationWorld) {
    assert!(!world.negotiated.is_empty(), "nothing was negotiated");
    let registry = world.registry.as_ref().expect("no registry given");
    assert_eq!(
        world.negotiated_empty.as_deref(),
        registry["default"].as_str(),
        "negotiating the empty request"
    );
    for (request, got, expected) in &world.negotiated {
        assert_eq!(got, expected, "negotiating {request:?}");
    }
}

#[then(expr = "this language's platform locales, aliases and default locale equal it")]
fn then_registry(world: &mut LocalizationWorld) {
    let registry = world.registry.as_ref().expect("no registry given");
    let strings = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .expect("an array")
            .iter()
            .map(|v| v.as_str().expect("a string").to_owned())
            .collect()
    };
    assert_eq!(
        api_bones_i18n::PLATFORM_LOCALES.map(str::to_owned).to_vec(),
        strings(&registry["locales"])
    );
    assert_eq!(
        Some(api_bones_i18n::DEFAULT_LOCALE),
        registry["default"].as_str()
    );
    let expected_aliases = registry["aliases"].as_object().expect("an object");
    let aliases: BTreeMap<String, String> = api_bones_i18n::ALIASES
        .iter()
        .map(|&(alias, code)| (alias.to_owned(), code.to_owned()))
        .collect();
    let expected: BTreeMap<String, String> = expected_aliases
        .iter()
        .map(|(alias, code)| (alias.clone(), code.as_str().expect("a string").to_owned()))
        .collect();
    assert_eq!(aliases, expected);
}

#[given(expr = "the CLDR release pinned in {string}")]
fn given_pin(world: &mut LocalizationWorld, file: String) {
    let pins = std::fs::read_to_string(repo_root().join(file)).expect("the CLDR pin file");
    let pins: serde_json::Value = serde_json::from_str(&pins).expect("the CLDR pin file is JSON");
    let release = pins["rust"].as_str().expect("a rust entry");
    world.cldr_pin = Some(release.to_owned());
}

/// The `[package.metadata.sources.cldr] tagged` of every `icu_*_data` package
/// `api-bones-i18n` compiles.
#[then(expr = "this language's formatting data is that CLDR release")]
fn then_cldr(world: &mut LocalizationWorld) {
    let pin = world.cldr_pin.as_deref().expect("no CLDR pin given");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(repo_root())
        .output()
        .expect("cargo metadata runs");
    assert!(output.status.success(), "cargo metadata failed");
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata JSON");
    let id_of = |name: &str| {
        metadata["packages"]
            .as_array()
            .expect("packages")
            .iter()
            .find(|package| package["name"] == name)
            .and_then(|package| package["id"].as_str())
            .unwrap_or_else(|| panic!("package {name} is in the workspace graph"))
            .to_owned()
    };
    let edges: BTreeMap<&str, Vec<&str>> = metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolve nodes")
        .iter()
        .map(|node| {
            let id = node["id"].as_str().expect("node id");
            let deps = node["deps"]
                .as_array()
                .expect("deps")
                .iter()
                .map(|dep| dep["pkg"].as_str().expect("dep id"))
                .collect();
            (id, deps)
        })
        .collect();
    let root = id_of("api-bones-i18n");
    let mut seen = std::collections::BTreeSet::new();
    let mut pending = vec![root.as_str()];
    while let Some(id) = pending.pop() {
        if seen.insert(id) {
            pending.extend(edges.get(id).into_iter().flatten().copied());
        }
    }
    let data: Vec<(String, String)> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter(|package| {
            let name = package["name"].as_str().unwrap_or_default();
            name.starts_with("icu_")
                && name.ends_with("_data")
                && package["id"].as_str().is_some_and(|id| seen.contains(id))
        })
        .map(|package| {
            (
                package["name"].as_str().unwrap_or_default().to_owned(),
                package["metadata"]["sources"]["cldr"]["tagged"]
                    .as_str()
                    .unwrap_or("untagged")
                    .to_owned(),
            )
        })
        .collect();
    assert!(
        !data.is_empty(),
        "api-bones-i18n compiles no icu data package"
    );
    for (name, tagged) in data {
        assert_eq!(tagged, pin, "{name} is not CLDR {pin}");
    }
}
