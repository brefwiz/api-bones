// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Constructor checks against changed generated method options.

use api_bones_batch::{Batched, CredentialNeutralClient};
use connectrpc::ConnectError;
use connectrpc::client::{ClientConfig, HttpClient};
use prost_reflect::{DescriptorPool, DynamicMessage, Value, prost::Message};

use crate::{CONTRACT_DESCRIPTOR, WidgetsClient};

/// Generated Widgets client bound to its changed method-option descriptor.
pub struct DescriptorClient {
    inner: WidgetsClient<HttpClient>,
    descriptor: Vec<u8>,
}

impl CredentialNeutralClient for DescriptorClient {
    fn batch_client_config(&self) -> &ClientConfig {
        self.inner.config()
    }

    fn batch_client_descriptor(&self) -> &[u8] {
        &self.descriptor
    }

    fn batch_client_service(&self) -> &str {
        "contract.v1.Widgets"
    }
}

fn descriptor_with_idempotency(level: Option<i32>) -> Vec<u8> {
    let pool = DescriptorPool::decode(CONTRACT_DESCRIPTOR).expect("generated descriptor");
    let descriptor = pool
        .get_message_by_name("google.protobuf.FileDescriptorSet")
        .expect("descriptor schema");
    let mut set = DynamicMessage::decode(descriptor, CONTRACT_DESCRIPTOR)
        .expect("generated descriptor message");
    let files = set
        .get_field_by_name_mut("file")
        .and_then(Value::as_list_mut)
        .expect("files");
    for file in files {
        let file = file.as_message_mut().expect("file descriptor");
        let services = file
            .get_field_by_name_mut("service")
            .and_then(Value::as_list_mut)
            .expect("services");
        for service in services {
            let service = service.as_message_mut().expect("service descriptor");
            if service
                .get_field_by_name("name")
                .and_then(|name| name.as_str().map(str::to_owned))
                .as_deref()
                != Some("Widgets")
            {
                continue;
            }
            let methods = service
                .get_field_by_name_mut("method")
                .and_then(Value::as_list_mut)
                .expect("methods");
            for method in methods {
                let method = method.as_message_mut().expect("method descriptor");
                if method
                    .get_field_by_name("name")
                    .and_then(|name| name.as_str().map(str::to_owned))
                    .as_deref()
                    != Some("BatchGetWidgets")
                {
                    continue;
                }
                let options = method
                    .get_field_by_name_mut("options")
                    .and_then(Value::as_message_mut)
                    .expect("method options");
                if let Some(level) = level {
                    options.set_field_by_name("idempotency_level", Value::EnumNumber(level));
                } else {
                    options.clear_field_by_name("idempotency_level");
                }
            }
        }
    }
    set.encode_to_vec()
}

fn client(descriptor: Vec<u8>) -> DescriptorClient {
    let inner = WidgetsClient::new(
        HttpClient::plaintext_http2_only(),
        ClientConfig::new("http://127.0.0.1:1".parse().expect("loopback URI")),
    );
    DescriptorClient { inner, descriptor }
}

fn construct(descriptor: Vec<u8>) -> Result<(), ConnectError> {
    Batched::anonymous(client(descriptor)).map(|_| ())
}

/// Bind the generated Widgets client to a changed batch method declaration.
pub fn client_with_idempotency(level: Option<i32>) -> DescriptorClient {
    client(descriptor_with_idempotency(level))
}

/// Attempt batch construction after changing the generated batch method's declaration.
///
/// # Errors
/// Refuses every declaration except `NO_SIDE_EFFECTS`.
pub fn construct_with_idempotency(level: Option<i32>) -> Result<(), ConnectError> {
    construct(descriptor_with_idempotency(level))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_descriptor_keeps_batch_options_and_standard_idempotency() {
        let pool = DescriptorPool::decode(CONTRACT_DESCRIPTOR).expect("built descriptor");
        let extension = pool
            .get_extension_by_name("bones.v1.batch_of")
            .expect("batch_of extension");
        let service = pool
            .get_service_by_name("contract.v1.Widgets")
            .expect("Widgets service");
        let method = service
            .methods()
            .find(|method| method.name() == "BatchGetWidgets")
            .expect("batch method");
        let options = method.options();
        assert!(options.has_extension(&extension));
        assert_eq!(
            options.get_extension(&extension).as_str(),
            Some("GetWidget")
        );
        assert_eq!(
            options
                .get_field_by_name("idempotency_level")
                .and_then(|value| value.as_enum_number()),
            Some(1)
        );
    }

    #[test]
    fn batch_construction_reads_declared_idempotency() {
        assert!(construct_with_idempotency(Some(1)).is_ok());
        for level in [None, Some(0), Some(2), Some(99)] {
            let error = construct_with_idempotency(level).expect_err("unsafe batch refused");
            assert_eq!(error.code, connectrpc::ErrorCode::InvalidArgument);
            assert_eq!(
                error.message.as_deref(),
                Some("batch method BatchGetWidgets must declare NO_SIDE_EFFECTS")
            );
        }
    }

    #[test]
    fn malformed_or_missing_descriptors_fail_closed() {
        for descriptor in [Vec::new(), vec![255]] {
            assert_eq!(
                construct(descriptor).expect_err("descriptor refused").code,
                connectrpc::ErrorCode::InvalidArgument
            );
        }
    }

    #[test]
    fn method_glue_cannot_dispatch_outside_its_validated_relation() {
        let inner = WidgetsClient::new(
            HttpClient::plaintext_http2_only(),
            ClientConfig::new("http://127.0.0.1:1".parse().expect("loopback URI")),
        );
        let client = Batched::anonymous(inner).expect("declared read batches");
        assert!(
            client
                .__validate_method(
                    "contract.v1.Widgets/GetWidget",
                    "contract.v1.Widgets/BatchGetWidgets"
                )
                .is_ok()
        );
        for (single, batch) in [
            (
                "contract.v1.Widgets/ListWidgetParts",
                "contract.v1.Widgets/BatchGetWidgets",
            ),
            (
                "contract.v1.Widgets/GetWidget",
                "contract.v1.Widgets/UpdateWidgetParts",
            ),
            (
                "other.Widgets/GetWidget",
                "contract.v1.Widgets/BatchGetWidgets",
            ),
        ] {
            assert_eq!(
                client
                    .__validate_method(single, batch)
                    .expect_err("wrong method pair")
                    .code,
                connectrpc::ErrorCode::InvalidArgument
            );
        }
    }
}
