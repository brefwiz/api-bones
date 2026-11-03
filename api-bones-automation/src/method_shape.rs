// SPDX-License-Identifier: MIT
//! Descriptor-derived paging and batching policy shared by SDK policy emitters.
use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, MethodDescriptor};
use serde::Serialize;

/// JSON field names of a canonical paginated RPC.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagingPolicy {
    /// Unary page or server stream of a declared list.
    pub kind: &'static str,
    /// Canonical list RPC, for a declared server stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_of: Option<String>,
    /// Repeated response field.
    pub item_field: String,
    /// Response page field.
    pub page_field: String,
    /// Request page field.
    pub request_page_field: String,
}
/// Canonical batch sibling of a single read RPC.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPolicy {
    /// Get or grouped list.
    pub kind: &'static str,
    /// Batch RPC name.
    pub method: String,
    /// Single request key.
    pub key_field: String,
    /// Batch request keys.
    pub keys_field: String,
    /// Batch response results or groups.
    pub results_field: String,
}
fn message_type(field: &FieldDescriptor, name: &str) -> bool {
    matches!(field.kind(), Kind::Message(message) if message.full_name() == name)
}
fn option(pool: &DescriptorPool, method: &MethodDescriptor, name: &str) -> Option<String> {
    let extension = pool.get_extension_by_name(name)?;
    let options = method.options();
    if !options.has_extension(&extension) {
        return None;
    }
    options
        .get_extension(&extension)
        .as_str()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}
/// Discover canonical page fields without product-specific configuration.
#[must_use]
pub fn paging_policy(pool: &DescriptorPool, method: &MethodDescriptor) -> Option<PagingPolicy> {
    let stream_of = option(pool, method, "bones.v1.list_stream_of");
    if method.is_client_streaming() || (method.is_server_streaming() && stream_of.is_none()) {
        return None;
    }
    let request = method
        .input()
        .fields()
        .find(|field| message_type(field, "bones.v1.PageRequest"))?;
    let page = method
        .output()
        .fields()
        .find(|field| message_type(field, "bones.v1.PageResponse"))?;
    let output = method.output();
    let mut items = output
        .fields()
        .filter(prost_reflect::FieldDescriptor::is_list);
    let item = items.next()?;
    if items.next().is_some() {
        return None;
    }
    Some(PagingPolicy {
        kind: if stream_of.is_some() {
            "stream"
        } else {
            "unary"
        },
        stream_of,
        item_field: item.json_name().into(),
        page_field: page.json_name().into(),
        request_page_field: request.json_name().into(),
    })
}
/// Read a checked `batch_of` sibling and derive its canonical key/result fields.
///
/// # Errors
/// Refuses invalid batch declarations or ambiguous/noncanonical request shapes.
pub fn batch_policy(
    pool: &DescriptorPool,
    single: &MethodDescriptor,
) -> Result<Option<BatchPolicy>, String> {
    let service = single.parent_service();
    let mut siblings = service.methods().filter(|batch| {
        option(pool, batch, "bones.v1.batch_of").is_some_and(|name| {
            name == single.name()
                || name == format!("{}/{}", single.parent_service().full_name(), single.name())
        })
    });
    let Some(batch) = siblings.next() else {
        return Ok(None);
    };
    if siblings.next().is_some() {
        return Err(format!(
            "{} has multiple batch siblings",
            single.full_name()
        ));
    }
    let keys: Vec<_> = batch
        .input()
        .fields()
        .filter(|field| field.is_list() && field.kind() == Kind::String)
        .collect();
    let [keys] = keys.as_slice() else {
        return Err(format!(
            "{} needs one repeated string key",
            batch.full_name()
        ));
    };
    let singular = keys
        .name()
        .strip_suffix('s')
        .ok_or_else(|| format!("{} key must be plural", batch.full_name()))?;
    let key = single
        .input()
        .get_field_by_name(singular)
        .filter(|field| field.kind() == Kind::String && !field.is_list())
        .ok_or_else(|| format!("{} lacks matching single key", batch.full_name()))?;
    let list = paging_policy(pool, single).is_some();
    let results = batch
        .output()
        .get_field_by_name(if list { "groups" } else { "results" })
        .filter(FieldDescriptor::is_list)
        .ok_or_else(|| format!("{} lacks canonical batch outcomes", batch.full_name()))?;
    Ok(Some(BatchPolicy {
        kind: if list { "list" } else { "get" },
        method: batch.name().into(),
        key_field: key.json_name().into(),
        keys_field: keys.json_name().into(),
        results_field: results.json_name().into(),
    }))
}
