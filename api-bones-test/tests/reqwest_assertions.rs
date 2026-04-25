#![cfg(feature = "reqwest")]

use api_bones::error::ErrorCode;
use api_bones::response::ApiResponse;
use api_bones_test::reqwest::{assert_envelope_reqwest, assert_problem_json_reqwest};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn start_mock_server() -> MockServer {
    MockServer::start().await
}

#[tokio::test]
async fn assert_envelope_reqwest_parses_ok_response() {
    let server = start_mock_server().await;
    let body: ApiResponse<u32> = ApiResponse::builder(99u32).build();

    Mock::given(method("GET"))
        .and(path("/items/1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(&body)
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/items/1", server.uri()))
        .send()
        .await
        .unwrap();

    let value: u32 = assert_envelope_reqwest(resp).await;
    assert_eq!(value, 99);
}

#[tokio::test]
async fn assert_problem_json_reqwest_parses_problem() {
    let server = start_mock_server().await;
    let err = api_bones::error::ApiError::not_found("not here");

    let body = serde_json::to_vec(&err).unwrap();
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_raw(body, "application/problem+json"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/missing", server.uri()))
        .send()
        .await
        .unwrap();

    let problem = assert_problem_json_reqwest(resp, ErrorCode::ResourceNotFound).await;
    assert_eq!(problem.code, ErrorCode::ResourceNotFound);
}
