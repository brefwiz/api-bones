// SPDX-License-Identifier: MIT
//! The module's entry points. `floor_call` is in every variant; each other
//! export exists only when its feature is on, so the linker keeps a
//! capability's code exactly in the variants that ask for it.

use api_bones_transport::HttpTransport;
use connectrpc::client::ClientConfig;
use wasm_bindgen::prelude::*;

use crate::generated::bones::probe::v1::{EchoRequest, Probe1Client};

fn fail(error: impl core::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

fn config(base: &str) -> Result<ClientConfig, JsError> {
    let uri = base.parse::<http::Uri>().map_err(fail)?;
    Ok(ClientConfig::new(uri))
}

fn request(base: &str) -> EchoRequest {
    EchoRequest {
        id: base.to_owned(),
        ..EchoRequest::default()
    }
}

/// One unary RPC over the fetch transport: the floor every variant carries.
///
/// # Errors
/// The RPC's failure.
#[wasm_bindgen]
pub async fn floor_call(base: String) -> Result<String, JsError> {
    let client = Probe1Client::new(HttpTransport::fetch(), config(&base)?);
    let reply = client.echo(request(&base)).await.map_err(fail)?;
    Ok(reply.into_owned().value)
}

/// The same RPC, replayed by the shared retry policy under a call deadline.
///
/// # Errors
/// The RPC's failure once the policy stops retrying.
#[cfg(feature = "retry")]
#[wasm_bindgen]
pub async fn retry_call(base: String) -> Result<String, JsError> {
    use api_bones::connect::{Idempotency, fetch_with_retry, retries_for};

    let client = Probe1Client::new(HttpTransport::fetch(), config(&base)?);
    let deadline = api_bones_rt::Instant::now().checked_add(core::time::Duration::from_secs(30));
    let reply = fetch_with_retry(
        || {
            let client = &client;
            let request = request(&base);
            async move { client.echo(request).await.map(|reply| reply.into_owned()) }
        },
        retries_for(Idempotency::NoSideEffects),
        deadline,
    )
    .await
    .map_err(fail)?;
    Ok(reply.value)
}

/// The same RPC with gzip request compression negotiated.
///
/// # Errors
/// The RPC's failure.
#[cfg(feature = "gzip")]
#[wasm_bindgen]
pub async fn gzip_call(base: String) -> Result<String, JsError> {
    let config = config(&base)?.compress_requests("gzip");
    let client = Probe1Client::new(HttpTransport::fetch(), config);
    let reply = client.echo(request(&base)).await.map_err(fail)?;
    Ok(reply.into_owned().value)
}

/// The same RPC with the Node host exchange and workload identity policy
/// linked in, so the variant's size over the floor is their cost. The call
/// takes the exchange path under Node and `fetch` elsewhere.
///
/// # Errors
/// The RPC's failure.
#[cfg(feature = "node-identity")]
#[wasm_bindgen]
pub async fn node_identity_call(base: String) -> Result<String, JsError> {
    let client = Probe1Client::new(HttpTransport::fetch(), config(&base)?);
    let reply = client.echo(request(&base)).await.map_err(fail)?;
    Ok(reply.into_owned().value)
}

/// One unary RPC on a second service, next to the first.
///
/// # Errors
/// The RPC's failure.
#[cfg(feature = "second-service")]
#[wasm_bindgen]
pub async fn second_service_call(base: String) -> Result<String, JsError> {
    use crate::generated::bones::probe::v1::SecondClient;

    let client = SecondClient::new(HttpTransport::fetch(), config(&base)?);
    let reply = client.echo(request(&base)).await.map_err(fail)?;
    Ok(reply.into_owned().value)
}

/// Calls every listed method of one service and counts the answers.
#[cfg(any(feature = "methods-10", feature = "methods-50"))]
macro_rules! call_each {
    ($client:expr, $base:expr, [$($method:ident),+ $(,)?]) => {{
        let mut answered = 0_u32;
        $(
            $client.$method(request($base)).await.map_err(fail)?;
            answered += 1;
        )+
        answered.to_string()
    }};
}

/// A service of ten unary methods, every one reached.
///
/// # Errors
/// The first RPC failure.
#[cfg(feature = "methods-10")]
#[wasm_bindgen]
pub async fn methods_10_call(base: String) -> Result<String, JsError> {
    use crate::generated::bones::probe::v1::Probe10Client;

    let client = Probe10Client::new(HttpTransport::fetch(), config(&base)?);
    Ok(call_each!(
        client,
        &base,
        [m01, m02, m03, m04, m05, m06, m07, m08, m09, m10]
    ))
}

/// A service of fifty unary methods, every one reached.
///
/// # Errors
/// The first RPC failure.
#[cfg(feature = "methods-50")]
#[wasm_bindgen]
pub async fn methods_50_call(base: String) -> Result<String, JsError> {
    use crate::generated::bones::probe::v1::Probe50Client;

    let client = Probe50Client::new(HttpTransport::fetch(), config(&base)?);
    Ok(call_each!(
        client,
        &base,
        [
            m01, m02, m03, m04, m05, m06, m07, m08, m09, m10, m11, m12, m13, m14, m15, m16, m17,
            m18, m19, m20, m21, m22, m23, m24, m25, m26, m27, m28, m29, m30, m31, m32, m33, m34,
            m35, m36, m37, m38, m39, m40, m41, m42, m43, m44, m45, m46, m47, m48, m49, m50
        ]
    ))
}

/// Every item of a keyset List, following its cursor.
///
/// # Errors
/// The first page failure.
#[cfg(feature = "page")]
#[wasm_bindgen]
pub async fn page_call(base: String) -> Result<String, JsError> {
    use api_bones_batch::Batched;
    use futures_util::StreamExt as _;

    use crate::capability::CatalogPaged as _;
    use crate::generated::bones::probe::v1::{CatalogClient, ListItemsRequest};

    let client = CatalogClient::new(HttpTransport::fetch(), config(&base)?);
    let client = Batched::anonymous(client).map_err(fail)?;
    let mut items = std::pin::pin!(client.list_items(ListItemsRequest::default()).items());
    let mut seen = 0_u32;
    while let Some(item) = items.next().await {
        item.map_err(fail)?;
        seen += 1;
    }
    Ok(seen.to_string())
}

/// One coalesced Get and one explicit batch Get.
///
/// # Errors
/// The single Get's failure.
#[cfg(feature = "batch")]
#[wasm_bindgen]
pub async fn batch_call(base: String) -> Result<String, JsError> {
    use api_bones_batch::Batched;

    use crate::capability::CatalogBatch as _;
    use crate::generated::bones::probe::v1::{BatchGetItemsRequest, CatalogClient, GetItemRequest};

    let client = CatalogClient::new(HttpTransport::fetch(), config(&base)?);
    let client = Batched::anonymous(client).map_err(fail)?;
    let one = client
        .get_item(GetItemRequest {
            id: base.clone(),
            ..GetItemRequest::default()
        })
        .await
        .map_err(fail)?;
    let many = client
        .batch_get_items(BatchGetItemsRequest {
            ids: vec![base, one.id],
            ..BatchGetItemsRequest::default()
        })
        .await;
    Ok(many.len().to_string())
}

/// The first message of a server stream that rotates itself.
///
/// # Errors
/// The failure of the stream, or an end before any message.
#[cfg(feature = "stream")]
#[wasm_bindgen]
pub async fn stream_call(base: String) -> Result<String, JsError> {
    use api_bones_stream::{OpenRequest, rotating_stream};
    use connectrpc::ConnectError;
    use connectrpc::client::CallOptions;
    use futures_util::StreamExt as _;

    use crate::generated::bones::probe::v1::{CatalogClient, WatchEvent, WatchRequest};

    let config = config(&base)?;
    let open = move |declare: OpenRequest| {
        let config = config.clone();
        async move {
            let client = CatalogClient::new(HttpTransport::fetch(), config);
            let options =
                CallOptions::default().with_header(declare.header_name, declare.header_value);
            let upstream = client
                .watch_with_options(WatchRequest::default(), options)
                .await?;
            Ok::<_, ConnectError>(Box::pin(futures_util::stream::unfold(
                upstream,
                |mut upstream| async move {
                    match upstream.message::<WatchEvent>().await {
                        Ok(Some(event)) => Some((Ok(event.to_owned_message()), upstream)),
                        Ok(None) => None,
                        Err(error) => Some((Err(error), upstream)),
                    }
                },
            )))
        }
    };
    let mut events = std::pin::pin!(rotating_stream(open, |event: &WatchEvent| Some(
        event.id.clone()
    )));
    let first = events
        .next()
        .await
        .ok_or_else(|| fail("the stream ended before its first message"))?
        .map_err(fail)?;
    Ok(first.id)
}
