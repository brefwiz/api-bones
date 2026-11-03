// SPDX-License-Identifier: MIT
//! Declared batch method contracts from the generated file descriptor set.

use std::collections::HashMap;

use connectrpc::ConnectError;
use prost_reflect::DescriptorPool;

pub(crate) type Methods = HashMap<String, String>;

pub(crate) fn batch_methods(bytes: &[u8], service: &str) -> Result<Methods, ConnectError> {
    let pool = DescriptorPool::decode(bytes).map_err(|error| {
        ConnectError::invalid_argument(format!("invalid batch client descriptor: {error}"))
    })?;
    let service = pool.get_service_by_name(service).ok_or_else(|| {
        ConnectError::invalid_argument("batch client service is missing from its descriptor")
    })?;
    let extension = pool
        .get_extension_by_name("bones.v1.batch_of")
        .ok_or_else(|| {
            ConnectError::invalid_argument("batch client descriptor has no batch policy discovery")
        })?;
    let mut methods = Methods::new();
    for method in service.methods() {
        let options = method.options();
        if !options.has_extension(&extension) {
            continue;
        }
        let relation = options.get_extension(&extension);
        let named = relation
            .as_str()
            .filter(|named| !named.is_empty())
            .ok_or_else(|| {
                ConnectError::invalid_argument(
                    "batch client descriptor has an empty batch relation",
                )
            })?;
        let single = match named.split_once('/') {
            Some((owner, single)) if owner == service.full_name() => single,
            Some(_) => {
                return Err(ConnectError::invalid_argument(
                    "batch relation names another service",
                ));
            }
            None => named,
        };
        if single == method.name()
            || !service
                .methods()
                .any(|candidate| candidate.name() == single)
        {
            return Err(ConnectError::invalid_argument(
                "batch relation must name a sibling single method",
            ));
        }
        let idempotency = options
            .get_field_by_name("idempotency_level")
            .and_then(|value| value.as_enum_number())
            .unwrap_or_default();
        if idempotency != 1 {
            return Err(ConnectError::invalid_argument(format!(
                "batch method {} must declare NO_SIDE_EFFECTS",
                method.name()
            )));
        }
        if method.is_client_streaming() || method.is_server_streaming() {
            return Err(ConnectError::invalid_argument("batch method must be unary"));
        }
        methods.insert(
            format!("{}/{}", service.full_name(), method.name()),
            format!("{}/{single}", service.full_name()),
        );
    }
    if methods.is_empty() {
        return Err(ConnectError::invalid_argument(
            "empty batch policy discovery",
        ));
    }
    Ok(methods)
}
