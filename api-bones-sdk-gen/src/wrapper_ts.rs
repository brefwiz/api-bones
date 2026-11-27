// SPDX-License-Identifier: MIT
//! `wrapper typescript`: the TypeScript package that fronts the wasm build of
//! a Layer 2 crate.
//!
//! Every file is generated from the crate's `#[record]`, `#[enumeration]`,
//! `#[error]` and `#[client]` items and the SPEC's `l2_methods`; nothing in the
//! package is written by hand. The package holds no decision logic: a method
//! marshals its arguments to JSON, calls the wasm export and revives the JSON
//! it answers, and the list, stream and batch shapes come from the runtime
//! package.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow};
use api_bones_sdk_model::{
    Export, HandleKind, L2Methods, Model, Param, Record, Return, SpecL2Methods, WireType,
    export_name, json_name, l2_methods, typescript_name,
};

/// What the SPEC front matter and the crate's manifest say about the package.
/// The manifest is resolved by `cargo metadata`, so workspace-inherited fields
/// carry their value. `out` is the directory the package is written to; the
/// runtime dependency link is relative to it.
///
/// # Errors
///
/// A SPEC without front matter, without the surface, or without a TypeScript
/// package for it; a manifest `cargo metadata` rejects or that declares no
/// license; an `out` path that cannot be resolved.
pub(crate) fn package_of(
    spec: &Path,
    krate: &Path,
    surface: &str,
    out: &Path,
) -> anyhow::Result<Package> {
    let text =
        std::fs::read_to_string(spec).with_context(|| format!("cannot read {}", spec.display()))?;
    let front = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---").next())
        .ok_or_else(|| anyhow!("{} has no front matter", spec.display()))?;
    let document: serde_yaml::Value = serde_yaml::from_str(front)?;
    let service = document["service"]
        .as_str()
        .ok_or_else(|| anyhow!("the SPEC names no service"))?
        .to_owned();
    let surfaces = document["sdk_surfaces"]
        .as_mapping()
        .ok_or_else(|| anyhow!("the SPEC lists no sdk_surfaces"))?;
    let mut spec_l2 = SpecL2Methods::default();
    for (id, _) in surfaces {
        spec_l2
            .surfaces
            .insert(id.as_str().unwrap_or_default().to_owned());
    }
    let entry = &document["sdk_surfaces"][surface];
    if let Some(endpoints) = entry["l2_methods"].as_mapping() {
        for (endpoint, languages) in endpoints {
            let rust = languages["rust"]
                .as_sequence()
                .into_iter()
                .flatten()
                .filter_map(|m| m.as_str().map(str::to_owned))
                .collect();
            spec_l2
                .endpoints
                .insert(endpoint.as_str().unwrap_or_default().to_owned(), rust);
        }
    }
    let name = entry["targets"]["typescript"]["packages"][0]
        .as_str()
        .ok_or_else(|| anyhow!("surface `{surface}` names no TypeScript package"))?
        .to_owned();
    let manifest_path = krate.join("Cargo.toml");
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest_path)
        .output()
        .context("cannot start `cargo metadata`")?;
    if !output.status.success() {
        return Err(anyhow!(
            "`cargo metadata` failed for {}: {}",
            manifest_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let manifest_path = manifest_path
        .canonicalize()
        .with_context(|| format!("cannot read the manifest of {}", krate.display()))?;
    let found = crate_metadata(&String::from_utf8_lossy(&output.stdout), &manifest_path)?;
    let (runtime, registry) = runtime_dependency(spec.parent().unwrap_or(Path::new("")), out)?;
    if !found.private && registry.is_none() {
        return Err(anyhow!(
            "a published wrapper publishes to its runtime's registry; the in-tree runtime package.json names no publishConfig.registry"
        ));
    }
    Ok(Package {
        service,
        crate_name: found.name,
        name,
        version: found.version,
        license: found.license,
        private: found.private,
        runtime,
        registry,
        spec: spec_l2,
    })
}

/// What the Layer 2 crate's manifest resolves to, workspace inheritance
/// included.
#[derive(Debug, PartialEq, Eq)]
struct CrateMetadata {
    name: String,
    version: String,
    license: String,
    /// The crate says `publish = false`.
    private: bool,
}

/// The package of `manifest` in the `cargo metadata --no-deps` document
/// `json`.
fn crate_metadata(json: &str, manifest: &Path) -> anyhow::Result<CrateMetadata> {
    let document: serde_json::Value =
        serde_json::from_str(json).context("`cargo metadata` did not answer JSON")?;
    let package = document["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| package["manifest_path"].as_str().map(Path::new) == Some(manifest))
        .ok_or_else(|| {
            anyhow!(
                "`cargo metadata` lists no package for {}",
                manifest.display()
            )
        })?;
    let text = |key: &str| package[key].as_str().map(str::to_owned);
    Ok(CrateMetadata {
        name: text("name").ok_or_else(|| anyhow!("the manifest names no crate"))?,
        version: text("version").ok_or_else(|| anyhow!("the manifest declares no version"))?,
        license: text("license")
            .ok_or_else(|| anyhow!("the manifest of {} declares no license", manifest.display()))?,
        private: package["publish"]
            .as_array()
            .is_some_and(std::vec::Vec::is_empty),
    })
}

/// The directory of the generated sources inside the package; the manifest's
/// entry fields point at its `dist` twin.
pub(crate) const SOURCE_DIR: &str = "src/generated-l2";

/// The declaration surface of the generated clients: the one source file the
/// package tracks, at the package root.
pub(crate) const SURFACE_FILE: &str = "api-surface.d.ts";

/// What the generator is told about the package it writes.
pub(crate) struct Package {
    /// The SPEC `service`.
    pub(crate) service: String,
    /// The Layer 2 crate, which names the wasm file.
    pub(crate) crate_name: String,
    /// The npm package name.
    pub(crate) name: String,
    /// The Layer 2 crate's version.
    pub(crate) version: String,
    /// The Layer 2 crate's SPDX license, published with the package.
    pub(crate) license: String,
    /// Private when the Layer 2 crate does not publish.
    pub(crate) private: bool,
    /// The runtime spec: a `file:` link when built inside the api-bones tree (the
    /// manifest keeps it as a dev dependency), the floor release otherwise (the
    /// manifest's peer range).
    pub(crate) runtime: String,
    /// The registry the in-tree runtime package publishes to; a published
    /// wrapper publishes to the same one.
    pub(crate) registry: Option<String>,
    /// The `sdk_surfaces` the exports name, and each endpoint's Rust methods.
    pub(crate) spec: SpecL2Methods,
}

/// One generated file: its path under the package directory and its text.
pub(crate) type File = (String, String);

const HEADER: &str =
    "// SPDX-License-Identifier: MIT\n// Generated by api-bones-sdk-gen; do not edit.\n";

/// Every file of the package, in path order.
///
/// # Errors
///
/// A name that cannot be spelled, an export no `l2_methods` entry lists, or a
/// type the model does not hold.
pub(crate) fn generate(model: &Model, package: &Package) -> anyhow::Result<Vec<File>> {
    let document = l2_methods(
        model,
        &package.spec,
        &package.service,
        &package.crate_name,
        &package.name,
    )?;
    let glue = package.crate_name.replace('-', "_");
    let options = client_options(model)?;
    let mut files = vec![
        (
            "package.json".to_owned(),
            manifest(package, &glue, &package.runtime),
        ),
        ("tsconfig.json".to_owned(), tsconfig()),
        ("l2-methods.json".to_owned(), document.to_json()),
        (format!("{SOURCE_DIR}/types.ts"), types_ts(model, &options)),
        (format!("{SOURCE_DIR}/errors.ts"), errors_ts(model)),
        (format!("{SOURCE_DIR}/init.ts"), init_ts(package, &glue)),
        (
            format!("{SOURCE_DIR}/client.ts"),
            client_ts(model, package, &document, &glue)?,
        ),
        (format!("{SOURCE_DIR}/index.ts"), index_ts(model)),
        (SURFACE_FILE.to_owned(), surface_dts(model)),
    ];
    files.sort();
    Ok(files)
}

/// Writes `files` under `out`.
///
/// # Errors
///
/// An I/O error.
pub(crate) fn write(out: &Path, files: &[File]) -> anyhow::Result<()> {
    for (path, text) in files {
        let target = out.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, text)
            .with_context(|| format!("cannot write {}", target.display()))?;
    }
    Ok(())
}

/// The npm name of the runtime package every wrapper imports.
pub(crate) const RUNTIME_PACKAGE: &str = "@brefwiz/api-bones-sdk-runtime";

/// The lowest runtime release the emitted code runs against. Raised when the
/// emitted code uses a runtime export a lower release lacks.
pub(crate) const RUNTIME_FLOOR: &str = "0.2.0";

/// The runtime dependency of the package written to `out`: a `file:` link to
/// the runtime package next to the SPEC when `spec_dir` holds it, the floor
/// release otherwise. The second value is the `publishConfig.registry` of that
/// in-tree runtime package, when it names one.
fn runtime_dependency(spec_dir: &Path, out: &Path) -> anyhow::Result<(String, Option<String>)> {
    let home = spec_dir.join("api-bones-sdk-runtime-ts");
    let in_tree = std::fs::read_to_string(home.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|manifest| manifest["name"] == RUNTIME_PACKAGE);
    let Some(in_tree) = in_tree else {
        return Ok((format!(">={RUNTIME_FLOOR}"), None));
    };
    let registry = in_tree["publishConfig"]["registry"]
        .as_str()
        .map(str::to_owned);
    let link = relative(&resolved(out)?, &resolved(&home)?);
    let spelled: Vec<_> = link
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    Ok((format!("file:{}", spelled.join("/")), registry))
}

/// `path` made absolute with its longest existing ancestor canonical; the part
/// that does not exist yet is kept as written.
fn resolved(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute =
        std::path::absolute(path).with_context(|| format!("cannot resolve {}", path.display()))?;
    let (existing, base) = absolute
        .ancestors()
        .find_map(|ancestor| Some((ancestor, ancestor.canonicalize().ok()?)))
        .ok_or_else(|| anyhow!("cannot resolve {}", path.display()))?;
    Ok(base.join(absolute.strip_prefix(existing)?))
}

/// The path from directory `from` to `to`, both absolute.
fn relative(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut link = PathBuf::new();
    for _ in shared..from.len() {
        link.push("..");
    }
    for part in &to[shared..] {
        link.push(part);
    }
    link
}

/// The manifest of the package. The runtime is a peer dependency, so the app
/// and the wrapper share one runtime instance. Built inside the api-bones tree
/// (`runtime` is a `file:` link) the link is a dev dependency and the peer range
/// is the floor release. Private exactly when the Layer 2 crate does not
/// publish; a published package publishes to the registry its runtime does.
fn manifest(package: &Package, glue: &str, runtime: &str) -> String {
    let in_tree = runtime.starts_with("file:").then_some(runtime);
    let mut dev = serde_json::Map::new();
    if let Some(link) = in_tree {
        dev.insert(RUNTIME_PACKAGE.to_owned(), link.into());
    }
    dev.insert("@types/node".to_owned(), "^22.20.1".into());
    dev.insert("typescript".to_owned(), "^5.0.0".into());
    let peer = if in_tree.is_some() {
        format!(">={RUNTIME_FLOOR}")
    } else {
        runtime.to_owned()
    };
    let entry = "./dist/generated-l2/index.js";
    let types = "./dist/generated-l2/index.d.ts";
    let mut document = serde_json::json!({
        "name": package.name,
        "version": package.version,
        "license": package.license,
        "type": "module",
        "main": entry,
        "types": types,
        "exports": { ".": { "types": types, "import": entry } },
        "files": ["dist"],
        "scripts": { "build": format!("tsc -p tsconfig.json && node -e \"const fs=require('fs');for (const f of ['{glue}.js','{glue}_bg.wasm']) fs.copyFileSync('{SOURCE_DIR}/'+f,'dist/generated-l2/'+f)\"") },
        "dependencies": { "@opentelemetry/api": ">=1.9.0 <1.10.0" },
        "peerDependencies": { (RUNTIME_PACKAGE): peer },
        "devDependencies": dev
    });
    if package.private {
        document["private"] = serde_json::Value::Bool(true);
    } else if let Some(registry) = &package.registry {
        document["publishConfig"] = serde_json::json!({ "registry": registry, "access": "public" });
    }
    let mut text = serde_json::to_string_pretty(&document).expect("json serialises");
    text.push('\n');
    text
}

fn tsconfig() -> String {
    format!(
        "{{\n  \"compilerOptions\": {{\n    \"target\": \"ES2022\",\n    \"module\": \"ES2022\",\n    \"moduleResolution\": \"bundler\",\n    \"strict\": true,\n    \"declaration\": true,\n    \"skipLibCheck\": true,\n    \"rootDir\": \"{SOURCE_DIR}\",\n    \"outDir\": \"dist/generated-l2\",\n    \"types\": [\"node\"]\n  }},\n  \"include\": [\"{SOURCE_DIR}/*.ts\"]\n}}\n"
    )
}

/// The TypeScript spelling of a wire type.
fn ts_type(ty: &WireType) -> String {
    match ty {
        WireType::Bool => "boolean".to_owned(),
        WireType::I32 | WireType::U32 | WireType::F64 | WireType::Duration => "number".to_owned(),
        WireType::String => "string".to_owned(),
        WireType::I64 | WireType::U64 => "bigint".to_owned(),
        WireType::Bytes => "Uint8Array".to_owned(),
        WireType::SystemTime => "Date".to_owned(),
        WireType::Option(inner) => format!("{} | undefined", ts_type(inner)),
        WireType::List(inner) => format!("readonly ({})[]", ts_type(inner)),
        WireType::Map(inner) => format!("Readonly<Record<string, {}>>", ts_type(inner)),
        WireType::Named(name) => name.clone(),
    }
}

/// The expression that turns the TypeScript value `v` into its JSON form.
fn encode(ty: &WireType, v: &str) -> String {
    match ty {
        WireType::Bool
        | WireType::I32
        | WireType::U32
        | WireType::F64
        | WireType::Duration
        | WireType::String => v.to_owned(),
        WireType::I64 | WireType::U64 => format!("{v}.toString()"),
        WireType::Bytes => format!("bytesToBase64({v})"),
        WireType::SystemTime => format!("{v}.toISOString()"),
        WireType::Option(inner) => format!("{v} === undefined ? undefined : {}", encode(inner, v)),
        WireType::List(inner) => format!("{v}.map((e) => {})", encode(inner, "e")),
        WireType::Map(inner) => format!(
            "Object.fromEntries(Object.entries({v}).map(([k, e]) => [k, {}]))",
            encode(inner, "e")
        ),
        WireType::Named(name) => format!("encode{name}({v})"),
    }
}

/// The expression that revives the JSON value `w` into its TypeScript value.
fn decode(ty: &WireType, w: &str) -> String {
    match ty {
        WireType::Bool
        | WireType::I32
        | WireType::U32
        | WireType::F64
        | WireType::Duration
        | WireType::String => w.to_owned(),
        WireType::I64 | WireType::U64 => format!("BigInt({w})"),
        WireType::Bytes => format!("base64ToBytes({w})"),
        WireType::SystemTime => format!("new Date({w})"),
        WireType::Option(inner) => format!(
            "{w} === null || {w} === undefined ? undefined : {}",
            decode(inner, w)
        ),
        WireType::List(inner) => format!("{w}.map((e: any) => {})", decode(inner, "e")),
        WireType::Map(inner) => format!(
            "Object.fromEntries(Object.entries({w}).map(([k, e]: [string, any]) => [k, {}]))",
            decode(inner, "e")
        ),
        WireType::Named(name) => format!("decode{name}({w})"),
    }
}

fn is_optional(ty: &WireType) -> bool {
    matches!(ty, WireType::Option(_))
}

fn field_decl(field: &Param) -> String {
    let name = json_name(&field.name);
    match &field.ty {
        WireType::Option(inner) => format!("  readonly {name}?: {} | undefined;\n", ts_type(inner)),
        ty => format!("  readonly {name}: {};\n", ts_type(ty)),
    }
}

fn record_ts(record: &Record, out: &mut String) {
    let name = &record.name;
    let _ = writeln!(out, "export interface {name} {{");
    for field in &record.fields {
        out.push_str(&field_decl(field));
    }
    out.push_str("}\n\n");
    let _ = writeln!(
        out,
        "export function decode{name}(w: any): {name} {{\n  return {{"
    );
    for field in &record.fields {
        let key = json_name(&field.name);
        let _ = writeln!(
            out,
            "    {key}: {},",
            decode(&field.ty, &format!("w.{key}"))
        );
    }
    let _ = writeln!(out, "  }};\n}}\n");
    let _ = writeln!(
        out,
        "export function encode{name}(v: {name}): unknown {{\n  return {{"
    );
    for field in &record.fields {
        let key = json_name(&field.name);
        let _ = writeln!(
            out,
            "    {key}: {},",
            encode(&field.ty, &format!("v.{key}"))
        );
    }
    let _ = writeln!(out, "  }};\n}}\n");
}

/// The options of the package's clients: the union of the optional fields
/// their `from_host` constructors take, in order of first appearance.
///
/// # Errors
///
/// Two clients that take the same option with different types.
fn client_options(model: &Model) -> anyhow::Result<Vec<Param>> {
    let mut union: Vec<Param> = Vec::new();
    for client in &model.clients {
        for option in &client.options {
            match union
                .iter()
                .find(|seen| seen.json_name() == option.json_name())
            {
                Some(seen) if seen.ty != option.ty => {
                    return Err(anyhow!(
                        "option `{}` of `{}` is {:?} but another client takes it as {:?}",
                        option.json_name(),
                        client.ty,
                        option.ty,
                        seen.ty
                    ));
                }
                Some(_) => {}
                None => union.push(option.clone()),
            }
        }
    }
    Ok(union)
}

/// The JSON text of the client options the wasm constructor reads: each
/// option by its camelCase name. An option the caller left unset is dropped by
/// `JSON.stringify`, and an `#[enumeration]` crosses as its string value.
fn options_json(model: &Model, options: &[Param]) -> String {
    let members: Vec<String> = options
        .iter()
        .map(|option| {
            let key = json_name(&option.name);
            let source = format!("options.{key}");
            let value = match &option.ty {
                WireType::Option(inner)
                    if is_enumeration(model, inner) || encode(inner, &source) == source =>
                {
                    source
                }
                ty => encode(ty, &source),
            };
            format!("{key}: {value}")
        })
        .collect();
    if members.is_empty() {
        "\"{}\"".to_owned()
    } else {
        format!("JSON.stringify({{ {} }})", members.join(", "))
    }
}

fn is_enumeration(model: &Model, ty: &WireType) -> bool {
    matches!(ty, WireType::Named(name) if model.enumerations.iter().any(|e| &e.name == name))
}

fn types_ts(model: &Model, options: &[Param]) -> String {
    let mut out = String::from(HEADER);
    out.push_str("import type { BonesError, Page } from \"@brefwiz/api-bones-sdk-runtime\";\n\n");
    out.push_str(
        "/** What the host reads about the caller before every send. */\nexport interface Credential {\n  readonly subject: string;\n  readonly org: string;\n  readonly headers: Readonly<Record<string, string>>;\n}\n\n",
    );
    out.push_str(
        "/** How a client authenticates: explicitly anonymous, or a credential read before every send, and the options its constructors take. */\nexport interface ClientOptions {\n  readonly credential: \"anonymous\" | (() => Credential);\n  /** Called when a server refuses the credential as unauthenticated. */\n  readonly onUnauthenticated?: () => void;\n",
    );
    for option in options {
        out.push_str(&field_decl(option));
    }
    out.push_str("}\n\n");
    out.push_str(
        "/** Per-call options. */\nexport interface CallOptions {\n  readonly signal?: AbortSignal;\n}\n\n",
    );
    out.push_str(
        "/** One result of a batch chunk, in request order. */\nexport type BatchItem<T> = { readonly ok: T } | { readonly error: BonesError };\n\n",
    );
    for enumeration in &model.enumerations {
        let values: Vec<String> = enumeration
            .values
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect();
        let _ = writeln!(
            out,
            "export type {} = {};\n",
            enumeration.name,
            values.join(" | ")
        );
    }
    for record in &model.records {
        record_ts(record, &mut out);
    }
    out.push_str(
        "/** What a list handle delivers for a page: its JSON, and the Rust-side hold on its items. */\nexport type PageDelivery = readonly [json: string, hold: object];\n\nconst PAGE_HOLD = Symbol(\"api-bones.page-hold\");\n\n/** The page, carrying the hold on its items for as long as the caller holds the page. */\nexport function decodePage<T>(delivered: PageDelivery, item: (w: any) => T): Page<T> {\n  const [json, hold] = delivered;\n  const w = JSON.parse(json);\n  const page: Page<T> = {\n    items: w.items.map(item),\n    revision: w.revision,\n    fromCache: w.fromCache,\n    hasNext: w.hasNext,\n    ...(w.total === undefined ? {} : { total: { value: BigInt(w.total.value), kind: w.total.kind } }),\n  };\n  return Object.defineProperty(page, PAGE_HOLD, { value: hold });\n}\n\n",
    );
    out.push_str(
        "export function bytesToBase64(bytes: Uint8Array): string {\n  let binary = \"\";\n  for (const byte of bytes) binary += String.fromCharCode(byte);\n  return btoa(binary);\n}\n\nexport function base64ToBytes(text: string): Uint8Array {\n  const binary = atob(text);\n  return Uint8Array.from(binary, (c) => c.charCodeAt(0));\n}\n",
    );
    out
}

fn errors_ts(model: &Model) -> String {
    let mut out = String::from(HEADER);
    out.push_str(
        "export { BonesError, PageError, isCursorRejected, parseBonesError } from \"@brefwiz/api-bones-sdk-runtime\";\n",
    );
    for error in &model.errors {
        let codes: Vec<String> = error
            .variants
            .iter()
            .map(|variant| format!("\"{}\"", variant.code))
            .collect();
        let _ = write!(
            out,
            "\n/** The codes `{}` carries. */\nexport type {}Code = {};\n",
            error.name,
            error.name,
            codes.join(" | ")
        );
    }
    out
}

/// The module that loads the wasm once and states its digest. `@GLUE@`,
/// `@CRATE@` and `@VERSION@` are replaced.
const INIT_TS: &str = r#"import { lazyOnce, wasmInput } from "@brefwiz/api-bones-sdk-runtime";
import initGlue, * as glue from "./@GLUE@.js";

export type Glue = typeof glue;

let digest = "";

type Host = { process?: { env?: Record<string, string | undefined> } };

/** Appends the provenance line of the loaded wasm when the run collects evidence. */
async function recordProvenance(wasmSha256: string): Promise<void> {
  const directory = (globalThis as Host).process?.env?.BREFWIZ_SDK_EVIDENCE;
  if (!directory) {
    return;
  }
  const specifier = "node:fs/promises";
  const fs = await import(specifier);
  const line = JSON.stringify({
    schema: "api-bones.sdk-provenance/v1",
    crate: "@CRATE@",
    version: "@VERSION@",
    wasmSha256,
  });
  await fs.mkdir(directory, { recursive: true });
  await fs.appendFile(`${directory}/sdk-provenance.jsonl`, `${line}\n`);
}

/** Loads the wasm module once; concurrent callers share the one load. */
export const init = lazyOnce(async (): Promise<Glue> => {
  const input = await wasmInput(new URL("./@GLUE@_bg.wasm", import.meta.url));
  const bytes =
    input instanceof URL ? new Uint8Array(await (await globalThis.fetch(input)).arrayBuffer()) : new Uint8Array(input);
  const hash = await globalThis.crypto.subtle.digest("SHA-256", bytes);
  digest = Array.from(new Uint8Array(hash), (b) => b.toString(16).padStart(2, "0")).join("");
  await initGlue({ module_or_path: bytes });
  await recordProvenance(digest);
  return glue;
});

/** The wasm module this package loaded: its SHA-256, crate and version. */
export async function sdkBuildInfo(): Promise<{ wasmSha256: string; crate: string; version: string }> {
  await init();
  return { wasmSha256: digest, crate: "@CRATE@", version: "@VERSION@" };
}
"#;

fn init_ts(package: &Package, glue: &str) -> String {
    let mut out = String::from(HEADER);
    out.push_str(
        &INIT_TS
            .replace("@GLUE@", glue)
            .replace("@CRATE@", &package.crate_name)
            .replace("@VERSION@", &package.version),
    );
    out
}

fn index_ts(model: &Model) -> String {
    let mut out = String::from(HEADER);
    let clients: Vec<&str> = model.clients.iter().map(|c| c.ty.as_str()).collect();
    let _ = writeln!(
        out,
        "export {{ {} }} from \"./client.js\";",
        clients.join(", ")
    );
    out.push_str("export type { BatchItem, CallOptions, ClientOptions, Credential");
    for enumeration in &model.enumerations {
        let _ = write!(out, ", {}", enumeration.name);
    }
    for record in &model.records {
        let _ = write!(out, ", {}", record.name);
    }
    out.push_str(" } from \"./types.js\";\n");
    out.push_str(
        "export * from \"./errors.js\";\nexport { init, sdkBuildInfo } from \"./init.js\";\n",
    );
    out
}

/// The parameters of the TypeScript method for `export`; an async one ends
/// with the per-call options.
fn call_params(export: &Export) -> Vec<String> {
    let mut params: Vec<String> = Vec::new();
    for param in &export.params {
        let ty = ts_type(&param.ty);
        let optional = if is_optional(&param.ty) { "?" } else { "" };
        let ty = ty.trim_end_matches(" | undefined");
        params.push(format!("{}{optional}: {ty}", json_name(&param.name)));
    }
    if !export.sync {
        params.push("opts?: CallOptions".to_owned());
    }
    params
}

/// The return type of the TypeScript method for `export`.
fn return_type(export: &Export) -> String {
    match &export.ret {
        Return::Handle { kind, item } => match kind {
            HandleKind::List => format!("ListCall<{item}>"),
            HandleKind::Stream => format!("AsyncIterable<{item}>"),
            HandleKind::Batch => format!("AsyncIterable<BatchItem<{item}>[]>"),
        },
        Return::Result { ok, .. } if !export.sync => {
            format!(
                "Promise<{}>",
                ok.as_ref().map_or("void".to_owned(), ts_type)
            )
        }
        Return::Value(ty) | Return::Result { ok: Some(ty), .. } => ts_type(ty),
        _ => "void".to_owned(),
    }
}

/// The TypeScript method for `export`, indented as a class member.
fn method_ts(export: &Export, wasm_name: &str) -> anyhow::Result<String> {
    let name = typescript_name(&export.method);
    let call_params = call_params(export);
    let ret = return_type(export);
    let args: Vec<String> = export
        .params
        .iter()
        .map(|p| {
            let key = json_name(&p.name);
            format!("{key}: {}", encode(&p.ty, &key))
        })
        .collect();
    let args = format!("JSON.stringify({{ {} }})", args.join(", "));
    let call = format!("this.#wasm.{wasm_name}");
    let ctx = "const ctx = callCtx(opts?.signal);\n    ";
    let tail = "args, ctx.traceparent, ctx.tracestate, ctx.signal";
    let mut out = String::new();
    match &export.ret {
        Return::Handle { kind, item } => {
            let item_decode = format!("(w: any) => decode{item}(w)");
            match kind {
                HandleKind::List => {
                    let _ = write!(
                        out,
                        "  {name}({}): {ret} {{\n    {ctx}const args = {args};\n    return listCallOf<{item}, PageDelivery>({{\n      openPages: () => {call}({tail}),\n      decodePage: (delivered) => decodePage(delivered, {item_decode}),\n      mapError,\n      sink,\n    }});\n  }}\n",
                        call_params.join(", ")
                    );
                }
                HandleKind::Stream => {
                    let _ = write!(
                        out,
                        "  {name}({}): {ret} {{\n    {ctx}const args = {args};\n    return iterableOf<{item}>(\n      () => {call}({tail}),\n      (json) => decode{item}(JSON.parse(json)),\n      mapError,\n    );\n  }}\n",
                        call_params.join(", ")
                    );
                }
                HandleKind::Batch => {
                    let _ = write!(
                        out,
                        "  {name}({}): {ret} {{\n    {ctx}const args = {args};\n    return iterableOf<BatchItem<{item}>[]>(\n      () => {call}({tail}),\n      (json) =>\n        JSON.parse(json).map((w: any) =>\n          w.error === undefined\n            ? {{ ok: decode{item}(w.ok) }}\n            : {{ error: parseBonesError(JSON.stringify(w.error)) }},\n        ),\n      mapError,\n    );\n  }}\n",
                        call_params.join(", ")
                    );
                }
            }
        }
        Return::Result { ok, .. } if !export.sync => {
            let revive = match ok {
                Some(ty) => format!("return {};", decode(ty, "JSON.parse(json as string)")),
                None => String::new(),
            };
            let _ = write!(
                out,
                "  async {name}({}): {ret} {{\n    {ctx}const args = {args};\n    try {{\n      const json = await {call}({tail});\n      {revive}\n    }} catch (rejection) {{\n      throw mapError(rejection);\n    }}\n  }}\n",
                call_params.join(", ")
            );
        }
        other => {
            let revive = match other {
                Return::Value(ty) | Return::Result { ok: Some(ty), .. } => {
                    format!("return {};", decode(ty, "JSON.parse(json)"))
                }
                _ => String::new(),
            };
            let _ = write!(
                out,
                "  {name}({}): {ret} {{\n    const args = {args};\n    try {{\n      const json = {call}(args);\n      {revive}\n    }} catch (rejection) {{\n      throw mapError(rejection);\n    }}\n  }}\n",
                call_params.join(", ")
            );
        }
    }
    Ok(out)
}

/// `api-surface.d.ts`: every client class with the signatures `client.ts`
/// implements, so a reader of the tracked surface sees the same parameter
/// and return types.
fn surface_dts(model: &Model) -> String {
    let mut out = String::from(HEADER);
    out.push_str("import type { ListCall } from \"@brefwiz/api-bones-sdk-runtime\";\n");
    let mut types = vec![
        "BatchItem".to_owned(),
        "CallOptions".to_owned(),
        "ClientOptions".to_owned(),
    ];
    types.extend(model.enumerations.iter().map(|e| e.name.clone()));
    types.extend(model.records.iter().map(|r| r.name.clone()));
    let _ = writeln!(
        out,
        "import type {{ {} }} from \"./{SOURCE_DIR}/types.js\";",
        types.join(", ")
    );
    for client in &model.clients {
        let ty = &client.ty;
        let _ = write!(
            out,
            "\nexport declare class {ty} {{\n  static create(baseUrl: string, options: ClientOptions): Promise<{ty}>;\n"
        );
        for export in &client.exports {
            let _ = writeln!(
                out,
                "  {}({}): {};",
                typescript_name(&export.method),
                call_params(export).join(", "),
                return_type(export)
            );
        }
        out.push_str("}\n");
    }
    out
}

fn client_ts(
    model: &Model,
    package: &Package,
    document: &L2Methods,
    glue: &str,
) -> anyhow::Result<String> {
    let listed: BTreeSet<&str> = document.methods.iter().map(|m| m.export.as_str()).collect();
    let mut out = String::from(HEADER);
    let runtime = "callCtx, iterableOf, JsSpanSink, listCallOf, parseBonesError, type ListCall";
    let _ = writeln!(
        out,
        "import {{ {runtime} }} from \"@brefwiz/api-bones-sdk-runtime\";\nimport type * as glue from \"./{glue}.js\";\nimport {{ init }} from \"./init.js\";"
    );
    let mut types = vec!["decodePage".to_owned(), "type PageDelivery".to_owned()];
    for record in &model.records {
        types.push(format!("decode{}", record.name));
        types.push(format!("encode{}", record.name));
        types.push(format!("type {}", record.name));
    }
    types.extend(["type BatchItem", "type CallOptions", "type ClientOptions"].map(str::to_owned));
    let _ = writeln!(
        out,
        "import {{ {} }} from \"./types.js\";",
        types.join(", ")
    );
    out.push_str(
        "\nconst sink = new JsSpanSink();\n\nfunction mapError(rejection: unknown): unknown {\n  return typeof rejection === \"string\" ? parseBonesError(rejection) : rejection;\n}\n\nfunction host(options: ClientOptions): object {\n  const credential = options.credential;\n  return {\n    anonymous: credential === \"anonymous\",\n    credential: () => JSON.stringify(typeof credential === \"function\" ? credential() : null),\n    unauthenticated: () => options.onUnauthenticated?.(),\n    start: (json: string) => sink.start(json),\n    end: (json: string) => sink.end(json),\n  };\n}\n",
    );
    for client in &model.clients {
        let options = options_json(model, &client.options);
        let class = export_name(&package.service, &client.ty, "new")?;
        let class = class.trim_end_matches("__new").to_owned();
        let _ = write!(
            out,
            "\nexport class {ty} {{\n  readonly #wasm: glue.{class};\n\n  private constructor(wasm: glue.{class}) {{\n    this.#wasm = wasm;\n  }}\n\n  /** A client of the server at `baseUrl`; rejects `invalid_argument` when it is not a URI. */\n  static async create(baseUrl: string, options: ClientOptions): Promise<{ty}> {{\n    const wasm = await init();\n    try {{\n      return new {ty}(wasm.{class}.{class}__new(baseUrl, host(options), {options}));\n    }} catch (rejection) {{\n      throw mapError(rejection);\n    }}\n  }}\n",
            ty = client.ty
        );
        for export in &client.exports {
            let wasm_name = export_name(&package.service, &client.ty, &export.method)?;
            if !export.sync && !listed.contains(wasm_name.as_str()) {
                return Err(anyhow!("`{wasm_name}` is not in l2-methods"));
            }
            out.push('\n');
            out.push_str(&method_ts(export, &wasm_name)?);
        }
        out.push_str("}\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "---\nservice: svc\nsdk_surfaces:\n  s:\n    targets:\n      typescript:\n        packages: [\"@x/y\"]\n---\n";

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wrapper-ts-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    /// A workspace whose one member `c` holds `member` as its manifest.
    fn package_with(tag: &str, member: &str) -> anyhow::Result<Package> {
        package_in(tag, member, None)
    }

    fn package_in(tag: &str, member: &str, runtime: Option<&str>) -> anyhow::Result<Package> {
        let dir = scratch(tag);
        if let Some(json) = runtime {
            runtime_at(&dir, json);
        }
        std::fs::write(dir.join("SPEC.md"), SPEC).expect("spec");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"c\"]\nresolver = \"3\"\n[workspace.package]\nversion = \"1.2.3\"\nlicense = \"Apache-2.0\"\n",
        )
        .expect("workspace");
        std::fs::create_dir_all(dir.join("c/src")).expect("member");
        std::fs::write(dir.join("c/src/lib.rs"), "").expect("source");
        std::fs::write(dir.join("c/Cargo.toml"), member).expect("manifest");
        let result = package_of(&dir.join("SPEC.md"), &dir.join("c"), "s", &dir.join("out"));
        std::fs::remove_dir_all(&dir).expect("cleaned");
        result
    }

    const INHERITING: &str = "[package]\nname = \"c\"\nversion.workspace = true\nedition = \"2024\"\nlicense.workspace = true\n";

    #[test]
    fn the_workspace_inherited_fields_resolve_to_their_values() {
        let package =
            package_in("inherit", INHERITING, Some(REGISTRY_RUNTIME_JSON)).expect("package");
        assert_eq!(package.crate_name, "c");
        assert_eq!(package.version, "1.2.3");
        assert_eq!(package.license, "Apache-2.0");
        assert!(!package.private);
        assert_eq!(package.registry.as_deref(), Some(TEST_REGISTRY));
    }

    #[test]
    fn a_published_wrapper_without_a_runtime_registry_is_refused() {
        for runtime in [None, Some(RUNTIME_JSON)] {
            let error = package_in("no-registry", INHERITING, runtime)
                .err()
                .expect("refused");
            assert!(
                error.to_string().contains("publishConfig.registry"),
                "{error}"
            );
        }
    }

    #[test]
    fn a_literal_manifest_resolves_and_publish_false_makes_the_package_private() {
        let package = package_with(
            "literal",
            "[package]\nname = \"c\"\nversion = \"4.5.6\"\nedition = \"2024\"\nlicense = \"MIT\"\npublish = false\n",
        )
        .expect("package");
        assert_eq!(package.version, "4.5.6");
        assert_eq!(package.license, "MIT");
        assert!(package.private);
        assert_eq!(package.runtime, format!(">={RUNTIME_FLOOR}"));
    }

    #[test]
    fn a_manifest_cargo_rejects_is_refused_with_its_message() {
        let error = package_with("broken", "[package\n").err().expect("refused");
        assert!(
            error.to_string().contains("`cargo metadata` failed"),
            "{error}"
        );
    }

    fn metadata(packages: &str) -> String {
        format!("{{\"packages\":[{packages}],\"workspace_members\":[]}}")
    }

    fn entry(manifest: &str, extra: &str) -> String {
        format!(
            "{{\"name\":\"n\",\"version\":\"1.0.0\",\"license\":\"MIT\",\"manifest_path\":\"{manifest}\"{extra}}}"
        )
    }

    #[test]
    fn the_package_of_the_manifest_is_picked_among_the_workspace_members() {
        let json = metadata(&format!(
            "{{\"name\":\"other\",\"version\":\"9.9.9\",\"license\":\"GPL-3.0\",\"manifest_path\":\"/w/o/Cargo.toml\",\"publish\":null}},{}",
            entry("/w/c/Cargo.toml", ",\"publish\":null")
        ));
        let found = crate_metadata(&json, Path::new("/w/c/Cargo.toml")).expect("found");
        assert_eq!(
            found,
            CrateMetadata {
                name: "n".to_owned(),
                version: "1.0.0".to_owned(),
                license: "MIT".to_owned(),
                private: false,
            }
        );
    }

    #[test]
    fn only_an_empty_publish_list_makes_the_package_private() {
        for (publish, private) in [
            (",\"publish\":[]", true),
            (",\"publish\":null", false),
            (",\"publish\":[\"crates-io\"]", false),
            ("", false),
        ] {
            let json = metadata(&entry("/w/Cargo.toml", publish));
            let found = crate_metadata(&json, Path::new("/w/Cargo.toml")).expect("found");
            assert_eq!(found.private, private, "{publish}");
        }
    }

    #[test]
    fn a_document_that_lists_no_package_for_the_manifest_is_refused() {
        let json = metadata(&entry("/w/o/Cargo.toml", ""));
        let error = crate_metadata(&json, Path::new("/w/c/Cargo.toml")).expect_err("absent");
        assert!(error.to_string().contains("lists no package"), "{error}");
        let error = crate_metadata("{}", Path::new("/w/c/Cargo.toml")).expect_err("no packages");
        assert!(error.to_string().contains("lists no package"), "{error}");
    }

    #[test]
    fn a_document_that_is_not_json_is_refused() {
        let error = crate_metadata("not json", Path::new("/w/Cargo.toml")).expect_err("refused");
        assert!(error.to_string().contains("did not answer JSON"), "{error}");
    }

    #[test]
    fn a_package_without_a_name_a_version_or_a_license_is_refused() {
        let at = "\"manifest_path\":\"/w/Cargo.toml\"";
        for (fields, message) in [
            (
                "\"version\":\"1.0.0\",\"license\":\"MIT\"",
                "names no crate",
            ),
            ("\"name\":\"n\",\"license\":\"MIT\"", "declares no version"),
            (
                "\"name\":\"n\",\"version\":\"1.0.0\",\"license\":null",
                "declares no license",
            ),
        ] {
            let json = metadata(&format!("{{{fields},{at}}}"));
            let error = crate_metadata(&json, Path::new("/w/Cargo.toml")).expect_err(message);
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    const TEST_REGISTRY: &str = "https://registry.example.test/npm/";

    fn package_for(private: bool) -> Package {
        Package {
            service: "svc".to_owned(),
            crate_name: "c".to_owned(),
            name: "@x/y".to_owned(),
            version: "1.2.3".to_owned(),
            license: "MIT".to_owned(),
            private,
            runtime: format!(">={RUNTIME_FLOOR}"),
            registry: Some(TEST_REGISTRY.to_owned()),
            spec: SpecL2Methods::default(),
        }
    }

    fn read_manifest(package: &Package, runtime: &str) -> serde_json::Value {
        serde_json::from_str(&manifest(package, "c", runtime)).expect("json")
    }

    #[test]
    fn the_manifest_is_private_exactly_when_the_crate_does_not_publish() {
        for runtime in [">=0.2.0", "file:../rt"] {
            let private = read_manifest(&package_for(true), runtime);
            assert_eq!(private["private"], true, "{runtime}");
            assert!(private.get("publishConfig").is_none(), "{runtime}");
            let published = read_manifest(&package_for(false), runtime);
            assert!(published.get("private").is_none(), "{runtime}");
            assert_eq!(
                published["publishConfig"],
                serde_json::json!({ "registry": TEST_REGISTRY, "access": "public" }),
                "{runtime}"
            );
            assert_eq!(published["version"], "1.2.3");
        }
    }

    #[test]
    fn an_in_tree_runtime_is_a_dev_link_behind_a_peer_range_at_the_floor() {
        let document = read_manifest(&package_for(false), "file:../rt");
        assert_eq!(
            document["peerDependencies"][RUNTIME_PACKAGE],
            format!(">={RUNTIME_FLOOR}")
        );
        assert_eq!(document["devDependencies"][RUNTIME_PACKAGE], "file:../rt");
        assert!(document["dependencies"].get(RUNTIME_PACKAGE).is_none());
    }

    #[test]
    fn a_released_runtime_is_the_peer_range_alone() {
        let document = read_manifest(&package_for(false), ">=0.2.0");
        assert_eq!(document["peerDependencies"][RUNTIME_PACKAGE], ">=0.2.0");
        assert!(document["devDependencies"].get(RUNTIME_PACKAGE).is_none());
        assert!(document["dependencies"].get(RUNTIME_PACKAGE).is_none());
    }

    fn runtime_at(spec_dir: &Path, package_json: &str) {
        let home = spec_dir.join("api-bones-sdk-runtime-ts");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(home.join("package.json"), package_json).expect("package.json");
    }

    const RUNTIME_JSON: &str = "{\"name\":\"@brefwiz/api-bones-sdk-runtime\"}";
    const REGISTRY_RUNTIME_JSON: &str = "{\"name\":\"@brefwiz/api-bones-sdk-runtime\",\"publishConfig\":{\"registry\":\"https://registry.example.test/npm/\"}}";

    #[test]
    fn a_runtime_package_next_to_the_spec_is_linked_relative_to_the_output() {
        let dir = scratch("link");
        runtime_at(&dir, RUNTIME_JSON);
        let out = dir.join("tests/l2/typescript");
        assert_eq!(
            runtime_dependency(&dir, &out).expect("dependency").0,
            "file:../../../api-bones-sdk-runtime-ts"
        );
        std::fs::create_dir_all(&out).expect("out");
        assert_eq!(
            runtime_dependency(&dir, &out).expect("dependency").0,
            "file:../../../api-bones-sdk-runtime-ts"
        );
        assert_eq!(
            runtime_dependency(&dir, &dir.join("sdk"))
                .expect("dependency")
                .0,
            "file:../api-bones-sdk-runtime-ts"
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn the_link_is_the_same_through_a_symlinked_output() {
        let dir = scratch("symlink");
        runtime_at(&dir, RUNTIME_JSON);
        std::fs::create_dir_all(dir.join("real/deep")).expect("real");
        std::os::unix::fs::symlink(dir.join("real/deep"), dir.join("alias")).expect("symlink");
        let through_alias = runtime_dependency(&dir, &dir.join("alias"))
            .expect("dependency")
            .0;
        assert_eq!(
            through_alias,
            runtime_dependency(&dir, &dir.join("real/deep"))
                .expect("dependency")
                .0
        );
        assert_eq!(through_alias, "file:../../api-bones-sdk-runtime-ts");
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn the_runtime_registry_is_read_from_its_publish_config() {
        let dir = scratch("registry");
        runtime_at(&dir, REGISTRY_RUNTIME_JSON);
        assert_eq!(
            runtime_dependency(&dir, &dir.join("out"))
                .expect("dependency")
                .1,
            Some(TEST_REGISTRY.to_owned())
        );
        runtime_at(&dir, RUNTIME_JSON);
        assert_eq!(
            runtime_dependency(&dir, &dir.join("out"))
                .expect("dependency")
                .1,
            None
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn a_missing_or_foreign_runtime_package_falls_back_to_the_floor() {
        let floor = format!(">={RUNTIME_FLOOR}");
        let dir = scratch("floor");
        assert_eq!(
            runtime_dependency(&dir, &dir.join("out")).expect("floor").0,
            floor
        );
        runtime_at(&dir, "{\"name\":\"@other/runtime\"}");
        assert_eq!(
            runtime_dependency(&dir, &dir.join("out")).expect("floor").0,
            floor
        );
        runtime_at(&dir, "not json");
        assert_eq!(
            runtime_dependency(&dir, &dir.join("out")).expect("floor").0,
            floor
        );
        runtime_at(&dir, "{}");
        assert_eq!(
            runtime_dependency(&dir, &dir.join("out")).expect("floor").0,
            floor
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn the_floor_is_a_single_minimum_the_connect_package_peers_on() {
        let parts: Vec<&str> = RUNTIME_FLOOR.split('.').collect();
        assert_eq!(parts.len(), 3, "{RUNTIME_FLOOR}");
        assert!(
            parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())),
            "{RUNTIME_FLOOR}"
        );
        let peer =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../api-bones-connect-ts/package.json");
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(peer).expect("the connect manifest"))
                .expect("json");
        assert_eq!(
            manifest["peerDependencies"][RUNTIME_PACKAGE],
            format!(">={RUNTIME_FLOOR}")
        );
    }

    #[test]
    fn a_relative_path_climbs_out_of_the_origin_then_descends() {
        let path = |from, to| relative(Path::new(from), Path::new(to));
        assert_eq!(path("/a/b/c", "/a/d"), Path::new("../../d"));
        assert_eq!(path("/a", "/a/b/c"), Path::new("b/c"));
        assert_eq!(path("/a/b", "/a/b"), Path::new(""));
    }

    #[test]
    fn a_path_resolves_through_its_longest_existing_ancestor() {
        let dir = scratch("resolved").canonicalize().expect("canonical");
        assert_eq!(resolved(&dir).expect("existing"), dir);
        assert_eq!(
            resolved(&dir.join("x/y")).expect("missing tail"),
            dir.join("x/y")
        );
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn types_encode_and_decode_through_their_wire_form() {
        let list = WireType::List(Box::new(WireType::I64));
        assert_eq!(ts_type(&list), "readonly (bigint)[]");
        assert_eq!(encode(&list, "v"), "v.map((e) => e.toString())");
        assert_eq!(decode(&list, "w"), "w.map((e: any) => BigInt(e))");
        let option = WireType::Option(Box::new(WireType::SystemTime));
        assert_eq!(ts_type(&option), "Date | undefined");
        assert_eq!(
            decode(&option, "w"),
            "w === null || w === undefined ? undefined : new Date(w)"
        );
    }

    fn export(method: &str, sync: bool, params: Vec<Param>, ret: Return) -> Export {
        Export {
            method: method.to_owned(),
            sync,
            surface: "s".to_owned(),
            params,
            ret,
        }
    }

    fn result(ok: Option<WireType>) -> Return {
        Return::Result {
            ok,
            error: api_bones_sdk_model::ErrorRef::Sdk,
        }
    }

    fn handle(kind: HandleKind) -> Return {
        Return::Handle {
            kind,
            item: "Widget".to_owned(),
        }
    }

    /// The page a caller holds carries the hold on its items under a module
    /// symbol, out of sight of enumeration and serialisation.
    #[test]
    fn a_decoded_page_carries_its_hold_where_enumeration_does_not_reach() {
        let types = types_ts(&Model::default(), &[]);
        assert!(
            types.contains("export type PageDelivery = readonly [json: string, hold: object];"),
            "{types}"
        );
        assert!(
            types.contains("const PAGE_HOLD = Symbol(\"api-bones.page-hold\");"),
            "{types}"
        );
        assert!(types.contains("const [json, hold] = delivered;"), "{types}");
        assert!(
            types.contains("return Object.defineProperty(page, PAGE_HOLD, { value: hold });"),
            "{types}"
        );
        assert!(!types.contains("enumerable: true"), "{types}");
    }

    #[test]
    fn a_list_method_decodes_the_delivered_page_with_its_hold() {
        let list = method_ts(
            &export("list_widgets", false, vec![], handle(HandleKind::List)),
            "w",
        )
        .expect("a list method");
        assert!(list.contains("listCallOf<Widget, PageDelivery>("), "{list}");
        assert!(
            list.contains(
                "decodePage: (delivered) => decodePage(delivered, (w: any) => decodeWidget(w)),"
            ),
            "{list}"
        );
        let stream = method_ts(
            &export("watch", false, vec![], handle(HandleKind::Stream)),
            "w",
        )
        .expect("a stream method");
        assert!(!stream.contains("PageDelivery"), "{stream}");
    }

    #[test]
    fn a_return_type_is_spelled_as_the_client_declares_it() {
        let cases = [
            (false, handle(HandleKind::List), "ListCall<Widget>"),
            (false, handle(HandleKind::Stream), "AsyncIterable<Widget>"),
            (
                false,
                handle(HandleKind::Batch),
                "AsyncIterable<BatchItem<Widget>[]>",
            ),
            (false, result(Some(WireType::I64)), "Promise<bigint>"),
            (false, result(None), "Promise<void>"),
            (true, result(Some(WireType::String)), "string"),
            (true, result(None), "void"),
            (true, Return::Value(WireType::Bool), "boolean"),
            (true, Return::Unit, "void"),
        ];
        for (sync, ret, expected) in cases {
            assert_eq!(return_type(&export("m", sync, vec![], ret)), expected);
        }
    }

    #[test]
    fn an_async_method_takes_the_call_options_and_a_sync_one_does_not() {
        let params = vec![
            Param {
                name: "page_size".to_owned(),
                ty: WireType::Option(Box::new(WireType::U32)),
            },
            Param {
                name: "name".to_owned(),
                ty: WireType::String,
            },
        ];
        let asynchronous = export("m", false, params.clone(), Return::Unit);
        assert_eq!(
            call_params(&asynchronous),
            ["pageSize?: number", "name: string", "opts?: CallOptions"]
        );
        let synchronous = export("m", true, params, Return::Unit);
        assert_eq!(
            call_params(&synchronous),
            ["pageSize?: number", "name: string"]
        );
    }

    #[test]
    fn the_surface_declares_every_client_with_its_signatures() {
        use api_bones_sdk_model::{Client, Enumeration};
        let model = Model {
            records: vec![Record {
                name: "Widget".to_owned(),
                fields: vec![],
            }],
            enumerations: vec![Enumeration {
                name: "Color".to_owned(),
                values: vec!["red".to_owned()],
            }],
            clients: vec![Client {
                ty: "WidgetClient".to_owned(),
                surface: "s".to_owned(),
                exports: vec![
                    export("list_widgets", false, vec![], handle(HandleKind::List)),
                    export("count", true, vec![], Return::Value(WireType::U32)),
                ],
                options: vec![],
            }],
            ..Model::default()
        };
        let text = surface_dts(&model);
        assert!(text.starts_with(HEADER), "{text}");
        assert!(
            text.contains("import type { ListCall } from \"@brefwiz/api-bones-sdk-runtime\";"),
            "{text}"
        );
        assert!(
            text.contains("import type { BatchItem, CallOptions, ClientOptions, Color, Widget } from \"./src/generated-l2/types.js\";"),
            "{text}"
        );
        assert!(
            text.contains("export declare class WidgetClient {"),
            "{text}"
        );
        assert!(
            text.contains(
                "  static create(baseUrl: string, options: ClientOptions): Promise<WidgetClient>;"
            ),
            "{text}"
        );
        assert!(
            text.contains("  listWidgets(opts?: CallOptions): ListCall<Widget>;"),
            "{text}"
        );
        assert!(text.contains("  count(): number;"), "{text}");
        assert!(text.ends_with("}\n"), "{text}");
    }

    fn option(name: &str, ty: WireType) -> Param {
        Param {
            name: name.to_owned(),
            ty: WireType::Option(Box::new(ty)),
        }
    }

    fn client_with(ty: &str, options: Vec<Param>) -> api_bones_sdk_model::Client {
        api_bones_sdk_model::Client {
            ty: ty.to_owned(),
            surface: "s".to_owned(),
            exports: vec![],
            options,
        }
    }

    fn model_of(clients: Vec<api_bones_sdk_model::Client>) -> Model {
        Model {
            enumerations: vec![api_bones_sdk_model::Enumeration {
                name: "Transport".to_owned(),
                values: vec!["connect".to_owned(), "grpc_web".to_owned()],
            }],
            clients,
            ..Model::default()
        }
    }

    #[test]
    fn the_client_options_are_the_union_of_the_options_of_every_client() {
        let model = model_of(vec![
            client_with(
                "A",
                vec![
                    option("default_page_size", WireType::U32),
                    option("transport", WireType::Named("Transport".to_owned())),
                ],
            ),
            client_with("B", vec![option("default_page_size", WireType::U32)]),
            client_with("C", vec![]),
        ]);
        let names: Vec<_> = client_options(&model)
            .expect("a union")
            .iter()
            .map(Param::json_name)
            .collect();
        assert_eq!(names, ["defaultPageSize", "transport"]);
        assert!(client_options(&Model::default()).expect("none").is_empty());
    }

    #[test]
    fn one_option_with_two_types_is_refused() {
        let model = model_of(vec![
            client_with("A", vec![option("size", WireType::U32)]),
            client_with("B", vec![option("size", WireType::String)]),
        ]);
        let message = client_options(&model).expect_err("refused").to_string();
        assert!(
            message.contains("`size`") && message.contains("`B`"),
            "{message}"
        );
    }

    #[test]
    fn each_client_sends_only_its_own_options() {
        let model = model_of(vec![
            client_with("Gadgets", vec![option("default_page_size", WireType::U32)]),
            client_with("Widgets", vec![]),
        ]);
        let package = package_for(false);
        let document = l2_methods(
            &model,
            &package.spec,
            &package.service,
            &package.crate_name,
            &package.name,
        )
        .expect("methods");
        let text = client_ts(&model, &package, &document, "c").expect("client");
        let create = |ty: &str| {
            let from = text.find(&format!("export class {ty} ")).expect("class");
            let body = &text[from..];
            let line = body
                .lines()
                .find(|line| line.contains("__new("))
                .expect("constructor call");
            line.to_owned()
        };
        let gadgets = create("Gadgets");
        let widgets = create("Widgets");
        assert!(
            gadgets.contains("JSON.stringify({ defaultPageSize: options.defaultPageSize })"),
            "{gadgets}"
        );
        assert!(widgets.contains("host(options), \"{}\")"), "{widgets}");
        assert!(!widgets.contains("defaultPageSize"), "{widgets}");
    }

    #[test]
    fn the_options_are_declared_on_the_client_options_and_sent_as_json() {
        let model = model_of(vec![client_with(
            "A",
            vec![
                option("default_page_size", WireType::U32),
                option("transport", WireType::Named("Transport".to_owned())),
                option("since", WireType::SystemTime),
            ],
        )]);
        let options = client_options(&model).expect("a union");
        let types = types_ts(&model, &options);
        assert!(
            types.contains(
                "  readonly onUnauthenticated?: () => void;\n  readonly defaultPageSize?: number | undefined;\n  readonly transport?: Transport | undefined;\n  readonly since?: Date | undefined;\n}\n"
            ),
            "{types}"
        );
        assert_eq!(
            options_json(&model, &options),
            "JSON.stringify({ defaultPageSize: options.defaultPageSize, transport: options.transport, since: options.since === undefined ? undefined : options.since.toISOString() })"
        );
    }

    #[test]
    fn a_package_without_options_sends_an_empty_object() {
        let model = model_of(vec![client_with("A", vec![])]);
        assert_eq!(options_json(&model, &[]), "\"{}\"");
        let types = types_ts(&model, &[]);
        assert!(
            types.contains("  readonly onUnauthenticated?: () => void;\n}\n"),
            "{types}"
        );
    }
}
