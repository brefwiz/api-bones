// SPDX-License-Identifier: MIT
//! The check a provider's delegation templates pass when its service
//! descriptor is validated.
//!
//! A template (`bones.v1.delegation_templates`) is a list of
//! `bones.v1.ProviderGrant`, the one grant grammar a delegation type is
//! composed of. [`check_delegation_templates`] refuses a template that names
//! a scope, a capability group or an attribute dimension its own service does
//! not declare, and a selector value that is not a literal of its dimension.
//!
//! What a service declares is read from its descriptor:
//!
//! - a scope: a `bones.v1.authz` `requires` entry of one of its RPCs whose
//!   kind is authenticated, or a `bones.v1.provided_broker_scopes` scope;
//! - a group: a `capability` of its `bones.v1.capability_scopes` entries,
//!   when the service provides that capability (`capability` or
//!   `provided_capabilities`) and every entry naming it lists at least one
//!   scope and only scopes the service declares;
//! - a dimension: a `bones.v1.attributes` entry, whose match kind and
//!   separator a selector on it inherits.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use prost_reflect::{DescriptorPool, DynamicMessage, ServiceDescriptor, Value};

const TEMPLATES: &str = "bones.v1.delegation_templates";
const ATTRIBUTES: &str = "bones.v1.attributes";
const AUTHZ: &str = "bones.v1.authz";
const BROKER_SCOPES: &str = "bones.v1.provided_broker_scopes";
const CAPABILITY_SCOPES: &str = "bones.v1.capability_scopes";
const CAPABILITY: &str = "bones.v1.capability";
const PROVIDED_CAPABILITIES: &str = "bones.v1.provided_capabilities";

/// `bones.v1.AttributeMatching.ATTRIBUTE_MATCHING_SEGMENT_PREFIX`.
const MATCHING_SEGMENT_PREFIX: i32 = 2;
/// `bones.v1.AttributeType.ATTRIBUTE_TYPE_UUID`.
const TYPE_UUID: i32 = 2;
/// `bones.v1.AuthzKind.AUTHZ_KIND_AUTHENTICATED`: the only kind whose
/// `requires` scopes a service declares.
const AUTHZ_KIND_AUTHENTICATED: i32 = 4;
/// `bones.v1.ApprovalMode.APPROVAL_MODE_UNSPECIFIED`.
const APPROVAL_UNSPECIFIED: i32 = 0;
/// `APPROVAL_MODE_PRE_APPROVED` and `APPROVAL_MODE_ASK`: the only modes a
/// grant may carry. The enum is open, so any other number is refused.
const APPROVAL_MODES: [i32; 2] = [1, 2];

/// Characters that read as a wildcard where a value is matched. A selector
/// value holds none of them, whatever the dimension.
const WILDCARDS: [char; 3] = ['*', '?', '>'];

/// Datalog punctuation that would let a value inject a fact into its block.
const PUNCTUATION: [char; 11] = ['"', '(', ')', ',', ';', '[', ']', '{', '}', '$', '\\'];

/// Every Unicode format character (`General_Category=Cf`) as sorted inclusive
/// ranges: they hide or reorder what a person reads. `std` has no predicate
/// for the category.
const FORMAT_CHARACTERS: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),
    ('\u{0600}', '\u{0605}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{06DD}', '\u{06DD}'),
    ('\u{070F}', '\u{070F}'),
    ('\u{0890}', '\u{0891}'),
    ('\u{08E2}', '\u{08E2}'),
    ('\u{180E}', '\u{180E}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206F}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFF9}', '\u{FFFB}'),
    ('\u{110BD}', '\u{110BD}'),
    ('\u{110CD}', '\u{110CD}'),
    ('\u{13430}', '\u{1343F}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0001}', '\u{E0001}'),
    ('\u{E0020}', '\u{E007F}'),
];

fn is_format(c: char) -> bool {
    FORMAT_CHARACTERS
        .iter()
        .any(|&(first, last)| (first..=last).contains(&c))
}

/// A character that must never reach output or a value: whitespace, a control
/// or format character, or Datalog punctuation.
fn is_hostile(c: char) -> bool {
    c.is_whitespace() || c.is_control() || is_format(c) || PUNCTUATION.contains(&c)
}

/// A character no value or name a grant carries may hold: a hostile character
/// or a wildcard.
fn is_unsafe(c: char) -> bool {
    is_hostile(c) || WILDCARDS.contains(&c)
}

/// Why one template, or one grant of it, is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The name is not `lower_snake_case`.
    InvalidName,
    /// Another template of the service has the same name.
    DuplicateName,
    /// The label has no text.
    MissingLabel,
    /// The template has no grant.
    NoGrants,
    /// The grant names neither a scope nor a group.
    GrantNamesNothing,
    /// The grant names a scope the service does not declare.
    UndeclaredScope(String),
    /// The grant names a group the service does not declare.
    UndeclaredGroup(String),
    /// A selector names a dimension the service does not declare.
    UndeclaredDimension(String),
    /// Two selectors of one grant name the same dimension.
    DuplicateDimension(String),
    /// A selector carries no value.
    EmptySelector(String),
    /// A selector value carries `*`, `?` or `>`.
    WildcardValue {
        /// The dimension the selector names.
        dimension: String,
        /// The refused value.
        value: String,
    },
    /// A selector value carries whitespace, a control or format character or
    /// Datalog punctuation.
    UnsafeValue {
        /// The dimension the selector names.
        dimension: String,
        /// The refused value.
        value: String,
    },
    /// A scope, group or dimension a grant names carries whitespace, a control
    /// or format character, Datalog punctuation or a wildcard.
    UnsafeName(String),
    /// A selector value is empty, has an empty segment, or is not a UUID on a
    /// UUID dimension.
    InvalidValue {
        /// The dimension the selector names.
        dimension: String,
        /// The refused value.
        value: String,
    },
    /// The approval mode is unspecified.
    ApprovalUnspecified,
    /// The approval mode is a number the grammar does not define.
    UnknownApprovalMode(i32),
    /// The budget is present and its count or window is zero.
    EmptyBudget,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("the name is not lower_snake_case"),
            Self::DuplicateName => f.write_str("another template of the service has this name"),
            Self::MissingLabel => f.write_str("the label has no text"),
            Self::NoGrants => f.write_str("the template has no grant"),
            Self::GrantNamesNothing => f.write_str("the grant names no scope and no group"),
            Self::UndeclaredScope(scope) => {
                write!(f, "the scope `{scope}` is not declared by the service")
            }
            Self::UndeclaredGroup(group) => {
                write!(f, "the group `{group}` is not declared by the service")
            }
            Self::UndeclaredDimension(name) => {
                write!(f, "the dimension `{name}` is not declared by the service")
            }
            Self::DuplicateDimension(name) => {
                write!(f, "two selectors name the dimension `{name}`")
            }
            Self::EmptySelector(name) => {
                write!(f, "the selector on `{name}` carries no value")
            }
            Self::WildcardValue { dimension, value } => write!(
                f,
                "the value `{value}` on `{dimension}` carries a wildcard character"
            ),
            Self::UnsafeValue { dimension, .. } => write!(
                f,
                "a value on `{dimension}` carries whitespace, a control or format character, \
                 or punctuation"
            ),
            Self::UnsafeName(_) => f.write_str(
                "a scope, group or dimension name carries whitespace, a control or format character, \
                 punctuation or a wildcard",
            ),
            Self::InvalidValue { dimension, value } => {
                write!(f, "the value `{value}` is not valid on `{dimension}`")
            }
            Self::ApprovalUnspecified => f.write_str("the approval mode is unspecified"),
            Self::UnknownApprovalMode(mode) => {
                write!(f, "the approval mode {mode} is not one the grammar defines")
            }
            Self::EmptyBudget => f.write_str("the budget has a zero count or window"),
        }
    }
}

/// One refusal: where it was found and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Fully-qualified name of the service declaring the template.
    pub service: String,
    /// Name of the template, as declared; empty when it declares none.
    pub template: String,
    /// Position of the grant in the template, or `None` when the refusal is
    /// of the template itself.
    pub grant: Option<usize>,
    /// Why it is refused.
    pub reason: Reason,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} template `", self.service)?;
        for c in self.template.chars() {
            if is_unsafe(c) {
                write!(f, "{}", c.escape_unicode())?;
            } else {
                write!(f, "{c}")?;
            }
        }
        f.write_str("`")?;
        if let Some(grant) = self.grant {
            write!(f, " grant {grant}")?;
        }
        write!(f, ": {}", self.reason)
    }
}

/// Every refusal found in one descriptor pool, in service, template and grant
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationTemplateError {
    /// The refusals; never empty.
    pub refusals: Vec<Refusal>,
}

impl fmt::Display for DelegationTemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, refusal) in self.refusals.iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{refusal}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DelegationTemplateError {}

/// Check the delegation templates of every service in `pool`.
///
/// A pool that does not import `bones/v1/delegation.proto` declares none and
/// passes.
///
/// # Errors
///
/// [`DelegationTemplateError`] listing every refusal.
pub fn check_delegation_templates(pool: &DescriptorPool) -> Result<(), DelegationTemplateError> {
    let Some(extension) = pool.get_extension_by_name(TEMPLATES) else {
        return Ok(());
    };
    let mut refusals = Vec::new();
    for service in pool.services() {
        let options = service.options();
        let value = options.get_extension(&extension);
        let templates: Vec<&DynamicMessage> = value
            .as_list()
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_message)
            .collect();
        if templates.is_empty() {
            continue;
        }
        let contract = Contract::of(pool, &service);
        let mut names = BTreeSet::new();
        for template in templates {
            refusals.extend(check_template(
                service.full_name(),
                template,
                &contract,
                &mut names,
            ));
        }
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(DelegationTemplateError { refusals })
    }
}

/// What one service declares and its templates may name.
struct Contract {
    scopes: BTreeSet<String>,
    groups: BTreeSet<String>,
    dimensions: BTreeMap<String, Dimension>,
}

/// How a selector on one dimension compares and what its values look like.
struct Dimension {
    /// The separator of a `SEGMENT_PREFIX` dimension; `None` for `EXACT`.
    separator: Option<char>,
    uuid: bool,
}

impl Contract {
    fn of(pool: &DescriptorPool, service: &ServiceDescriptor) -> Self {
        let mut scopes = BTreeSet::new();
        for entry in service_entries(pool, service, BROKER_SCOPES) {
            scopes.insert(text(&entry, "scope"));
        }
        if let Some(authz) = pool.get_extension_by_name(AUTHZ) {
            for method in service.methods() {
                let options = method.options();
                if !options.has_extension(&authz) {
                    continue;
                }
                let value = options.get_extension(&authz);
                let authenticated = value
                    .as_message()
                    .filter(|rule| number(rule, "kind") == AUTHZ_KIND_AUTHENTICATED);
                if let Some(rule) = authenticated {
                    scopes.extend(strings(rule, "requires"));
                }
            }
        }
        let provided: BTreeSet<String> = service_strings(pool, service, CAPABILITY)
            .into_iter()
            .chain(service_strings(pool, service, PROVIDED_CAPABILITIES))
            .collect();
        let entries = service_entries(pool, service, CAPABILITY_SCOPES);
        let refused: BTreeSet<String> = entries
            .iter()
            .filter(|entry| !confers_declared_scopes(entry, &scopes))
            .map(|entry| text(entry, "capability"))
            .collect();
        let groups = entries
            .iter()
            .map(|entry| text(entry, "capability"))
            .filter(|capability| provided.contains(capability) && !refused.contains(capability))
            .collect();
        let dimensions = service_entries(pool, service, ATTRIBUTES)
            .iter()
            .map(|entry| {
                let prefix = number(entry, "matching") == MATCHING_SEGMENT_PREFIX;
                let dimension = Dimension {
                    separator: if prefix {
                        text(entry, "separator").chars().next()
                    } else {
                        None
                    },
                    uuid: number(entry, "type") == TYPE_UUID,
                };
                (text(entry, "name"), dimension)
            })
            .collect();
        Self {
            scopes,
            groups,
            dimensions,
        }
    }
}

/// Whether a `capability_scopes` entry lists at least one scope and only
/// scopes the service declares.
fn confers_declared_scopes(entry: &DynamicMessage, scopes: &BTreeSet<String>) -> bool {
    let listed = strings(entry, "scopes");
    !listed.is_empty() && listed.iter().all(|scope| scopes.contains(scope))
}

fn check_template(
    service: &str,
    template: &DynamicMessage,
    contract: &Contract,
    names: &mut BTreeSet<String>,
) -> Vec<Refusal> {
    let name = text(template, "name");
    let refuse = |grant: Option<usize>, reason: Reason| Refusal {
        service: service.to_owned(),
        template: name.clone(),
        grant,
        reason,
    };
    let mut refusals = Vec::new();
    if !is_lower_snake_case(&name) {
        refusals.push(refuse(None, Reason::InvalidName));
    } else if !names.insert(name.clone()) {
        refusals.push(refuse(None, Reason::DuplicateName));
    }
    let label = template
        .get_field_by_name("label")
        .and_then(|label| label.as_message().map(|label| text(label, "text")))
        .unwrap_or_default();
    if label.trim().is_empty() {
        refusals.push(refuse(None, Reason::MissingLabel));
    }
    let grants = messages(template, "grants");
    if grants.is_empty() {
        refusals.push(refuse(None, Reason::NoGrants));
    }
    for (index, grant) in grants.iter().enumerate() {
        refusals.extend(
            check_grant(grant, contract)
                .into_iter()
                .map(|reason| refuse(Some(index), reason)),
        );
    }
    refusals
}

fn check_grant(grant: &DynamicMessage, contract: &Contract) -> Vec<Reason> {
    let mut reasons = Vec::new();
    let scope = text(grant, "scope");
    let group = text(grant, "group");
    if scope.is_empty() && group.is_empty() {
        reasons.push(Reason::GrantNamesNothing);
    }
    if scope.chars().any(is_unsafe) {
        reasons.push(Reason::UnsafeName(scope));
    } else if !scope.is_empty() && !contract.scopes.contains(&scope) {
        reasons.push(Reason::UndeclaredScope(scope));
    }
    if group.chars().any(is_unsafe) {
        reasons.push(Reason::UnsafeName(group));
    } else if !group.is_empty() && !contract.groups.contains(&group) {
        reasons.push(Reason::UndeclaredGroup(group));
    }
    let mut named = BTreeSet::new();
    for selector in messages(grant, "selectors") {
        reasons.extend(check_selector(&selector, contract, &mut named));
    }
    let approval = number(grant, "approval_mode");
    if approval == APPROVAL_UNSPECIFIED {
        reasons.push(Reason::ApprovalUnspecified);
    } else if !APPROVAL_MODES.contains(&approval) {
        reasons.push(Reason::UnknownApprovalMode(approval));
    }
    if grant.has_field_by_name("budget") {
        let empty = grant
            .get_field_by_name("budget")
            .and_then(|budget| {
                budget.as_message().map(|budget| {
                    count(budget, "count") == 0 || count(budget, "window_seconds") == 0
                })
            })
            .unwrap_or(true);
        if empty {
            reasons.push(Reason::EmptyBudget);
        }
    }
    reasons
}

fn check_selector(
    selector: &DynamicMessage,
    contract: &Contract,
    named: &mut BTreeSet<String>,
) -> Vec<Reason> {
    let dimension = text(selector, "dimension");
    if dimension.chars().any(is_unsafe) {
        return vec![Reason::UnsafeName(dimension)];
    }
    if !named.insert(dimension.clone()) {
        return vec![Reason::DuplicateDimension(dimension)];
    }
    let Some(declared) = contract.dimensions.get(&dimension) else {
        return vec![Reason::UndeclaredDimension(dimension)];
    };
    let values = strings(selector, "values");
    if values.is_empty() {
        return vec![Reason::EmptySelector(dimension)];
    }
    values
        .into_iter()
        .filter_map(|value| check_value(&dimension, declared, value))
        .collect()
}

fn check_value(name: &str, dimension: &Dimension, value: String) -> Option<Reason> {
    if value.chars().any(is_hostile) {
        return Some(Reason::UnsafeValue {
            dimension: name.to_owned(),
            value,
        });
    }
    if value.contains(WILDCARDS) {
        return Some(Reason::WildcardValue {
            dimension: name.to_owned(),
            value,
        });
    }
    let malformed = value.is_empty()
        || dimension
            .separator
            .is_some_and(|separator| value.split(separator).any(str::is_empty))
        || (dimension.uuid && !is_uuid(&value));
    malformed.then(|| Reason::InvalidValue {
        dimension: name.to_owned(),
        value,
    })
}

/// A `lower_snake_case` identifier: a lowercase letter, then lowercase letters,
/// digits and underscores.
fn is_lower_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The canonical hyphenated form: 8-4-4-4-12 hexadecimal digits.
fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// The strings of the service option `extension` on `service`, whether it is
/// repeated or singular; a singular option that is not set yields none.
fn service_strings(
    pool: &DescriptorPool,
    service: &ServiceDescriptor,
    extension: &str,
) -> Vec<String> {
    pool.get_extension_by_name(extension)
        .into_iter()
        .flat_map(
            |extension| match &*service.options().get_extension(&extension) {
                Value::List(items) => items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect::<Vec<_>>(),
                single => single.as_str().map(str::to_owned).into_iter().collect(),
            },
        )
        .filter(|name| !name.is_empty())
        .collect()
}

/// The entries of the repeated service option `extension` on `service`.
fn service_entries(
    pool: &DescriptorPool,
    service: &ServiceDescriptor,
    extension: &str,
) -> Vec<DynamicMessage> {
    let Some(extension) = pool.get_extension_by_name(extension) else {
        return Vec::new();
    };
    let options = service.options();
    let value = options.get_extension(&extension);
    value
        .as_list()
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| entry.as_message().cloned())
        .collect()
}

fn text(message: &DynamicMessage, field: &str) -> String {
    message
        .get_field_by_name(field)
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn number(message: &DynamicMessage, field: &str) -> i32 {
    message
        .get_field_by_name(field)
        .and_then(|value| value.as_enum_number())
        .unwrap_or_default()
}

fn count(message: &DynamicMessage, field: &str) -> u32 {
    message
        .get_field_by_name(field)
        .and_then(|value| value.as_u32())
        .unwrap_or_default()
}

fn strings(message: &DynamicMessage, field: &str) -> Vec<String> {
    message
        .get_field_by_name(field)
        .and_then(|value| {
            value.as_list().map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn messages(message: &DynamicMessage, field: &str) -> Vec<DynamicMessage> {
    message
        .get_field_by_name(field)
        .and_then(|value| {
            value.as_list().map(|list| {
                list.iter()
                    .filter_map(|item| item.as_message().cloned())
                    .collect()
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lower_snake_case() {
        assert!(is_lower_snake_case("departure_assistant"));
        assert!(is_lower_snake_case("a1"));
        for refused in ["", "Departure", "_x", "1x", "a-b", "a b", "a.b"] {
            assert!(!is_lower_snake_case(refused), "{refused:?}");
        }
    }

    #[test]
    fn a_uuid_is_the_hyphenated_form_only() {
        assert!(is_uuid("0190f2a4-7b1c-7d3e-8a52-3f6c1d9e4b70"));
        for refused in [
            "",
            "0190f2a47b1c7d3e8a523f6c1d9e4b70",
            "0190f2a4-7b1c-7d3e-8a52-3f6c1d9e4b7",
            "0190f2a4-7b1c-7d3e-8a52-3f6c1d9e4b7g",
            "0190f2a4_7b1c-7d3e-8a52-3f6c1d9e4b70",
        ] {
            assert!(!is_uuid(refused), "{refused:?}");
        }
    }

    #[test]
    fn a_value_is_judged_by_its_dimensions_match_kind() {
        let exact = Dimension {
            separator: None,
            uuid: false,
        };
        let prefix = Dimension {
            separator: Some('/'),
            uuid: false,
        };
        let uuid = Dimension {
            separator: None,
            uuid: true,
        };
        assert_eq!(check_value("d", &exact, "a/b".into()), None);
        assert_eq!(check_value("d", &prefix, "a/b".into()), None);
        assert!(check_value("d", &prefix, "a//b".into()).is_some());
        assert!(check_value("d", &prefix, "/a".into()).is_some());
        assert!(check_value("d", &exact, String::new()).is_some());
        assert!(check_value("d", &uuid, "not-a-uuid".into()).is_some());
        for wildcard in ["a*", "?", "a/>"] {
            assert_eq!(
                check_value("d", &prefix, wildcard.into()),
                Some(Reason::WildcardValue {
                    dimension: "d".into(),
                    value: wildcard.into(),
                })
            );
        }
    }

    #[test]
    fn a_value_with_whitespace_control_format_or_punctuation_is_unsafe() {
        let exact = Dimension {
            separator: None,
            uuid: false,
        };
        for unsafe_value in [
            "a b",
            "a\nb",
            "a\tb",
            "a\u{0}b",
            "a\"b",
            "a(b",
            "a)b",
            "a,b",
            "a;b",
            "a[b",
            "a]b",
            "a{b",
            "a}b",
            "a$b",
            "a\\b",
            "a\u{200B}b",
            "a\u{202E}b",
            "a\u{2060}b",
            "a\u{2067}b",
            "\u{FEFF}a",
        ] {
            assert_eq!(
                check_value("d", &exact, unsafe_value.into()),
                Some(Reason::UnsafeValue {
                    dimension: "d".into(),
                    value: unsafe_value.into(),
                }),
                "{unsafe_value:?}"
            );
        }
        for safe_value in ["a-b_c.d:e/f", "caf\u{e9}"] {
            assert_eq!(
                check_value("d", &exact, safe_value.into()),
                None,
                "{safe_value:?}"
            );
        }
    }

    #[test]
    fn every_unicode_format_character_is_unsafe_and_a_letter_is_not() {
        let exact = Dimension {
            separator: None,
            uuid: false,
        };
        for format in [
            '\u{00AD}',
            '\u{0600}',
            '\u{061C}',
            '\u{180E}',
            '\u{2066}',
            '\u{206F}',
            '\u{FFFB}',
            '\u{110BD}',
            '\u{1D17A}',
            '\u{E0001}',
            '\u{E0041}',
            '\u{E007F}',
        ] {
            assert!(is_format(format), "{format:?}");
            assert!(is_unsafe(format), "{format:?}");
            let value = format!("a{format}b");
            assert_eq!(
                check_value("d", &exact, value.clone()),
                Some(Reason::UnsafeValue {
                    dimension: "d".into(),
                    value,
                })
            );
        }
        for ordinary in ['\u{e9}', 'a', '\u{4e2d}', '\u{E0080}', '\u{2065}'] {
            assert!(!is_format(ordinary), "{ordinary:?}");
            assert!(!is_unsafe(ordinary), "{ordinary:?}");
        }
    }

    #[test]
    fn a_value_with_a_wildcard_and_a_hostile_character_echoes_nothing() {
        let exact = Dimension {
            separator: None,
            uuid: false,
        };
        for value in ["*\u{1b}[2J", "a*\u{202E}b", "?\n", ">\u{00AD}"] {
            let reason = check_value("d", &exact, value.into()).expect("refused");
            assert_eq!(
                reason,
                Reason::UnsafeValue {
                    dimension: "d".into(),
                    value: value.into(),
                }
            );
            assert!(!reason.to_string().contains(value));
        }
    }

    #[test]
    fn a_refusal_never_prints_a_hostile_template_name_raw() {
        let refusal = Refusal {
            service: "p.v1.S".into(),
            template: "a\u{1b}b\u{202E}c d".into(),
            grant: None,
            reason: Reason::InvalidName,
        };
        let rendered = refusal.to_string();
        assert_eq!(
            rendered,
            "p.v1.S template `a\\u{1b}b\\u{202e}c\\u{20}d`: the name is not lower_snake_case"
        );
        assert!(!rendered.chars().any(|c| c.is_control() || is_format(c)));
    }

    #[test]
    fn a_refusal_names_its_service_template_grant_and_reason() {
        let error = DelegationTemplateError {
            refusals: vec![
                Refusal {
                    service: "p.v1.S".into(),
                    template: "t".into(),
                    grant: Some(1),
                    reason: Reason::UndeclaredScope("x:y".into()),
                },
                Refusal {
                    service: "p.v1.S".into(),
                    template: "t".into(),
                    grant: None,
                    reason: Reason::NoGrants,
                },
            ],
        };
        assert_eq!(
            error.to_string(),
            "p.v1.S template `t` grant 1: the scope `x:y` is not declared by the service\n\
             p.v1.S template `t`: the template has no grant"
        );
    }

    #[test]
    fn every_reason_reads_as_text() {
        let reasons = [
            Reason::InvalidName,
            Reason::DuplicateName,
            Reason::MissingLabel,
            Reason::NoGrants,
            Reason::GrantNamesNothing,
            Reason::UndeclaredScope("s".into()),
            Reason::UndeclaredGroup("g".into()),
            Reason::UndeclaredDimension("d".into()),
            Reason::DuplicateDimension("d".into()),
            Reason::EmptySelector("d".into()),
            Reason::WildcardValue {
                dimension: "d".into(),
                value: "v".into(),
            },
            Reason::InvalidValue {
                dimension: "d".into(),
                value: "v".into(),
            },
            Reason::UnsafeValue {
                dimension: "d".into(),
                value: "v".into(),
            },
            Reason::UnsafeName("n".into()),
            Reason::ApprovalUnspecified,
            Reason::UnknownApprovalMode(7),
            Reason::EmptyBudget,
        ];
        let rendered: BTreeSet<String> = reasons.iter().map(ToString::to_string).collect();
        assert_eq!(rendered.len(), reasons.len());
    }
}
