// SPDX-License-Identifier: MIT
//! The document built for a small Layer 2 crate equals the golden text, byte
//! for byte: the wrapper's snapshot gate compares the same bytes.

use std::collections::BTreeMap;

use api_bones_sdk_model::{Model, SpecL2Methods, l2_methods};

const SOURCE: &str = r#"
    #[client(surface = "connect-pagination", proto = generated::contract::sdk::v1)]
    impl NumbersClient {
        #[export]
        pub async fn list(&self, limit: u32) -> ListCall<generated::contract::sdk::v1::Number> { body() }
    }

    #[client(surface = "connect-batch", proto = generated::contract::sdk::v1)]
    impl WidgetsClient {
        #[export]
        pub async fn get_widget(&self, id: String) -> Result<generated::contract::sdk::v1::Number, Error> { body() }
        #[export]
        pub async fn batch_get_widgets(&self, ids: Vec<String>) -> Batch<generated::contract::sdk::v1::Number> { body() }
        #[export(sync)]
        pub fn build_label(&self) -> String { body() }
    }
"#;

fn spec() -> SpecL2Methods {
    let endpoints = [
        (
            "connect-pagination",
            "contract.v1.Numbers/List",
            vec!["list"],
        ),
        (
            "connect-batch",
            "contract.v1.Widgets/GetWidget",
            vec!["get_widget", "batch_get_widgets"],
        ),
        (
            "connect-batch",
            "contract.v1.Widgets/BatchGetWidgets",
            vec!["batch_get_widgets"],
        ),
    ];
    let mut spec = SpecL2Methods {
        surfaces: ["connect-pagination", "connect-batch"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        endpoints: BTreeMap::new(),
    };
    for (surface, endpoint, rust) in endpoints {
        spec.endpoints
            .entry(surface.to_owned())
            .or_default()
            .insert(
                endpoint.to_owned(),
                rust.into_iter().map(str::to_owned).collect(),
            );
    }
    spec
}

#[test]
fn the_document_equals_the_golden_text() {
    let model = Model::from_source(SOURCE).expect("a model");
    let document = l2_methods(
        &model,
        &spec(),
        "api-bones-test",
        "api-bones-test-service-l2",
        "@brefwiz/api-bones-test-service",
    )
    .expect("a document");
    assert_eq!(
        document.to_json(),
        include_str!("fixtures/numbers.golden.json")
    );
}
