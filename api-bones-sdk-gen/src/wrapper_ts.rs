// SPDX-License-Identifier: MIT
//! `wrapper typescript`: the TypeScript package that fronts the wasm build of
//! a Layer 2 crate.
//!
//! Every file is generated from the crate's `#[error]` and `#[client]` items,
//! the SPEC's `l2_methods` and the descriptor set that declares the messages
//! the exports cross as; nothing in the package is written by hand. The package
//! holds no decision logic: a method builds the protobuf message of its
//! arguments, calls the wasm export with the bytes and decodes the bytes it
//! answers, and the list, stream and batch shapes come from the runtime
//! package.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, anyhow, ensure};
use api_bones_sdk_model::{
    Client, Export, HandleKind, L2Methods, Model, Param, Return, SpecL2Methods, Wire, Wrapper,
    export_name, l2_methods, typescript_name,
};
use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, MessageDescriptor};

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
        license: found.license,
        private: found.private,
        runtime,
        registry,
        spec: spec_l2,
        wasm_sha256: String::new(),
    })
}

/// What the Layer 2 crate's manifest resolves to, workspace inheritance
/// included.
#[derive(Debug, PartialEq, Eq)]
struct CrateMetadata {
    name: String,
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
        .find(|package| {
            package["manifest_path"].as_str().is_some_and(|listed| {
                // Cargo lists the path as spelled to it; the caller's is
                // resolved, so a symlinked directory still matches.
                let listed = Path::new(listed);
                listed == manifest || listed.canonicalize().is_ok_and(|real| real == manifest)
            })
        })
        .ok_or_else(|| {
            anyhow!(
                "`cargo metadata` lists no package for {}",
                manifest.display()
            )
        })?;
    let text = |key: &str| package[key].as_str().map(str::to_owned);
    Ok(CrateMetadata {
        name: text("name").ok_or_else(|| anyhow!("the manifest names no crate"))?,
        license: text("license")
            .ok_or_else(|| anyhow!("the manifest of {} declares no license", manifest.display()))?,
        private: package["publish"]
            .as_array()
            .is_some_and(std::vec::Vec::is_empty),
    })
}

/// The version every generated wrapper carries in its `package.json`,
/// provenance and `sdkBuildInfo`. It is constant so a crate release never
/// changes the generated output; the package lane stamps the tag version into
/// `package.json` at pack time.
pub(crate) const WRAPPER_PLACEHOLDER_VERSION: &str = "0.0.0";

/// The directory of the generated sources inside the package; the manifest's
/// entry fields point at its `dist` twin.
pub(crate) const SOURCE_DIR: &str = "src/generated";

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
    /// The SHA-256 of the optimised wasm module, which `init.ts` states: the
    /// build stamps it, so a wrapper written without a module carries none.
    pub(crate) wasm_sha256: String,
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
/// message the descriptor set lacks or declares other than the export's
/// parameters.
pub(crate) fn generate(
    model: &Model,
    package: &Package,
    contract: &Contract,
) -> anyhow::Result<Vec<File>> {
    let document = l2_methods(
        model,
        &package.spec,
        &package.service,
        &package.crate_name,
        &package.name,
    )?;
    let glue = package.crate_name.replace('-', "_");
    let rendered = client_ts(model, package, &document, contract, &glue)?;
    let mut files = vec![
        (
            "package.json".to_owned(),
            manifest(package, &glue, &package.runtime),
        ),
        ("tsconfig.json".to_owned(), tsconfig()),
        ("l2-methods.json".to_owned(), document.to_json()),
        (format!("{SOURCE_DIR}/types.ts"), types_ts()),
        (format!("{SOURCE_DIR}/errors.ts"), errors_ts(model)),
        (format!("{SOURCE_DIR}/init.ts"), init_ts(package, &glue)),
        (format!("{SOURCE_DIR}/client.ts"), rendered.client.clone()),
        (format!("{SOURCE_DIR}/index.ts"), index_ts(model, &rendered)),
        (SURFACE_FILE.to_owned(), surface_dts(&rendered)),
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
pub(crate) const RUNTIME_FLOOR: &str = "0.3.0";

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

/// The protobuf-es runtime the generated messages and the codec run on.
pub(crate) const PROTOBUF_ES_RANGE: &str = "^2.2.0";

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
    dev.insert("@bufbuild/buf".to_owned(), "^1.50.0".into());
    dev.insert("@bufbuild/protoc-gen-es".to_owned(), "^2.16.0".into());
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
        "version": WRAPPER_PLACEHOLDER_VERSION,
        "license": package.license,
        "type": "module",
        "main": entry,
        "types": types,
        "exports": { ".": { "types": types, "import": entry } },
        "files": ["dist"],
        "scripts": { "build": format!("tsc -p tsconfig.json && node -e \"const fs=require('fs');for (const f of ['{glue}.js','{glue}_bg.wasm']) fs.copyFileSync('{SOURCE_DIR}/'+f,'dist/generated-l2/'+f)\"") },
        "dependencies": { "@bufbuild/protobuf": PROTOBUF_ES_RANGE, "@opentelemetry/api": ">=1.9.0 <1.10.0" },
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
        "{{\n  \"compilerOptions\": {{\n    \"target\": \"ES2022\",\n    \"module\": \"ES2022\",\n    \"moduleResolution\": \"bundler\",\n    \"strict\": true,\n    \"declaration\": true,\n    \"skipLibCheck\": true,\n    \"rootDir\": \"{SOURCE_DIR}\",\n    \"outDir\": \"dist/generated-l2\",\n    \"types\": [\"node\"]\n  }},\n  \"include\": [\"{SOURCE_DIR}/**/*.ts\"]\n}}\n"
    )
}

/// The `bones.sdk.v1` boundary version this generator's output speaks. The
/// core refuses a wrapper stamped for another one; a test pins it to the core's
/// `BOUNDARY`.
pub(crate) const WRAPPER_BOUNDARY: u32 = 1;

/// The directory of the protobuf-es output inside [`SOURCE_DIR`]; the imports
/// of every message point into it.
pub(crate) const PROTO_DIR: &str = "proto";

/// Names a call option may not take: they are the per-call context every
/// method already carries, or a local the generated body owns.
const RESERVED: [&str; 8] = [
    "signal",
    "prefetch",
    "cache",
    "traceparent",
    "tracestate",
    "opts",
    "ctx",
    "request",
];

/// Names the TypeScript global scope owns; a message called so is imported
/// under its package-qualified name.
const GLOBALS: [&str; 13] = [
    "Number", "String", "Boolean", "Date", "Error", "Map", "Set", "Object", "Array", "Symbol",
    "Promise", "Function", "BigInt",
];

/// The file descriptor set that declares every message the exports cross as.
#[derive(Debug)]
pub(crate) struct Contract {
    pool: DescriptorPool,
}

impl Contract {
    /// Reads the descriptor set at `path`.
    ///
    /// # Errors
    ///
    /// A file that cannot be read, or one [`Contract::decode`] refuses.
    pub(crate) fn read(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("cannot read the descriptor set {}", path.display()))?;
        Self::decode(&bytes).with_context(|| format!("invalid descriptor set {}", path.display()))
    }

    /// The contract of an encoded `FileDescriptorSet`.
    ///
    /// # Errors
    ///
    /// Bytes that are not a descriptor set, or a set that holds no file.
    pub(crate) fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let pool = DescriptorPool::decode(bytes)?;
        ensure!(pool.files().next().is_some(), "the descriptor set is empty");
        Ok(Self { pool })
    }

    fn message(&self, full_name: &str) -> anyhow::Result<MessageDescriptor> {
        self.pool
            .get_message_by_name(full_name)
            .ok_or_else(|| anyhow!("message `{full_name}` is missing from the descriptor set"))
    }
}

/// What one TypeScript module imports from protobuf-es output.
#[derive(Default)]
struct Imports {
    /// Each message or enum by full name: its file, its protobuf-es name and
    /// the name it is imported as.
    named: BTreeMap<String, Imported>,
    /// The names taken, and by which full name.
    taken: BTreeMap<String, String>,
}

struct Imported {
    file: String,
    es: String,
    alias: String,
    is_message: bool,
}

impl Imports {
    fn add(&mut self, full: &str, package: &str, file: &str, is_message: bool) -> String {
        if let Some(seen) = self.named.get(full) {
            return seen.alias.clone();
        }
        let local = full.strip_prefix(package).unwrap_or(full);
        let es = local.trim_start_matches('.').replace('.', "_");
        let qualified = || format!("{}_{es}", package.replace('.', "_"));
        let alias = if GLOBALS.contains(&es.as_str()) || self.taken.contains_key(&es) {
            qualified()
        } else {
            es.clone()
        };
        self.taken.insert(alias.clone(), full.to_owned());
        self.named.insert(
            full.to_owned(),
            Imported {
                file: file.to_owned(),
                es,
                alias: alias.clone(),
                is_message,
            },
        );
        alias
    }

    /// The name `message` is imported as.
    fn message(&mut self, message: &MessageDescriptor) -> String {
        self.add(
            message.full_name(),
            message.package_name(),
            message.parent_file().name(),
            true,
        )
    }

    fn enumeration(&mut self, value: &prost_reflect::EnumDescriptor) -> String {
        self.add(
            value.full_name(),
            value.package_name(),
            value.parent_file().name(),
            false,
        )
    }

    /// The import lines: one per file, in file order. `base` is the directory
    /// the generated sources are written in, relative to the importing
    /// module; a `type_only` import is for a declaration file.
    fn lines(&self, base: &str, type_only: bool) -> String {
        let mut files: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for imported in self.named.values() {
            let entry = files.entry(imported.file.as_str()).or_default();
            let prefix = if type_only { "" } else { "type " };
            entry.push(format!("{prefix}{}", spell(&imported.es, &imported.alias)));
            if imported.is_message {
                entry.push(spell(
                    &format!("{}Schema", imported.es),
                    &format!("{}Schema", imported.alias),
                ));
            }
        }
        let keyword = if type_only { "import type" } else { "import" };
        let mut out = String::new();
        for (file, mut names) in files {
            names.sort();
            let _ = writeln!(
                out,
                "{keyword} {{ {} }} from \"{}\";",
                names.join(", "),
                proto_specifier(file, base)
            );
        }
        out
    }

    /// The names the package re-exports so a caller can spell the types of
    /// its signatures, as `export type { .. } from` lines.
    fn reexports(&self) -> String {
        let mut files: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for imported in self.named.values() {
            files
                .entry(imported.file.as_str())
                .or_default()
                .push(spell(&imported.es, &imported.alias));
        }
        let mut out = String::new();
        for (file, mut names) in files {
            names.sort();
            let _ = writeln!(
                out,
                "export type {{ {} }} from \"{}\";",
                names.join(", "),
                proto_specifier(file, "./")
            );
        }
        out
    }
}

/// `name`, or `name as alias` when the two differ.
fn spell(name: &str, alias: &str) -> String {
    if name == alias {
        name.to_owned()
    } else {
        format!("{name} as {alias}")
    }
}

/// The module specifier of the protobuf-es output of the proto file `file`:
/// the well-known types live in the protobuf-es package, every other file
/// under [`PROTO_DIR`] of `base`.
fn proto_specifier(file: &str, base: &str) -> String {
    if file.starts_with("google/protobuf/") {
        "@bufbuild/protobuf/wkt".to_owned()
    } else {
        format!(
            "{base}{PROTO_DIR}/{}_pb.js",
            file.strip_suffix(".proto").unwrap_or(file)
        )
    }
}

/// The TypeScript type a caller passes for `field`.
fn field_ts(imports: &mut Imports, field: &FieldDescriptor) -> String {
    if let (true, Kind::Message(entry)) = (field.is_map(), field.kind()) {
        let value = entry.map_entry_value_field();
        return format!(
            "Readonly<Record<string, {}>>",
            kind_ts(imports, &value.kind())
        );
    }
    let ty = kind_ts(imports, &field.kind());
    if field.is_list() {
        format!("readonly ({ty})[]")
    } else {
        ty
    }
}

fn kind_ts(imports: &mut Imports, kind: &Kind) -> String {
    match kind {
        Kind::Bool => "boolean".to_owned(),
        Kind::Int32
        | Kind::Uint32
        | Kind::Sint32
        | Kind::Fixed32
        | Kind::Sfixed32
        | Kind::Float
        | Kind::Double => "number".to_owned(),
        Kind::Int64 | Kind::Uint64 | Kind::Sint64 | Kind::Fixed64 | Kind::Sfixed64 => {
            "bigint".to_owned()
        }
        Kind::String => "string".to_owned(),
        Kind::Bytes => "Uint8Array".to_owned(),
        Kind::Message(message) => {
            format!(
                "MessageInitShape<typeof {}Schema>",
                imports.message(message)
            )
        }
        Kind::Enum(value) => imports.enumeration(value),
    }
}

/// One parameter of a method, bound to the field of the args message it
/// travels in.
struct Arg {
    /// The TypeScript name: the field's camel case name.
    name: String,
    ty: String,
    /// A field declared `optional` in proto: it goes in the trailing options.
    optional: bool,
    /// A `repeated` field: the parameter is read-only, the message init's
    /// array is not, so the request copies it.
    list: bool,
}

/// The parameters of `export` bound to the fields of the message its args
/// cross as, in parameter order.
///
/// # Errors
///
/// An args message the descriptor set lacks, one whose field names are not
/// exactly the parameter names, a zero-parameter export whose args are not
/// `google.protobuf.Empty`, or a parameter that cannot be spelled safely.
fn args_of(
    contract: &Contract,
    imports: &mut Imports,
    export: &Export,
) -> anyhow::Result<Vec<Arg>> {
    let args = args_name(export);
    let message = contract.message(args)?;
    let at = || format!("`{}` (args `{args}`)", export.method);
    if export.params.is_empty() {
        ensure!(
            args == "google.protobuf.Empty",
            "{} takes no parameter, so its args are `google.protobuf.Empty`",
            at()
        );
    }
    let wanted: BTreeSet<String> = export.params.iter().map(Param::json_name).collect();
    let declared: BTreeSet<String> = message.fields().map(|f| f.json_name().to_owned()).collect();
    ensure!(
        wanted == declared,
        "{}: the message declares fields {declared:?} but the parameters are {wanted:?}",
        at()
    );
    let mut bound = Vec::new();
    for param in &export.params {
        let name = param.json_name();
        ensure!(
            !RESERVED.contains(&name.as_str()),
            "{}: parameter `{name}` takes a name the call context owns ({})",
            at(),
            RESERVED.join(", ")
        );
        let field = message
            .fields()
            .find(|f| f.json_name() == name)
            .ok_or_else(|| anyhow!("{}: no field `{name}`", at()))?;
        bound.push(Arg {
            ty: field_ts(imports, &field),
            list: field.is_list(),
            optional: field
                .field_descriptor_proto()
                .proto3_optional
                .unwrap_or(false),
            name,
        });
    }
    Ok(bound)
}

/// The full proto name of the message `export`'s arguments cross as.
fn args_name(export: &Export) -> &str {
    export.args.full_name()
}

/// The full proto name of the options message of `client`, when it has one.
fn options_name(client: &Client) -> Option<&str> {
    client
        .options
        .as_ref()
        .map(|options| options.full_name.as_str())
}

/// How a TypeScript value is read out of the message bytes `bytes` of `wire`:
/// its type and the expression. A scalar wrapper is unwrapped to its value;
/// `Timestamp` and `Duration` stay the protobuf-es well-known messages, and
/// `Empty` is no value.
fn decoded(
    contract: &Contract,
    imports: &mut Imports,
    wire: &Wire,
    bytes: &str,
) -> anyhow::Result<Option<(String, String)>> {
    if matches!(wire, Wire::Empty) {
        return Ok(None);
    }
    let name = imports.message(&contract.message(wire.full_name())?);
    let read = format!("fromBinary({name}Schema, {bytes})");
    Ok(Some(match wire {
        Wire::Wrapper(wrapper) => match wrapper {
            Wrapper::Bool => ("boolean".to_owned(), format!("{read}.value")),
            Wrapper::Int32 | Wrapper::UInt32 | Wrapper::Double => {
                ("number".to_owned(), format!("{read}.value"))
            }
            Wrapper::Int64 | Wrapper::UInt64 => ("bigint".to_owned(), format!("{read}.value")),
            Wrapper::String => ("string".to_owned(), format!("{read}.value")),
            Wrapper::Bytes => ("Uint8Array".to_owned(), format!("{read}.value")),
            Wrapper::Timestamp | Wrapper::Duration => (name, read),
        },
        _ => (name, read),
    }))
}

/// The message `item` of a list or batch: those handles decode through the
/// item's schema, so the item is a generated message.
fn item_of(
    contract: &Contract,
    imports: &mut Imports,
    item: &Wire,
    at: &str,
) -> anyhow::Result<String> {
    ensure!(
        matches!(item, Wire::Message(_)),
        "{at}: a list or batch yields a generated message, not `{}`",
        item.full_name()
    );
    Ok(imports.message(&contract.message(item.full_name())?))
}

fn pascal(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The TypeScript type of the options `export` takes: its call options
/// (list options for a list) and the optional fields of its args. The second
/// value is the declaration of a named options type, when there are optional
/// fields to name.
fn options_type(export: &Export, args: &[Arg]) -> (String, Option<String>) {
    let list = is_list(export);
    let base = if list { "ListOptions" } else { "CallOptions" };
    let own: Vec<&Arg> = args
        .iter()
        .filter(|arg| arg.optional && !(list && arg.name == "pageSize"))
        .collect();
    if own.is_empty() {
        return (base.to_owned(), None);
    }
    let name = format!("{}Options", pascal(&typescript_name(&export.method)));
    let mut text = format!("export type {name} = {base} & {{\n");
    for arg in own {
        let _ = writeln!(text, "  readonly {}?: {};", arg.name, arg.ty);
    }
    text.push_str("};\n");
    (name, Some(text))
}

fn is_list(export: &Export) -> bool {
    matches!(
        export.ret,
        Return::Handle {
            kind: HandleKind::List,
            ..
        }
    )
}

/// Whether `arg` of `export` travels in the trailing options rather than
/// positionally: an optional field, or the page size of a list, which
/// `ListOptions` carries.
fn in_options(export: &Export, arg: &Arg) -> bool {
    arg.optional || (is_list(export) && arg.name == "pageSize")
}

/// The `request` bytes of a call: the args message, built from the
/// parameters and the optional fields of the options.
fn request_ts(export: &Export, schema: &str, args: &[Arg]) -> String {
    let members: Vec<String> = args
        .iter()
        .map(|arg| {
            if in_options(export, arg) {
                format!("{0}: opts?.{0}", arg.name)
            } else if arg.list {
                format!("{0}: [...{0}]", arg.name)
            } else {
                arg.name.clone()
            }
        })
        .collect();
    let init = if members.is_empty() {
        String::new()
    } else {
        format!(", {{ {} }}", members.join(", "))
    };
    format!("toBinary({schema}, create({schema}{init}))")
}

/// One method of a generated client.
struct Method {
    /// The class member.
    text: String,
    /// The name and declaration of the options type the method names, when
    /// it declares one.
    options: Option<(String, String)>,
    /// The signature, as the surface declares it.
    signature: String,
}

/// The TypeScript method for `export`, indented as a class member.
///
/// # Errors
///
/// As [`args_of`], and a list or batch item that is not a generated message.
fn method_ts(
    contract: &Contract,
    imports: &mut Imports,
    export: &Export,
    wasm_name: &str,
) -> anyhow::Result<Method> {
    let name = typescript_name(&export.method);
    let args = args_of(contract, imports, export)?;
    let (opts_ty, declaration) = options_type(export, &args);
    let schema = format!(
        "{}Schema",
        imports.message(&contract.message(args_name(export))?)
    );
    let request = request_ts(export, &schema, &args);
    let mut params: Vec<String> = args
        .iter()
        .filter(|arg| !in_options(export, arg))
        .map(|arg| format!("{}: {}", arg.name, arg.ty))
        .collect();
    if !export.sync || declaration.is_some() {
        params.push(format!("opts?: {opts_ty}"));
    }
    let params = params.join(", ");
    let at = format!("`{}`", export.method);
    let call = format!("this.#wasm.{wasm_name}");
    let ctx = "const ctx = callCtx(opts?.signal);\n    ";
    let tail = "request, ctx.traceparent, ctx.tracestate, ctx.signal";
    let guard = "    } catch (rejection) {\n      throw mapError(rejection);\n    }\n  }\n";
    let (ret, text) = match &export.ret {
        Return::Handle { kind, item } => {
            let item_ty = item_of(contract, imports, item, &at)?;
            match kind {
                HandleKind::List => (
                    format!("ListCall<{item_ty}>"),
                    format!(
                        "  {name}({params}): ListCall<{item_ty}> {{\n    {ctx}const request = {request};\n    return listCallOf<{item_ty}, Uint8Array>({{\n      openPages: (prefetch, firstOnly) => {call}({tail}, prefetch ?? opts?.prefetch, firstOnly ?? false),\n      decodePage: (delivered) => decodePage(delivered, {item_ty}Schema),\n      mapError,\n      sink,\n    }});\n  }}\n"
                    ),
                ),
                HandleKind::Stream => (
                    format!("AsyncIterable<{item_ty}>"),
                    format!(
                        "  {name}({params}): AsyncIterable<{item_ty}> {{\n    {ctx}const request = {request};\n    return iterableOf<{item_ty}>(\n      () => {call}({tail}),\n      (bytes) => fromBinary({item_ty}Schema, bytes),\n      mapError,\n    );\n  }}\n"
                    ),
                ),
                HandleKind::Batch => (
                    format!("AsyncIterable<BatchItem<{item_ty}>[]>"),
                    format!(
                        "  {name}({params}): AsyncIterable<BatchItem<{item_ty}>[]> {{\n    {ctx}const request = {request};\n    return iterableOf<BatchItem<{item_ty}>[]>(\n      () => {call}({tail}),\n      (bytes) => decodeBatchChunk(bytes, {item_ty}Schema),\n      mapError,\n    );\n  }}\n"
                    ),
                ),
            }
        }
        Return::Result { ok, .. } if !export.sync => {
            let (ty, body) = match decoded(contract, imports, ok, "bytes")? {
                Some((ty, expr)) => (
                    ty,
                    format!("const bytes = await {call}({tail});\n      return {expr};"),
                ),
                None => ("void".to_owned(), format!("await {call}({tail});")),
            };
            (
                format!("Promise<{ty}>"),
                format!(
                    "  async {name}({params}): Promise<{ty}> {{\n    {ctx}const request = {request};\n    try {{\n      {body}\n{guard}"
                ),
            )
        }
        Return::Value(wire) | Return::Result { ok: wire, .. } => {
            let (ty, body) = match decoded(contract, imports, wire, "bytes")? {
                Some((ty, expr)) => (
                    ty,
                    format!("const bytes = {call}(request);\n      return {expr};"),
                ),
                None => ("void".to_owned(), format!("{call}(request);")),
            };
            (
                ty.clone(),
                format!(
                    "  {name}({params}): {ty} {{\n    const request = {request};\n    try {{\n      {body}\n{guard}"
                ),
            )
        }
        Return::Unit => (
            "void".to_owned(),
            format!(
                "  {name}({params}): void {{\n    const request = {request};\n    try {{\n      {call}(request);\n{guard}"
            ),
        ),
    };
    Ok(Method {
        text,
        options: declaration.map(|text| (opts_ty, text)),
        signature: format!("  {name}({params}): {ret};\n"),
    })
}

/// What `client.ts` is made of, and the other generated files refer to.
struct Rendered {
    /// `client.ts`.
    client: String,
    /// The messages and enums the signatures name.
    imports: Imports,
    /// The names `client.ts` declares for callers: the method options types,
    /// the options of the entry and its result.
    declared: Vec<String>,
    /// The declarations of the surface, after its imports.
    surface: String,
    /// The entry function's name.
    entry: String,
}

/// `Svc` of the service `svc`, as a type-name prefix.
fn service_pascal(service: &str) -> String {
    service
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(pascal)
        .collect()
}

fn lower_first(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The options the clients' options messages declare, as one list in order of
/// first appearance.
///
/// # Errors
///
/// An options message the descriptor set lacks, an option that takes the name
/// of the entry's base address, or one name with two types.
fn client_options(
    contract: &Contract,
    imports: &mut Imports,
    model: &Model,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut union: Vec<(String, String)> = Vec::new();
    for client in &model.clients {
        let Some(full) = options_name(client) else {
            continue;
        };
        for field in contract.message(full)?.fields() {
            let name = field.json_name().to_owned();
            ensure!(
                name != "baseUrl",
                "option `{name}` of `{}` takes the name of the entry's base address",
                client.ty
            );
            let ty = field_ts(imports, &field);
            match union.iter().find(|(seen, _)| *seen == name) {
                Some((_, seen)) if *seen != ty => {
                    return Err(anyhow!(
                        "option `{name}` of `{}` is `{ty}` but another client takes it as `{seen}`",
                        client.ty
                    ));
                }
                Some(_) => {}
                None => union.push((name, ty)),
            }
        }
    }
    Ok(union)
}

/// The wasm class of `client`: the glue's name for it.
fn wasm_class(package: &Package, client: &Client) -> anyhow::Result<String> {
    let constructor = export_name(&package.service, &client.ty, "new")?;
    Ok(constructor.trim_end_matches("__new").to_owned())
}

/// `client.ts`, with what the other generated files need of it.
///
/// # Errors
///
/// As [`method_ts`] and [`client_options`], and an async export `l2-methods`
/// does not list.
fn client_ts(
    model: &Model,
    package: &Package,
    document: &L2Methods,
    contract: &Contract,
    glue: &str,
) -> anyhow::Result<Rendered> {
    let listed: BTreeSet<&str> = document.methods.iter().map(|m| m.export.as_str()).collect();
    let mut imports = Imports::default();
    let union = client_options(contract, &mut imports, model)?;
    let service = service_pascal(&package.service);
    let entry = format!("{}Sdk", lower_first(&service));
    let sdk = format!("{service}Sdk");
    let options_ty = format!("{sdk}Options");
    let mut classes = String::new();
    let mut surface_classes = String::new();
    let mut declarations = String::new();
    let mut declared = Vec::new();
    let mut fields = String::new();
    let mut construct = String::new();
    for client in &model.clients {
        let ty = &client.ty;
        let class = wasm_class(package, client)?;
        let _ = write!(
            classes,
            "\nexport class {ty} {{\n  readonly #wasm: glue.{class};\n\n  /** Built by `{entry}`; not for callers. */\n  constructor(wasm: glue.{class}) {{\n    this.#wasm = wasm;\n  }}\n"
        );
        let _ = write!(
            surface_classes,
            "\nexport declare class {ty} {{\n  private constructor();\n"
        );
        for export in &client.exports {
            let wasm_name = export_name(&package.service, ty, &export.method)?;
            if !export.sync && !listed.contains(wasm_name.as_str()) {
                return Err(anyhow!("`{wasm_name}` is not in l2-methods"));
            }
            let method = method_ts(contract, &mut imports, export, &wasm_name)?;
            classes.push('\n');
            classes.push_str(&method.text);
            surface_classes.push_str(&method.signature);
            if let Some((name, text)) = method.options {
                declarations.push_str(&text);
                declarations.push('\n');
                declared.push(name);
            }
        }
        classes.push_str("}\n");
        surface_classes.push_str("}\n");
        let _ = writeln!(fields, "  readonly {}: {ty};", lower_first(ty));
        let options = match options_name(client) {
            None => "new Uint8Array(0)".to_owned(),
            Some(full) => {
                let message = contract.message(full)?;
                let schema = format!("{}Schema", imports.message(&message));
                let picks: Vec<String> = message
                    .fields()
                    .map(|f| format!("{0}: opts?.{0}", f.json_name()))
                    .collect();
                format!(
                    "toBinary({schema}, create({schema}, {{ {} }}))",
                    picks.join(", ")
                )
            }
        };
        let _ = writeln!(
            construct,
            "      {}: new {ty}(wasm.{class}.{class}__new(baseUrl, hostOf(session, sink), {options}, WRAPPER_BOUNDARY, RUNTIME_BOUNDARY)),",
            lower_first(ty)
        );
    }
    let mut options_decl = format!(
        "/** What the entry takes besides the session: the server's address and the options of its clients. */\nexport interface {options_ty} {{\n  /** The server's address; the page's own origin when omitted. */\n  readonly baseUrl?: string;\n"
    );
    for (name, ty) in &union {
        let _ = writeln!(options_decl, "  readonly {name}?: {ty};");
    }
    options_decl.push_str("}\n");
    let sdk_decl = format!(
        "/** Every client of the service, opened from one session. */\nexport interface {sdk} {{\n{fields}}}\n"
    );
    let entry_fn = format!(
        "\n/** Opens every client of the service as `session` says, each with its own host; the base address is the page's own origin when omitted. */\nexport async function {entry}(session: SdkSession, opts?: {options_ty}): Promise<{sdk}> {{\n  const baseUrl = opts?.baseUrl ?? pageOrigin();\n  const wasm = await init();\n  try {{\n    return {{\n{construct}    }};\n  }} catch (rejection) {{\n    throw mapError(rejection);\n  }}\n}}\n"
    );
    let mut client = String::from(HEADER);
    client.push_str(
        "import { create, fromBinary, toBinary } from \"@bufbuild/protobuf\";\nimport {\n  callCtx,\n  decodeBatchChunk,\n  decodePage,\n  hostOf,\n  iterableOf,\n  JsSpanSink,\n  listCallOf,\n  mapCallError,\n  pageOrigin,\n  RUNTIME_BOUNDARY,\n  type BatchItem,\n  type ListCall,\n  type SdkSession,\n} from \"@brefwiz/api-bones-sdk-runtime\";\n",
    );
    let _ = writeln!(
        client,
        "import type * as glue from \"./{glue}.js\";\nimport {{ init, WRAPPER_BOUNDARY }} from \"./init.js\";\nimport type {{ CallOptions, ListOptions }} from \"./types.js\";"
    );
    client.push_str(&imports.lines("./", false));
    client.push_str(
        "\nconst sink = new JsSpanSink();\n\nfunction mapError(rejection: unknown): unknown {\n  return mapCallError(rejection);\n}\n\n",
    );
    client.push_str(&declarations);
    client.push_str(&options_decl);
    client.push('\n');
    client.push_str(&sdk_decl);
    client.push_str(&classes);
    client.push_str(&entry_fn);
    declared.push(options_ty.clone());
    declared.push(sdk.clone());
    let surface = format!(
        "{declarations}{options_decl}\n{sdk_decl}{surface_classes}\nexport declare function {entry}(session: SdkSession, opts?: {options_ty}): Promise<{sdk}>;\n"
    );
    Ok(Rendered {
        client,
        imports,
        declared,
        surface,
        entry,
    })
}

/// `api-surface.d.ts`: every client class with the signatures `client.ts`
/// implements, so a reader of the tracked surface sees the same parameter
/// and return types.
fn surface_dts(rendered: &Rendered) -> String {
    let mut out = String::from(HEADER);
    out.push_str(
        "import type { BatchItem, ListCall, SdkSession } from \"@brefwiz/api-bones-sdk-runtime\";\nimport type { MessageInitShape } from \"@bufbuild/protobuf\";\n",
    );
    let _ = writeln!(
        out,
        "import type {{ CallOptions, ListOptions }} from \"./{SOURCE_DIR}/types.js\";"
    );
    out.push_str(&rendered.imports.lines(&format!("./{SOURCE_DIR}/"), true));
    out.push('\n');
    out.push_str(&rendered.surface);
    out
}

fn index_ts(model: &Model, rendered: &Rendered) -> String {
    let mut out = String::from(HEADER);
    let clients: Vec<&str> = model.clients.iter().map(|c| c.ty.as_str()).collect();
    let _ = writeln!(
        out,
        "export {{ {}, {} }} from \"./client.js\";",
        clients.join(", "),
        rendered.entry
    );
    let _ = writeln!(
        out,
        "export type {{ {} }} from \"./client.js\";",
        rendered.declared.join(", ")
    );
    out.push_str(
        "export type { BatchItem, CallOptions, CredentialSource, ListOptions, SdkSession } from \"./types.js\";\n",
    );
    out.push_str(&rendered.imports.reexports());
    out.push_str(
        "export * from \"./errors.js\";\nexport { init, sdkBuildInfo } from \"./init.js\";\n",
    );
    out
}

fn types_ts() -> String {
    let mut out = String::from(HEADER);
    out.push_str(
        "import type { PageOptions } from \"@brefwiz/api-bones-sdk-runtime\";\n\nexport type { BatchItem, Credential, CredentialAsk, CredentialSource, SdkSession } from \"@brefwiz/api-bones-sdk-runtime\";\n\n",
    );
    out.push_str(
        "/** Per-call options. */\nexport interface CallOptions {\n  readonly signal?: AbortSignal;\n}\n\n",
    );
    out.push_str(
        "/** Per-call options of a list: the page size, how many pages it reads ahead (0 or 1) and the signal that cancels it. */\nexport type ListOptions = Pick<PageOptions, \"pageSize\" | \"prefetch\" | \"signal\">;\n",
    );
    out
}

fn errors_ts(model: &Model) -> String {
    let mut out = String::from(HEADER);
    out.push_str(
        "export { BonesError, PageError, isCursorRejected } from \"@brefwiz/api-bones-sdk-runtime\";\n",
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
/// `@CRATE@`, `@VERSION@`, `@SHA256@` and `@BOUNDARY@` are replaced.
///
/// The digest is the one `build-ts` stamped when it built the module. The
/// loader hashes the bytes it loads only when the run collects evidence, and
/// then refuses a module that is not the stamped one; otherwise the module is
/// handed to the engine as it is, so it streams. The digest check is
/// evidence-only: hashing needs the whole module in memory before it compiles,
/// so a run that collects no evidence keeps the streaming load.
///
/// The behavior is proven in `tests/internal-bdd/tests/features/sdk_wasm_integrity.feature`.
const INIT_TS: &str = r#"import { lazyOnce, wasmInput } from "@brefwiz/api-bones-sdk-runtime";
import initGlue, * as glue from "./@GLUE@.js";

export type Glue = typeof glue;

/** The `bones.sdk.v1` boundary version this wrapper speaks. */
export const WRAPPER_BOUNDARY = @BOUNDARY@;

/** The SHA-256 of the wasm module, stamped when the package was built. */
const WASM_SHA256 = "@SHA256@";

type Host = { process?: { env?: Record<string, string | undefined> } };

/** The directory the run collects evidence in, when it does. */
function evidenceDirectory(): string | undefined {
  return (globalThis as Host).process?.env?.BREFWIZ_SDK_EVIDENCE || undefined;
}

/** Checks the module against the stamp and appends its provenance line. */
async function recordProvenance(directory: string, bytes: Uint8Array<ArrayBuffer>): Promise<void> {
  const hash = await globalThis.crypto.subtle.digest("SHA-256", bytes);
  const wasmSha256 = Array.from(new Uint8Array(hash), (b) => b.toString(16).padStart(2, "0")).join("");
  if (wasmSha256 !== WASM_SHA256) {
    throw new Error(`the wasm module is ${wasmSha256}, not the stamped ${WASM_SHA256}`);
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
  const directory = evidenceDirectory();
  if (directory === undefined) {
    await initGlue({ module_or_path: input });
    return glue;
  }
  const bytes = input instanceof URL ? new Uint8Array(await (await globalThis.fetch(input)).arrayBuffer()) : new Uint8Array(input);
  await recordProvenance(directory, bytes);
  await initGlue({ module_or_path: bytes });
  return glue;
});

/** The wasm module this package loads: its stamped SHA-256, crate and version. */
export async function sdkBuildInfo(): Promise<{ wasmSha256: string; crate: string; version: string }> {
  await init();
  return { wasmSha256: WASM_SHA256, crate: "@CRATE@", version: "@VERSION@" };
}
"#;

fn init_ts(package: &Package, glue: &str) -> String {
    let mut out = String::from(HEADER);
    out.push_str(
        &INIT_TS
            .replace("@GLUE@", glue)
            .replace("@CRATE@", &package.crate_name)
            .replace("@VERSION@", WRAPPER_PLACEHOLDER_VERSION)
            .replace("@SHA256@", &package.wasm_sha256)
            .replace("@BOUNDARY@", &WRAPPER_BOUNDARY.to_string()),
    );
    out
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
    fn a_package_without_a_name_or_a_license_is_refused() {
        let at = "\"manifest_path\":\"/w/Cargo.toml\"";
        for (fields, message) in [
            (
                "\"version\":\"1.0.0\",\"license\":\"MIT\"",
                "names no crate",
            ),
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
            license: "MIT".to_owned(),
            private,
            runtime: format!(">={RUNTIME_FLOOR}"),
            registry: Some(TEST_REGISTRY.to_owned()),
            spec: SpecL2Methods::default(),
            wasm_sha256: String::new(),
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
            assert_eq!(published["version"], WRAPPER_PLACEHOLDER_VERSION);
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
    fn the_floor_contract_lane_pins_the_runtime_at_the_floor() {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/typescript-floor/package.json");
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(manifest).expect("the floor lane manifest"),
        )
        .expect("json");
        assert_eq!(
            manifest["devDependencies"][RUNTIME_PACKAGE], RUNTIME_FLOOR,
            "the floor lane installs the runtime at exactly the floor"
        );
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
    fn the_package_exports_one_entry_with_its_types_and_depends_on_protobuf_es() {
        let document = read_manifest(&package_for(false), ">=0.2.0");
        assert_eq!(
            document["exports"],
            serde_json::json!({
                ".": {
                    "types": "./dist/generated-l2/index.d.ts",
                    "import": "./dist/generated-l2/index.js"
                }
            })
        );
        assert_eq!(
            document["dependencies"]["@bufbuild/protobuf"],
            PROTOBUF_ES_RANGE
        );
    }

    #[test]
    fn the_compiler_reads_the_protobuf_es_output_next_to_the_generated_sources() {
        assert!(tsconfig().contains("\"include\": [\"src/generated/**/*.ts\"]"));
    }

    const PROTO: &str = r#"syntax = "proto3";
package t.v1;
import "google/protobuf/duration.proto";
import "google/protobuf/empty.proto";
import "google/protobuf/timestamp.proto";
import "google/protobuf/wrappers.proto";
enum Color { COLOR_UNSPECIFIED = 0; COLOR_RED = 1; }
message Widget { string id = 1; }
message Number { uint32 value = 1; }
message Tick { int64 at = 1; }
message GetWidgetArgs { string id = 1; optional string label = 2; }
message FindArgs {
  repeated string tags = 1;
  map<string, int64> counts = 2;
  Color color = 3;
  google.protobuf.Timestamp since = 4;
}
message ListNumbersArgs { uint32 limit = 1; optional uint32 page_size = 2; }
message WatchArgs { string from = 1; }
message ChunksArgs { string key = 1; }
message CountArgs { string key = 1; }
message NoFields {}
message SignalArgs { string signal = 1; }
message LabelArgs { string kind = 1; optional string suffix = 2; }
message WrongArgs { string other = 1; }
message WidgetsOptions { string tenant = 1; optional int64 retries = 2; }
message OtherOptions { string tenant = 1; }
message MismatchOptions { int64 tenant = 1; }
message BaseOptions { string base_url = 1; }
"#;

    const WIDGETS: &str = r#"
#[client(surface = "s", proto = generated::t::v1)]
impl Widgets {
    #[export]
    pub async fn get_widget(&self, id: String, label: Option<String>) -> Result<generated::t::v1::Widget, Error> { body() }
    #[export]
    pub async fn find(&self, tags: Vec<String>, counts: HashMap<String, i64>, color: Color, since: SystemTime) -> Result<(), Error> { body() }
    #[export]
    pub async fn ping(&self) -> Result<(), Error> { body() }
    #[export]
    pub async fn list_numbers(&self, limit: u32, page_size: Option<u32>) -> ListCall<generated::t::v1::Number> { body() }
    #[export]
    pub async fn watch(&self, from: String) -> Stream<generated::t::v1::Tick> { body() }
    #[export]
    pub async fn chunks(&self, key: String) -> Batch<generated::t::v1::Widget> { body() }
    #[export(sync)]
    pub fn label(&self, kind: &str, suffix: Option<String>) -> String { body() }
    #[export(sync)]
    pub fn count(&self, key: String) -> Result<i64, Error> { body() }
}
"#;

    fn model_of(source: &str) -> Model {
        Model::from_source(source).expect("a model")
    }

    fn spec_of(model: &Model) -> SpecL2Methods {
        let mut spec = SpecL2Methods::default();
        spec.surfaces.insert("s".to_owned());
        for export in model.clients.iter().flat_map(|c| &c.exports) {
            spec.endpoints.insert(
                format!("t.v1.W/{}", export.method),
                vec![export.method.clone()],
            );
        }
        spec
    }

    fn generated(source: &str, proto: &str) -> anyhow::Result<BTreeMap<String, String>> {
        let model = model_of(source);
        let mut package = package_for(false);
        package.spec = spec_of(&model);
        package.wasm_sha256 = "ab".repeat(32);
        let contract = testkit::compile(proto);
        generate(&model, &package, &contract).map(|files| files.into_iter().collect())
    }

    fn client() -> String {
        generated(WIDGETS, PROTO)
            .expect("generated")
            .remove("src/generated/client.ts")
            .expect("client.ts")
    }

    fn refused(source: &str, proto: &str) -> String {
        format!("{:#}", generated(source, proto).expect_err("refused"))
    }

    #[test]
    fn required_parameters_are_positional_and_optional_fields_ride_the_trailing_options() {
        let client = client();
        assert!(
            client.contains(
                "  async getWidget(id: string, opts?: GetWidgetOptions): Promise<Widget> {"
            )
        );
        assert!(client.contains(
            "export type GetWidgetOptions = CallOptions & {\n  readonly label?: string;\n};"
        ));
        assert!(client.contains(
            "toBinary(GetWidgetArgsSchema, create(GetWidgetArgsSchema, { id, label: opts?.label }))"
        ));
        assert!(client.contains("return fromBinary(WidgetSchema, bytes);"));
    }

    #[test]
    fn a_call_is_built_from_the_args_message_and_the_call_context() {
        let client = client();
        assert!(client.contains("const ctx = callCtx(opts?.signal);"));
        assert!(client.contains(
            "await this.#wasm.svc__Widgets__get_widget(request, ctx.traceparent, ctx.tracestate, ctx.signal)"
        ));
        assert!(client.contains("throw mapError(rejection);"));
    }

    #[test]
    fn field_types_follow_the_proto_kinds() {
        let client = client();
        assert!(client.contains(
            "async find(tags: readonly (string)[], counts: Readonly<Record<string, bigint>>, color: Color, since: MessageInitShape<typeof TimestampSchema>, opts?: CallOptions): Promise<void> {"
        ));
        assert!(client.contains("create(FindArgsSchema, { tags: [...tags], counts,"));
        assert!(client.contains("type Color"));
        assert!(client.contains("type Widget"));
        assert!(client.contains("WidgetSchema"));
        assert!(client.contains("from \"@bufbuild/protobuf/wkt\";"));
        assert!(client.contains("from \"./proto/t/v1/t_pb.js\";"));
    }

    #[test]
    fn a_unit_result_awaits_and_a_zero_parameter_export_builds_the_empty_message() {
        let client = client();
        assert!(client.contains("async ping(opts?: CallOptions): Promise<void> {"));
        assert!(client.contains("toBinary(EmptySchema, create(EmptySchema))"));
        assert!(client.contains(
            "      await this.#wasm.svc__Widgets__ping(request, ctx.traceparent, ctx.tracestate, ctx.signal);"
        ));
    }

    #[test]
    fn a_list_takes_the_list_options_and_reads_its_page_size_there() {
        let client = client();
        assert!(
            client.contains(
                "  listNumbers(limit: number, opts?: ListOptions): ListCall<t_v1_Number> {"
            )
        );
        assert!(
            client.contains("create(ListNumbersArgsSchema, { limit, pageSize: opts?.pageSize })")
        );
        assert!(client.contains(
            "openPages: (prefetch, firstOnly) => this.#wasm.svc__Widgets__list_numbers(request, ctx.traceparent, ctx.tracestate, ctx.signal, prefetch ?? opts?.prefetch, firstOnly ?? false),"
        ));
        assert!(
            client.contains("decodePage: (delivered) => decodePage(delivered, t_v1_NumberSchema),")
        );
        assert!(!client.contains("GetNumbersOptions"));
        assert!(!client.contains("ListNumbersOptions"));
    }

    #[test]
    fn a_message_named_like_a_global_is_imported_under_its_package_name() {
        let client = client();
        assert!(client.contains("type Number as t_v1_Number"));
        assert!(client.contains("NumberSchema as t_v1_NumberSchema"));
    }

    #[test]
    fn a_stream_decodes_each_item_and_a_batch_each_chunk() {
        let client = client();
        assert!(client.contains("watch(from: string, opts?: CallOptions): AsyncIterable<Tick> {"));
        assert!(client.contains("(bytes) => fromBinary(TickSchema, bytes),"));
        assert!(client.contains(
            "chunks(key: string, opts?: CallOptions): AsyncIterable<BatchItem<Widget>[]> {"
        ));
        assert!(client.contains("(bytes) => decodeBatchChunk(bytes, WidgetSchema),"));
    }

    #[test]
    fn a_sync_export_has_no_call_context_and_takes_options_only_for_optional_fields() {
        let client = client();
        assert!(client.contains("  label(kind: string, opts?: LabelOptions): string {"));
        assert!(client.contains("const bytes = this.#wasm.svc__Widgets__label(request);"));
        assert!(client.contains("return fromBinary(StringValueSchema, bytes).value;"));
        assert!(client.contains("  count(key: string): bigint {"));
        assert!(client.contains("return fromBinary(Int64ValueSchema, bytes).value;"));
    }

    #[test]
    fn the_entry_opens_every_client_from_one_session_and_clients_have_no_create() {
        let client = client();
        assert!(!client.contains("static create"));
        assert!(client.contains(
            "export async function svcSdk(session: SdkSession, opts?: SvcSdkOptions): Promise<SvcSdk> {"
        ));
        assert!(client.contains("const baseUrl = opts?.baseUrl ?? pageOrigin();"));
        assert!(!client.contains("const host ="));
        assert!(client.contains(
            "widgets: new Widgets(wasm.svc__Widgets.svc__Widgets__new(baseUrl, hostOf(session, sink), new Uint8Array(0), WRAPPER_BOUNDARY, RUNTIME_BOUNDARY)),"
        ));
        assert!(client.contains("export interface SvcSdk {\n  readonly widgets: Widgets;\n}"));
        assert!(client.contains("  readonly baseUrl?: string;\n}"));
    }

    #[test]
    fn the_surface_declares_the_signatures_the_client_implements() {
        let files = generated(WIDGETS, PROTO).expect("generated");
        let surface = &files["api-surface.d.ts"];
        assert!(
            surface.contains("  getWidget(id: string, opts?: GetWidgetOptions): Promise<Widget>;")
        );
        assert!(surface.contains("export declare class Widgets {\n  private constructor();"));
        assert!(surface.contains(
            "export declare function svcSdk(session: SdkSession, opts?: SvcSdkOptions): Promise<SvcSdk>;"
        ));
        assert!(surface.contains("import type { MessageInitShape } from \"@bufbuild/protobuf\";"));
        assert!(surface.contains("from \"./src/generated/proto/t/v1/t_pb.js\";"));
        assert!(!surface.contains("static create"));
    }

    #[test]
    fn the_index_exports_the_entry_the_options_and_the_message_types() {
        let files = generated(WIDGETS, PROTO).expect("generated");
        let index = &files["src/generated/index.ts"];
        assert!(index.contains("export { Widgets, svcSdk } from \"./client.js\";"));
        assert!(index.contains("GetWidgetOptions"));
        assert!(index.contains("SvcSdkOptions, SvcSdk }"));
        assert!(index.contains("CredentialSource"));
        assert!(index.contains("Number as t_v1_Number"));
        assert!(index.contains("from \"@bufbuild/protobuf/wkt\";"));
        assert!(index.contains("export { init, sdkBuildInfo } from \"./init.js\";"));
    }

    #[test]
    fn the_types_module_takes_the_credential_source_and_the_session_from_the_runtime() {
        let files = generated(WIDGETS, PROTO).expect("generated");
        let types = &files["src/generated/types.ts"];
        assert!(types.contains("export type { BatchItem, Credential, CredentialAsk, CredentialSource, SdkSession } from \"@brefwiz/api-bones-sdk-runtime\";"));
        assert!(types.contains("Pick<PageOptions, \"pageSize\" | \"prefetch\" | \"signal\">"));
        assert!(!types.contains("revive"));
    }

    #[test]
    fn the_stamped_digest_and_the_boundary_are_in_init() {
        let files = generated(WIDGETS, PROTO).expect("generated");
        let init = &files["src/generated/init.ts"];
        assert!(init.contains(&format!("const WASM_SHA256 = \"{}\";", "ab".repeat(32))));
        assert!(init.contains(&format!(
            "export const WRAPPER_BOUNDARY = {WRAPPER_BOUNDARY};"
        )));
        assert!(init.contains("BREFWIZ_SDK_EVIDENCE"));
        assert!(init.contains("await initGlue({ module_or_path: input });"));
        for placeholder in ["@GLUE@", "@CRATE@", "@VERSION@", "@SHA256@", "@BOUNDARY@"] {
            assert!(!init.contains(placeholder), "{placeholder}");
        }
    }

    fn init_of_package() -> String {
        generated(WIDGETS, PROTO)
            .expect("generated")
            .remove("src/generated/init.ts")
            .expect("init.ts")
    }

    #[test]
    fn init_hashes_the_module_only_when_the_run_collects_evidence() {
        let init = init_of_package();
        let gate = init
            .find("const directory = evidenceDirectory();")
            .expect("the gate reads the evidence directory");
        let plain = init[gate..]
            .find("if (directory === undefined) {\n    await initGlue({ module_or_path: input });\n    return glue;\n  }")
            .expect("no evidence: the module streams as it is");
        let hashed = init[gate..]
            .find("await recordProvenance(directory, bytes);")
            .expect("evidence: the module is checked");
        assert!(
            plain < hashed,
            "the unhashed path returns before any hashing"
        );
        assert!(init.contains(
            "return (globalThis as Host).process?.env?.BREFWIZ_SDK_EVIDENCE || undefined;"
        ));
        assert_eq!(
            init.matches("subtle.digest").count(),
            1,
            "the one hash is inside recordProvenance"
        );
        let record = init
            .find("async function recordProvenance")
            .expect("record");
        let digest = init.find("subtle.digest").expect("digest");
        let load = init.find("export const init = lazyOnce").expect("init");
        assert!(record < digest && digest < load);
    }

    #[test]
    fn init_refuses_a_module_that_is_not_the_stamped_one_before_loading_it() {
        let init = init_of_package();
        let stamp = "ab".repeat(32);
        assert!(init.contains(
            "if (wasmSha256 !== WASM_SHA256) {\n    throw new Error(`the wasm module is ${wasmSha256}, not the stamped ${WASM_SHA256}`);\n  }"
        ));
        assert!(init.contains(&format!("const WASM_SHA256 = \"{stamp}\";")));
        let check = init.find("wasmSha256 !== WASM_SHA256").expect("check");
        let write = init.find("fs.appendFile").expect("provenance line");
        assert!(check < write, "nothing is recorded for a refused module");
        let verified = init
            .find("await recordProvenance(directory, bytes);")
            .expect("verified");
        let load = init
            .find("await initGlue({ module_or_path: bytes });")
            .expect("loaded");
        assert!(verified < load, "the module loads only once it matched");
        assert!(init.contains("wasmSha256,\n  });"));
        assert!(init.contains("schema: \"api-bones.sdk-provenance/v1\""));
    }

    #[test]
    fn init_hashes_the_bytes_it_loads_for_a_url_input_and_a_buffer_input_alike() {
        let init = init_of_package();
        assert!(init.contains(
            "const bytes = input instanceof URL ? new Uint8Array(await (await globalThis.fetch(input)).arrayBuffer()) : new Uint8Array(input);"
        ));
    }

    #[test]
    fn the_wrapper_speaks_boundary_one() {
        assert_eq!(WRAPPER_BOUNDARY, 1);
    }

    #[test]
    fn an_args_message_the_descriptor_set_lacks_is_refused_by_name() {
        let proto = PROTO.replace("message GetWidgetArgs", "message Renamed");
        let message = refused(WIDGETS, &proto);
        assert!(
            message.contains("`t.v1.GetWidgetArgs` is missing from the descriptor set"),
            "{message}"
        );
    }

    #[test]
    fn an_args_message_whose_fields_are_not_the_parameters_is_refused() {
        let source = WIDGETS.replace(
            "pub async fn watch(&self, from: String)",
            "pub async fn watch(&self, other: String, extra: String)",
        );
        let source = source.replace(
            "#[export]\n    pub async fn watch",
            "#[export(args = generated::t::v1::WrongArgs)]\n    pub async fn watch",
        );
        let message = refused(&source, PROTO);
        assert!(message.contains("`watch`"), "{message}");
        assert!(message.contains("the message declares fields"), "{message}");
    }

    #[test]
    fn a_zero_parameter_export_arrives_as_the_empty_message() {
        let source = WIDGETS.replace(
            "#[export]\n    pub async fn ping",
            "#[export(args = generated::t::v1::NoFields)]\n    pub async fn ping",
        );
        let message = refused(&source, PROTO);
        assert!(message.contains("`google.protobuf.Empty`"), "{message}");
    }

    #[test]
    fn a_parameter_may_not_take_a_name_the_call_context_owns() {
        let source = WIDGETS.replace(
            "pub async fn ping(&self)",
            "pub async fn ping(&self, signal: String)",
        );
        let source = source.replace(
            "#[export]\n    pub async fn ping",
            "#[export(args = generated::t::v1::SignalArgs)]\n    pub async fn ping",
        );
        let message = refused(&source, PROTO);
        assert!(message.contains("parameter `signal`"), "{message}");
        for name in ["prefetch", "cache", "traceparent", "tracestate"] {
            assert!(RESERVED.contains(&name), "{name}");
        }
    }

    #[test]
    fn a_result_message_the_descriptor_set_lacks_is_refused() {
        let proto = PROTO.replace("message Tick { int64 at = 1; }", "");
        let message = refused(WIDGETS, &proto);
        assert!(message.contains("`t.v1.Tick` is missing"), "{message}");
    }

    #[test]
    fn a_list_or_batch_of_scalars_is_refused() {
        let item = Wire::Wrapper(Wrapper::String);
        let contract = testkit::compile(PROTO);
        let mut imports = Imports::default();
        let message = item_of(&contract, &mut imports, &item, "`names`")
            .expect_err("refused")
            .to_string();
        assert!(message.contains("a generated message, not `google.protobuf.StringValue`"));
    }

    #[test]
    fn a_wrapper_result_is_unwrapped_and_a_well_known_message_is_kept() {
        let contract = testkit::compile(PROTO);
        let mut imports = Imports::default();
        let cases = [
            (Wrapper::Bool, "boolean", "BoolValueSchema"),
            (Wrapper::Int32, "number", "Int32ValueSchema"),
            (Wrapper::UInt32, "number", "UInt32ValueSchema"),
            (Wrapper::Double, "number", "DoubleValueSchema"),
            (Wrapper::Int64, "bigint", "Int64ValueSchema"),
            (Wrapper::UInt64, "bigint", "UInt64ValueSchema"),
            (Wrapper::String, "string", "StringValueSchema"),
            (Wrapper::Bytes, "Uint8Array", "BytesValueSchema"),
        ];
        for (wrapper, ty, schema) in cases {
            let (found, read) = decoded(&contract, &mut imports, &Wire::Wrapper(wrapper), "b")
                .expect("decoded")
                .expect("a value");
            assert_eq!(found, ty);
            assert_eq!(read, format!("fromBinary({schema}, b).value"));
        }
        for (wrapper, name) in [
            (Wrapper::Timestamp, "Timestamp"),
            (Wrapper::Duration, "Duration"),
        ] {
            let (found, read) = decoded(&contract, &mut imports, &Wire::Wrapper(wrapper), "b")
                .expect("decoded")
                .expect("a value");
            assert_eq!(found, name);
            assert_eq!(read, format!("fromBinary({name}Schema, b)"));
        }
        assert!(
            decoded(&contract, &mut imports, &Wire::Empty, "b")
                .expect("decoded")
                .is_none()
        );
    }

    const OPTIONED: &str = r#"
#[client(surface = "s")]
impl A {
    pub fn from_host(base_url: String, host: JsValue, options: generated::t::v1::WidgetsOptions) -> Result<Self, Error> { body() }
    #[export]
    pub async fn ping(&self) -> Result<(), Error> { body() }
}
#[client(surface = "s")]
impl B {
    pub fn from_host(base_url: String, host: JsValue, options: generated::t::v1::OtherOptions) -> Result<Self, Error> { body() }
    #[export]
    pub async fn pong(&self) -> Result<(), Error> { body() }
}
#[client(surface = "s")]
impl C {
    pub fn from_host(base_url: String, host: JsValue) -> Result<Self, Error> { body() }
}
"#;

    #[test]
    fn the_entry_options_are_the_union_of_the_options_of_every_client() {
        let files = generated(OPTIONED, PROTO).expect("generated");
        let client = &files["src/generated/client.ts"];
        assert!(client.contains(
            "export interface SvcSdkOptions {\n  /** The server's address; the page's own origin when omitted. */\n  readonly baseUrl?: string;\n  readonly tenant?: string;\n  readonly retries?: bigint;\n}"
        ));
    }

    #[test]
    fn each_client_sends_only_its_own_options_message() {
        let files = generated(OPTIONED, PROTO).expect("generated");
        let client = &files["src/generated/client.ts"];
        assert!(client.contains(
            "a: new A(wasm.svc__A.svc__A__new(baseUrl, hostOf(session, sink), toBinary(WidgetsOptionsSchema, create(WidgetsOptionsSchema, { tenant: opts?.tenant, retries: opts?.retries })), WRAPPER_BOUNDARY, RUNTIME_BOUNDARY)),"
        ));
        assert!(client.contains(
            "b: new B(wasm.svc__B.svc__B__new(baseUrl, hostOf(session, sink), toBinary(OtherOptionsSchema, create(OtherOptionsSchema, { tenant: opts?.tenant })), WRAPPER_BOUNDARY, RUNTIME_BOUNDARY)),"
        ));
        assert!(client.contains(
            "c: new C(wasm.svc__C.svc__C__new(baseUrl, hostOf(session, sink), new Uint8Array(0), WRAPPER_BOUNDARY, RUNTIME_BOUNDARY)),"
        ));
    }

    #[test]
    fn one_option_with_two_types_is_refused() {
        let source = OPTIONED.replace("OtherOptions", "MismatchOptions");
        let message = refused(&source, PROTO);
        assert!(
            message.contains("option `tenant` of `B` is `bigint`"),
            "{message}"
        );
        assert!(
            message.contains("another client takes it as `string`"),
            "{message}"
        );
    }

    #[test]
    fn an_option_may_not_take_the_name_of_the_base_address() {
        let source = OPTIONED.replace("OtherOptions", "BaseOptions");
        let message = refused(&source, PROTO);
        assert!(message.contains("option `baseUrl` of `B`"), "{message}");
    }

    #[test]
    fn an_options_message_the_descriptor_set_lacks_is_refused() {
        let proto = PROTO.replace("message OtherOptions { string tenant = 1; }", "");
        let message = refused(OPTIONED, &proto);
        assert!(
            message.contains("`t.v1.OtherOptions` is missing"),
            "{message}"
        );
    }

    #[test]
    fn an_async_export_no_l2_methods_entry_lists_is_refused() {
        let model = model_of(WIDGETS);
        let mut package = package_for(false);
        package.spec = spec_of(&model);
        let document = l2_methods(&model, &package.spec, "svc", "c", "@x/y").expect("document");
        let mut trimmed = document.clone();
        trimmed.methods.retain(|m| !m.export.ends_with("__ping"));
        let contract = testkit::compile(PROTO);
        let message = match client_ts(&model, &package, &trimmed, &contract, "c") {
            Err(refused) => refused.to_string(),
            Ok(_) => panic!("an unlisted export is refused"),
        };
        assert!(message.contains("svc__Widgets__ping"), "{message}");
    }

    #[test]
    fn a_nested_message_is_imported_under_its_protobuf_es_name() {
        let contract = testkit::compile(
            "syntax = \"proto3\";\npackage t.v1;\nmessage Outer { message Inner { string a = 1; } }\n",
        );
        let mut imports = Imports::default();
        let inner = contract.message("t.v1.Outer.Inner").expect("message");
        assert_eq!(imports.message(&inner), "Outer_Inner");
        let lines = imports.lines("./", false);
        assert!(lines.contains("type Outer_Inner"), "{lines}");
        assert!(lines.contains("Outer_InnerSchema"), "{lines}");
    }

    #[test]
    fn the_package_prefix_resolves_a_name_two_messages_share() {
        let contract = testkit::compile(
            "syntax = \"proto3\";\npackage a.v1;\nmessage Item { string a = 1; }\nmessage Holder { string b = 1; }\n",
        );
        let other = testkit::compile(
            "syntax = \"proto3\";\npackage b.v1;\nmessage Item { string a = 1; }\n",
        );
        let mut imports = Imports::default();
        let first = imports.message(&contract.message("a.v1.Item").expect("a"));
        let second = imports.message(&other.message("b.v1.Item").expect("b"));
        assert_eq!(first, "Item");
        assert_eq!(second, "b_v1_Item");
        assert_eq!(
            imports.message(&contract.message("a.v1.Item").expect("a")),
            "Item"
        );
    }

    #[test]
    fn a_descriptor_set_must_hold_a_file_and_be_readable() {
        assert!(
            Contract::decode(&[])
                .expect_err("empty")
                .to_string()
                .contains("empty")
        );
        assert!(Contract::decode(&[0xff, 0xff]).is_err());
        let missing = Contract::read(Path::new("/nonexistent/contract.fds")).expect_err("refused");
        assert!(format!("{missing:#}").contains("cannot read the descriptor set"));
        let dir = scratch("fds-invalid");
        std::fs::write(dir.join("c.fds"), [0xff, 0xff]).expect("file");
        let invalid = Contract::read(&dir.join("c.fds")).expect_err("refused");
        assert!(format!("{invalid:#}").contains("invalid descriptor set"));
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }

    #[test]
    fn a_descriptor_set_on_disk_is_read_back() {
        let path = testkit::descriptor_file("read-back");
        let contract = Contract::read(&path).expect("read");
        assert!(contract.message("contract.v1.Widget").is_ok());
        std::fs::remove_file(path).expect("cleaned");
    }

    #[test]
    fn the_errors_module_names_the_codes_of_each_error_type() {
        let model = model_of(
            r#"#[error(emitter = "w")]
            pub enum WidgetError {
                #[error(code = "GONE", platform = "NOT_FOUND")] Gone,
                #[error(code = "BUSY", platform = "CONFLICT")] Busy,
            }"#,
        );
        let errors = errors_ts(&model);
        assert!(errors.contains("export { BonesError, PageError, isCursorRejected } from"));
        assert!(errors.contains("export type WidgetErrorCode = \"GONE\" | \"BUSY\";"));
    }

    #[test]
    fn an_unspellable_service_still_names_an_entry() {
        assert_eq!(service_pascal("api-bones test"), "ApiBonesTest");
        assert_eq!(lower_first("ApiBonesTest"), "apiBonesTest");
        assert_eq!(lower_first(""), "");
        assert_eq!(pascal(""), "");
    }
}

/// What the tests build contracts from: protos compiled in memory or the
/// contract protos of the test service.
#[cfg(test)]
pub(crate) mod testkit {
    use super::Contract;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn scratch(tag: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wrapper-ts-kit-{}-{tag}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        dir
    }

    /// The contract of one proto file, `t/v1/t.proto`, with the well-known
    /// imports protox ships.
    pub(crate) fn compile(proto: &str) -> Contract {
        let dir = scratch("compile");
        std::fs::create_dir_all(dir.join("t/v1")).expect("a directory");
        std::fs::write(dir.join("t/v1/t.proto"), proto).expect("a proto");
        let mut compiler = protox::Compiler::new([&dir]).expect("include paths");
        compiler.include_imports(true);
        compiler.open_file("t/v1/t.proto").expect("a valid proto");
        let contract = Contract::decode(&compiler.encode_file_descriptor_set()).expect("a set");
        std::fs::remove_dir_all(&dir).expect("cleaned");
        contract
    }

    fn protos_under(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("a proto directory")
            .map(|entry| entry.expect("an entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                protos_under(root, &path, out);
            } else if path.extension().is_some_and(|ext| ext == "proto") {
                let relative = path.strip_prefix(root).expect("under the root");
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    /// The encoded descriptor set of every contract proto of the test
    /// service, with the shipped `bones` protos they import.
    fn encoded(tag: &str) -> Vec<u8> {
        let dir = scratch(tag);
        for (relative, bytes) in api_bones_protos::files() {
            let path = dir.join(relative);
            std::fs::create_dir_all(path.parent().expect("a directory")).expect("created");
            std::fs::write(path, bytes).expect("a proto");
        }
        let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/l2/proto");
        let mut files = Vec::new();
        protos_under(&contract, &contract, &mut files);
        let mut compiler = protox::Compiler::new([&contract, &dir]).expect("include paths");
        compiler.include_imports(true);
        compiler
            .open_files(&files)
            .expect("the contract protos compile");
        let bytes = compiler.encode_file_descriptor_set();
        std::fs::remove_dir_all(&dir).expect("cleaned");
        bytes
    }

    /// The contract of the test service.
    pub(crate) fn contract() -> Contract {
        Contract::decode(&encoded("contract")).expect("a set")
    }

    /// The descriptor set of the test service, written to a file of its own.
    pub(crate) fn descriptor_file(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("wrapper-ts-{}-{tag}.fds", std::process::id()));
        std::fs::write(&path, encoded(&format!("file-{tag}"))).expect("a descriptor set");
        path
    }
}
