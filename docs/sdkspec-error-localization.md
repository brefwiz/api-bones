# SDKSpec — error localization

Contract for the `error-localization` SDK surface: Layer 2 rendering of a
refused call in the viewer's locale. Implementation input for the
ImplementationSpec; it owns no implementation, architecture or QA verdict.

## Sources

- Locked decision: brefwiz-architecture `atlas/adr/keys-substrate-plumbing.md`
  § wire-contract-proto-connect decision 9 (ARB-keys-10 ruled A by the CTO,
  2026-11-20), at brefwiz-architecture@f8782a1f503821a796cd0aef0ae6eab19fbc0295
  (worktree `adr-error-i18n-codes-params`); decision 12d (a crate independent
  of api-bones majors, precedent `api-bones-batch`, decision 16).
- CTO carve-out 2026-11-20 (approved, "driven by the English Gherkin corpus"):
  CLDR plural/number/date formatting is native per language; every other
  brefwiz-defined rule has one definition tested exactly in both languages.
- Locale registry and negotiation: core-ui@8e3478d5f8c2e12fb793f66623bf4612c0ea9c33
  `src/i18n/platformLocales.ts` (`PLATFORM_LOCALES`, `ALIASES`, `matchLocale`,
  `negotiateLocale`); catalog and sidecar format `src/i18n/sourceHash.ts`
  (`fnv1a-64/utf8/nfc/lf:v1`); Connect-code map
  `src/api/error-code-mappers.ts` (`connectCodeToErrorCode`); platform strings
  moved from `src/i18n/messages/en.json` `core.errors.*`.
- Wire: api-bones@6326300cfb65439e99f4267307d9020224b2376c
  `api-bones-protos/proto/bones/v1/errors.proto`, `src/connect/error_info.rs`,
  `src/error.rs` (`ErrorCode`, `ApiError`, `ValidationError`).
- Mobile architecture: not applicable. No surface of this repository declares
  Dart or a mobile consumer; Dart, Kotlin and Swift are out of scope and the
  mobile gate does not apply.

## SDKSpec

Status: locked
SPEC revision: the commit that adds this file, whose parent is
6326300cfb65439e99f4267307d9020224b2376c; `SPEC.md` blob
bfbda774c0deb842a4165f428cc561ca89a6323a. The lock is valid only while
`git rev-parse <reviewed commit>:SPEC.md` equals that blob.
SDK targets: [rust, typescript]
SDK surfaces:
  - id: error-localization
    contract: tests/features/error_localization.feature
    targets: [rust, typescript]
    packages: {rust: [api-bones-i18n, api-bones], typescript: ["@brefwiz/api-bones-connect"]}
    canonical_endpoints: []
    canonical_scenarios: [S01 … S26, listed in the parity matrix]

Normalized by `spec-load --sdk-surfaces` at that blob: `languages: [rust,
typescript]`, `ownership: {}`, surface `error-localization` with `owns: []`,
targets rust `delivery: package, packages: [api-bones-i18n, api-bones]` and
typescript `delivery: package, packages: ["@brefwiz/api-bones-connect"]`. The
surface owns no RPC endpoint, so the matrix has no endpoint rows. `api-bones-i18n`
is in `library_crates`.

### C1. API

Rust, crate `api-bones-i18n` (no dependency on `api-bones`, `api-bones-protos`
or `connectrpc`, so it versions independently of api-bones majors):

```rust
pub enum ArgValue { String(String), Int64(i64), Double(f64), Timestamp { seconds: i64, nanos: i32 } }
pub struct Args;                       // checked map, ascending key order
impl Args { pub fn new() -> Self; pub fn insert(&mut self, key: &str, value: ArgValue) -> Result<(), ArgError>; }
pub struct ArgError { pub rule: ArgRule }   // ArgRule::as_str() is the Gherkin rule
pub fn error_args<'a, I: IntoIterator<Item = (&'a str, ArgValue)>>(entries: I) -> Result<Args, ArgError>;  // Args::insert folded in input order
pub fn negotiate_locale(requested: &str) -> &'static str;   // tag or Accept-Language to a platform locale
pub const PLATFORM_LOCALES: [&str; 29];  pub const ALIASES: &[(&str, &str)];  pub const DEFAULT_LOCALE: &str;
pub struct ErrorIdentity { /* platform code, Option<code>, Option<emitter>, args: Result<Args, ArgError> */ }
pub trait Localizable { fn identity(&self) -> ErrorIdentity; }
pub struct LocalizeOptions;            // Default = UTC
impl LocalizeOptions { pub fn time_zone(self, iana: &str) -> Result<Self, LocalizeError>; }
pub struct LocalizeError { pub rule: &'static str }   // "unknown_time_zone"
pub struct Localized { pub text: String, pub hint: Option<String> }
pub fn localize(error: &impl Localizable, locale: &str, options: &LocalizeOptions) -> Localized;
pub fn register_catalog(emitter: &str, locale: &str, json: &str);   // same store as the macro
#[macro_export] macro_rules! error_catalogs { ($emitter:literal) => { /* include! of the build table + inventory::submit! */ } }
// api_bones_i18n::build::emit_error_catalogs("<dir relative to CARGO_MANIFEST_DIR>"), called from the emitter's build.rs
```

The emitter's `build.rs` calls `api_bones_i18n::build::emit_error_catalogs(dir)`:
it scans `<dir>/*.json` (files whose stem holds a `.` are sidecars, not
catalogs), writes the sorted `(locale, include_str!(absolute path))` table to
`OUT_DIR`, prints `cargo:rerun-if-changed=<dir>` and fails the build when
`en.json` is missing. `error_catalogs!("<emitter>")` in the library submits that
table; adding a locale is a new file and no code.

Rust, crate `api-bones` (feature `i18n`, depends on `api-bones-i18n`):
`api_bones::i18n::Localizable` (an `api_bones_i18n::Localizable` widened to the
foreign `connectrpc::ConnectError`, which the orphan rule keeps `api-bones-i18n`
from implementing) for `ConnectError`, `ApiError`, `ProblemJson`,
`ValidationFailureDetail` (Connect detail codec), `Violation` and
`ErrorIdentity`, with `api_bones::i18n::localize` taking it; `ApiError`,
`ProblemJson` and `ValidationFailureDetail` also implement
`api_bones_i18n::Localizable`. `pub fn violations_of(error: &impl
HasViolations) -> Vec<Violation>` (trait `HasViolations`, in `api_bones::i18n`)
for `ConnectError`, `ApiError` and `ProblemJson`; the Connect-only
`api_bones::connect::violations_of` keeps returning the wire messages; `ErrorInfoMember::with_arg` and
`ValidationError::with_arg(self, key, impl Into<ErrorArg>) -> Result<Self, ErrorArgError>` keep their released
signature (C8: two construction layers); `ValidationError` (problem+json) and
`ValidationFailure` gain `args`; re-export `api_bones::i18n::{localize, LocalizeOptions, Localized}`.

TypeScript, `@brefwiz/api-bones-connect` (module `src/error-localization.ts`,
exported from the package root, tree-shakable):

```ts
type ErrorArgValue =
  | { kind: "string"; value: string } | { kind: "int64"; value: bigint }
  | { kind: "double"; value: number } | { kind: "timestamp"; seconds: bigint; nanos: number };
function errorArgs(entries: Readonly<Record<string, ErrorArgValue>>): ErrorArgs;   // throws ErrorArgError
class ErrorArgError extends Error { readonly rule: ErrorArgRule }
class LocalizeOptionsError extends Error { readonly rule: "unknown_time_zone" | "locale_not_loaded" }
type Localizable = ConnectError | ProblemJson | ValidationFailure | Violation;
function negotiateLocale(requested: string): string;   // tag or Accept-Language to a platform locale
const PLATFORM_LOCALES: readonly string[];  const LOCALE_ALIASES: Readonly<Record<string, string>>;  const DEFAULT_LOCALE: string;
function loadErrorCatalog(locale: string): Promise<void>;   // makes a platform locale renderable; en and unmatched tags are always ready
function localize(error: Localizable, locale: string, options?: { timeZone?: string }): { text: string; hint?: string };
function violationsOf(error: ConnectError | ProblemJson): Violation[];   // exported as the namespace `i18n.violationsOf`
interface Violation { readonly field: string; readonly rule: string; readonly message: string; readonly args: ErrorArgs | ErrorArgError }
type ErrorCatalogLoader = () => Promise<{ readonly default: Readonly<Record<string, string>> }>;   // a dynamic import() of one catalog
function registerErrorCatalogs(emitter: string, catalogs: Readonly<Record<string, Readonly<Record<string, string>> | ErrorCatalogLoader>>): void;   // each locale: entries, or a loader fetched by loadErrorCatalog
```

`ProblemJson` is the parsed body (`unknown` narrowed to an object with a
`type` or `status` member). Input discrimination order: `ConnectError`
instance, `Violation` brand, generated `bones.v1.ValidationFailure`
(`$typeName`), else problem+json. The canonical entry is `i18n.violationsOf` (TypeScript, `export * as i18n`) /
`api_bones::i18n::violations_of` (Rust); the root `violationsOf` / `violations_of` is the released 0.12
ConnectError accessor, unchanged and outside C1. `localize` and `i18n.violationsOf` are
synchronous, pure and do no I/O in both languages; `localize` never throws
except the time-zone refusal and, in TypeScript only, `locale_not_loaded`: the
TypeScript base module ships `en` and every other platform locale is a chunk that
`loadErrorCatalog(locale)` (a tag or Accept-Language value, negotiated like
`localize`) fetches once and shares between concurrent calls. `localize` for a
platform locale other than `en` whose chunk has not loaded throws
`LocalizeOptionsError` with rule `locale_not_loaded` rather than render English
by mistake; a tag no platform locale matches negotiates to `en` and renders.
`loadErrorCatalog` rejects only when a chunk fails to load and a later call
retries. One `loadErrorCatalog(locale)` call is all a page makes per locale: it
fetches api-bones's chunk and the chunk of every emitter registered with a loader
for that locale, and `en` of each such emitter (the end of the fallback chain),
sharing concurrent calls; an emitter registered after the locale loaded is
fetched by the next call. `localize` of an error whose emitter has a registered
loader not yet loaded for the viewer locale or `en` throws `locale_not_loaded`. Rust embeds every locale and has no counterpart. Rust `int64` is `i64`, so a TypeScript `bigint`
outside the int64 range is refused by `errorArgs` with rule `int64_range` — an
input Rust cannot represent, not a capability difference.

### C2. Identity extraction

- Connect error: platform code from the Connect code (C3 map); `code` and
  `emitter` from the first `bones.v1.ErrorInfo` detail; args from its
  `args` (field 4). Empty emitter = unstamped.
- problem+json: `error_info` member `{code, emitter, args}` (C9). Platform
  code: the `ErrorCode` whose slug equals the last segment of `type` after
  the final `:` or `/` (any `error_type_mode`); else the first `ErrorCode`
  variant, in `src/error.rs` declaration order, whose HTTP status equals
  `status`; else `BAD_REQUEST` for an unlisted 4xx and `INTERNAL_SERVER_ERROR`
  otherwise. `detail` and `title` are never read.
- ValidationFailure / Violation: `rule`, `field`, `args`; Connect violations
  are the `bones.v1.ValidationFailure` details in detail order, problem+json
  violations are `errors[]` in array order.
- An identity whose args fail C8 bounds or C9 decoding is rendered as if it
  carried no ErrorInfo (Connect/problem+json) or no own string (violation).

### C3. Resolution chain

Keys, tried in order; each key is tried in the negotiated locale, then `en`,
before the next key:

1. `<emitter>.errors.<code>` — only when both are present;
2. `bones.errors.<code>` — only when `code` is present;
3. `bones.errors.<PLATFORM>` — the platform code of C2.

A violation: `bones.validation.<rule>` (only when `rule` is non-empty), then
`bones.errors.VALIDATION_FAILED`. Codes and rules match byte-exactly (case
sensitive). Platform codes are the SCREAMING_SNAKE form of `ErrorCode`
variants (`ResourceNotFound` → `RESOURCE_NOT_FOUND`). Connect → platform map
(core-ui `connectCodeToErrorCode`, verbatim): unauthenticated → UNAUTHORIZED,
permission_denied → FORBIDDEN, not_found → RESOURCE_NOT_FOUND, already_exists
→ RESOURCE_ALREADY_EXISTS, resource_exhausted → RATE_LIMITED,
failed_precondition → PRECONDITION_FAILED, invalid_argument →
VALIDATION_FAILED, unavailable → SERVICE_UNAVAILABLE, deadline_exceeded →
GATEWAY_TIMEOUT, unimplemented → NOT_IMPLEMENTED, aborted → CONFLICT,
out_of_range → UNPROCESSABLE_ENTITY, every other code → INTERNAL_SERVER_ERROR.
Step 3 always resolves: every platform code has an en `title` without
placeholders, so the raw code and the operator `message`/`detail` are never
rendered.

### C4. Catalog entries

A key resolves in a locale when `<key>.title` exists in that locale's catalog,
parses inside the C6 subset and every placeholder has a type-compatible
argument; otherwise the chain continues. `text` = that rendered title; `hint`
= `<key>.description` of the same key and locale, rendered with the same
arguments, absent when missing or not renderable. Extra arguments are ignored.
English strings for the platform codes are core-ui's `core.errors.<x>.title`/
`.description` moved to `bones.errors.<PLATFORM>.*`; `core.errors.fallback`
is retired (step 3 is total). New: `bones.validation.max.title` = "Must be at
most {max, number}." Further `bones.validation.<rule>` strings follow the
`bones.v1.ValidationFailure.rule` vocabulary and ride the `error-catalog` check.

### C5. Locale negotiation

`locale` is a BCP 47 tag or an Accept-Language value; a tag is a one-entry
list. Semantics are core-ui `negotiateLocale(locale)` with no `supported`
restriction and fallback `en`, over the 29 `PLATFORM_LOCALES`, with
`matchLocale`: trim, `_` → `-`, case-insensitive; exact code, else alias
(`no`, `nn` → nb; `iw` → he; `pt` → pt-BR; `zh`, `zh-cn`, `zh-sg` → zh-Hans),
else language subtag (through the alias table); `zh-*` selects zh-Hans only
when a subtag is `hans`; `*` and empty select nothing. Entries sorted by
descending `q`, ties in header order, `q=0` refused. The negotiated locale is
final: a catalog without the key falls back to `en`, never to the list's next
entry. Rust ports the registry and both functions; TypeScript implements them
in the package (no core-ui dependency). Both assert the 29 codes, alias table
and default equal the committed copy of core-ui's registry,
`tests/features/fixtures/error_localization/platform_locales.json` (S25).

### C6. MessageFormat subset

ICU MessageFormat with ICU apostrophe quoting (`''` → `'`; `'{…}'` literal),
tags as literal text (TS `ignoreTag: true`). Supported placeholders and the
argument kinds each accepts:

| Placeholder | Accepts |
|---|---|
| `{x}` | string |
| `{x, number}`, `{x, number, percent}` | int64, double |
| `{x, plural, =N {…} zero/one/two/few/many/other {…}}` with `#`, `offset:` refused | int64, double |
| `{x, select, <word> {…} other {…}}` | string (byte-exact branch match) |
| `{x, date, short\|medium\|long\|full}` | timestamp |
| `{x, time, short\|medium}` | timestamp |

Anything else (selectordinal, `::` skeletons, other styles, `spellout`,
`duration`, unbalanced braces, a plural/select without `other`) does not
parse; the entry is unresolved. Rust uses its own parser, shared with the
`error-catalog` check; TypeScript parses with `intl-messageformat`'s parser and
rejects any AST node outside this table before formatting. Argument values are
substituted as values and never parsed as patterns.

### C7. Formatting (native per language)

Rust: ICU4X 2.x (`icu_plurals`, `icu_decimal`, `icu_datetime`) with `jiff`
for the time zone; TypeScript: `intl-messageformat` with `ignoreTag: true` and
explicit `dateStyle`/`timeStyle` formats. Numbers: grouping on, at most 3
fraction digits (percent: 0), rounding half away from zero (ICU4X
`HalfExpand`, Intl default). Plural operands are exact: TypeScript passes an
int64 within ±(2^53−1) as `number`, otherwise as `bigint`, and selects the
plural category from the exact decimal; Rust uses `i64`/`f64` operands.
Dates/times: the four date styles and two time styles of C6 only, rendered in
`options.timeZone` (IANA name), default `UTC`; an unknown or empty zone is
refused with rule `unknown_time_zone` before any lookup. Equality: plural and
select branch and argument substitution exact; numbers and dates exact after
every Unicode `White_Space` character becomes U+0020. CLDR pin: the file
`api-bones-i18n/cldr.json` records one CLDR release per runtime, as
`{"rust": "...", "node": "..."}`: the Rust contract asserts the `[package.metadata.sources.cldr] tagged`
of every `icu_*_data` package the crate compiles (read with `cargo metadata`)
equals the `rust` entry, the TypeScript contract asserts
`process.versions.cldr` equals the `node` entry (S26). The `node` entry is read from the CI image's Node 24.21.0 (`process.versions.cldr` = 48.0, ICU 78.3).

### C8. Argument bounds (typed, one definition)

Checked on construction (Rust `api_bones_i18n::Args::insert`, returning
`ArgError { rule: ArgRule }`, and `api_bones::error::ErrorArgError` through
`with_arg`/`validate_arg`/`validate_args`, whose `rule()` returns the same
snake_case rule names below; TS `errorArgs`) and on every decode; never truncated. Order of checks: the count
(> 32 → `too_many_args`), then each argument in ascending key byte order: key
`^[a-z][a-z0-9_]{0,63}$` (else `arg_key_format`); string at most 1024 UTF-8
bytes (`string_too_long`), no code point of general category Cc nor U+061C,
U+200E, U+200F, U+202A–U+202E, U+2066–U+2069 (`string_forbidden_char`);
double finite (`non_finite_double`); timestamp seconds within
[-62135596800, 253402300799] and nanos within [0, 999999999]
(`timestamp_range`). First failing rule wins. TypeScript only: a `bigint`
outside int64 → `int64_range`.

### C9. Wire and proto3 JSON mapping

`bones.v1.ErrorInfo` gains `map<string, ErrorArg> args = 4`;
`bones.v1.ValidationFailure` gains `map<string, ErrorArg> args = 4`;
`message ErrorArg { oneof value { string string_value = 1; int64 int64_value
= 2; double double_value = 3; google.protobuf.Timestamp timestamp_value = 4; } }`,
field numbers pinned by the Gherkin wire bytes. problem+json: `error_info`
`{code, emitter, args}` and `errors[].args`, each argument an object with
exactly one of `stringValue` (string), `int64Value` (decimal string
`^-?(0|[1-9][0-9]*)$` within int64; a JSON number is malformed),
`doubleValue` (finite JSON number; `"NaN"`/`"Infinity"` strings are
malformed), `timestampValue` (RFC 3339 with `Z` or offset). Any other shape
makes the whole `args` malformed (C2 last rule).

### C10. Packaging and registry

- api-bones's catalogs: `api-bones-i18n/catalogs/errors/<locale>.json` for
  the 29 platform locales plus `<locale>.sources.json` sidecars for every
  non-en locale (`fnv1a-64/utf8/nfc/lf:v1` of the en source per key), keys
  `bones.errors.*` and `bones.validation.*`. The TypeScript package ships the
  same files, copied at build from that one source: the base module carries
  `en` and every other locale is a lazily imported chunk loaded by
  `loadErrorCatalog`, so a page that renders English carries no other
  language. Rust embeds all 29 at compile time.
- `bones.*` keys resolve only from the rendering package's own embedded
  catalogs, never from the registry; a registration for emitter `bones` is
  ignored.
- Emitter catalogs: Rust `api_bones_i18n::build::emit_error_catalogs("<dir>")` (build
  script) plus `api_bones_i18n::error_catalogs!("<emitter>")` embed each present
  `<dir>/<locale>.json` at compile time and submit it through `inventory`, into
  the store `register_catalog` writes; TypeScript `registerErrorCatalogs` writes to
  `globalThis[Symbol.for("@brefwiz/api-bones-connect/error-catalogs/v1")]`, so
  every copy of the package in one realm shares it. A locale may be registered as
  a loader instead of entries (`ErrorCatalogLoader`, a dynamic `import()`):
  `loadErrorCatalog` fetches it and registers its entries as above, so the same
  no-op and conflict rules apply to eager and lazy registrations alike, a
  loader whose module is not a catalog object rejects with a `TypeError`, and
  which catalogs are loaded or pending is kept on
  `globalThis[Symbol.for("@brefwiz/api-bones-connect/error-catalog-loading/v1")]`,
  shared like the registry. Rust registers at compile time and has no lazy form. A catalog contributes only
  keys prefixed `<emitter>.errors.`; others are ignored. Registering the same
  (emitter, locale) entries again is a no-op; a second registration with
  different entries makes that (emitter, locale) unusable for both, order
  independent. Locale file names must be platform codes; others are ignored.
- core-ui's `core.errors.*` keys move to `bones.errors.*` (core-ui work,
  outside this surface).

### C11. Layer boundary

Layer 1 is the wire contract `api-bones-protos/proto/bones/v1/errors.proto`.
TypeScript decodes it with the protoc-gen-es output
`api-bones-connect-ts/src/generated/bones/v1/errors_pb.ts` (regenerated by its
`buf.gen.yaml`); Rust decodes with the `src/connect/error_info.rs` codec, which
the `errors_proto_declares_error_info` field-lock test in
`api-bones-protos/src/lib.rs` binds to the proto (extended to `args = 4` and
`ErrorArg`), as consumer-generated Rust types (the `tests/rust/build.rs`
`bones/v1/errors.proto` output) encode the unchecked Gherkin inputs. Layer 2
is C1, hand-written, idiomatic per language: `Result` and snake_case in Rust,
exceptions only for construction/options refusals and camelCase in
TypeScript. Auth, pagination, retry/timeout and streaming do not apply: the
surface performs no call.

### C12. Drift and codegen failure

Both lanes fail (never skip) on: a missing generator or generated root
(`errors_pb.ts` absent or stale against `errors.proto` per the existing
`generated_paths` drift check; `tests/rust/build.rs` output missing); an
undefined, pending or skipped step; zero scenarios discovered in
`tests/features/error_localization.feature`; a fixture file missing; a
platform code without an en `title`; the registry differing from
`platform_locales.json` (S25); the CLDR pin mismatch (S26).
Unsupported target: any language other than rust or typescript added to this
surface without its own matrix rows fails the lock.

### Parity matrix

Every row: Complete true, Deferred false, Partial false. Auth, Pagination,
Retry/timeout and Streaming are `n/a — no call` and Async is `sync` for every
row. Naming: rust `localize`/`violations_of`/`Args::insert`/`error_catalogs!`,
typescript `localize`/`i18n.violationsOf`/`errorArgs`/`registerErrorCatalogs`/`loadErrorCatalog`. L1
root: rust `errors.proto` via field-locked codec, typescript
`generated/bones/v1/errors_pb.ts`. L2 surface: rust `api_bones_i18n` +
`api_bones::i18n`, typescript `@brefwiz/api-bones-connect`. Proof: rust
`cargo test --locked -p api-bones-contract-rust --test error_localization`,
typescript `make ts-test` (feature listed in `tests/typescript/cucumber.cjs`).

| Surface | Target | Kind | Contract ID | Complete | Deferred | Partial | Behavior | Typed errors | Auth | Async | Pagination | Retry/timeout | Streaming | Naming | L1 root | L2 surface | Proof |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| error-localization | rust | scenario | S01 A key resolves in the viewer's locale, then English, before the next key | true | false | false | C3, C4: key-major chain, viewer then en, hint from same key+locale | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S01 A key resolves in the viewer's locale, then English, before the next key | true | false | false | C3, C4: key-major chain, viewer then en, hint from same key+locale | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S02 An error without ErrorInfo renders the platform code of its Connect code | true | false | false | C3: Connect map verbatim, step 3 total | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S02 An error without ErrorInfo renders the platform code of its Connect code | true | false | false | C3: Connect map verbatim, step 3 total | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S03 The viewer's locale is negotiated over the platform locales | true | false | false | C5: negotiateLocale/matchLocale, aliases, zh-Hans, fallback en | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S03 The viewer's locale is negotiated over the platform locales | true | false | false | C5: negotiateLocale/matchLocale, aliases, zh-Hans, fallback en | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S04 Plural branches and numbers follow the viewer's locale | true | false | false | C6, C7: plural =N and CLDR category, exact operands | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S04 Plural branches and numbers follow the viewer's locale | true | false | false | C6, C7: plural =N and CLDR category, exact operands | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S05 An int64 beyond 2^53 renders exactly | true | false | false | C7: int64 exact beyond 2^53 (TS bigint path) | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S05 An int64 beyond 2^53 renders exactly | true | false | false | C7: int64 exact beyond 2^53 (TS bigint path) | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S06 Select branches match the string argument exactly | true | false | false | C6: select byte-exact, other | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S06 Select branches match the string argument exactly | true | false | false | C6: select byte-exact, other | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S07 Numbers round half away from zero | true | false | false | C7: grouping, max 3 fraction digits, percent 0, half away from zero | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S07 Numbers round half away from zero | true | false | false | C7: grouping, max 3 fraction digits, percent 0, half away from zero | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S08 A timestamp renders in a fixed date or time style | true | false | false | C6, C7: four fixed date styles, time medium | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S08 A timestamp renders in a fixed date or time style | true | false | false | C6, C7: four fixed date styles, time medium | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S09 A timestamp renders in the requested time zone, UTC by default | true | false | false | C7: options.timeZone IANA, default UTC | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S09 A timestamp renders in the requested time zone, UTC by default | true | false | false | C7: options.timeZone IANA, default UTC | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S10 An unknown time zone is refused | true | false | false | C7: zone checked before lookup | LocalizeError / LocalizeOptionsError rule unknown_time_zone | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S10 An unknown time zone is refused | true | false | false | C7: zone checked before lookup | LocalizeError / LocalizeOptionsError rule unknown_time_zone, locale_not_loaded | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S11 Arguments are values, never patterns | true | false | false | C6: args are values; ignoreTag; apostrophe quoting | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S11 Arguments are values, never patterns | true | false | false | C6: args are values; ignoreTag; apostrophe quoting | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S12 An entry that cannot render with the error's arguments falls through to the next key | true | false | false | C4, C6: type table, subset, unresolved falls through | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S12 An entry that cannot render with the error's arguments falls through to the next key | true | false | false | C4, C6: type table, subset, unresolved falls through | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S13 The SDK builds only arguments within bounds | true | false | false | C8: inclusive bounds accepted | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::error_args | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S13 The SDK builds only arguments within bounds | true | false | false | C8: inclusive bounds accepted | none | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect errorArgs | `make ts-test` |
| error-localization | rust | scenario | S14 The SDK refuses an argument out of bounds with a typed rule | true | false | false | C8: rule order, first failure wins | ArgError / ErrorArgError rule too_many_args, arg_key_format, string_too_long, string_forbidden_char, non_finite_double, timestamp_range | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::error_args | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S14 The SDK refuses an argument out of bounds with a typed rule | true | false | false | C8: rule order, first failure wins | ArgError / ErrorArgError rule too_many_args, arg_key_format, string_too_long, string_forbidden_char, non_finite_double, timestamp_range | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect errorArgs | `make ts-test` |
| error-localization | rust | scenario | S15 A received error with arguments out of bounds renders as if it had no ErrorInfo | true | false | false | C2, C8: decode-side bounds, render as no ErrorInfo | none (no throw) | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S15 A received error with arguments out of bounds renders as if it had no ErrorInfo | true | false | false | C2, C8: decode-side bounds, render as no ErrorInfo | none (no throw), except locale_not_loaded | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S16 Wire bytes written by another implementation render | true | false | false | C9: ErrorInfo args=4, ErrorArg int64=2 | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S16 Wire bytes written by another implementation render | true | false | false | C9: ErrorInfo args=4, ErrorArg int64=2 | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S17 A timestamp argument written by another implementation renders | true | false | false | C9: ErrorArg timestamp=4 | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S17 A timestamp argument written by another implementation renders | true | false | false | C9: ErrorArg timestamp=4 | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S18 A Connect error's validation failures render one by one | true | false | false | C2, C3: violationsOf detail order, bones.validation.<rule> | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S18 A Connect error's validation failures render one by one | true | false | false | C2, C3: violationsOf detail order, bones.validation.<rule> | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S19 A bare ValidationFailure renders | true | false | false | C2, C3: bare ValidationFailure input | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S19 A bare ValidationFailure renders | true | false | false | C2, C3: bare ValidationFailure input | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S20 A validation failure without its own string renders the platform validation text | true | false | false | C3, C8: rule then VALIDATION_FAILED | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S20 A validation failure without its own string renders the platform validation text | true | false | false | C3, C8: rule then VALIDATION_FAILED | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S21 A problem+json body renders from its error_info member, else its type, else its status | true | false | false | C2, C9: error_info, type slug, status map | none (malformed args ignored) | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S21 A problem+json body renders from its error_info member, else its type, else its status | true | false | false | C2, C9: error_info, type slug, status map | none (malformed args ignored), except locale_not_loaded | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S22 A problem+json body's validation errors render one by one | true | false | false | C2: errors[] array order | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S22 A problem+json body's validation errors render one by one | true | false | false | C2: errors[] array order | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S23 A catalog registered twice with different entries is not used for that locale | true | false | false | C10: conflicting (emitter, locale) unusable, order independent | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S23 A catalog registered twice with different entries is not used for that locale | true | false | false | C10: conflicting (emitter, locale) unusable, order independent | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S24 A registered catalog supplies only its own emitter's keys | true | false | false | C10: prefix-only keys, bones never from registry | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S24 A registered catalog supplies only its own emitter's keys | true | false | false | C10: prefix-only keys, bones never from registry | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization | rust | scenario | S25 The platform locales and aliases are core-ui's | true | false | false | C5, C12: registry equals committed core-ui copy | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::negotiate_locale | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S25 The platform locales and aliases are core-ui's | true | false | false | C5, C12: registry equals committed core-ui copy | none | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect negotiateLocale | `make ts-test` |
| error-localization | rust | scenario | S26 Formatting data is the pinned CLDR release | true | false | false | C7, C12: CLDR pin file | none | n/a — no call | sync | n/a | n/a | n/a | snake_case, Result | errors.proto via field-locked codec | api_bones_i18n::localize, api_bones::i18n::violations_of | `cargo test --locked -p api-bones-contract-rust --test error_localization` |
| error-localization | typescript | scenario | S26 Formatting data is the pinned CLDR release | true | false | false | C7, C12: CLDR pin file | LocalizeOptionsError rule locale_not_loaded (locale not loaded) | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog, i18n.violationsOf | `make ts-test` |
| error-localization-loading | rust | scenario | S27 Before a catalog loads, only English and an unmatched tag render | true | false | false | n/a — Rust embeds every locale at compile time | none | n/a — no call | sync | n/a | n/a | n/a | n/a | n/a | n/a | n/a — Rust embeds every locale at compile time |
| error-localization-loading | typescript | scenario | S27 Before a catalog loads, only English and an unmatched tag render | true | false | false | C1, C10: en built in, other locales refused until loaded | LocalizeOptionsError rule locale_not_loaded | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog | `make ts-test` |
| error-localization-loading | rust | scenario | S28 A locale renders after its catalog is loaded | true | false | false | n/a — Rust embeds every locale at compile time | none | n/a — no call | sync | n/a | n/a | n/a | n/a | n/a | n/a | n/a — Rust embeds every locale at compile time |
| error-localization-loading | typescript | scenario | S28 A locale renders after its catalog is loaded | true | false | false | C1, C10: loadErrorCatalog fetches the locale's chunk | none | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize, loadErrorCatalog | `make ts-test` |
| error-localization-loading | rust | scenario | S30 Loading one locale loads every registered service's catalog | true | false | false | n/a — Rust registers catalogs at compile time, `register_catalog` is eager | none | n/a — no call | sync | n/a | n/a | n/a | n/a | n/a | n/a | n/a — Rust registers catalogs at compile time |
| error-localization-loading | typescript | scenario | S30 Loading one locale loads every registered service's catalog | true | false | false | C1, C10: one loadErrorCatalog call loads bones and every registered loader | none | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect registerErrorCatalogs, localize, loadErrorCatalog | `make ts-test` |
| connect-tree-shaking | rust | scenario | S29 Importing the error renderer carries English and no other platform catalog | true | false | false | n/a — Rust embeds every locale at compile time | none | n/a — no call | sync | n/a | n/a | n/a | n/a | n/a | n/a | n/a — Rust embeds every locale at compile time |
| connect-tree-shaking | typescript | scenario | S29 Importing the error renderer carries English and no other platform catalog | true | false | false | C10: other locales are lazily imported chunks | none | n/a — no call | sync | n/a | n/a | n/a | camelCase, throws on refusal only | generated/bones/v1/errors_pb.ts | @brefwiz/api-bones-connect localize | `make ts-test` |

Open questions: 0
Contradictions: 0
Deferred cells: 0
Partial cells: 0
