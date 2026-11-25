// SPDX-License-Identifier: MIT
//! `api-bones.wasm-size/v1`: the report of the wasm size probe.
//!
//! The probe builds the same Connect client in capability variants; this module
//! turns the built modules into one report: the size of every variant as
//! shipped (raw, gzip, brotli) and the costs derived from them. It reads and
//! writes files only through [`run`]; everything else is a pure function of the
//! variants, so the derivation is testable without a toolchain.
//!
//! The report carries no threshold. The numbers feed the ruling on how a
//! browser page pays for the capabilities it renders.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The schema name a report declares and a consumer checks.
pub const SCHEMA: &str = "api-bones.wasm-size/v1";

/// The login-page weight ceiling the estimates are set against, in bytes.
pub const LOGIN_PAGE_CEILING: u64 = 149_261;

/// The variants every report holds. The others (an `s` optimisation level of
/// `floor` and `all`) are extra points, not requirements.
pub const REQUIRED_VARIANTS: [&str; 11] = [
    "floor",
    "floor-no-opt",
    "page",
    "batch",
    "stream",
    "retry",
    "gzip",
    "methods-10",
    "methods-50",
    "second-service",
    "all",
];

/// The one variant built without `wasm-opt`.
const UNOPTIMISED: &str = "floor-no-opt";

/// The capabilities whose delta over the floor the report derives.
const CAPABILITIES: [&str; 5] = ["page", "batch", "stream", "retry", "gzip"];

/// The reference page of the option estimates: services and methods per service.
const PAGE_SERVICES: i64 = 3;
const PAGE_METHODS_PER_SERVICE: i64 = 10;
/// The reference platform SDK of option C: services and methods in total.
const PLATFORM_SERVICES: i64 = 20;
const PLATFORM_METHODS: i64 = 500;

/// A failure to read, derive or validate a report.
#[derive(Debug)]
pub struct ReportError(String);

impl ReportError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ReportError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&self.0)
    }
}

impl std::error::Error for ReportError {}

impl From<std::io::Error> for ReportError {
    fn from(error: std::io::Error) -> Self {
        Self::new(format!("io: {error}"))
    }
}

impl From<serde_json::Error> for ReportError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(format!("json: {error}"))
    }
}

/// How a variant was built: the `meta.json` next to its module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    /// The cargo features of the build.
    pub features: Vec<String>,
    /// The cargo profile of the build.
    pub profile: String,
    /// Whether `wasm-opt` ran over the module.
    pub wasm_opt: bool,
}

/// One built variant and its sizes in bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Variant {
    /// The variant's name.
    pub name: String,
    /// The cargo features of the build.
    pub features: Vec<String>,
    /// The cargo profile of the build.
    pub profile: String,
    /// Whether `wasm-opt` ran over the module.
    pub wasm_opt: bool,
    /// The module as built.
    pub raw: u64,
    /// The module gzipped.
    pub gzip: u64,
    /// The module brotli-compressed.
    pub brotli: u64,
    /// The wasm-bindgen glue gzipped: it ships beside the module.
    pub glue_gzip: u64,
}

impl Variant {
    /// Measure a built module and its glue.
    ///
    /// # Errors
    /// The compressor's failure.
    pub fn measure(name: &str, meta: &Meta, wasm: &[u8], glue: &[u8]) -> Result<Self, ReportError> {
        Ok(Self {
            name: name.to_owned(),
            features: meta.features.clone(),
            profile: meta.profile.clone(),
            wasm_opt: meta.wasm_opt,
            raw: len(wasm),
            gzip: len(&gzip(wasm)?),
            brotli: len(&brotli(wasm)?),
            glue_gzip: len(&gzip(glue)?),
        })
    }

    fn triple(&self) -> Result<Triple, ReportError> {
        Ok(Triple {
            raw: signed(self.raw)?,
            gzip: signed(self.gzip)?,
            brotli: signed(self.brotli)?,
        })
    }
}

fn len(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

fn signed(value: u64) -> Result<i64, ReportError> {
    i64::try_from(value).map_err(|_| ReportError::new("a size does not fit a signed 64-bit value"))
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, ReportError> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}

fn brotli(bytes: &[u8]) -> Result<Vec<u8>, ReportError> {
    let mut out = Vec::new();
    let params = brotli::enc::BrotliEncoderParams {
        quality: 11,
        ..brotli::enc::BrotliEncoderParams::default()
    };
    brotli::BrotliCompress(&mut &bytes[..], &mut out, &params)?;
    Ok(out)
}

/// A cost in the three shipped encodings, in signed bytes: a delta can be
/// negative when a capability shares code the floor already carries.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Triple {
    /// Uncompressed.
    pub raw: i64,
    /// Gzip.
    pub gzip: i64,
    /// Brotli.
    pub brotli: i64,
}

impl Triple {
    fn zip(self, other: Self, op: impl Fn(i64, i64) -> i64) -> Self {
        Self {
            raw: op(self.raw, other.raw),
            gzip: op(self.gzip, other.gzip),
            brotli: op(self.brotli, other.brotli),
        }
    }

    fn map(self, op: impl Fn(i64) -> i64) -> Self {
        Self {
            raw: op(self.raw),
            gzip: op(self.gzip),
            brotli: op(self.brotli),
        }
    }

    fn minus(self, other: Self) -> Self {
        self.zip(other, i64::saturating_sub)
    }

    fn plus(self, other: Self) -> Self {
        self.zip(other, i64::saturating_add)
    }

    fn times(self, factor: i64) -> Self {
        self.map(|value| value.saturating_mul(factor))
    }
}

/// One option's estimated size for its reference page and the room it leaves
/// under the login-page ceiling (negative when it does not fit).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Estimate {
    /// What the estimate assumes, in the terms of the per-capability costs.
    pub assumption: String,
    /// The estimated size.
    pub size: Triple,
    /// The ceiling minus the gzip size.
    pub headroom_gzip: i64,
    /// The ceiling minus the brotli size.
    pub headroom_brotli: i64,
}

/// The costs derived from the variants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Derived {
    /// F: the module with one RPC.
    pub floor: Triple,
    /// What each capability adds to the floor.
    pub deltas: BTreeMap<String, Triple>,
    /// One more unary method: `(methods-50 - methods-10) / 40`.
    pub per_method: Triple,
    /// A second service with one method.
    pub second_service: Triple,
    /// The login-page ceiling the estimates are set against.
    pub ceiling: u64,
    /// Options A, B and C of the wasm cut.
    pub options: BTreeMap<String, Estimate>,
}

fn find<'a>(variants: &'a [Variant], name: &str) -> Result<&'a Variant, ReportError> {
    variants
        .iter()
        .find(|variant| variant.name == name)
        .ok_or_else(|| ReportError::new(format!("variant `{name}` is missing")))
}

fn estimate(assumption: String, size: Triple, ceiling: i64) -> Estimate {
    Estimate {
        assumption,
        size,
        headroom_gzip: ceiling.saturating_sub(size.gzip),
        headroom_brotli: ceiling.saturating_sub(size.brotli),
    }
}

/// Derive the costs from the variants.
///
/// # Errors
/// A required variant is missing.
pub fn derive(variants: &[Variant]) -> Result<Derived, ReportError> {
    let floor = find(variants, "floor")?.triple()?;
    let mut deltas = BTreeMap::new();
    for capability in CAPABILITIES {
        let delta = find(variants, capability)?.triple()?.minus(floor);
        deltas.insert(capability.to_owned(), delta);
    }
    let per_method = find(variants, "methods-50")?
        .triple()?
        .minus(find(variants, "methods-10")?.triple()?)
        .map(|value| value / 40);
    let second_service = find(variants, "second-service")?.triple()?.minus(floor);
    let all = find(variants, "all")?.triple()?;
    let ceiling = signed(LOGIN_PAGE_CEILING)?;

    let page_methods = PAGE_SERVICES * PAGE_METHODS_PER_SERVICE;
    let a = all
        .plus(second_service.times(PAGE_SERVICES - 1))
        .plus(per_method.times(page_methods - 1));
    let b = all
        .plus(per_method.times(PAGE_METHODS_PER_SERVICE - 1))
        .times(PAGE_SERVICES);
    let c = all
        .plus(second_service.times(PLATFORM_SERVICES - 1))
        .plus(per_method.times(PLATFORM_METHODS - 1));
    let mut options = BTreeMap::new();
    options.insert(
        "A".to_owned(),
        estimate(
            format!(
                "one module per route: one floor, every capability, {PAGE_SERVICES} services of \
                 {PAGE_METHODS_PER_SERVICE} methods"
            ),
            a,
            ceiling,
        ),
    );
    options.insert(
        "B".to_owned(),
        estimate(
            format!(
                "one module per service: a floor and every capability in each of \
                 {PAGE_SERVICES} services of {PAGE_METHODS_PER_SERVICE} methods"
            ),
            b,
            ceiling,
        ),
    );
    options.insert(
        "C".to_owned(),
        estimate(
            format!(
                "one module for the platform SDK: one floor, every capability, \
                 {PLATFORM_SERVICES} services and {PLATFORM_METHODS} methods"
            ),
            c,
            ceiling,
        ),
    );
    Ok(Derived {
        floor,
        deltas,
        per_method,
        second_service,
        ceiling: LOGIN_PAGE_CEILING,
        options,
    })
}

/// The report: the variants and what is derived from them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Report {
    /// Always [`SCHEMA`].
    pub schema: String,
    /// The api-bones commit the variants were built from.
    pub commit: String,
    /// `rustc --version`.
    pub rustc: String,
    /// The `wasm-bindgen` version that processed the modules.
    pub wasm_bindgen: String,
    /// The `wasm-opt` (binaryen) version that optimised them.
    pub binaryen: String,
    /// Every built variant.
    pub variants: Vec<Variant>,
    /// The derived costs.
    pub derived: Derived,
}

impl Report {
    /// Build a report from the built variants.
    ///
    /// # Errors
    /// A required variant is missing, or the report fails [`Report::validate`].
    pub fn new(
        commit: &str,
        rustc: &str,
        wasm_bindgen: &str,
        binaryen: &str,
        variants: Vec<Variant>,
    ) -> Result<Self, ReportError> {
        let derived = derive(&variants)?;
        let report = Self {
            schema: SCHEMA.to_owned(),
            commit: commit.to_owned(),
            rustc: rustc.to_owned(),
            wasm_bindgen: wasm_bindgen.to_owned(),
            binaryen: binaryen.to_owned(),
            variants,
            derived,
        };
        report.validate()?;
        Ok(report)
    }

    /// Check the report against `api-bones.wasm-size/v1`: the schema name,
    /// every provenance field, every required variant sized and optimised
    /// as declared, and the derived costs equal to what the variants give.
    ///
    /// # Errors
    /// The first rule the report breaks.
    pub fn validate(&self) -> Result<(), ReportError> {
        if self.schema != SCHEMA {
            return Err(ReportError::new(format!("schema is not {SCHEMA}")));
        }
        for (field, value) in [
            ("commit", &self.commit),
            ("rustc", &self.rustc),
            ("wasm_bindgen", &self.wasm_bindgen),
            ("binaryen", &self.binaryen),
        ] {
            if value.trim().is_empty() {
                return Err(ReportError::new(format!("{field} is empty")));
            }
        }
        for required in REQUIRED_VARIANTS {
            find(&self.variants, required)?;
        }
        for variant in &self.variants {
            if variant.raw == 0 || variant.gzip == 0 || variant.brotli == 0 {
                return Err(ReportError::new(format!(
                    "variant `{}` has an empty size",
                    variant.name
                )));
            }
            if variant.wasm_opt == (variant.name == UNOPTIMISED) {
                return Err(ReportError::new(format!(
                    "variant `{}` has the wrong wasm-opt setting",
                    variant.name
                )));
            }
        }
        if derive(&self.variants)? != self.derived {
            return Err(ReportError::new(
                "derived costs do not follow from the variants",
            ));
        }
        Ok(())
    }

    /// The report as a markdown table, for a CI step summary.
    #[must_use]
    pub fn markdown(&self) -> String {
        let mut out = format!(
            "### wasm size probe ({})\n\ncommit `{}`, {}, wasm-bindgen {}, binaryen {}\n\n",
            self.schema, self.commit, self.rustc, self.wasm_bindgen, self.binaryen
        );
        out.push_str("| variant | wasm-opt | raw | gzip | brotli | glue gzip |\n");
        out.push_str("|---|---|---:|---:|---:|---:|\n");
        for variant in &self.variants {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} |",
                variant.name,
                variant.wasm_opt,
                variant.raw,
                variant.gzip,
                variant.brotli,
                variant.glue_gzip
            );
        }
        out.push_str("\n| derived | raw | gzip | brotli |\n|---|---:|---:|---:|\n");
        let derived = &self.derived;
        let mut rows = vec![
            ("floor F".to_owned(), derived.floor),
            ("per method".to_owned(), derived.per_method),
            ("second service".to_owned(), derived.second_service),
        ];
        rows.extend(
            derived
                .deltas
                .iter()
                .map(|(name, delta)| (format!("delta {name}"), *delta)),
        );
        for (label, cost) in rows {
            let _ = writeln!(
                out,
                "| {label} | {} | {} | {} |",
                cost.raw, cost.gzip, cost.brotli
            );
        }
        let _ = write!(
            out,
            "\n| option | raw | gzip | brotli | headroom gzip | headroom brotli |\n\
             |---|---:|---:|---:|---:|---:|\n"
        );
        for (name, option) in &derived.options {
            let _ = writeln!(
                out,
                "| {name} | {} | {} | {} | {} | {} |",
                option.size.raw,
                option.size.gzip,
                option.size.brotli,
                option.headroom_gzip,
                option.headroom_brotli
            );
        }
        let _ = writeln!(
            out,
            "\nHeadroom is against the {} B login-page ceiling.",
            derived.ceiling
        );
        for (name, option) in &derived.options {
            let _ = writeln!(out, "- {name}: {}", option.assumption);
        }
        out
    }
}

/// Read the variant directories of `dir` (`<name>/module.wasm`, `glue.js` and
/// `meta.json`), in name order.
fn read_variants(dir: &Path) -> Result<Vec<Variant>, ReportError> {
    let mut names = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    names
        .iter()
        .map(|name| {
            let root = dir.join(name);
            let meta: Meta = serde_json::from_slice(&std::fs::read(root.join("meta.json"))?)?;
            let wasm = std::fs::read(root.join("module.wasm"))?;
            let glue = std::fs::read(root.join("glue.js"))?;
            Variant::measure(name, &meta, &wasm, &glue)
        })
        .collect()
}

const FLAGS: [&str; 7] = [
    "dir",
    "commit",
    "rustc",
    "wasm-bindgen",
    "binaryen",
    "out",
    "summary",
];

/// Run the `wasm-size-report` command: measure every variant directory, write
/// the report and its markdown summary, and return the markdown.
///
/// Flags, all required, each followed by its value: `--dir`, `--commit`,
/// `--rustc`, `--wasm-bindgen`, `--binaryen`, `--out`, `--summary`.
///
/// # Errors
/// A missing or unknown flag, an unreadable variant, an empty variant set, a
/// missing required variant, or a report that fails validation.
pub fn run(args: &[String]) -> Result<String, ReportError> {
    let mut flags = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let name = flag
            .strip_prefix("--")
            .filter(|name| FLAGS.contains(name))
            .ok_or_else(|| ReportError::new(format!("unknown argument `{flag}`")))?;
        let value = rest
            .next()
            .ok_or_else(|| ReportError::new(format!("`{flag}` needs a value")))?;
        flags.insert(name, value.as_str());
    }
    let flag = |name: &str| {
        flags
            .get(name)
            .copied()
            .ok_or_else(|| ReportError::new(format!("`--{name}` is required")))
    };
    let variants = read_variants(Path::new(flag("dir")?))?;
    if variants.is_empty() {
        return Err(ReportError::new("no variant was built"));
    }
    let report = Report::new(
        flag("commit")?,
        flag("rustc")?,
        flag("wasm-bindgen")?,
        flag("binaryen")?,
        variants,
    )?;
    let markdown = report.markdown();
    std::fs::write(flag("out")?, serde_json::to_vec_pretty(&report)?)?;
    std::fs::write(flag("summary")?, &markdown)?;
    Ok(markdown)
}
