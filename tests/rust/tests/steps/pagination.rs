// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Pagination over the PostgreSQL-backed Widgets fixture and generated SDK.
use std::sync::Arc;
use std::time::Duration;

use crate::world::{PagingWorld, WidgetRequest, token_credential, writer_credential};
use api_bones_connect::{ErrorInfo, Idempotency, with_error_info};
use api_bones_contract_rust::{
    BatchListWidgetPartsRequest, ListWidgetPartsRequest, ListWidgetPartsResponse,
    UpdateWidgetPartsRequest,
};
use api_bones_page::{ListCall, Page, PageError, PageErrorKind, PageOptions, PagedResponse};
use connectrpc::{ConnectError, ErrorCode};
use cucumber::{given, then, when};
use futures_util::StreamExt;

const METHOD: &str = "contract.v1.Widgets/ListWidgetParts";
const PARENT: &str = "paged";

#[given(expr = "the server reports a total of kind {int}")]
fn given_total_kind(world: &mut PagingWorld, number: u8) {
    use api_bones_contract_rust::field_wire::Total;
    world.reported_total = Some(Total {
        value: 42,
        kind: i32::from(number).into(),
        ..Default::default()
    });
}

#[when("the caller fetches the first page with a list call")]
async fn when_first_total_page(world: &mut PagingWorld) {
    use api_bones_contract_rust::field_wire::Total;
    use api_bones_contract_rust::{ListNumbersRequest, ListNumbersResponse, PageResponse};
    use buffa::Message;

    let wire = world
        .reported_total
        .as_ref()
        .expect("reported total")
        .encode_to_vec();
    let decoded = Total::decode_from_slice(&wire).expect("decode response total");
    let fetch: api_bones_page::Fetch<ListNumbersRequest, ListNumbersResponse> =
        Arc::new(move |_| {
            let total = decoded.clone();
            Box::pin(async move {
                Ok(ListNumbersResponse {
                    items: vec![1],
                    page: PageResponse {
                        revision: "total-revision".into(),
                        total: total.into(),
                        ..Default::default()
                    }
                    .into(),
                    ..Default::default()
                })
            })
        });
    let call = ListCall::new(
        "contract.v1.Numbers/ListNoSideEffects",
        Idempotency::NoSideEffects,
        fetch,
        ListNumbersRequest::default(),
    );
    world.total_page = Some(
        api_bones_page::ListCall::first_page(call)
            .await
            .expect("SDK page with total"),
    );
}

#[then(expr = "the page total kind is {string}")]
fn then_page_total_kind(world: &mut PagingWorld, kind: String) {
    let expected = match kind.as_str() {
        "estimate" => api_bones_page::TotalKind::Estimate,
        "exact" => api_bones_page::TotalKind::Exact,
        _ => panic!("unknown expected total kind"),
    };
    assert_eq!(
        world
            .total_page
            .as_ref()
            .expect("SDK page with total")
            .total,
        Some(api_bones_page::Total {
            value: 42,
            kind: expected
        })
    );
}

#[then(expr = "the total kind travels as {int} on the wire and {string} in JSON")]
fn then_total_encoding(world: &mut PagingWorld, number: u8, name: String) {
    use api_bones_contract_rust::field_wire::Total;
    use buffa::Message;

    let total = world.reported_total.as_ref().expect("reported total");
    let json = serde_json::to_value(total).expect("serialize canonical total");
    assert_eq!(json, serde_json::json!({ "value": "42", "kind": name }));
    let from_json: Total = serde_json::from_value(json).expect("deserialize canonical total");
    assert_eq!(&from_json, total);
    let wire = total.encode_to_vec();
    assert_eq!(wire, vec![8, 42, 16, number]);
    assert_eq!(
        &Total::decode_from_slice(&wire).expect("decode total"),
        total
    );
}

#[derive(Default)]
struct ListTelemetry {
    open: std::collections::HashMap<u64, std::collections::BTreeMap<String, String>>,
    spans: Vec<std::collections::BTreeMap<String, String>>,
    events: Vec<std::collections::BTreeMap<String, String>>,
}

static LIST_TELEMETRY: std::sync::LazyLock<std::sync::Mutex<ListTelemetry>> =
    std::sync::LazyLock::new(Default::default);
#[derive(Default)]
struct ListVisitor(std::collections::BTreeMap<String, String>);
impl tracing::field::Visit for ListVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }
}
pub(super) struct ListLayer;
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ListLayer {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() != "bones.list" {
            return;
        }
        let mut fields = ListVisitor::default();
        attrs.record(&mut fields);
        LIST_TELEMETRY
            .lock()
            .expect("telemetry lock")
            .open
            .insert(id.into_u64(), fields.0);
    }
    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = ListVisitor::default();
        values.record(&mut fields);
        if let Some(span) = LIST_TELEMETRY
            .lock()
            .expect("telemetry lock")
            .open
            .get_mut(&id.into_u64())
        {
            span.extend(fields.0);
        }
    }
    fn on_close(&self, id: tracing::span::Id, _: tracing_subscriber::layer::Context<'_, S>) {
        let mut telemetry = LIST_TELEMETRY.lock().expect("telemetry lock");
        if let Some(span) = telemetry.open.remove(&id.into_u64()) {
            telemetry.spans.push(span);
        }
    }
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != "bones.list" {
            return;
        }
        let mut fields = ListVisitor::default();
        event.record(&mut fields);
        LIST_TELEMETRY
            .lock()
            .expect("telemetry lock")
            .events
            .push(fields.0);
    }
}

fn reset_telemetry() {
    *LIST_TELEMETRY.lock().expect("telemetry lock") = ListTelemetry::default();
}

fn parts(text: &str) -> Vec<String> {
    text.split(',')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn request() -> ListWidgetPartsRequest {
    ListWidgetPartsRequest {
        widget_id: PARENT.into(),
        ..Default::default()
    }
}

fn db(world: &PagingWorld) -> Arc<api_bones_contract_rust::widget_fixture::BatchDb> {
    world
        .batch
        .served
        .as_ref()
        .expect("widgets provider running")
        .db
        .clone()
}

fn organization(world: &PagingWorld) -> String {
    world
        .batch
        .credential
        .lock()
        .expect("credential")
        .identity
        .org
        .clone()
}

async fn stored(world: &PagingWorld) -> (Vec<String>, String) {
    db(world)
        .parts_snapshot(&organization(world), PARENT)
        .await
        .expect("stored widget")
}

fn seen(world: &PagingWorld) -> Vec<WidgetRequest> {
    let seed_request_count = world.seed_request_count;
    world
        .batch
        .data
        .lock()
        .expect("widget observations")
        .seen
        .iter()
        .filter(|request| request.rpc.starts_with("ListWidgetParts"))
        .skip(seed_request_count)
        .cloned()
        .collect()
}

async fn generated_call(
    world: &mut PagingWorld,
) -> ListCall<ListWidgetPartsRequest, ListWidgetPartsResponse> {
    let client = super::batch::client(&mut world.batch).await;
    let credential = world.batch.credential.clone();
    let database = db(world);
    let lock_after_first = world.lock_after_first;
    let outage_page = world.outage_page;
    let recovery_after = world.recovery_after;
    let failed_attempts = world.failed_attempts.clone();
    let server = world
        .batch
        .served
        .as_ref()
        .expect("provider lifecycle")
        .clone();
    let idempotency = world.idempotency;
    let fetch: api_bones_page::MetadataFetch<ListWidgetPartsRequest, ListWidgetPartsResponse> =
        Arc::new(move |request, metadata| {
            let client = client.clone();
            let database = database.clone();
            let server = server.clone();
            let failed_attempts = failed_attempts.clone();
            let mut options =
                super::batch::credential_options(&credential.lock().expect("credential"));
            if metadata.cache_opt_in {
                options = options.with_header("bones-list-cache", "1");
            }
            if let Some(revision) = metadata.if_none_match {
                options = options.with_header("if-none-match", revision);
            }
            Box::pin(async move {
                let first = request
                    .page
                    .as_option()
                    .is_none_or(|page| page.cursor.is_empty());
                let response = match idempotency {
                    Idempotency::NoSideEffects => {
                        client
                            .raw()
                            .list_widget_parts_with_options(request, options)
                            .await
                    }
                    Idempotency::Idempotent => {
                        client
                            .raw()
                            .list_widget_parts_idempotent_with_options(request, options)
                            .await
                    }
                    Idempotency::NonIdempotent | Idempotency::Unspecified => {
                        client
                            .raw()
                            .list_widget_parts_unspecified_with_options(request, options)
                            .await
                    }
                };
                let response = match response {
                    Ok(response) => response,
                    Err(error) => {
                        if outage_page.is_some() {
                            let count = {
                                let mut attempts =
                                    failed_attempts.lock().expect("actual failed attempts");
                                attempts.push(error.code);
                                attempts.len()
                            };
                            if count == recovery_after {
                                server.restart().await;
                            }
                        }
                        return Err(error);
                    }
                };
                let not_modified = response
                    .headers()
                    .get("bones-not-modified")
                    .is_some_and(|value| value == "1");
                if lock_after_first && first {
                    database.lock_reads().await;
                }
                if outage_page == Some(2) && first {
                    server.stop().await;
                }
                Ok(api_bones_page::ListResponse {
                    body: response.into_owned(),
                    not_modified,
                })
            })
        });
    let identity = world.identity.clone();
    use buffa::Message as _;
    let request = request();
    let encoded = request.encode_to_vec();
    ListCall::generated(
        match idempotency {
            Idempotency::NoSideEffects => METHOD,
            Idempotency::Idempotent => "contract.v1.Widgets/ListWidgetPartsIdempotent",
            Idempotency::NonIdempotent | Idempotency::Unspecified => {
                "contract.v1.Widgets/ListWidgetPartsUnspecified"
            }
        },
        world.idempotency,
        fetch,
        request,
        world.cache.clone(),
        Arc::new(move || identity.lock().expect("identity").clone()),
        &encoded,
    )
}

/// The pages (or items) a finished stream delivered before its failure, and
/// the failure: a stream ends with its first error.
fn split_results<T>(
    results: Vec<Result<T, api_bones_page::PageError>>,
) -> (Vec<T>, Option<api_bones_page::PageError>) {
    if let Some(index) = results.iter().position(Result::is_err) {
        assert_eq!(
            index + 1,
            results.len(),
            "stream terminates after its error"
        );
    }
    let (delivered, failures): (Vec<_>, Vec<_>) = results.into_iter().partition(Result::is_ok);
    let delivered = delivered.into_iter().map(Result::unwrap).collect();
    let failure = failures.into_iter().find_map(Result::err);
    (delivered, failure)
}

fn record_pages(world: &mut PagingWorld) {
    world.items = world
        .latest
        .iter()
        .flat_map(|page| page.items.iter().cloned())
        .collect();
    world.pages = world
        .latest
        .iter()
        .map(|page| page.items.to_vec())
        .collect();
    world.revisions = world
        .latest
        .iter()
        .map(|page| page.revision.clone())
        .collect();
    world.ended = true;
}

fn without_prefetch() -> PageOptions {
    PageOptions {
        prefetch: 0,
        ..PageOptions::default()
    }
}

#[given(regex = r#"^a list of pages ((?:"[^"]*" ?)+)$"#)]
async fn given_pages(world: &mut PagingWorld, quoted: String) {
    reset_telemetry();
    let pages: Vec<Vec<String>> = quoted
        .split('"')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, text)| parts(text))
        .collect();
    let page_size = pages.first().expect("fixture pages").len();
    assert!(page_size > 0);
    assert!(
        pages[..pages.len() - 1]
            .iter()
            .all(|page| page.len() == page_size)
    );
    let values: Vec<_> = pages.into_iter().flatten().collect();
    {
        let mut data = world.batch.data.lock().expect("widget fixtures");
        data.part_page_size = page_size;
        for org in ["acme", "org-b"] {
            data.widgets
                .insert((org.into(), PARENT.into()), PARENT.into());
            data.parts
                .insert((org.into(), PARENT.into()), values.clone());
        }
    }
    let credential = writer_credential(&world.batch.issuers, "ada", "acme").await;
    *world.batch.credential.lock().expect("credential") = credential;
    *world.identity.lock().expect("identity") = "ada:acme".into();
    super::batch::client(&mut world.batch).await;
}

#[given(expr = "the current stored list revision is remembered as {string}")]
async fn given_revision(world: &mut PagingWorld, label: String) {
    let (_, revision) = stored(world).await;
    assert!(!revision.is_empty());
    world.remembered_revisions.insert(label, revision);
}

async fn replace_parts(world: &mut PagingWorld, items: Vec<String>, label: String) {
    let client = super::batch::client(&mut world.batch).await;
    let options = super::batch::raw_options(&world.batch);
    let response = client
        .raw()
        .update_widget_parts_with_options(
            UpdateWidgetPartsRequest {
                widget_id: PARENT.into(),
                items,
                ..Default::default()
            },
            options,
        )
        .await
        .expect("authorized widget update")
        .into_owned();
    let (stored_items, revision) = stored(world).await;
    assert_eq!(response.items, stored_items);
    assert_eq!(
        response
            .page
            .as_option()
            .expect("updated revision")
            .revision,
        revision
    );
    world.remembered_revisions.insert(label, revision);
}

#[given(
    expr = "the owner replaces stored widget parts with {string} and remembers revision as {string}"
)]
async fn given_write(world: &mut PagingWorld, items: String, label: String) {
    replace_parts(world, parts(&items), label).await;
}

#[given("the generated list client uses a revision cache")]
async fn given_cache(world: &mut PagingWorld) {
    super::batch::client(&mut world.batch).await;
}

#[given("the consumer retains every generated list page")]
async fn given_retains(world: &mut PagingWorld) {
    let stream = Box::pin(api_bones_page::ListCall::pages(
        generated_call(world).await.options(without_prefetch()),
    ));
    let results: Vec<_> = stream.collect().await;
    let (pages, error) = split_results(results);
    assert!(error.is_none());
    world.retained = pages;
}

#[given(
    expr = "the consumer retains every generated list page across a write changing revision to {string}"
)]
async fn given_mixed_revision(world: &mut PagingWorld, label: String) {
    let mut pages = Box::pin(api_bones_page::ListCall::pages(
        generated_call(world).await.options(without_prefetch()),
    ));
    let first = pages
        .next()
        .await
        .expect("first page")
        .expect("initial page");
    let initial_revision = first.revision.clone();
    world.retained.push(first);
    let (mut items, _) = stored(world).await;
    let next = items
        .iter()
        .map(|part| part.parse::<u64>().expect("numbered fixture part"))
        .max()
        .unwrap_or(0)
        + 1;
    items.push(next.to_string());
    replace_parts(world, items, label.clone()).await;
    let revision = world
        .remembered_revisions
        .get(&label)
        .expect("saved revision")
        .clone();
    assert_ne!(initial_revision, revision);
    while let Some(page) = pages.next().await {
        let page = page.expect("remaining real pages");
        assert_eq!(page.revision, revision);
        world.retained.push(page);
    }
    assert!(world.retained.len() > 1);
}

#[given("the caller holds the first stored list page")]
async fn given_first(world: &mut PagingWorld) {
    let client = super::batch::client(&mut world.batch).await;
    let response = client
        .raw()
        .list_widget_parts_with_options(request(), super::batch::raw_options(&world.batch))
        .await
        .expect("first stored page")
        .into_owned();
    let (items, metadata) = response.split();
    world.seed = Some(Page {
        items: items.into(),
        revision: metadata.revision,
        total: metadata.total,
        from_cache: false,
        has_next: !metadata.next_cursor.is_empty(),
    });
    world.seed_cursor = metadata.next_cursor;
    world.seed_request_count = world
        .batch
        .data
        .lock()
        .expect("observations")
        .seen
        .iter()
        .filter(|request| request.rpc == "ListWidgetParts")
        .count();
}

#[given("the caller holds the first grouped widget list page")]
async fn given_grouped_first(world: &mut PagingWorld) {
    let client = super::batch::client(&mut world.batch).await;
    let response = client
        .raw()
        .batch_list_widget_parts_with_options(
            BatchListWidgetPartsRequest {
                widget_ids: vec![PARENT.into()],
                ..Default::default()
            },
            super::batch::raw_options(&world.batch),
        )
        .await
        .expect("real grouped page")
        .into_owned();
    let group = response
        .groups
        .into_iter()
        .next()
        .expect("requested parent");
    assert_eq!(group.widget_id, PARENT);
    assert!(group.error.as_option().is_none());
    let metadata = group.page.into_option().expect("group paging");
    world.seed = Some(Page {
        items: group.items.into(),
        revision: metadata.revision,
        total: None,
        from_cache: false,
        has_next: !metadata.next_cursor.is_empty(),
    });
    world.seed_cursor = metadata.next_cursor;
}

#[given(expr = "the stored list read for page {int} waits on another transaction")]
async fn given_block(world: &mut PagingWorld, page: usize) {
    assert!(page == 1 || page == 2);
    if page == 1 {
        db(world).lock_reads().await;
    } else {
        world.lock_after_first = true;
    }
}

#[given(expr = "the RPC is declared {string}")]
fn given_contract(world: &mut PagingWorld, contract: String) {
    world.idempotency = match contract.as_str() {
        "NO_SIDE_EFFECTS" => Idempotency::NoSideEffects,
        "IDEMPOTENT" => Idempotency::Idempotent,
        "NON_IDEMPOTENT" => Idempotency::NonIdempotent,
        "UNSPECIFIED" => Idempotency::Unspecified,
        _ => panic!("unknown fixture declaration"),
    };
}

#[given(
    expr = "the widgets provider stops before page {int} and restarts after {int} failed attempts"
)]
async fn given_outage(world: &mut PagingWorld, page: usize, recovery_after: usize) {
    assert!(page == 1 || page == 2);
    assert!(recovery_after > 0);
    world.outage_page = Some(page);
    world.recovery_after = recovery_after;
    if page == 1 {
        world
            .batch
            .served
            .as_ref()
            .expect("real provider")
            .stop()
            .await;
    }
}

#[given("the producer credential is no longer valid")]
fn given_invalid_credential(world: &mut PagingWorld) {
    let mut credential = world.batch.credential.lock().expect("credential");
    let (_, authorization) = credential
        .headers
        .iter_mut()
        .find(|(name, _)| name == "authorization")
        .expect("signed credential");
    authorization.push(b'!');
}

#[given(expr = "the caller identity changes to {string}")]
async fn given_identity(world: &mut PagingWorld, identity: String) {
    let (subject, org) = identity.split_once(':').expect("subject and organization");
    let credential = token_credential(&world.batch.issuers, subject, org).await;
    *world.batch.credential.lock().expect("credential") = credential;
    *world.identity.lock().expect("identity") = identity;
}

#[given("the consumer releases retained list pages")]
fn given_release(world: &mut PagingWorld) {
    world.retained.clear();
    world.latest.clear();
}

#[when("the consumer reads every item")]
async fn when_items(world: &mut PagingWorld) {
    let stream = Box::pin(api_bones_page::ListCall::items(generated_call(world).await));
    let results: Vec<_> = stream.collect().await;
    let (items, error) = split_results(results);
    world.items = items;
    world.error = error;
    world.ended = true;
}

#[when("the consumer reads every item through the platform list call")]
async fn when_platform_items(world: &mut PagingWorld) {
    use api_bones_contract_rust::WidgetsLists;

    let client = super::batch::client(&mut world.batch).await;
    let call = client.list_widget_parts(request());
    let results = api_bones_page::ListCall::items(call).collect().await;
    let (items, error) = split_results(results);
    world.items = items;
    world.error = error;
    world.ended = true;
}

async fn platform_pages(
    call: ListCall<ListWidgetPartsRequest, ListWidgetPartsResponse>,
) -> Vec<Page<String>> {
    api_bones_page::ListCall::pages(call.options(without_prefetch()))
        .map(|page| page.expect("platform list succeeds"))
        .collect()
        .await
}

#[when("the platform client batch-gets the parent and replays its list call across a clone")]
async fn when_platform_composition(world: &mut PagingWorld) {
    use api_bones_contract_rust::{BatchGetWidgetsRequest, WidgetGets, WidgetsLists};

    let client = super::batch::client(&mut world.batch).await;
    let batch = client
        .batch_get_widgets(BatchGetWidgetsRequest {
            ids: vec![PARENT.into()],
            ..Default::default()
        })
        .await
        .into_vec();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].as_ref().expect("parent widget").id, PARENT);
    let retained = platform_pages(client.list_widget_parts(request())).await;
    assert!(retained.iter().all(|page| !page.from_cache));
    let clone = client.clone();
    world.latest = platform_pages(clone.list_widget_parts(request())).await;
    assert!(world.latest.iter().all(|page| page.from_cache));
    assert_eq!(seen(world).len(), retained.len() + 1);
    world.retained = retained;
    record_pages(world);
}

#[then("an independently constructed platform client fetches its own pages")]
async fn then_independent_platform_pages(world: &mut PagingWorld) {
    use api_bones_contract_rust::WidgetsLists;

    let client = super::batch::client(&mut world.batch).await;
    let source = world.batch.credential.clone();
    let independent = api_bones_batch::Batched::new(client.raw().clone(), move || {
        Ok(source.lock().expect("credential").clone())
    })
    .expect("independent platform client");
    let before = seen(world).len();
    let pages = platform_pages(independent.list_widget_parts(request())).await;
    assert!(pages.iter().all(|page| !page.from_cache));
    assert_eq!(seen(world).len() - before, world.retained.len());
    assert_eq!(
        pages
            .iter()
            .flat_map(|page| page.items.iter().cloned())
            .collect::<Vec<_>>(),
        world.items
    );
}

#[when(expr = "the consumer reads every item with page size {int}")]
async fn when_items_sized(world: &mut PagingWorld, size: u32) {
    let stream = Box::pin(api_bones_page::ListCall::items(
        generated_call(world).await.options(PageOptions {
            page_size: Some(size),
            ..PageOptions::default()
        }),
    ));
    let results: Vec<_> = stream.collect().await;
    let (items, error) = split_results(results);
    world.items = items;
    world.error = error;
    world.ended = true;
}

#[when("the consumer reads every page")]
async fn when_pages(world: &mut PagingWorld) {
    let stream = Box::pin(api_bones_page::ListCall::pages(generated_call(world).await));
    let results: Vec<_> = stream.collect().await;
    let (pages, error) = split_results(results);
    world.latest = pages;
    world.error = error;
    record_pages(world);
}

#[when("the consumer lists again")]
async fn when_again(world: &mut PagingWorld) {
    world.revalidation_start = seen(world).len();
    let stream = Box::pin(api_bones_page::ListCall::pages(
        generated_call(world).await.options(without_prefetch()),
    ));
    let results: Vec<_> = stream.collect().await;
    let (pages, error) = split_results(results);
    world.latest = pages;
    world.error = error;
    record_pages(world);
}

#[when("the consumer lists bypassing the revision cache")]
async fn when_bypass(world: &mut PagingWorld) {
    world.revalidation_start = seen(world).len();
    let stream = Box::pin(api_bones_page::ListCall::pages(
        generated_call(world).await.options(PageOptions {
            cache: api_bones_page::CacheMode::Bypass,
            ..without_prefetch()
        }),
    ));
    let results: Vec<_> = stream.collect().await;
    let (pages, error) = split_results(results);
    world.latest = pages;
    world.error = error;
    record_pages(world);
}

#[given("the consumer has listed bypassing the revision cache")]
async fn given_bypassed(world: &mut PagingWorld) {
    when_bypass(world).await;
    then_cached(world, "false".into()).await;
    then_revalidation(world, String::new());
}

#[when("the consumer reads one page and stops")]
async fn when_one(world: &mut PagingWorld) {
    let mut stream = Box::pin(api_bones_page::ListCall::pages(generated_call(world).await));
    let first = stream
        .next()
        .await
        .expect("first result")
        .expect("first page");
    if world.lock_after_first {
        db(world).wait_for_blocked_read().await;
    }
    drop(stream);
    world.latest = Vec::from([first]);
    world.error = None;
    record_pages(world);
}

#[when("the consumer reads one page with prefetch off and stops")]
async fn when_one_no_prefetch(world: &mut PagingWorld) {
    let mut stream = Box::pin(api_bones_page::ListCall::pages(
        generated_call(world).await.options(without_prefetch()),
    ));
    let first = stream
        .next()
        .await
        .expect("first result")
        .expect("first page");
    world.latest = Vec::from([first]);
    world.error = None;
    record_pages(world);
    drop(stream);
}

#[when("the consumer reads the first page only")]
async fn when_first(world: &mut PagingWorld) {
    let call: ListCall<ListWidgetPartsRequest, ListWidgetPartsResponse> =
        generated_call(world).await;
    let result = call.first_page().await;
    match result {
        Ok(page) => {
            world.latest = Vec::from([page]);
            world.error = None;
            record_pages(world);
        }
        Err(error) => world.error = Some(error),
    }
}

#[when("the consumer continues from the held first page")]
#[when("the consumer continues a batch-seeded generated list")]
async fn when_continue(world: &mut PagingWorld) {
    world.revalidation_start = seen(world).len();
    let first = world.seed.take().expect("real first page held");
    let cursor = world.seed_cursor.clone();
    let stream = Box::pin(api_bones_page::ListCall::pages(
        generated_call(world).await.seed_first_page(&first, cursor),
    ));
    let results: Vec<_> = stream.collect().await;
    let (pages, error) = split_results(results);
    world.latest = pages;
    world.error = error;
    record_pages(world);
}

#[given("the consumer has continued a batch-seeded generated list")]
async fn given_batch_continued(world: &mut PagingWorld) {
    when_continue(world).await;
    then_cached(world, "false".into()).await;
    then_batch_headers(world);
}

#[when("the caller makes a list call with a matching revision without the list cache opt-in")]
async fn when_no_opt_in(world: &mut PagingWorld) {
    let client = super::batch::client(&mut world.batch).await;
    let revision = world
        .remembered_revisions
        .get("r1")
        .expect("revision captured in Given")
        .clone();
    let options = super::batch::raw_options(&world.batch)
        .with_header("if-none-match", format!("\"{revision}\""));
    let raw_response = Arc::new(std::sync::Mutex::new(None));
    let captured = raw_response.clone();
    let fetch: api_bones_page::Fetch<ListWidgetPartsRequest, ListWidgetPartsResponse> =
        Arc::new(move |request| {
            let client = client.clone();
            let options = options.clone();
            let captured = captured.clone();
            Box::pin(async move {
                let response = client
                    .raw()
                    .list_widget_parts_with_options(request, options)
                    .await
                    .expect("ordinary list response");
                assert!(response.headers().get("bones-not-modified").is_none());
                let response = response.into_owned();
                *captured.lock().expect("raw response") = Some(response.clone());
                Ok(response)
            })
        });
    let call: ListCall<ListWidgetPartsRequest, ListWidgetPartsResponse> =
        ListCall::new(METHOD, Idempotency::NoSideEffects, fetch, request());
    let page = call.first_page().await.expect("SDK first page");
    let response = raw_response
        .lock()
        .expect("raw response")
        .take()
        .expect("one raw response");
    assert_eq!(page.items.as_ref(), response.items.as_slice());
    assert_eq!(
        page.revision,
        response
            .page
            .as_option()
            .expect("normal page metadata")
            .revision
    );
    world.latest = Vec::from([page]);
    world.error = None;
    record_pages(world);
}

async fn paused_replay(
    world: &mut PagingWorld,
) -> std::pin::Pin<
    Box<dyn futures_util::Stream<Item = Result<Page<String>, api_bones_page::PageError>> + Send>,
> {
    let mut pages = Box::pin(
        generated_call(world)
            .await
            .options(without_prefetch())
            .pages(),
    );
    assert!(
        pages
            .next()
            .await
            .expect("cached page")
            .expect("cached replay")
            .from_cache
    );
    pages
}

#[given("the consumer has paused a cached replay")]
async fn given_paused(world: &mut PagingWorld) {
    world.paused = Some(paused_replay(world).await);
}

#[given(expr = "the consumer has delivered {int} stored list pages")]
async fn given_prefix(world: &mut PagingWorld, count: usize) {
    let call: ListCall<ListWidgetPartsRequest, ListWidgetPartsResponse> =
        generated_call(world).await;
    let mut stream = Box::pin(call.options(without_prefetch()).pages());
    let results: Vec<_> = (&mut stream).take(count).collect().await;
    let (prefix, error) = split_results(results);
    assert_eq!(prefix.len(), count, "the stored list has the prefix pages");
    world.latest = prefix;
    world.error = error;
    record_pages(world);
    world.paused = Some(stream);
}

#[given(expr = "a Connect refusal carries cursor code {string}")]
fn given_cursor_refusal(world: &mut PagingWorld, code: String) {
    world.cursor_refusal = Some(with_error_info(
        ConnectError::new(ErrorCode::InvalidArgument, "cursor refused"),
        &ErrorInfo {
            code,
            emitter: "widgets".into(),
        },
    ));
}

#[when("the consumer classifies the cursor refusal as a page error")]
fn when_classify_cursor(world: &mut PagingWorld) {
    drop(world.paused.take());
    world.error = Some(PageError::from_connect(
        world.cursor_refusal.take().expect("Connect refusal input"),
        world.pages.len().try_into().expect("delivered page count"),
        world.items.len().try_into().expect("delivered item count"),
    ));
}

#[given(expr = "the owner transfers the stored widget to {string}")]
async fn given_owner_transfer(world: &mut PagingWorld, owner: String) {
    let (items, _) = stored(world).await;
    let client = super::batch::client(&mut world.batch).await;
    let response = client
        .raw()
        .update_widget_parts_with_options(
            UpdateWidgetPartsRequest {
                widget_id: PARENT.into(),
                items,
                owner: Some(owner.clone()),
                ..Default::default()
            },
            super::batch::raw_options(&world.batch),
        )
        .await
        .expect("authorized owner transfer")
        .into_owned();
    let (stored_items, revision) = stored(world).await;
    assert_eq!(response.items, stored_items);
    assert_eq!(
        response
            .page
            .as_option()
            .expect("updated revision")
            .revision,
        revision
    );
    assert_eq!(
        db(world).owner(&organization(world), PARENT).await,
        Some(owner)
    );
}

#[when("the consumer resumes the stored list")]
async fn when_resume(world: &mut PagingWorld) {
    let mut stream = world
        .paused
        .take()
        .expect("delivered prefix and live iterator");
    world.error = stream.next().await.expect("continued result").err();
    world.ended = stream.next().await.is_none();
}

#[when(expr = "the consumer changes identity to {string} and resumes the cached replay")]
async fn when_replay_identity(world: &mut PagingWorld, identity: String) {
    given_identity(world, identity).await;
    let mut pages = world.paused.take().expect("paused real replay");
    world.error = pages.next().await.expect("guard result").err();
    assert!(pages.next().await.is_none());
}

#[when("the consumer clears the revision cache and resumes the cached replay")]
async fn when_replay_clear(world: &mut PagingWorld) {
    world.cache.clear();
    let mut pages = world.paused.take().expect("paused real replay");
    world.error = pages.next().await.expect("guard result").err();
    assert!(pages.next().await.is_none());
}

async fn collect_after_mutation(
    world: &mut PagingWorld,
    mut stream: std::pin::Pin<
        Box<
            impl futures_util::Stream<Item = Result<Page<String>, api_bones_page::PageError>>
            + Send
            + 'static,
        >,
    >,
    page: usize,
    identity: Option<String>,
) {
    let database = db(world);
    let mut prefix = Vec::new();
    for _ in 1..page {
        prefix.push(
            stream
                .next()
                .await
                .expect("prefix page")
                .expect("real prefix"),
        );
    }
    let pending = tokio::spawn(async move {
        let result = stream.next().await;
        (stream, result)
    });
    database.wait_for_blocked_read().await;
    if let Some(identity) = identity {
        given_identity(world, identity).await;
    } else {
        world.cache.clear();
    }
    database.unlock_reads().await;
    let (mut stream, result) = pending.await.expect("page consumer task");
    let error = result
        .expect("guard result")
        .expect_err("changed identity/cache discards real response");
    assert!(stream.next().await.is_none());
    world.latest = prefix;
    world.error = Some(error);
    record_pages(world);
}

async fn mutate_in_flight(
    world: &mut PagingWorld,
    identity: Option<String>,
    page: usize,
    prefetch: u8,
) {
    assert!((page == 1 && prefetch == 0) || (page == 2 && prefetch == 1));
    let stream = Box::pin(api_bones_page::ListCall::pages(
        generated_call(world).await.options(PageOptions {
            prefetch,
            ..PageOptions::default()
        }),
    ));
    collect_after_mutation(world, stream, page, identity).await;
}

#[when(
    expr = "the consumer changes identity to {string} while page {int} is in flight with prefetch {int}"
)]
async fn when_identity_in_flight(
    world: &mut PagingWorld,
    identity: String,
    page: usize,
    prefetch: u8,
) {
    mutate_in_flight(world, Some(identity), page, prefetch).await;
}

#[when(
    expr = "the consumer clears the revision cache while page {int} is in flight with prefetch {int}"
)]
async fn when_clear_in_flight(world: &mut PagingWorld, page: usize, prefetch: u8) {
    mutate_in_flight(world, None, page, prefetch).await;
}

#[then(expr = "the consumer receives items {string}")]
fn then_items(world: &mut PagingWorld, expected: String) {
    assert_eq!(world.items, parts(&expected));
}

#[then(expr = "the consumer receives pages {string}")]
fn then_pages(world: &mut PagingWorld, expected: String) {
    assert_eq!(
        world.pages,
        expected.split('|').map(parts).collect::<Vec<_>>()
    );
}

#[then(expr = "the stored widget parts are {string}")]
async fn then_stored(world: &mut PagingWorld, expected: String) {
    let (items, _) = stored(world).await;
    assert_eq!(items, parts(&expected));
    assert_eq!(
        world
            .latest
            .iter()
            .flat_map(|page| page.items.iter().cloned())
            .collect::<Vec<_>>(),
        items
    );
}

#[then(expr = "the stored widget owner is {string}")]
async fn then_owner(world: &mut PagingWorld, expected: String) {
    assert_eq!(
        db(world).owner(&organization(world), PARENT).await,
        Some(expected)
    );
}

#[then(expr = "the server saw {int} request(s) for the cursor {string}")]
fn then_cursor_count(world: &mut PagingWorld, expected: usize, cursor: String) {
    assert_eq!(
        seen(world)
            .iter()
            .filter(|request| request.cursor == cursor)
            .count(),
        expected
    );
}

#[then(expr = "the SDK made {int} failed page attempts")]
fn then_failed_attempts(world: &mut PagingWorld, expected: usize) {
    let attempts = world
        .failed_attempts
        .lock()
        .expect("actual failed SDK attempts");
    assert_eq!(attempts.len(), expected);
    assert!(attempts.iter().all(|code| *code == ErrorCode::Unavailable));
}

#[then(expr = "every page carries the revision {string}")]
#[then(expr = "every generated list page carries revision {string}")]
async fn then_revision(world: &mut PagingWorld, label: String) {
    let (_, revision) = stored(world).await;
    assert_eq!(world.remembered_revisions.get(&label), Some(&revision));
    assert!(!world.latest.is_empty());
    assert!(world.latest.iter().all(|page| page.revision == revision));
}

#[then(expr = "the delivered pages report whether another page follows as {string}")]
fn then_has_next(world: &mut PagingWorld, expected: String) {
    let expected: Vec<bool> = expected.split(',').map(|flag| flag == "true").collect();
    let actual: Vec<bool> = world.latest.iter().map(|page| page.has_next).collect();
    assert_eq!(actual, expected);
}

#[then(expr = "every generated list page is marked from cache {word}")]
async fn then_cached(world: &mut PagingWorld, expected: String) {
    assert!(!world.latest.is_empty());
    assert!(
        world
            .latest
            .iter()
            .all(|page| page.from_cache == (expected == "true"))
    );
    let (items, _) = stored(world).await;
    assert_eq!(
        world
            .latest
            .iter()
            .flat_map(|page| page.items.iter().cloned())
            .collect::<Vec<_>>(),
        items
    );
}

#[then(expr = "the first revalidation carried if-none-match {string}")]
fn then_revalidation(world: &mut PagingWorld, expected: String) {
    let expected: String = serde_json::from_str(&format!("\"{expected}\""))
        .expect("Cucumber string with escaped revision quotes");
    let expected = if expected.is_empty() {
        String::new()
    } else {
        let label = expected.trim_matches('"');
        format!(
            "\"{}\"",
            world
                .remembered_revisions
                .get(label)
                .expect("captured database revision")
        )
    };
    let requests = seen(world);
    assert_eq!(requests[world.revalidation_start].if_none_match, expected);
    assert!(
        requests[world.revalidation_start..]
            .iter()
            .all(|request| request.list_cache == "1")
    );
    assert!(
        requests[world.revalidation_start + 1..]
            .iter()
            .all(|request| request.if_none_match.is_empty())
    );
}

#[then("the batch continuation sent no conditional revision")]
fn then_batch_headers(world: &mut PagingWorld) {
    let requests = seen(world);
    let continuation = &requests[world.revalidation_start..];
    assert!(!continuation.is_empty());
    assert!(
        continuation
            .iter()
            .all(|request| request.if_none_match.is_empty())
    );
    assert_eq!(continuation[0].cursor, world.seed_cursor);
}

#[then(expr = "the server saw the cursors {string}")]
fn then_cursors(world: &mut PagingWorld, expected: String) {
    assert_eq!(
        seen(world)
            .iter()
            .map(|request| request.cursor.as_str())
            .collect::<Vec<_>>()
            .join(","),
        expected
    );
}

#[then(expr = "the server saw the page size {int} on every request")]
async fn then_page_size(world: &mut PagingWorld, expected: u32) {
    let delivered = world.items.clone();
    let requests = seen(world);
    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .all(|request| request.page_size == Some(expected))
    );
    let (items, _) = stored(world).await;
    assert_eq!(delivered, items);
}

#[then("the server saw no page size on every request")]
async fn then_no_size(world: &mut PagingWorld) {
    let delivered = world.items.clone();
    let requests = seen(world);
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|request| request.page_size.is_none()));
    let (items, _) = stored(world).await;
    assert_eq!(delivered, items);
    assert!(world.error.is_none());
}

#[then(expr = "the server saw {int} request(s)")]
fn then_count(world: &mut PagingWorld, expected: usize) {
    assert_eq!(seen(world).len(), expected);
}

#[then(expr = "the server saw at most {int} requests")]
fn then_at_most(world: &mut PagingWorld, expected: usize) {
    assert!(!world.latest.is_empty());
    assert!(seen(world).len() <= expected);
}

async fn wait_for_aborted_fetch(world: &PagingWorld) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if world
                .batch
                .data
                .lock()
                .expect("in-flight requests")
                .in_flight
                == 0
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled transport drops blocked server request");
}

#[then("the fetch for page 2 was aborted")]
async fn then_aborted(world: &mut PagingWorld) {
    assert_eq!(
        world.latest.first().expect("delivered page").items.to_vec(),
        Vec::from(["1".to_owned()])
    );
    wait_for_aborted_fetch(world).await;
    db(world).unlock_reads().await;
}

#[then("the stream ended without an error")]
fn then_success(world: &mut PagingWorld) {
    assert!(world.error.is_none());
    assert!(world.ended);
}

#[then("the revision cache is empty")]
fn then_cache_empty(world: &mut PagingWorld) {
    assert!(world.cache.is_empty());
}

#[then("the paused replay ends with an unauthenticated error")]
fn then_unauthenticated(world: &mut PagingWorld) {
    assert!(
        matches!(&world.error.as_ref().expect("guard failure").kind, PageErrorKind::Rpc(error) if error.code == ErrorCode::Unauthenticated)
    );
}

#[then(expr = "the stream ended with an error after {int} pages and {int} items")]
#[then(expr = "the classified refusal preserves delivery context of {int} pages and {int} items")]
fn then_error_counts(world: &mut PagingWorld, pages: u32, items: u64) {
    let error = world.error.as_ref().expect("iteration failed");
    assert_eq!(error.delivered_pages, pages);
    assert_eq!(error.delivered_items, items);
    assert_eq!(
        world.items.len(),
        usize::try_from(items).expect("item count")
    );
    assert!(
        world.pages.is_empty() || world.pages.len() == usize::try_from(pages).expect("page count")
    );
}

#[then(expr = "the error is an RPC failure with code {string}")]
fn then_error_code(world: &mut PagingWorld, code: String) {
    assert!(
        matches!(&world.error.as_ref().expect("iteration failed").kind, PageErrorKind::Rpc(error) if error.code.as_str() == code)
    );
}

#[then("the error is not a rejected cursor")]
fn then_not_cursor(world: &mut PagingWorld) {
    assert!(
        !world
            .error
            .as_ref()
            .expect("iteration failed")
            .is_cursor_rejected()
    );
}

#[then(expr = "the cursor refusal is classified as kind {string}")]
fn then_cursor_kind(world: &mut PagingWorld, expected: String) {
    let error = world.error.as_ref().expect("iteration failed");
    assert!(error.is_cursor_rejected());
    let kind = match &error.kind {
        PageErrorKind::CursorInvalid => "cursor_invalid",
        PageErrorKind::CursorExpired => "cursor_expired",
        PageErrorKind::CursorQueryMismatch => "cursor_query_mismatch",
        _ => panic!("the actual SDK error is not a cursor refusal"),
    };
    assert_eq!(kind, expected);
}

#[then(expr = "the list completion records {int} pages and {int} not-modified hits")]
fn then_completion(_world: &mut PagingWorld, pages: u32, hits: u32) {
    let telemetry = LIST_TELEMETRY.lock().expect("telemetry");
    assert_eq!(telemetry.spans.len(), 2);
    assert_eq!(telemetry.events.len(), 2);
    for fields in [
        telemetry.events.last().expect("completion"),
        telemetry.spans.last().expect("span"),
    ] {
        assert_eq!(fields["rpc.method"], METHOD);
        assert_eq!(fields["bones.list.pages"], pages.to_string());
        assert_eq!(fields["bones.list.not_modified_hits"], hits.to_string());
        assert_eq!(fields["bones.list.cancelled"], "false");
        assert!(
            fields["bones.list.time_to_first_item_ms"]
                .parse::<u64>()
                .is_ok()
        );
        assert!(
            fields["bones.list.time_to_full_list_ms"]
                .parse::<u64>()
                .is_ok()
        );
    }
}

#[then(expr = "the list completion records cancelled {word} without full-list time")]
fn then_incomplete(_world: &mut PagingWorld, cancelled: String) {
    let telemetry = LIST_TELEMETRY.lock().expect("telemetry");
    assert_eq!(telemetry.spans.len(), 1);
    assert_eq!(telemetry.events.len(), 1);
    for fields in [&telemetry.spans[0], &telemetry.events[0]] {
        assert_eq!(fields["rpc.method"], METHOD);
        assert_eq!(fields["bones.list.cancelled"], cancelled);
        assert!(!fields.contains_key("bones.list.time_to_full_list_ms"));
        assert!(
            fields["bones.list.time_to_first_item_ms"]
                .parse::<u64>()
                .is_ok()
        );
    }
}
