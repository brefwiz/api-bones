use std::path::PathBuf;

/// Build-script helper: generate a progenitor Rust SDK from an OpenAPI spec
/// with the `ApiResponse<T>` envelope stripped transparently.
///
/// # Usage (in `build.rs`)
///
/// ```no_run
/// fn main() {
///     let spec = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
///         .join("../schema/openapi.json");
///     println!("cargo:rerun-if-changed={}", spec.display());
///     api_bones_progenitor::SdkBuilder::new(spec).build().unwrap();
/// }
/// ```
pub struct SdkBuilder {
    spec_path: PathBuf,
}

impl SdkBuilder {
    pub fn new(spec_path: impl Into<PathBuf>) -> Self {
        Self {
            spec_path: spec_path.into(),
        }
    }

    /// Generate `$OUT_DIR/client.rs` from the OpenAPI spec.
    pub fn build(self) -> anyhow::Result<()> {
        let file = std::fs::File::open(&self.spec_path)?;
        let mut raw: serde_json::Value = serde_json::from_reader(file)?;

        // utoipa 5 emits OpenAPI 3.1 which progenitor doesn't fully understand.
        if let Some(v) = raw.get_mut("openapi") {
            *v = serde_json::Value::String("3.0.3".to_string());
        }
        normalize_nullable(&mut raw);
        unwrap_api_response_envelope(&mut raw);

        let spec: openapiv3::OpenAPI = serde_json::from_value(raw)?;
        let mut generator = progenitor::Generator::default();
        let tokens = generator
            .generate_tokens(&spec)
            .map_err(|e| anyhow::anyhow!("progenitor codegen failed: {e}"))?;

        let code = tokens.to_string().replace(
            "impl ClientHooks < () > for & Client { }",
            ENVELOPE_STRIPPING_HOOKS,
        );

        let out = PathBuf::from(std::env::var("OUT_DIR")?);
        std::fs::write(out.join("client.rs"), code)?;
        Ok(())
    }
}

/// Custom `ClientHooks` impl injected into the generated client.
///
/// Intercepts every HTTP response at the byte level, detects the
/// `{"data":<payload>,"meta":{...}}` envelope, and reconstructs the response
/// with just the inner payload so progenitor's `ResponseValue::from_response`
/// deserializes the correct type.
const ENVELOPE_STRIPPING_HOOKS: &str = r#"
impl ClientHooks<()> for &Client {
    async fn exec(
        &self,
        request: reqwest::Request,
        _info: &progenitor_client::OperationInfo,
    ) -> reqwest::Result<reqwest::Response> {
        let resp = self.client().execute(request).await?;
        let status = resp.status();
        let mut headers = resp.headers().clone();
        let body = resp.bytes().await?;

        let stripped: bytes::Bytes = (|| {
            let env: serde_json::Value = serde_json::from_slice(&body).ok()?;
            if env.get("meta").is_none() {
                return None;
            }
            let data = env.get("data")?;
            let serialized = serde_json::to_vec(data).ok()?;
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&serialized.len().to_string()) {
                headers.insert(reqwest::header::CONTENT_LENGTH, val);
            }
            Some(bytes::Bytes::from(serialized))
        })()
        .unwrap_or(body);

        let mut builder = http::Response::builder().status(status);
        for (k, v) in &headers {
            builder = builder.header(k, v);
        }
        let http_resp = builder.body(stripped).unwrap();
        Ok(reqwest::Response::from(http_resp))
    }
}
"#;

/// Recursively convert OpenAPI 3.1 nullable syntax (`"type": ["T", "null"]`)
/// to OpenAPI 3.0 format (`"type": "T", "nullable": true`).
fn normalize_nullable(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(arr)) = map.get("type") {
                let types: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if types.len() == 2 && types.contains(&"null".to_string()) {
                    let real = types.into_iter().find(|t| t != "null").unwrap();
                    map.insert("type".to_string(), serde_json::Value::String(real));
                    map.insert("nullable".to_string(), serde_json::Value::Bool(true));
                }
            }
            for v in map.values_mut() {
                normalize_nullable(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                normalize_nullable(v);
            }
        }
        _ => {}
    }
}

/// Replace every operation response body schema that matches the `ApiResponse`
/// envelope shape with just the inner `data` sub-schema.
fn unwrap_api_response_envelope(raw: &mut serde_json::Value) {
    let paths = match raw.get_mut("paths").and_then(|p| p.as_object_mut()) {
        Some(p) => p,
        None => return,
    };
    for (_path, path_item) in paths.iter_mut() {
        let methods = match path_item.as_object_mut() {
            Some(m) => m,
            None => continue,
        };
        for (_method, operation) in methods.iter_mut() {
            let responses = match operation
                .get_mut("responses")
                .and_then(|r| r.as_object_mut())
            {
                Some(r) => r,
                None => continue,
            };
            for (_status, response) in responses.iter_mut() {
                if let Some(schema) = response.pointer_mut("/content/application~1json/schema") {
                    if let Some(inner) = extract_envelope_data(schema) {
                        *schema = inner;
                    }
                }
            }
        }
    }
}

/// If `schema` looks like `{ properties: { data: <payload>, meta: ..., links?: ... }, required: ["data","meta"] }`,
/// return the `data` sub-schema. Otherwise return `None`.
fn extract_envelope_data(schema: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = schema.as_object()?;
    let props = obj.get("properties")?.as_object()?;
    if !props.contains_key("data") || !props.contains_key("meta") {
        return None;
    }
    let required: Vec<&str> = obj
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !required.contains(&"data") || !required.contains(&"meta") {
        return None;
    }
    Some(props["data"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_envelope_spec(data_schema: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "openapi": "3.1.0",
            "info": { "title": "test", "version": "0.0.1" },
            "paths": {
                "/items": {
                    "get": {
                        "operationId": "list_items",
                        "responses": {
                            "200": {
                                "description": "ok",
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "properties": {
                                                "data": data_schema,
                                                "meta": { "type": "object" }
                                            },
                                            "required": ["data", "meta"]
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn envelope_unwrap_replaces_schema_with_data() {
        let inner = serde_json::json!({ "type": "string" });
        let mut spec = minimal_envelope_spec(inner.clone());
        unwrap_api_response_envelope(&mut spec);
        let schema = spec
            .pointer("/paths/~1items/get/responses/200/content/application~1json/schema")
            .unwrap();
        assert_eq!(
            schema, &inner,
            "schema should be replaced with the data sub-schema"
        );
    }

    #[test]
    fn envelope_unwrap_ignores_non_envelope() {
        let plain =
            serde_json::json!({ "type": "object", "properties": { "id": { "type": "string" } } });
        let mut spec = serde_json::json!({
            "paths": {
                "/items": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": { "schema": plain.clone() }
                                }
                            }
                        }
                    }
                }
            }
        });
        unwrap_api_response_envelope(&mut spec);
        let schema = spec
            .pointer("/paths/~1items/get/responses/200/content/application~1json/schema")
            .unwrap();
        assert_eq!(
            schema, &plain,
            "non-envelope schema should pass through unchanged"
        );
    }

    #[test]
    fn normalize_nullable_converts_array_type() {
        let mut val = serde_json::json!({ "type": ["string", "null"] });
        normalize_nullable(&mut val);
        assert_eq!(val["type"], "string");
        assert_eq!(val["nullable"], true);
    }

    #[test]
    fn hooks_string_contains_exec() {
        assert!(ENVELOPE_STRIPPING_HOOKS.contains("async fn exec"));
        assert!(ENVELOPE_STRIPPING_HOOKS.contains("CONTENT_LENGTH"));
    }
}
