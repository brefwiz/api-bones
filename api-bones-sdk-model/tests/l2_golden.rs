// SPDX-License-Identifier: MIT
//! The document built for a small Layer 2 crate equals the golden text, byte
//! for byte: the wrapper's snapshot gate compares the same bytes.

use api_bones_sdk_model::{Model, SpecL2Methods, l2_methods};

const SOURCE: &str = r#"
    #[record]
    pub struct Number { pub value: i64 }

    #[client(surface = "connect-pagination")]
    impl NumbersClient {
        #[export]
        pub async fn list(&self, limit: u32) -> ListCall<Number> { body() }
    }

    #[client(surface = "connect-batch")]
    impl WidgetsClient {
        #[export]
        pub async fn get_widget(&self, id: String) -> Result<Number, Error> { body() }
        #[export]
        pub async fn batch_get_widgets(&self, ids: Vec<String>) -> Batch<Number> { body() }
        #[export(sync)]
        pub fn build_label(&self) -> String { body() }
    }
"#;

fn spec() -> SpecL2Methods {
    let endpoints = [
        ("contract.v1.Numbers/List", vec!["list"]),
        (
            "contract.v1.Widgets/GetWidget",
            vec!["get_widget", "batch_get_widgets"],
        ),
        (
            "contract.v1.Widgets/BatchGetWidgets",
            vec!["batch_get_widgets"],
        ),
    ];
    SpecL2Methods {
        surfaces: ["connect-pagination", "connect-batch"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        endpoints: endpoints
            .into_iter()
            .map(|(endpoint, rust)| {
                (
                    endpoint.to_owned(),
                    rust.into_iter().map(str::to_owned).collect(),
                )
            })
            .collect(),
    }
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
