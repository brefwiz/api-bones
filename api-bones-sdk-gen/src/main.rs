use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

const PROGENITOR_VERSION: &str = env!("CARGO_PKG_VERSION");
const OPENAPI_GENERATOR_VERSION: &str = "7.12.0";
const OPENAPI_GENERATOR_JAR_URL: &str = "https://repo1.maven.org/maven2/org/openapitools/openapi-generator-cli/7.12.0/openapi-generator-cli-7.12.0.jar";

#[derive(Parser)]
#[command(
    name = "api-bones-sdk-gen",
    version,
    about = "Generate Brefwiz Rust + TS SDKs"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Dump the OpenAPI schema from a service binary
    Schema {
        /// Cargo binary name to invoke (e.g. generate-openapi)
        #[arg(long)]
        server_bin: String,
        /// Destination path (e.g. sdk/schema/openapi.json)
        #[arg(long, default_value = "sdk/schema/openapi.json")]
        out: PathBuf,
    },
    /// Generate the Rust progenitor SDK tree
    Rust {
        /// Path to openapi.json
        #[arg(long, default_value = "sdk/schema/openapi.json")]
        spec: PathBuf,
        /// Output directory for the Rust crate (e.g. sdk/rust-api)
        #[arg(long, default_value = "sdk/rust-api")]
        out: PathBuf,
        /// Cargo crate name (e.g. itinerwiz-sdk)
        #[arg(long)]
        crate_name: String,
        /// Human-readable service name for the package description
        #[arg(long, default_value = "")]
        service_name: String,
    },
    /// Generate the TypeScript axios SDK tree
    Ts {
        /// Path to openapi.json
        #[arg(long, default_value = "sdk/schema/openapi.json")]
        spec: PathBuf,
        /// Output directory for the TS package (e.g. sdk/typescript)
        #[arg(long, default_value = "sdk/typescript")]
        out: PathBuf,
        /// npm package name (e.g. @itinerwiz/sdk)
        #[arg(long)]
        pkg_name: String,
        /// Path to a cached openapi-generator-cli jar (downloads if absent)
        #[arg(long)]
        jar: Option<PathBuf>,
    },
    /// Run schema + rust + ts in sequence
    All {
        #[arg(long)]
        server_bin: String,
        #[arg(long)]
        crate_name: String,
        #[arg(long)]
        pkg_name: String,
        #[arg(long, default_value = "sdk/schema/openapi.json")]
        schema: PathBuf,
        #[arg(long)]
        jar: Option<PathBuf>,
    },
    /// Emit the shared api-bones-sdk.mk Makefile fragment to stdout
    Makefile,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Schema { server_bin, out } => cmd_schema(&server_bin, &out),
        Cmd::Rust {
            spec,
            out,
            crate_name,
            service_name,
        } => cmd_rust(&spec, &out, &crate_name, &service_name),
        Cmd::Ts {
            spec,
            out,
            pkg_name,
            jar,
        } => cmd_ts(&spec, &out, &pkg_name, jar.as_deref()),
        Cmd::All {
            server_bin,
            crate_name,
            pkg_name,
            schema,
            jar,
        } => {
            cmd_schema(&server_bin, &schema)?;
            cmd_rust(&schema, Path::new("sdk/rust-api"), &crate_name, "")?;
            cmd_ts(
                &schema,
                Path::new("sdk/typescript"),
                &pkg_name,
                jar.as_deref(),
            )
        }
        Cmd::Makefile => {
            print!("{}", MAKEFILE_FRAGMENT);
            Ok(())
        }
    }
}

fn cmd_schema(server_bin: &str, out: &Path) -> anyhow::Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new("cargo")
        .args(["run", "--bin", server_bin, "--"])
        .stdout(std::fs::File::create(out)?)
        .status()
        .with_context(|| format!("failed to run cargo run --bin {server_bin}"))?;
    anyhow::ensure!(status.success(), "generate-openapi exited with {status}");
    eprintln!("OpenAPI schema written to {}", out.display());
    Ok(())
}

fn cmd_rust(_spec: &Path, out: &Path, crate_name: &str, service_name: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(out.join("src"))?;

    let service_desc = if service_name.is_empty() {
        crate_name.to_string()
    } else {
        service_name.to_string()
    };

    let cargo_toml = include_str!("../templates/rust-Cargo.toml.tmpl")
        .replace("{{crate_name}}", crate_name)
        .replace("{{service_name}}", &service_desc)
        .replace("{{api_bones_progenitor_version}}", PROGENITOR_VERSION);
    let build_rs = include_str!("../templates/rust-build.rs.tmpl");
    let lib_rs = include_str!("../templates/rust-src-lib.rs.tmpl");

    std::fs::write(out.join("Cargo.toml"), cargo_toml)?;
    std::fs::write(out.join("build.rs"), build_rs)?;
    std::fs::write(out.join("src/lib.rs"), lib_rs)?;

    // Trigger the actual progenitor codegen via `cargo build` so that
    // OUT_DIR/client.rs is produced and the crate compiles.
    let status = Command::new("cargo")
        .args(["build"])
        .current_dir(out)
        .status()
        .context("cargo build of generated Rust SDK failed")?;
    anyhow::ensure!(status.success(), "cargo build exited with {status}");

    eprintln!("Rust SDK generated at {}", out.display());
    Ok(())
}

fn cmd_ts(spec: &Path, out: &Path, pkg_name: &str, jar: Option<&Path>) -> anyhow::Result<()> {
    let jar_path = match jar {
        Some(p) => p.to_path_buf(),
        None => {
            let tmp = std::env::temp_dir().join(format!(
                "openapi-generator-cli-{OPENAPI_GENERATOR_VERSION}.jar"
            ));
            if !tmp.exists() {
                eprintln!("Downloading openapi-generator-cli {OPENAPI_GENERATOR_VERSION}…");
                let status = Command::new("curl")
                    .args([
                        "-fsSL",
                        "-o",
                        tmp.to_str().unwrap(),
                        OPENAPI_GENERATOR_JAR_URL,
                    ])
                    .status()
                    .context("curl download of openapi-generator-cli failed")?;
                anyhow::ensure!(status.success(), "curl exited with {status}");
            }
            tmp
        }
    };

    std::fs::create_dir_all(out)?;

    // Run openapi-generator-cli
    let status = Command::new("java")
        .args([
            "-jar",
            jar_path.to_str().unwrap(),
            "generate",
            "-i",
            spec.to_str().unwrap(),
            "-g",
            "typescript-axios",
            "-o",
            out.to_str().unwrap(),
            "--additional-properties",
            &format!("npmName={pkg_name},npmVersion=0.1.0,supportsES6=true"),
        ])
        .status()
        .context("openapi-generator-cli failed")?;
    anyhow::ensure!(
        status.success(),
        "openapi-generator-cli exited with {status}"
    );

    // Splice the @brefwiz/api-bones-axios interceptor wiring into the
    // generated index.ts and patch package.json.
    splice_envelope_interceptor(out, pkg_name)?;

    // Rewrite AxiosPromise<EnvelopeWrapper> → AxiosPromise<InnerData> so that
    // resp.data is statically typed as the unwrapped payload, matching what the
    // interceptor delivers at runtime.
    let n = rewrite_envelope_types(out)?;
    if n > 0 {
        eprintln!("Rewrote {n} envelope return type(s) in api.ts");
    }

    eprintln!("TypeScript SDK generated at {}", out.display());
    Ok(())
}

/// Append the envelope interceptor bootstrap to the generated `index.ts` and
/// add `@brefwiz/api-bones-axios` as a dependency in `package.json`.
fn splice_envelope_interceptor(out: &Path, _pkg_name: &str) -> anyhow::Result<()> {
    // Append to index.ts
    let index = out.join("index.ts");
    if index.exists() {
        let mut content = std::fs::read_to_string(&index)?;
        if !content.contains("api-bones-axios") {
            content.push_str(ENVELOPE_INTERCEPTOR_APPEND);
            std::fs::write(&index, content)?;
        }
    }

    // Patch package.json
    let pkg_json_path = out.join("package.json");
    if pkg_json_path.exists() {
        let raw = std::fs::read_to_string(&pkg_json_path)?;
        let mut pkg: serde_json::Value = serde_json::from_str(&raw)?;
        if let Some(deps) = pkg.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
            deps.entry("@brefwiz/api-bones-axios")
                .or_insert_with(|| serde_json::Value::String("^0.1.0".to_string()));
        } else {
            pkg["dependencies"] = serde_json::json!({
                "@brefwiz/api-bones-axios": "^0.1.0"
            });
        }
        std::fs::write(pkg_json_path, serde_json::to_string_pretty(&pkg)?)?;
    }

    Ok(())
}

// ── Envelope return-type rewriting ───────────────────────────────────────────

/// Rewrite `AxiosPromise<EnvelopeWrapper>` → `AxiosPromise<DataType>` in the
/// generated `api.ts` so that `resp.data` is statically typed as the unwrapped
/// payload — matching what `addEnvelopeUnwrapInterceptor` delivers at runtime.
///
/// The openapi-generator emits per-operation response interfaces like:
/// ```ts
/// export interface CreateBlock201Response {
///     'data': CreateBlock201ResponseData;
///     'links'?: Array<Link>;
///     'meta': ResponseMeta;       // ← envelope sentinel
/// }
/// ```
/// This function detects those wrappers (presence of `'meta': ResponseMeta`)
/// and rewrites every `AxiosPromise<CreateBlock201Response>` reference to
/// `AxiosPromise<CreateBlock201ResponseData>`.  The wrapper interface itself
/// is kept but marked `@deprecated`.
fn rewrite_envelope_types(out: &Path) -> anyhow::Result<usize> {
    let api_ts = out.join("api.ts");
    if !api_ts.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(&api_ts)?;
    let envelope_map = collect_envelope_interfaces(&content);
    if envelope_map.is_empty() {
        return Ok(0);
    }

    let rewritten = apply_envelope_rewrites(&content, &envelope_map);
    if rewritten != content {
        std::fs::write(&api_ts, &rewritten)?;
    }

    Ok(envelope_map.len())
}

/// Scan `api.ts` content and return a map of
/// `envelope_interface_name → data_field_type` for every interface that
/// contains both `'data': T` and `'meta': ResponseMeta` (the envelope shape).
fn collect_envelope_interfaces(content: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let marker = "\nexport interface ";
    let mut pos = 0;

    while let Some(rel) = content[pos..].find(marker) {
        let abs = pos + rel + marker.len();

        let Some(brace_rel) = content[abs..].find(" {") else {
            break;
        };
        let name = content[abs..abs + brace_rel].trim().to_string();
        let body_start = abs + brace_rel + 2;

        let Some(body_end) = find_interface_end(content, body_start) else {
            break;
        };
        let body = &content[body_start..body_end];

        if body.contains("'meta': ResponseMeta")
            && let Some(data_type) = extract_data_field_type(body)
        {
            map.insert(name, data_type);
        }

        pos = body_end + 1;
    }

    map
}

/// Find the byte position of the `}` that closes the interface body opened at
/// `body_start` (i.e. the first `{` is already consumed).
fn find_interface_end(content: &str, body_start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth: i32 = 1;
    let mut i = body_start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract the type name from a `'data': TypeName;` field in an interface body.
/// Returns `None` for inline object literals (`'data': { ... }`).
fn extract_data_field_type(body: &str) -> Option<String> {
    let needle = "'data': ";
    let start = body.find(needle)? + needle.len();
    let semi = body[start..].find(';')?;
    let data_type = body[start..start + semi].trim();
    if data_type.starts_with('{') {
        return None; // inline object — no clean name to rewrite to
    }
    Some(data_type.to_string())
}

/// Rewrite `AxiosPromise<Wrapper>` → `AxiosPromise<Inner>` everywhere in the
/// content and mark each wrapper interface `@deprecated`.
fn apply_envelope_rewrites(
    content: &str,
    envelope_map: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = content.to_string();

    for (wrapper, inner) in envelope_map {
        // Rewrite all three call-site patterns the generator emits:
        //   AxiosPromise<Wrapper>
        let old = format!("AxiosPromise<{wrapper}>");
        let new = format!("AxiosPromise<{inner}>");
        out = out.replace(&old, &new);

        // Mark the wrapper interface deprecated so downstream tooling is aware.
        let old_decl = format!("\nexport interface {wrapper} {{");
        let new_decl = format!(
            "\n/** @deprecated Envelope wrapper — \
            `resp.data` is typed as `{inner}` after interceptor unwrapping. */\
            \nexport interface {wrapper} {{"
        );
        out = out.replace(&old_decl, &new_decl);
    }

    out
}

const ENVELOPE_INTERCEPTOR_APPEND: &str = r#"
// --- api-bones-sdk-gen: envelope interceptor ---
import axios from "axios";
import { addEnvelopeUnwrapInterceptor } from "@brefwiz/api-bones-axios";
addEnvelopeUnwrapInterceptor(axios);
export { addEnvelopeUnwrapInterceptor, getEnvelopeMeta, getEnvelopeLinks } from "@brefwiz/api-bones-axios";
// ------------------------------------------------
"#;

const MAKEFILE_FRAGMENT: &str = r#"# api-bones-sdk.mk — shared SDK codegen targets for Brefwiz services.
# Include this file in your Makefile after setting the three variables below:
#
#   SERVER_OPENAPI_BIN ?= generate-openapi   # cargo binary name
#   SDK_RUST_CRATE     ?= my-service-sdk     # crate name
#   SDK_TS_PKG         ?= @myorg/my-sdk      # npm package name
#   OPENAPI_SCHEMA     ?= sdk/schema/openapi.json

.PHONY: openapi-generate codegen-rust codegen-typescript codegen-all

openapi-generate: ## Dump OpenAPI schema from the server binary
	api-bones-sdk-gen schema \
		--server-bin $(SERVER_OPENAPI_BIN) \
		--out $(OPENAPI_SCHEMA)

codegen-rust: openapi-generate ## Generate Rust progenitor SDK
	api-bones-sdk-gen rust \
		--spec $(OPENAPI_SCHEMA) \
		--crate-name $(SDK_RUST_CRATE) \
		--out sdk/rust-api

codegen-typescript: openapi-generate ## Generate TypeScript axios SDK
	api-bones-sdk-gen ts \
		--spec $(OPENAPI_SCHEMA) \
		--pkg-name $(SDK_TS_PKG) \
		--out sdk/typescript

codegen-all: codegen-rust codegen-typescript ## Generate all SDKs
"#;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_API_TS: &str = r#"
export interface CreateBlock201ResponseData {
    'id': string;
    'resource_instance_id': string;
}

export interface CreateBlock201Response {
    'data': CreateBlock201ResponseData;
    'links'?: Array<Link>;
    'meta': ResponseMeta;
}

export interface ListBlocks200ResponseDataInner {
    'id': string;
}

export interface ListBlocks200Response {
    'data': Array<ListBlocks200ResponseDataInner>;
    'meta': ResponseMeta;
}

export interface ResourceBlock {
    'id': string;
    'reason': string;
}

export const SchedulingApiAxiosParamCreator = function (configuration?: Configuration) {
    return {
        createBlock: async (...): Promise<RequestArgs> => { ... },
        listBlocks: async (...): Promise<RequestArgs> => { ... },
    }
};

export const SchedulingApiFp = function (configuration?: Configuration) {
    return {
        async createBlock(...): Promise<(axios?: AxiosInstance) => AxiosPromise<CreateBlock201Response>> { ... },
        async listBlocks(...): Promise<(axios?: AxiosInstance) => AxiosPromise<ListBlocks200Response>> { ... },
    }
};

export class SchedulingApi extends BaseAPI {
    public createBlock(...): AxiosPromise<CreateBlock201Response> { ... }
    public listBlocks(...): AxiosPromise<ListBlocks200Response> { ... }
}
"#;

    #[test]
    fn detects_envelope_interfaces() {
        let map = collect_envelope_interfaces(SAMPLE_API_TS);
        assert_eq!(
            map.get("CreateBlock201Response").map(String::as_str),
            Some("CreateBlock201ResponseData")
        );
        assert_eq!(
            map.get("ListBlocks200Response").map(String::as_str),
            Some("Array<ListBlocks200ResponseDataInner>")
        );
        // Non-envelope interfaces must not be included
        assert!(!map.contains_key("ResourceBlock"));
        assert!(!map.contains_key("CreateBlock201ResponseData"));
    }

    #[test]
    fn rewrites_axios_promise_return_types() {
        let map = collect_envelope_interfaces(SAMPLE_API_TS);
        let rewritten = apply_envelope_rewrites(SAMPLE_API_TS, &map);
        // Return types are rewritten
        assert!(rewritten.contains("AxiosPromise<CreateBlock201ResponseData>"));
        assert!(rewritten.contains("AxiosPromise<Array<ListBlocks200ResponseDataInner>>"));
        // Original envelope references are gone from return positions
        assert!(!rewritten.contains("AxiosPromise<CreateBlock201Response>"));
        assert!(!rewritten.contains("AxiosPromise<ListBlocks200Response>"));
        // Wrapper interfaces are deprecated but still present
        assert!(rewritten.contains("@deprecated"));
        assert!(rewritten.contains("export interface CreateBlock201Response {"));
        assert!(rewritten.contains("export interface ListBlocks200Response {"));
    }

    #[test]
    fn ignores_non_envelope_interfaces() {
        let no_envelope = r#"
export interface ResourceBlock {
    'id': string;
    'reason': string;
}
export class Foo extends BaseAPI {
    public getBlock(...): AxiosPromise<ResourceBlock> { ... }
}
"#;
        let map = collect_envelope_interfaces(no_envelope);
        assert!(map.is_empty());
        let rewritten = apply_envelope_rewrites(no_envelope, &map);
        assert_eq!(rewritten, no_envelope);
    }
}
