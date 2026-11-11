// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_batch.feature` and
//! `../../../features/connect_batch_identity_guard.feature`.
//!
//! The steps serve the contract's `Widgets` service from multi-organization
//! data with a real connectrpc server on loopback and call it only through the
//! generated SDK and batch runtime's `Batched` client. The server reads the
//! caller's organization from the bearer token each request arrives with and
//! records every request, so a step can read what the runtime sent: which RPC,
//! which ids, under which credential and in what order.

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use crate::world::{
    BatchWorld, FailRule, Gate, GroupRead, WidgetPartsCall, WidgetRequest, WidgetsData,
    WidgetsServed, token_credential,
};
use api_bones_batch::{BatchRecord, BatchResults, Batched, CallHeaders, Credential, Observer};
use api_bones_connect::{ErrorInfo, error_info, with_error_info};
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, BatchListWidgetPartsRequest,
    BatchListWidgetPartsResponse, GetWidgetRequest, ItemError, ListWidgetPartsRequest,
    ListWidgetPartsResponse, PageResponse, UpdateWidgetPartsRequest, ValidationFailure, Widget,
    WidgetGets, WidgetPartLists, WidgetPartsGroup, WidgetResult, Widgets, WidgetsClient,
    WidgetsExt, WireErrorInfo, widget_result,
};
use api_bones_transport::HttpTransport;
use connectrpc::client::{CallOptions, ClientConfig};
use connectrpc::{
    ConnectError, ErrorCode, RequestContext, Response, Router, Server, ServiceRequest,
    ServiceResult,
};
use cucumber::{given, then, when};
use futures_util::StreamExt;
use futures_util::future::join_all;

// Capture tracing itself, independently of the optional runtime observer.
type TelemetryFields = std::collections::BTreeMap<String, serde_json::Value>;
#[derive(Default)]
struct BatchTelemetry {
    open: std::collections::HashMap<u64, TelemetryFields>,
    spans: Vec<TelemetryFields>,
    completions: Vec<TelemetryFields>,
}
static TELEMETRY: std::sync::LazyLock<Mutex<BatchTelemetry>> =
    std::sync::LazyLock::new(|| Mutex::new(BatchTelemetry::default()));
struct Fields(TelemetryFields);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().into(), format!("{value:?}").into());
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().into(), value.into());
    }
}
struct BatchTelemetryLayer;
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for BatchTelemetryLayer {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() == "bones.batch" {
            let mut fields = Fields(TelemetryFields::new());
            attrs.record(&mut fields);
            lock(&TELEMETRY).open.insert(id.into_u64(), fields.0);
        }
    }
    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = Fields(TelemetryFields::new());
        values.record(&mut fields);
        if let Some(span) = lock(&TELEMETRY).open.get_mut(&id.into_u64()) {
            span.extend(fields.0);
        }
    }
    fn on_close(&self, id: tracing::span::Id, _: tracing_subscriber::layer::Context<'_, S>) {
        let mut data = lock(&TELEMETRY);
        if let Some(span) = data.open.remove(&id.into_u64()) {
            data.spans.push(span);
        }
    }
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() == "bones.batch" {
            let mut fields = Fields(TelemetryFields::new());
            event.record(&mut fields);
            lock(&TELEMETRY).completions.push(fields.0);
        }
    }
}
pub(super) fn reset_telemetry() {
    use tracing_subscriber::prelude::*;
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry()
                .with(BatchTelemetryLayer)
                .with(super::pagination::ListLayer),
        )
        .expect("batch tracing subscriber")
    });
    *lock(&TELEMETRY) = BatchTelemetry::default();
}

const EMITTER: &str = "widgets";
const MIB: usize = 1_048_576;

type WidgetsBatched = Batched<WidgetsClient<HttpTransport>>;
type Outcome = Result<Widget, ConnectError>;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn list_of(list: &str) -> Vec<String> {
    list.split(',')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn code_named(name: &str) -> ErrorCode {
    name.parse()
        .unwrap_or_else(|_| panic!("the contract names a code this step cannot build: {name}"))
}

// ---- the widgets server -----------------------------------------------------

/// Why one id is not served; the single Get and the batch answer it the same.
enum Refusal {
    NotFound,
    Malformed,
    NotOwner,
}

impl Refusal {
    fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound => ErrorCode::NotFound,
            Self::Malformed => ErrorCode::InvalidArgument,
            Self::NotOwner => ErrorCode::PermissionDenied,
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Self::NotFound => "widget not found",
            Self::Malformed => "widget id is malformed",
            Self::NotOwner => "widget access denied",
        }
    }

    fn info(&self) -> &'static str {
        match self {
            Self::NotFound => "WIDGET_NOT_FOUND",
            Self::Malformed => "WIDGET_ID_MALFORMED",
            Self::NotOwner => "WIDGET_ACCESS_DENIED",
        }
    }

    fn single(&self) -> ConnectError {
        with_error_info(
            ConnectError::new(self.code(), self.message()),
            &ErrorInfo {
                code: self.info().to_owned(),
                emitter: EMITTER.to_owned(),
            },
        )
    }

    fn item(&self) -> ItemError {
        let violations = match self {
            Self::NotFound | Self::NotOwner => Vec::new(),
            Self::Malformed => vec![ValidationFailure {
                field: "id".to_owned(),
                message: self.message().to_owned(),
                rule: "widget_id_format".to_owned(),
                ..Default::default()
            }],
        };
        ItemError {
            code: self.code().as_str().to_owned(),
            message: self.message().to_owned(),
            info: buffa::MessageField::some(WireErrorInfo {
                code: self.info().to_owned(),
                emitter: EMITTER.to_owned(),
                ..Default::default()
            }),
            violations,
            ..Default::default()
        }
    }
}

/// The widget `id` of `org`. An id of another organization is not found: the
/// answer is the one an unknown id gets.
async fn lookup(
    db: &api_bones_contract_rust::widget_fixture::BatchDb,
    org: &str,
    id: &str,
) -> Result<(String, Option<String>), Refusal> {
    validate_widget_id(id)?;
    db.widget(org, id).await.ok_or(Refusal::NotFound)
}

fn validate_widget_id(id: &str) -> Result<(), Refusal> {
    if id.is_empty() || id.chars().any(char::is_whitespace) {
        return Err(Refusal::Malformed);
    }
    Ok(())
}

fn authorize_widget(subject: &str, owner: Option<&str>) -> Result<(), ConnectError> {
    if owner.is_some_and(|owner| owner != subject) {
        Err(Refusal::NotOwner.single())
    } else {
        Ok(())
    }
}

fn item_error(error: &ConnectError) -> ItemError {
    let info = error_info(error).expect("fixture refusal has ErrorInfo");
    if info.code == Refusal::Malformed.info() {
        return Refusal::Malformed.item();
    }
    ItemError {
        code: error.code.as_str().to_owned(),
        message: error.message.clone().unwrap_or_default(),
        info: buffa::MessageField::some(WireErrorInfo {
            code: info.code,
            emitter: info.emitter,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn claim(ctx: &RequestContext) -> &quorum_workload_identity::VerifiedCanonicalClaim {
    ctx.extensions()
        .get::<quorum_workload_identity::VerifiedCanonicalClaim>()
        .expect("ingress verified the canonical claim")
}

/// Counts a request in flight until the handler ends or is dropped.
struct Flight(Arc<Mutex<WidgetsData>>);

impl Drop for Flight {
    fn drop(&mut self) {
        let mut data = lock(&self.0);
        data.in_flight -= 1;
        if let Some(gate) = &data.gate {
            gate.finished.add_permits(1);
        }
    }
}

#[derive(Clone)]
struct WidgetService {
    db: Arc<api_bones_contract_rust::widget_fixture::BatchDb>,
    source_failed: Arc<std::sync::atomic::AtomicBool>,
    data: Arc<Mutex<WidgetsData>>,
    credential: Arc<Mutex<Credential>>,
}

fn header(ctx: &RequestContext, name: &str) -> String {
    ctx.header(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

struct ProviderFault(ConnectError);
impl api_bones::connect::IntoDomainErrorKind for ProviderFault {
    fn kind(&self) -> api_bones::connect::DomainErrorKind {
        api_bones::connect::DomainErrorKind::Internal(self.0.to_string())
    }
}

impl WidgetService {
    /// Record the request and return the caller's organization.
    fn record(
        &self,
        ctx: &RequestContext,
        rpc: &'static str,
        ids: Vec<String>,
        cursor: &str,
        page_size: Option<u32>,
        encoded_len: usize,
    ) -> String {
        let org = claim(ctx)
            .org_id()
            .expect("signed organization")
            .as_str()
            .to_owned();
        lock(&self.data).seen.push(WidgetRequest {
            rpc,
            ids,
            cursor: cursor.to_owned(),
            page_size,
            trace: header(ctx, "x-trace"),
            lane: header(ctx, "x-token-lane"),
            timeout_ms: header(ctx, "connect-timeout-ms"),
            if_none_match: header(ctx, "if-none-match"),
            list_cache: header(ctx, "bones-list-cache"),
            encoded_len,
        });
        org
    }

    /// Count the request in flight and wait out the scripted delay.
    async fn occupy(&self) -> Flight {
        let (delay, gate) = {
            let mut data = lock(&self.data);
            data.in_flight += 1;
            data.max_in_flight = data.max_in_flight.max(data.in_flight);
            (data.delay, data.gate.clone())
        };
        let flight = Flight(self.data.clone());
        if let Some(gate) = gate {
            gate.arrived.add_permits(1);
            gate.release
                .acquire()
                .await
                .expect("the gate is never closed")
                .forget();
        } else if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        flight
    }

    /// The scripted failure of a batch request carrying `ids`, and the
    /// credential swap a batch request triggers.
    fn script_batch(&self, ids: &[String]) -> Result<(), ConnectError> {
        let fault = lock(&self.data).source_fault_on_batch.take();
        if let Some(fault) = fault {
            corrupt_credential(&mut lock(&self.credential), &fault);
        }
        if lock(&self.data).source_fails_on_batch {
            self.source_failed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let swap = lock(&self.data).swap_on_batch.take();
        if let Some(next) = swap {
            *lock(&self.credential) = next;
        }
        let mut data = lock(&self.data);
        for rule in &mut data.failures {
            let applies = rule
                .carrying
                .as_ref()
                .is_none_or(|id| ids.iter().any(|asked| asked == id));
            if applies && rule.times > 0 {
                rule.times -= 1;
                return Err(ConnectError::new(rule.code, "scripted failure"));
            }
        }
        Ok(())
    }

    /// One page of the parts of `parent`, and the cursor of the next.
    async fn parts_page(
        &self,
        org: &str,
        principal: &str,
        parent: &str,
        cursor: &str,
        page_size: u32,
    ) -> Result<(Vec<String>, String, String), Refusal> {
        validate_widget_id(parent)?;
        let (all, revision) = self
            .db
            .authorized_parts_snapshot(org, parent, |owner| authorize_widget(principal, owner))
            .await
            .map_err(|_| Refusal::NotOwner)?
            .ok_or(Refusal::NotFound)?;
        let start = cursor
            .strip_prefix('c')
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0)
            .min(all.len());
        let size = if page_size > 0 {
            page_size as usize
        } else {
            lock(&self.data).part_page_size
        };
        let end = (start + size).min(all.len());
        let next = if end < all.len() {
            format!("c{end}")
        } else {
            String::new()
        };
        Ok((all[start..end].to_vec(), next, revision))
    }
}

fn widget(id: &str, name: String, kind: &str) -> Widget {
    Widget {
        id: id.to_owned(),
        name,
        kind: kind.to_owned(),
        ..Default::default()
    }
}

impl WidgetService {
    async fn list_parts(
        &self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListWidgetPartsRequest>,
        rpc: &'static str,
    ) -> ServiceResult<ListWidgetPartsResponse> {
        let encoded_len = request.bytes().len();
        let asked = request.to_owned_message();
        let (cursor, size) = asked
            .page
            .as_option()
            .map(|page| (page.cursor.clone(), page.page_size))
            .unwrap_or_default();
        let org = self.record(
            &ctx,
            rpc,
            vec![asked.widget_id.clone()],
            &cursor,
            (size > 0).then_some(size),
            encoded_len,
        );
        let _flight = self.occupy().await;
        match self
            .parts_page(
                &org,
                claim(&ctx).principal(),
                &asked.widget_id,
                &cursor,
                size,
            )
            .await
        {
            Ok((items, next_cursor, revision)) => {
                if cursor.is_empty()
                    && header(&ctx, "bones-list-cache") == "1"
                    && header(&ctx, "if-none-match") == format!("\"{revision}\"")
                {
                    return Ok(Response::new(ListWidgetPartsResponse::default())
                        .with_header("bones-not-modified", "1"));
                }
                Response::ok(ListWidgetPartsResponse {
                    items,
                    page: buffa::MessageField::some(PageResponse {
                        next_cursor,
                        revision,
                        ..Default::default()
                    }),
                    ..Default::default()
                })
            }
            Err(refusal) => Err(refusal.single()),
        }
    }
}

impl Widgets for WidgetService {
    async fn get_widget<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, GetWidgetRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<Widget> + Send + use<'a>> {
        let asked = request.to_owned_message();
        let org = self.record(
            &ctx,
            "GetWidget",
            vec![asked.id.clone()],
            "",
            None,
            request.bytes().len(),
        );
        let _flight = self.occupy().await;
        let named = lookup(&self.db, &org, &asked.id).await;
        match named {
            Ok((name, owner)) => {
                authorize_widget(claim(&ctx).principal(), owner.as_deref())?;
                Response::ok(widget(&asked.id, name, &asked.kind))
            }
            Err(refusal) => Err(refusal.single()),
        }
    }

    async fn batch_get_widgets<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, BatchGetWidgetsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<BatchGetWidgetsResponse> + Send + use<'a>> {
        let asked = request.to_owned_message();
        let org = self.record(
            &ctx,
            "BatchGetWidgets",
            asked.ids.clone(),
            "",
            None,
            request.bytes().len(),
        );
        let _flight = self.occupy().await;
        self.script_batch(&asked.ids)?;
        let principal = claim(&ctx).principal();
        let authorized = BatchResults::serve(
            &asked.ids,
            |id| {
                let org = &org;
                let kind = &asked.kind;
                async move {
                    lookup(&self.db, org, id)
                        .await
                        .map(|(name, owner)| (widget(id, name, kind), owner))
                        .map_err(|refusal| refusal.single())
                }
            },
            |(item, owner)| {
                if let Some((id, message)) = &lock(&self.data).authorization_fault {
                    if item.id == *id {
                        return Err(ConnectError::internal(message.clone()));
                    }
                }
                authorize_widget(principal, owner.as_deref())
            },
        )
        .await?;
        let mut results: Vec<_> = asked
            .ids
            .iter()
            .zip(authorized.into_vec())
            .map(|(id, outcome)| WidgetResult {
                id: id.clone(),
                outcome: Some(match outcome {
                    Ok((item, _)) => widget_result::Outcome::Item(Box::new(item)),
                    Err(error) => widget_result::Outcome::Error(Box::new(item_error(&error))),
                }),
                ..Default::default()
            })
            .collect();
        if lock(&self.data).reverse_answers {
            results.reverse();
        }
        if lock(&self.data).truncate_answers {
            results.pop();
        }
        Response::ok(BatchGetWidgetsResponse {
            results,
            ..Default::default()
        })
    }

    async fn list_widget_parts<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        self.list_parts(ctx, request, "ListWidgetParts").await
    }

    async fn list_widget_parts_idempotent<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        self.list_parts(ctx, request, "ListWidgetPartsIdempotent")
            .await
    }

    async fn list_widget_parts_unspecified<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, ListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        self.list_parts(ctx, request, "ListWidgetPartsUnspecified")
            .await
    }

    async fn update_widget_parts<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, UpdateWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListWidgetPartsResponse> + Send + use<'a>> {
        if !claim(&ctx).effective().permits("widgets:write") {
            return Err(with_error_info(
                ConnectError::permission_denied("widget write authority required"),
                &ErrorInfo {
                    code: "WIDGET_WRITE_REQUIRED".into(),
                    emitter: EMITTER.into(),
                },
            ));
        }
        let asked = request.to_owned_message();
        let org = self.record(
            &ctx,
            "UpdateWidgetParts",
            vec![asked.widget_id.clone()],
            "",
            None,
            request.bytes().len(),
        );
        let _flight = self.occupy().await;
        validate_widget_id(&asked.widget_id).map_err(|refusal| refusal.single())?;
        let principal = claim(&ctx).principal();
        let (items, revision) = self
            .db
            .replace_parts(
                &org,
                &asked.widget_id,
                &asked.items,
                asked.owner.as_deref(),
                |owner| authorize_widget(principal, owner),
            )
            .await?
            .ok_or_else(|| Refusal::NotFound.single())?;
        Response::ok(ListWidgetPartsResponse {
            items,
            page: buffa::MessageField::some(PageResponse {
                revision,
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    async fn batch_list_widget_parts<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, BatchListWidgetPartsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<BatchListWidgetPartsResponse> + Send + use<'a>>
    {
        let asked = request.to_owned_message();
        let size = asked.page.as_option().map_or(0, |page| page.page_size);
        let org = self.record(
            &ctx,
            "BatchListWidgetParts",
            asked.widget_ids.clone(),
            "",
            (size > 0).then_some(size),
            request.bytes().len(),
        );
        let _flight = self.occupy().await;
        self.script_batch(&asked.widget_ids)?;
        let outcomes = BatchResults::serve(
            &asked.widget_ids,
            |parent| {
                let org = &org;
                let ctx = &ctx;
                async move {
                    self.parts_page(org, claim(ctx).principal(), parent, "", size)
                        .await
                        .map_err(|refusal| refusal.single())
                }
            },
            |_| Ok(()),
        )
        .await?;
        let mut groups: Vec<_> = asked
            .widget_ids
            .iter()
            .zip(outcomes.into_vec())
            .map(|(parent, outcome)| match outcome {
                Ok((items, next_cursor, revision)) => WidgetPartsGroup {
                    widget_id: parent.clone(),
                    items,
                    page: buffa::MessageField::some(PageResponse {
                        next_cursor,
                        revision,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Err(error) => WidgetPartsGroup {
                    widget_id: parent.clone(),
                    error: buffa::MessageField::some(item_error(&error)),
                    ..Default::default()
                },
            })
            .collect();
        if lock(&self.data).reverse_answers {
            groups.reverse();
        }
        if lock(&self.data).truncate_answers {
            groups.pop();
        }
        Response::ok(BatchListWidgetPartsResponse {
            groups,
            ..Default::default()
        })
    }
}

// ---- the client under test ----------------------------------------------------

/// Start the server and build the batch client, once per scenario. The client's
/// token source and observer are the world's, so a step that changes the
/// credential changes what the runtime reads.
pub(super) async fn client(world: &mut BatchWorld) -> WidgetsBatched {
    if let Some(client) = &world.client {
        return client.clone();
    }
    reset_telemetry();
    let bound = Server::bind("127.0.0.1:0")
        .await
        .expect("the widgets server binds");
    let addr = bound
        .local_addr()
        .expect("the widgets server has an address");
    let seed_data = {
        let data = lock(&world.data);
        WidgetsData {
            widgets: data.widgets.clone(),
            parts: data.parts.clone(),
            owners: data.owners.clone(),
            ..Default::default()
        }
    };
    let db = Arc::new(
        api_bones_contract_rust::widget_fixture::BatchDb::seeded(
            &seed_data.widgets,
            &seed_data.parts,
            &seed_data.owners,
        )
        .await,
    );
    let service = WidgetService {
        db: db.clone(),
        source_failed: world.source_failed.clone(),
        data: world.data.clone(),
        credential: world.credential.clone(),
    };
    let issuers = world.issuers.clone();
    let start: Arc<dyn Fn(connectrpc::BoundServer) -> tokio::task::JoinHandle<()> + Send + Sync> =
        Arc::new(move |bound| {
            let router = WidgetsExt::register(Arc::new(service.clone()), Router::new());
            let issuers = issuers.clone();
            let service = connectrpc::ConnectRpcService::new(router).with_interceptor(
                connectrpc::unary_interceptor(move |mut request, next| {
                    let issuers = issuers.clone();
                    Box::pin(async move {
                        let authorization = request
                            .ctx
                            .header("authorization")
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| value.strip_prefix("Bearer "))
                            .ok_or_else(|| {
                                ConnectError::unauthenticated("canonical bearer required")
                            })?;
                        let verified = {
                            let issuers = lock(&issuers);
                            issuers.iter().find_map(|(org, issuer)| {
                                quorum_workload_identity::verify_canonical_claim_biscuit(
                                    authorization,
                                    &issuer.public_key(),
                                )
                                .ok()
                                .filter(|claim| {
                                    claim.org_id().is_some_and(|signed| signed.as_str() == org)
                                        && claim.effective().permits("widgets:read")
                                })
                            })
                        }
                        .ok_or_else(|| ConnectError::unauthenticated("invalid canonical claim"))?;
                        request.ctx.extensions_mut().insert(verified);
                        next.run(request).await.map_err(|error| {
                            if !matches!(
                                error.code,
                                ErrorCode::NotFound
                                    | ErrorCode::PermissionDenied
                                    | ErrorCode::InvalidArgument
                                    | ErrorCode::FailedPrecondition
                            ) {
                                let code = error.code;
                                let mut redacted =
                                    api_bones::connect::domain_to_connect(&ProviderFault(error));
                                redacted.code = code;
                                redacted
                            } else {
                                error
                            }
                        })
                    })
                }),
            );
            tokio::spawn(async move {
                let _ = bound.serve_with_service(service).await;
            })
        });
    let task = Mutex::new(Some(start(bound)));
    world.served = Some(Arc::new(WidgetsServed {
        task,
        db,
        address: addr,
        start,
    }));

    let source = world.credential.clone();
    let source_failed = world.source_failed.clone();
    let records = world.records.clone();
    let after_first_chunk = world.after_first_chunk.clone();
    let switch = world.credential.clone();
    let observer: Observer = Arc::new(move |record: &BatchRecord| {
        lock(&records).push(record.clone());
        if record.chunk_index == 0 {
            if let Some(next) = lock(&after_first_chunk).take() {
                *lock(&switch) = next;
            }
        }
    });
    let config = ClientConfig::new(format!("http://{addr}").parse().expect("a loopback uri"));
    let batched = Batched::new(
        WidgetsClient::new(HttpTransport::h2c(), config),
        move || -> Result<Credential, ConnectError> {
            if source_failed.load(std::sync::atomic::Ordering::SeqCst) {
                Err(ConnectError::unauthenticated("session source unavailable"))
            } else {
                Ok(lock(&source).clone())
            }
        },
    )
    .expect("producer binds the credential-neutral client")
    .observed(observer);
    world.client = Some(batched.clone());
    batched
}

/// The headers an unbatched call sends: the caller's current credential.
pub(super) fn raw_options(world: &BatchWorld) -> CallOptions {
    let credential = lock(&world.credential).clone();
    credential_options(&credential)
}

pub(super) fn credential_options(credential: &Credential) -> CallOptions {
    credential
        .headers
        .iter()
        .fold(CallOptions::default(), |options, (name, value)| {
            options.with_header(
                http::header::HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                http::header::HeaderValue::from_bytes(value).expect("a header value"),
            )
        })
}

fn seen(world: &BatchWorld) -> Vec<WidgetRequest> {
    lock(&world.data).seen.clone()
}

fn seen_rpc(world: &BatchWorld, rpc: &str) -> Vec<WidgetRequest> {
    seen(world)
        .into_iter()
        .filter(|request| request.rpc == rpc)
        .collect()
}

async fn same_identity_new_token(world: &BatchWorld) -> Credential {
    let identity = lock(&world.credential).identity.clone();
    token_credential(&world.issuers, &identity.subject, &identity.org).await
}

/// One member of a concurrent set polled in one pass.
enum Member {
    Get(GetWidgetRequest, CallHeaders),
    Do(Box<dyn FnOnce() + Send>),
}

type Answered = Pin<Box<dyn Future<Output = Option<Outcome>> + Send>>;

/// Poll every member in order, once per round, until all are done: the gets
/// enqueue in one tick, and a `Do` runs between the enqueues around it.
async fn drive(client: &WidgetsBatched, members: Vec<Member>) -> Vec<Outcome> {
    let futures: Vec<Answered> =
        members
            .into_iter()
            .map(|member| -> Answered {
                let client = client.clone();
                match member {
                    Member::Get(request, headers) => Box::pin(async move {
                        Some(client.get_widget_with_options(request, headers).await)
                    }),
                    Member::Do(action) => Box::pin(async move {
                        action();
                        None
                    }),
                }
            })
            .collect();
    join_all(futures).await.into_iter().flatten().collect()
}

/// The first member as the lead call's inputs, and the members that follow.
fn split_lead(mut members: Vec<Member>) -> (GetWidgetRequest, CallHeaders, Vec<Member>) {
    match members.remove(0) {
        Member::Get(request, headers) => (request, headers, members),
        Member::Do(_) => panic!("the first member is a get"),
    }
}

async fn concurrent_members(
    lead: impl Future<Output = Outcome>,
    batched: &WidgetsBatched,
    others: Vec<Member>,
) -> Vec<Outcome> {
    collect_concurrent(lead, drive(batched, others)).await
}

async fn collect_concurrent(
    lead: impl Future<Output = Outcome>,
    rest: impl Future<Output = Vec<Outcome>>,
) -> Vec<Outcome> {
    let (head, tail) = futures_util::future::join(lead, rest).await;
    std::iter::once(head).chain(tail).collect()
}

fn gets(ids: &str) -> Vec<Member> {
    list_of(ids)
        .into_iter()
        .map(|id| {
            Member::Get(
                GetWidgetRequest {
                    id,
                    ..Default::default()
                },
                CallHeaders::new(),
            )
        })
        .collect()
}

// ---- Given ----------------------------------------------------------------------

fn seed(world: &BatchWorld, org: &str, id: &str) {
    lock(&world.data).widgets.insert(
        (org.to_owned(), id.to_owned()),
        format!("Widget {}", id.get(..8).unwrap_or(id)),
    );
}

#[given(expr = "the organization {string} has widgets {string}")]
fn given_widgets(world: &mut BatchWorld, org: String, ids: String) {
    for id in list_of(&ids) {
        seed(world, &org, &id);
    }
}

#[given(expr = "the organization {string} has {int} widgets")]
fn given_generated(world: &mut BatchWorld, org: String, count: usize) {
    world.generated = (0..count).map(|n| format!("w-{n:03}")).collect();
    for id in world.generated.clone() {
        seed(world, &org, &id);
    }
}

#[given(expr = "the organization {string} has {int} widgets with ids of {int} bytes")]
fn given_generated_long(world: &mut BatchWorld, org: String, count: usize, bytes: usize) {
    world.generated = (0..count)
        .map(|n| format!("w-{n:03}-{}", "x".repeat(bytes.saturating_sub(6))))
        .collect();
    for id in world.generated.clone() {
        seed(world, &org, &id);
    }
}

#[given(expr = "the organization {string} has the widget {string} with parts {string}")]
fn given_parts(world: &mut BatchWorld, org: String, id: String, parts: String) {
    seed(world, &org, &id);
    lock(&world.data).parts.insert((org, id), list_of(&parts));
}

#[given(expr = "the widget {string} of organization {string} is owned by {string}")]
fn given_owner(world: &mut BatchWorld, id: String, org: String, owner: String) {
    assert!(
        lock(&world.data)
            .widgets
            .contains_key(&(org.clone(), id.clone()))
    );
    lock(&world.data).owners.insert((org, id), owner);
}

#[given("the caller supplies revision cache headers with ordinary metadata")]
fn given_continuation_headers(world: &mut BatchWorld) {
    world.batch_headers = CallHeaders::new()
        .header("if-none-match", b"cached-revision".to_vec())
        .header("bones-list-cache", b"1".to_vec())
        .header("x-trace", b"preserved".to_vec());
}

fn corrupt_credential(credential: &mut Credential, fault: &str) {
    let (remove, name) = match fault {
        "missing bearer" => (true, "authorization"),
        "missing relay" => (true, "brefwiz-relayed-credential"),
        "malformed bearer" => (false, "authorization"),
        "malformed relay" => (false, "brefwiz-relayed-credential"),
        other => panic!("unknown credential source fault: {other}"),
    };
    if remove {
        credential
            .headers
            .retain(|(header, _)| !header.eq_ignore_ascii_case(name));
    } else {
        let (_, bytes) = credential
            .headers
            .iter_mut()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .expect("the source initially supplied the credential");
        bytes.push(b'\n');
    }
}

#[given(expr = "the source returns {string} while the server handles a batch request")]
fn given_source_fault_on_batch(world: &mut BatchWorld, fault: String) {
    lock(&world.data).source_fault_on_batch = Some(fault);
}

#[given("the bound session supplies a relayed credential")]
async fn given_source_relay(world: &mut BatchWorld) {
    let issued = same_identity_new_token(world).await;
    let bytes = issued
        .headers
        .iter()
        .find(|(name, _)| name == "authorization")
        .and_then(|(_, bytes)| bytes.strip_prefix(b"Bearer "))
        .expect("the producer issued a signed credential")
        .to_vec();
    let mut source = lock(&world.credential);
    source
        .headers
        .retain(|(name, _)| name != "brefwiz-relayed-credential");
    source
        .headers
        .push(("brefwiz-relayed-credential".into(), bytes));
}

#[given(expr = "the part lists page {int} at a time")]
fn given_part_page(world: &mut BatchWorld, size: usize) {
    lock(&world.data).part_page_size = size;
}

#[given(expr = "the caller is {string} of organization {string}")]
async fn given_caller(world: &mut BatchWorld, subject: String, org: String) {
    let issuers = world.issuers.clone();
    let slot = world.credential.clone();
    let credential = token_credential(&issuers, &subject, &org).await;
    *lock(&slot) = credential;
}

#[given(expr = "the server takes {int} ms to answer")]
fn given_delay(world: &mut BatchWorld, millis: u64) {
    lock(&world.data).delay = Duration::from_millis(millis);
}

#[given(expr = "the server fails the batch request carrying {string} with {string}")]
fn given_fails_carrying(world: &mut BatchWorld, id: String, code: String) {
    lock(&world.data).failures.push(FailRule {
        carrying: Some(id),
        code: code_named(&code),
        times: u32::MAX,
    });
}

#[given(expr = "the server fails the first batch request with {string}")]
fn given_fails_first(world: &mut BatchWorld, code: String) {
    lock(&world.data).failures.push(FailRule {
        carrying: None,
        code: code_named(&code),
        times: 1,
    });
}

#[given("the server answers each batch in reverse order")]
fn given_reverse(world: &mut BatchWorld) {
    lock(&world.data).reverse_answers = true;
}

#[given(
    expr = "the server switches the caller to subject {string} of organization {string} while it handles a batch request"
)]
async fn given_swap_on_batch(world: &mut BatchWorld, subject: String, org: String) {
    let next = token_credential(&world.issuers, &subject, &org).await;
    lock(&world.data).swap_on_batch = Some(next);
}

#[given(
    expr = "the token refresh moves to subject {string} of organization {string} once the first chunk is delivered"
)]
async fn given_refresh_after_first(world: &mut BatchWorld, subject: String, org: String) {
    let next = token_credential(&world.issuers, &subject, &org).await;
    *lock(&world.after_first_chunk) = Some(next);
}

#[given(expr = "the token refresh changes the {word} once the first chunk is delivered")]
async fn given_refresh_component(world: &mut BatchWorld, what: String) {
    let current = lock(&world.credential).identity.clone();
    let next = match what.as_str() {
        "subject" => token_credential(&world.issuers, "someone-else", &current.org).await,
        "organization" => token_credential(&world.issuers, &current.subject, "another-org").await,
        other => panic!("the contract names an identity part this step cannot change: {other}"),
    };
    *lock(&world.after_first_chunk) = Some(next);
}

// ---- When: explicit batches -------------------------------------------------------

#[when(expr = "the caller batch-gets {string}")]
async fn when_batch_get(world: &mut BatchWorld, ids: String) {
    let batched = client(world).await;
    world.results = batched
        .batch_get_widgets(BatchGetWidgetsRequest {
            ids: list_of(&ids),
            ..Default::default()
        })
        .await
        .into_vec();
}

#[when("the caller batch-gets the generated widgets")]
async fn when_batch_get_generated(world: &mut BatchWorld) {
    let batched = client(world).await;
    world.results = batched
        .batch_get_widgets(BatchGetWidgetsRequest {
            ids: world.generated.clone(),
            ..Default::default()
        })
        .await
        .into_vec();
}

#[when(expr = "the caller batch-gets {string} and one id of {int} bytes")]
async fn when_batch_get_large(world: &mut BatchWorld, ids: String, bytes: usize) {
    let mut all = list_of(&ids);
    all.push("l".repeat(bytes));
    let batched = client(world).await;
    world.results = batched
        .batch_get_widgets(BatchGetWidgetsRequest {
            ids: all,
            ..Default::default()
        })
        .await
        .into_vec();
}

#[when(expr = "the caller gets {string} with the unbatched client")]
async fn when_unbatched_get(world: &mut BatchWorld, id: String) {
    let source = world.credential.clone();
    let credential = lock(&source).clone();
    let options = credential_options(&credential);
    let batched = world.client.as_ref().expect("the batch client was built");
    let single = batched
        .raw()
        .get_widget_with_options(
            GetWidgetRequest {
                id,
                ..Default::default()
            },
            options,
        )
        .await
        .map(connectrpc::client::UnaryResponse::into_owned);
    world.unbatched_widget = Some(single);
}

#[given(expr = "the batch response has mismatched {string}")]
fn given_malformed_batch(world: &mut BatchWorld, malformed: String) {
    let mut data = lock(&world.data);
    data.reverse_answers = malformed == "order";
    data.truncate_answers = malformed == "count";
    assert!(data.reverse_answers || data.truncate_answers);
}

#[when(expr = "the caller receives a malformed batch {string} response")]
async fn when_malformed_batches(world: &mut BatchWorld, operation: String) {
    let batched = client(world).await;
    let errors = match operation.as_str() {
        "list" => batched
            .batch_list_widget_parts(BatchListWidgetPartsRequest {
                widget_ids: list_of("w1,w2,w3"),
                ..Default::default()
            })
            .await
            .groups()
            .into_iter()
            .map(|group| match group.outcome {
                Ok(_) => panic!("malformed group was accepted"),
                Err(error) => error.into_connect_error(),
            })
            .collect(),
        "get" => batched
            .batch_get_widgets(BatchGetWidgetsRequest {
                ids: list_of("w1,w2,w3"),
                ..Default::default()
            })
            .await
            .into_vec()
            .into_iter()
            .map(|outcome| outcome.expect_err("malformed item was accepted"))
            .collect(),
        other => panic!("unknown batch operation: {other}"),
    };
    world.malformed_batches = vec![errors];
}

#[when("the SDK constructs a batched client with credentials outside its session binding")]
fn when_unbound_client(world: &mut BatchWorld) {
    let credential = lock(&world.credential).clone();
    let authorization = credential
        .headers
        .iter()
        .find(|(name, _)| name == "authorization")
        .map(|(_, value)| String::from_utf8(value.clone()).expect("bearer header"))
        .expect("the caller has a credential");
    let client = WidgetsClient::new(
        HttpTransport::h2c(),
        ClientConfig::new("http://127.0.0.1:1".parse().expect("loopback URI"))
            .with_default_header("authorization", authorization),
    );
    let built = Batched::new(client, move || Ok(credential.clone()));
    world.construction_refusal = built.err();
}

#[given(expr = "the caller has started the parts of {string}")]
async fn given_parts_continuation(world: &mut BatchWorld, parent: String) {
    let batched = client(world).await;
    let group = batched
        .batch_list_widget_parts(BatchListWidgetPartsRequest {
            widget_ids: vec![parent.clone()],
            ..Default::default()
        })
        .await
        .groups()
        .pop()
        .expect("one group");
    world.groups = vec![GroupRead {
        parent_id: group.parent_id.clone(),
        items: Vec::new(),
        error: None,
    }];
    world.continuation = Some(group.outcome.expect("seeded group"));
}

#[when("the caller continues the seeded parts group")]
async fn when_identity_before_continuation(world: &mut BatchWorld) {
    let seeded_list = world.continuation.take().expect("seeded group");
    let mut items = Box::pin(seeded_list.items());
    let prefix = items
        .next()
        .await
        .expect("seeded item")
        .expect("first page succeeded");
    let failure = items
        .next()
        .await
        .expect("terminal failure")
        .expect_err("changed identity refused");
    assert_eq!(failure.delivered_items, 1);
    let api_bones_page::PageErrorKind::Rpc(error) = failure.kind else {
        panic!("continuation must return identity failure");
    };
    assert!(items.next().await.is_none());
    let parent_id = world.groups[0].parent_id.clone();
    world.groups = vec![GroupRead {
        parent_id,
        items: vec![prefix],
        error: Some(error),
    }];
}

#[given(expr = "authorizing widget {string} fails internally with {string}")]
fn given_authorization_fault(world: &mut BatchWorld, id: String, message: String) {
    lock(&world.data).authorization_fault = Some((id, message));
}

#[when(expr = "the provider is asked for batch results with {int} ids")]
async fn when_provider_over_cap(world: &mut BatchWorld, count: usize) {
    let ids: Vec<String> = (0..count).map(|id| id.to_string()).collect();
    let mut fetched = 0;
    let mut authorized = 0;
    world.whole_batch_error = Some(
        BatchResults::serve(
            &ids,
            |_| {
                fetched += 1;
                async { Ok(()) }
            },
            |_: &()| {
                authorized += 1;
                Ok(())
            },
        )
        .await
        .expect_err("whole provider call must fail"),
    );
    world.provider_work = [fetched, authorized];
}

#[then(expr = "the provider performs no per-item work")]
fn then_provider_did_no_work(world: &mut BatchWorld) {
    assert_eq!(world.provider_work, [0, 0]);
}

#[when(expr = "the caller requests a batch for {string} while authorization fails internally")]
async fn when_provider_request(world: &mut BatchWorld, ids: String) {
    let batched = client(world).await;
    world.generated = list_of(&ids);
    world.results = batched
        .batch_get_widgets(BatchGetWidgetsRequest {
            ids: world.generated.clone(),
            ..Default::default()
        })
        .await
        .into_vec();
}

#[then(expr = "the whole batch call fails with {string}")]
fn then_whole_batch_code(world: &mut BatchWorld, code: String) {
    assert_eq!(
        world
            .whole_batch_error
            .as_ref()
            .expect("whole-call error")
            .code,
        code_named(&code)
    );
}

#[then(expr = "the whole batch call fails with {string} and ErrorInfo {string}")]
fn then_whole_batch_info(world: &mut BatchWorld, code: String, info: String) {
    then_whole_batch_code(world, code);
    assert_eq!(
        info_code(world.whole_batch_error.as_ref().expect("whole-call error")),
        Some(info)
    );
}

#[then("every item reports the same redacted internal error")]
fn then_batch_internal(world: &mut BatchWorld) {
    assert_eq!(world.results.len(), world.generated.len());
    let first = world
        .results
        .first()
        .expect("requested items")
        .as_ref()
        .expect_err("whole-call failure");
    let expected = serde_json::to_value(first).expect("error representation");
    for outcome in &world.results {
        let error = outcome
            .as_ref()
            .expect_err("every item carries the whole-call failure");
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.message.as_deref(), Some("internal error"));
        assert_eq!(
            serde_json::to_value(error).expect("error representation"),
            expected
        );
    }
}

#[then(expr = "no item error contains {string} or the requested ids")]
fn then_batch_redacted(world: &mut BatchWorld, private: String) {
    for outcome in &world.results {
        let error = outcome.as_ref().expect_err("whole-call failure");
        let exposed = format!("{error:?}");
        assert!(!exposed.contains(&private));
        for id in &world.generated {
            assert!(!exposed.contains(id), "item id leaked in error");
        }
    }
}

async fn prepare_call(
    world: &mut BatchWorld,
    call: impl Future<Output = Vec<Outcome>> + Send + 'static,
) {
    let mut call = Box::pin(call);
    let initial = std::future::poll_fn(|cx| Poll::Ready(call.as_mut().poll(cx))).await;
    let task = match initial {
        Poll::Ready(results) => tokio::spawn(async move { results }),
        Poll::Pending => tokio::spawn(call),
    };
    world.pending_batches.push(task);
}

fn hold_prepared_calls(world: &BatchWorld) {
    lock(&world.data).gate = Some(Arc::new(Gate::default()));
}

/// Every prepared call's results, once each has finished.
async fn collect_prepared(world: &mut BatchWorld) -> Vec<Outcome> {
    join_all(std::mem::take(&mut world.pending_batches))
        .await
        .into_iter()
        .flat_map(|task| task.expect("prepared SDK call completed"))
        .collect()
}

fn release_prepared_calls(world: &BatchWorld) {
    lock(&world.data)
        .gate
        .as_ref()
        .expect("prepared wire barrier")
        .open();
}

#[given(expr = "{int} explicit batches and {int} coalesced windows of two gets are waiting")]
async fn given_mixed_in_flight(world: &mut BatchWorld, explicit: usize, windows: usize) {
    let batched = client(world).await;
    hold_prepared_calls(world);
    for _ in 0..explicit {
        let client = batched.clone();
        prepare_call(world, async move {
            client
                .batch_get_widgets(BatchGetWidgetsRequest {
                    ids: list_of("w1,w2"),
                    ..Default::default()
                })
                .await
                .into_vec()
        })
        .await;
    }
    for window in 0..windows {
        let client = batched.clone();
        prepare_call(world, async move {
            join_all(["w1", "w2"].into_iter().map(|id| {
                client.get_widget(GetWidgetRequest {
                    id: id.to_owned(),
                    kind: format!("window-{window}"),
                    ..Default::default()
                })
            }))
            .await
        })
        .await;
    }
    let gate = lock(&world.data)
        .gate
        .as_ref()
        .expect("prepared wire barrier")
        .clone();
    gate.arrived
        .acquire_many(u32::try_from((explicit + windows).min(4)).expect("wire count"))
        .await
        .expect("prepared requests reached the server")
        .forget();
}

#[when(expr = "the final coalesced window {int} gets {string}")]
async fn when_final_window(world: &mut BatchWorld, window: usize, ids: String) {
    let batched = client(world).await;
    let mut ids = list_of(&ids).into_iter();
    let first = GetWidgetRequest {
        id: ids.next().expect("the step names at least one id"),
        kind: format!("window-{window}"),
        ..Default::default()
    };
    let lead = async { batched.get_widget(first).await };
    let rest = join_all(ids.map(|id| {
        batched.get_widget(GetWidgetRequest {
            id,
            kind: format!("window-{window}"),
            ..Default::default()
        })
    }));
    let mut calls = Box::pin(async { futures_util::future::join(lead, rest).await });
    std::future::poll_fn(|cx| {
        assert!(
            calls.as_mut().poll(cx).is_pending(),
            "prepared workload is held"
        );
        Poll::Ready(())
    })
    .await;
    release_prepared_calls(world);
    let (head, tail) = calls.await;
    let prepared = collect_prepared(world).await;
    world.results = prepared
        .into_iter()
        .chain(std::iter::once(head))
        .chain(tail)
        .collect();
}

#[given(
    expr = "{int} calls of the first token and {int} calls of the second token are batch-getting {string}"
)]
async fn given_two_tokens(world: &mut BatchWorld, first: usize, second: usize, ids: String) {
    let batched = client(world).await;
    let ids = list_of(&ids);
    let next = same_identity_new_token(world).await;
    hold_prepared_calls(world);
    for (count, lane) in [(first, "t1"), (second, "t2")] {
        for _ in 0..count {
            let client = batched.clone();
            let ids = ids.clone();
            prepare_call(world, async move {
                client
                    .batch_get_widgets_with_options(
                        BatchGetWidgetsRequest {
                            ids,
                            ..Default::default()
                        },
                        CallHeaders::new().header("x-token-lane", lane.as_bytes().to_vec()),
                    )
                    .await
                    .into_vec()
            })
            .await;
        }
        *lock(&world.credential) = next.clone();
    }
    world.first_token_calls = first;
}

#[when(expr = "the second token makes its final batch-get of {string}")]
async fn when_final_token_call(world: &mut BatchWorld, ids: String) {
    let batched = client(world).await;
    let mut call = Box::pin(async {
        batched
            .batch_get_widgets_with_options(
                BatchGetWidgetsRequest {
                    ids: list_of(&ids),
                    ..Default::default()
                },
                CallHeaders::new().header("x-token-lane", b"t2".to_vec()),
            )
            .await
    });
    std::future::poll_fn(|cx| {
        assert!(
            call.as_mut().poll(cx).is_pending(),
            "final identity has wire capacity"
        );
        Poll::Ready(())
    })
    .await;
    release_prepared_calls(world);
    let finished = call.await;
    let prepared = collect_prepared(world).await;
    world.results = prepared.into_iter().chain(finished.into_vec()).collect();
}

#[then("all prepared batch calls have finished")]
fn then_prepared_calls(world: &mut BatchWorld) {
    assert!(
        world.pending_batches.is_empty(),
        "every prepared call finished and was collected"
    );
}

#[when(
    expr = "the caller aborts the batch-get of the generated widgets after its first chunk was sent"
)]
async fn when_abort_explicit(world: &mut BatchWorld) {
    let batched = client(world).await;
    let template = BatchGetWidgetsRequest {
        ids: world.generated.clone(),
        ..Default::default()
    };
    // The server holds the first chunk, so the caller stops while it is on the
    // wire; once the held handler has ended, any further chunk would already
    // have arrived.
    let gate = Arc::new(Gate::default());
    lock(&world.data).gate = Some(gate.clone());
    let call = Box::pin(async { batched.batch_get_widgets(template).await });
    let arrived = Box::pin(gate.arrived.acquire());
    world.results = match futures_util::future::select(call, arrived).await {
        futures_util::future::Either::Left((finished, _)) => finished.into_vec(),
        futures_util::future::Either::Right((arrived, call)) => {
            arrived.expect("the gate is never closed").forget();
            drop(call);
            Vec::new()
        }
    };
    assert!(
        world.results.is_empty(),
        "the batch finished before it was aborted"
    );
    gate.open();
    gate.finished
        .acquire()
        .await
        .expect("the gate is never closed")
        .forget();
}

// ---- When: coalesced gets ---------------------------------------------------------

fn first_get(ids: &mut impl Iterator<Item = String>, empty_message: &str) -> GetWidgetRequest {
    GetWidgetRequest {
        id: ids.next().expect(empty_message),
        ..Default::default()
    }
}

async fn remaining_gets(
    batched: &WidgetsBatched,
    ids: impl Iterator<Item = String>,
) -> Vec<Outcome> {
    join_all(ids.map(|id| {
        batched.get_widget(GetWidgetRequest {
            id,
            ..Default::default()
        })
    }))
    .await
}

#[when(expr = "the callers get {string} at the same time")]
async fn when_gets(world: &mut BatchWorld, ids: String) {
    let batched = client(world).await;
    let mut ids = list_of(&ids).into_iter();
    let first = first_get(&mut ids, "the step names at least one id");
    world.results = collect_concurrent(
        async { batched.get_widget(first).await },
        remaining_gets(&batched, ids),
    )
    .await;
}

#[when("the callers get the generated widgets at the same time")]
async fn when_gets_generated(world: &mut BatchWorld) {
    let batched = client(world).await;
    let mut ids = world.generated.iter().cloned();
    let first = first_get(&mut ids, "generated widgets exist");
    world.results = collect_concurrent(
        async { batched.get_widget(first).await },
        remaining_gets(&batched, ids),
    )
    .await;
}

#[when(expr = "the callers get {string} under different tokens")]
async fn when_gets_tokens(world: &mut BatchWorld, ids: String) {
    let batched = client(world).await;
    let next = same_identity_new_token(world).await;
    let credential = world.credential.clone();
    let mut members = gets(&ids);
    members.insert(1, Member::Do(Box::new(move || *lock(&credential) = next)));
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

#[when(expr = "the callers get {string} with the x-trace values {string}")]
async fn when_gets_traces(world: &mut BatchWorld, ids: String, traces: String) {
    let batched = client(world).await;
    let members = list_of(&ids)
        .into_iter()
        .zip(list_of(&traces))
        .map(|(id, trace)| {
            Member::Get(
                GetWidgetRequest {
                    id,
                    ..Default::default()
                },
                CallHeaders::new().header("x-trace", trace.into_bytes()),
            )
        })
        .collect();
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

#[when(expr = "the callers get {string} with the deadlines {string} ms")]
async fn when_gets_deadlines(world: &mut BatchWorld, ids: String, deadlines: String) {
    let batched = client(world).await;
    let members = list_of(&ids)
        .into_iter()
        .zip(list_of(&deadlines))
        .map(|(id, millis)| {
            Member::Get(
                GetWidgetRequest {
                    id,
                    ..Default::default()
                },
                CallHeaders::new().timeout(Duration::from_millis(millis.parse().expect("ms"))),
            )
        })
        .collect();
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

#[when(expr = "the callers get {string} with the kinds {string}")]
async fn when_gets_kinds(world: &mut BatchWorld, ids: String, kinds: String) {
    let batched = client(world).await;
    let members = list_of(&ids)
        .into_iter()
        .zip(list_of(&kinds))
        .map(|(id, kind)| {
            Member::Get(
                GetWidgetRequest {
                    id,
                    kind,
                    ..Default::default()
                },
                CallHeaders::new(),
            )
        })
        .collect();
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

#[when(
    expr = "the callers get {string} at the same time and the token refresh changes the subject before the flush"
)]
async fn when_gets_identity_change(world: &mut BatchWorld, ids: String) {
    let batched = client(world).await;
    let org = lock(&world.credential).identity.org.clone();
    let next = token_credential(&world.issuers, "someone-else", &org).await;
    let credential = world.credential.clone();
    let mut members = gets(&ids);
    members.push(Member::Do(Box::new(move || {
        *lock(&credential) = next;
    })));
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

/// Poll both futures once per round, with a yield between rounds.
async fn poll_both(
    first: &mut Pin<Box<impl Future<Output = Outcome> + ?Sized>>,
    second: &mut Pin<Box<impl Future<Output = Outcome> + ?Sized>>,
    rounds: usize,
) {
    for _ in 0..rounds {
        std::future::poll_fn(|cx| {
            let _ = first.as_mut().poll(cx);
            let _ = second.as_mut().poll(cx);
            Poll::Ready(())
        })
        .await;
        tokio::task::yield_now().await;
    }
}

fn get_of(id: &str) -> GetWidgetRequest {
    GetWidgetRequest {
        id: id.to_owned(),
        ..Default::default()
    }
}

#[when(expr = "the caller aborts the get of {string} before the flush while {string} waits")]
async fn when_abort_before(world: &mut BatchWorld, aborted: String, kept: String) {
    let batched = client(world).await;
    let mut keep = Box::pin(async { batched.get_widget(get_of(&kept)).await });
    let mut leave = Box::pin(async { batched.get_widget(get_of(&aborted)).await });
    poll_both(&mut keep, &mut leave, 1).await;
    drop(leave);
    let served = keep.await;
    world.results = vec![served];
}

/// Abort the get once the server holds the batch request that carries both
/// gets, then let the server answer.
#[when(expr = "the caller aborts the get of {string} after the flush while {string} waits")]
async fn when_abort_after(world: &mut BatchWorld, aborted: String, kept: String) {
    let batched = client(world).await;
    let gate = Arc::new(Gate::default());
    lock(&world.data).gate = Some(gate.clone());
    let mut keep = Box::pin(async { batched.get_widget(get_of(&kept)).await });
    let mut leave = Box::pin(async { batched.get_widget(get_of(&aborted)).await });
    tokio::select! {
        biased;
        arrived = gate.arrived.acquire() => {
            arrived.expect("the gate is never closed").forget();
        }
        _ = &mut keep => panic!("the kept get finished before the server answered"),
        _ = &mut leave => panic!("the aborted get finished before it was aborted"),
    }
    drop(leave);
    gate.open();
    let served = keep.await;
    world.results = vec![served];
}

// ---- When: grouped lists ----------------------------------------------------------

/// Each group's parent and failure, and the page-iterator call of every group
/// that succeeded, kept for a later continuation.
fn read_groups(
    answered: Vec<api_bones_batch::Group<ListWidgetPartsRequest, ListWidgetPartsResponse>>,
) -> (Vec<GroupRead>, Vec<Option<WidgetPartsCall>>) {
    let rows = answered
        .iter()
        .map(|group| GroupRead {
            parent_id: group.parent_id.clone(),
            items: Vec::new(),
            error: group
                .outcome
                .as_ref()
                .err()
                .map(|error| error.connect_error().clone()),
        })
        .collect();
    let lists = answered
        .into_iter()
        .map(|group| group.outcome.ok())
        .collect();
    (rows, lists)
}

fn remember_groups(
    world: &mut BatchWorld,
    groups: Vec<api_bones_batch::Group<ListWidgetPartsRequest, ListWidgetPartsResponse>>,
) {
    let (reads, calls) = read_groups(groups);
    world.groups = reads;
    world.group_calls = calls;
}

fn batch_list_request(parents: &str) -> BatchListWidgetPartsRequest {
    BatchListWidgetPartsRequest {
        widget_ids: list_of(parents),
        ..Default::default()
    }
}

#[when(expr = "the caller batch-lists the parts of {string}")]
async fn when_batch_list(world: &mut BatchWorld, parents: String) {
    let batched = client(world).await;
    let groups = batched
        .batch_list_widget_parts_with_options(
            batch_list_request(&parents),
            world.batch_headers.clone(),
        )
        .await
        .groups();
    remember_groups(world, groups);
}

#[given(expr = "the caller has batch-listed the parts of {string}")]
async fn given_batch_list(world: &mut BatchWorld, parents: String) {
    let batched = client(world).await;
    let groups = batched
        .batch_list_widget_parts_with_options(
            batch_list_request(&parents),
            world.batch_headers.clone(),
        )
        .await
        .groups();
    remember_groups(world, groups);
}

async fn drain_parts(
    stream: impl futures_util::Stream<Item = Result<String, api_bones_page::PageError>>,
) -> (Vec<String>, Option<ConnectError>) {
    let mut pages = Box::pin(stream);
    let mut delivered = Vec::new();
    while let Some(step) = pages.next().await {
        match step {
            Ok(part) => delivered.push(part),
            Err(refused) => {
                let api_bones_page::PageErrorKind::Rpc(cause) = refused.kind else {
                    panic!("unexpected cursor refusal in group continuation");
                };
                return (delivered, Some(cause));
            }
        }
    }
    (delivered, None)
}

#[when(expr = "the consumer continues parts group {int}")]
async fn when_continue_group(world: &mut BatchWorld, position: usize) {
    let call = world
        .group_calls
        .get_mut(position - 1)
        .expect("seeded group")
        .take()
        .expect("seeded group call");
    let (parts, refusal) = drain_parts(call.items()).await;
    world.groups[position - 1].items = parts;
    world.groups[position - 1].error = refusal;
}

// ---- Then: results ------------------------------------------------------------------

#[given("the caller has started the grouped parts continuation")]
async fn given_grouped_continuation(world: &mut BatchWorld) {
    let batched = client(world).await;
    world.continuation_identity = Some(token_credential(&world.issuers, "bob", "acme").await);
    let mut groups = batched
        .batch_list_widget_parts(BatchListWidgetPartsRequest {
            widget_ids: list_of("complete,continuing"),
            ..Default::default()
        })
        .await
        .groups()
        .into_iter();
    let independent = groups.next().expect("independent group");
    assert_eq!(independent.parent_id, "complete");
    let kept: Vec<_> = independent
        .outcome
        .expect("independent list")
        .items()
        .map(|item| item.expect("independent delivered item"))
        .collect()
        .await;
    world.groups = vec![GroupRead {
        parent_id: "complete".into(),
        items: kept,
        error: None,
    }];
    let affected = groups.next().expect("continuing group");
    assert_eq!(affected.parent_id, "continuing");
    assert!(groups.next().is_none());
    world.continuation = Some(affected.outcome.expect("seeded list"));
}

#[when(expr = "a grouped single-List continuation encounters a {string} in flight")]
async fn when_grouped_identity_in_flight(world: &mut BatchWorld, failure: String) {
    let changed = world
        .continuation_identity
        .take()
        .expect("prepared credential");
    let mut stream = Box::pin(world.continuation.take().expect("seeded list").items());
    let gate = Arc::new(Gate::default());
    lock(&world.data).gate = Some(gate.clone());
    let prefix = stream
        .next()
        .await
        .expect("first item")
        .expect("delivered prefix");
    assert_eq!(prefix, "prefix");
    let failure = {
        let mut pending = pin!(stream.next());
        tokio::select! {
            biased;
            arrived = gate.arrived.acquire() => { arrived.expect("open arrival semaphore").forget(); }
            _ = &mut pending => panic!("continuation finished before server barrier"),
        }
        let requests = seen_rpc(world, "ListWidgetParts");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].cursor, "c1");
        match failure.as_str() {
            "source failure" => world
                .source_failed
                .store(true, std::sync::atomic::Ordering::SeqCst),
            "identity change" => *lock(&world.credential) = changed,
            other => panic!("unknown identity failure: {other}"),
        }
        gate.open();
        pending
            .await
            .expect("terminal error")
            .expect_err("in-flight response discarded")
    };
    assert_eq!(failure.delivered_items, 1);
    assert_eq!(failure.delivered_pages, 1);
    let api_bones_page::PageErrorKind::Rpc(cause) = failure.kind else {
        panic!("identity guard must return typed RPC failure")
    };
    assert!(stream.next().await.is_none());
    assert!(stream.next().await.is_none());
    world.groups.push(GroupRead {
        parent_id: "continuing".into(),
        items: vec![prefix],
        error: Some(cause),
    });
}

#[then(expr = "the grouped continuation retains its delivered prefix and independent group")]
fn then_grouped_identity_prefix(world: &mut BatchWorld) {
    assert_eq!(world.groups.len(), 2);
    assert_eq!(world.groups[0].parent_id, "complete");
    assert_eq!(world.groups[0].items, list_of("kept"));
    assert!(world.groups[0].error.is_none());
    assert_eq!(world.groups[1].parent_id, "continuing");
    assert_eq!(world.groups[1].items, list_of("prefix"));
    let error = world.groups[1].error.as_ref().expect("identity rejection");
    assert_eq!(error.code, ErrorCode::Unauthenticated);
    assert_eq!(
        error_info(error).expect("ErrorInfo").code,
        "BATCH_IDENTITY_CHANGED"
    );
    assert_eq!(
        seen(world).len(),
        2,
        "terminal iterator sends no subsequent request"
    );
}

fn outcome(world: &BatchWorld, position: usize) -> &Outcome {
    world
        .results
        .get(position - 1)
        .unwrap_or_else(|| panic!("the caller got no result at position {position}"))
}

fn widget_at(world: &BatchWorld, position: usize) -> &Widget {
    outcome(world, position)
        .as_ref()
        .unwrap_or_else(|error| panic!("position {position} failed: {error:?}"))
}

fn failure_at(world: &BatchWorld, position: usize) -> &ConnectError {
    outcome(world, position)
        .as_ref()
        .expect_err(&format!("position {position} was served"))
}

fn info_code(error: &ConnectError) -> Option<String> {
    error_info(error).map(|info| info.code)
}

#[then(expr = "the results are the widgets {string}")]
fn then_widgets(world: &mut BatchWorld, ids: String) {
    let expected = list_of(&ids);
    assert_eq!(world.results.len(), expected.len(), "one result per id");
    for (position, id) in expected.iter().enumerate() {
        assert_eq!(&widget_at(world, position + 1).id, id);
    }
}

#[then("the results are the generated widgets")]
fn then_generated(world: &mut BatchWorld) {
    let count = world.generated.len();
    assert_eq!(world.results.len(), count, "one result per id");
    then_generated_range(world, 1, count);
}

#[then(expr = "positions {int} to {int} are the generated widgets")]
fn then_generated_range(world: &mut BatchWorld, from: usize, to: usize) {
    for position in from..=to {
        let expected = world.generated[position - 1].clone();
        assert_eq!(widget_at(world, position).id, expected);
    }
}

#[then("there are no results")]
fn then_no_results(world: &mut BatchWorld) {
    assert!(world.results.is_empty());
}

#[then(expr = "position {int} is the widget {string}")]
fn then_position_widget(world: &mut BatchWorld, position: usize, id: String) {
    assert_eq!(
        world.results[position - 1]
            .as_ref()
            .expect("the position was served")
            .id,
        id
    );
}

#[then(expr = "position {int} fails with {string}")]
fn then_position_fails(world: &mut BatchWorld, position: usize, code: String) {
    assert_eq!(
        world.results[position - 1]
            .as_ref()
            .expect_err("the position was served")
            .code,
        code_named(&code)
    );
}

#[then(expr = "position {int} carries the ErrorInfo {string}")]
fn then_position_info(world: &mut BatchWorld, position: usize, code: String) {
    assert_eq!(
        error_info(
            world.results[position - 1]
                .as_ref()
                .expect_err("the position was served")
        )
        .map(|info| info.code),
        Some(code)
    );
}

#[then(expr = "positions {int} to {int} fail with {string}")]
fn then_range_fails(world: &mut BatchWorld, from: usize, to: usize, code: String) {
    assert_eq!(
        world.results[from - 1..to]
            .iter()
            .map(|outcome| outcome.as_ref().expect_err("the position was served").code)
            .collect::<Vec<_>>(),
        vec![code_named(&code); to + 1 - from]
    );
}

#[then(expr = "positions {int} to {int} carry the ErrorInfo {string}")]
fn then_range_info(world: &mut BatchWorld, from: usize, to: usize, code: String) {
    assert_eq!(
        world.results[from - 1..to]
            .iter()
            .map(
                |outcome| error_info(outcome.as_ref().expect_err("the position was served"))
                    .map(|info| info.code)
            )
            .collect::<Vec<_>>(),
        vec![Some(code); to + 1 - from]
    );
}

#[then(expr = "every position fails with {string}")]
fn then_every_fails(world: &mut BatchWorld, code: String) {
    assert!(!world.results.is_empty(), "the caller got no results");
    assert_eq!(
        world
            .results
            .iter()
            .map(|outcome| outcome.as_ref().expect_err("the position was served").code)
            .collect::<Vec<_>>(),
        vec![code_named(&code); world.results.len()]
    );
}

#[then(expr = "every position carries the ErrorInfo {string}")]
fn then_every_info(world: &mut BatchWorld, code: String) {
    assert_eq!(
        world
            .results
            .iter()
            .map(
                |outcome| error_info(outcome.as_ref().expect_err("the position was served"))
                    .map(|info| info.code)
            )
            .collect::<Vec<_>>(),
        vec![Some(code); world.results.len()]
    );
}

fn shape(error: &ConnectError) -> (ErrorCode, Option<String>, Option<String>) {
    (error.code, error.message.clone(), info_code(error))
}

#[then(expr = "positions {int} and {int} fail identically")]
fn then_identical(world: &mut BatchWorld, first: usize, second: usize) {
    assert_eq!(
        shape(
            world.results[first - 1]
                .as_ref()
                .expect_err("the position was served")
        ),
        shape(
            world.results[second - 1]
                .as_ref()
                .expect_err("the position was served")
        )
    );
}

#[then(expr = "position {int} carries the code, message and ErrorInfo of the unbatched get")]
fn then_same_as_single(world: &mut BatchWorld, position: usize) {
    let single = world
        .unbatched_widget
        .as_ref()
        .expect("the unbatched get was made")
        .as_ref()
        .expect_err("the unbatched get was refused");
    assert_eq!(
        shape(
            world.results[position - 1]
                .as_ref()
                .expect_err("the position was served")
        ),
        shape(single)
    );
}

#[then(expr = "the coalesced denial of {string} has the single get's code, message and ErrorInfo")]
async fn then_owner_denial(world: &mut BatchWorld, id: String) {
    when_unbatched_get(world, id).await;
    then_same_as_single(world, 1);
    let single = world
        .unbatched_widget
        .as_ref()
        .expect("single result")
        .as_ref()
        .expect_err("owner denial");
    let batched = failure_at(world, 1);
    assert_eq!(
        error_info(single).expect("single ErrorInfo").emitter,
        error_info(batched).expect("batch ErrorInfo").emitter
    );
}

#[then(expr = "the stored owner of widget {string} is {string}")]
async fn then_owner_readback(world: &mut BatchWorld, id: String, owner: String) {
    let org = lock(&world.credential).identity.org.clone();
    let db = &world
        .served
        .as_ref()
        .expect("the real service was called")
        .db;
    assert_eq!(db.owner(&org, &id).await, Some(owner));
}

#[then(expr = "every malformed batch response fails its chunk with {string}")]
fn then_malformed_batches(world: &mut BatchWorld, info: String) {
    assert_eq!(world.malformed_batches.len(), 1);
    for batch in &world.malformed_batches {
        assert_eq!(batch.len(), 3);
        for error in batch {
            assert_eq!(error.code, ErrorCode::Internal);
            assert_eq!(info_code(error), Some(info.clone()));
        }
    }
}

#[then("batch client construction is refused")]
fn then_unbound_client(world: &mut BatchWorld) {
    assert_eq!(
        world
            .construction_refusal
            .as_ref()
            .expect("constructor was called")
            .code,
        ErrorCode::InvalidArgument
    );
}

#[when(expr = "the SDK constructs a batched client with {string} policy drift")]
fn when_batch_policy_drift(world: &mut BatchWorld, drift: String) {
    assert_eq!(drift, "mutating batch");
    let client = api_bones_contract_rust::batch_descriptor::client_with_idempotency(Some(2));
    world.construction_refusal = Batched::anonymous(client).err();
}

#[then(expr = "construction fails with {string}")]
fn then_batch_policy_refused(world: &mut BatchWorld, message: String) {
    let error = world
        .construction_refusal
        .as_ref()
        .expect("construction refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|actual| actual.contains(&message)),
        "expected {message:?}, got {:?}",
        error.message
    );
}

#[then(expr = "the group continuation fails with {string} and ErrorInfo {string}")]
fn then_identity_before_continuation(world: &mut BatchWorld, code: String, info: String) {
    assert_eq!(world.groups.len(), 1);
    assert_eq!(world.groups[0].items, list_of("a"));
    let error = world.groups[0]
        .error
        .as_ref()
        .expect("continuation was refused");
    assert_eq!(error.code, code_named(&code));
    assert_eq!(info_code(error), Some(info));
}

#[then(expr = "the widget at position {int} echoes the kind {string}")]
fn then_kind(world: &mut BatchWorld, position: usize, kind: String) {
    assert_eq!(
        world.results[position - 1]
            .as_ref()
            .expect("the position was served")
            .kind,
        kind
    );
}

#[then(expr = "exactly {int} calls of the first token fail with {string}")]
fn then_first_token_fails(world: &mut BatchWorld, exactly: usize, code: String) {
    let failed = world.results[..world.first_token_calls]
        .iter()
        .filter(|result| matches!(result, Err(error) if error.code == code_named(&code)))
        .count();
    assert_eq!(
        failed, exactly,
        "{failed} of the first token's calls failed"
    );
    let admitted = api_bones_batch::MAX_WIRE_IN_FLIGHT + api_bones_batch::MAX_QUEUED_CHUNKS;
    assert_eq!(
        world.first_token_calls - failed,
        admitted,
        "one identity is served its wire slots plus its queue, no more"
    );
}

#[then(expr = "every other call of the first token is the widget {string}")]
fn then_first_token_served(world: &mut BatchWorld, id: String) {
    for served in world.results[..world.first_token_calls].iter().flatten() {
        assert_eq!(served.id, id);
    }
}

#[then(expr = "every call of the second token is the widget {string}")]
fn then_second_token_served(world: &mut BatchWorld, id: String) {
    let second = &world.results[world.first_token_calls..];
    assert!(!second.is_empty());
    for result in second {
        assert_eq!(
            result
                .as_ref()
                .expect("the second token's call is served")
                .id,
            id
        );
    }
}

#[then(expr = "every call is the widget {string}")]
fn then_every_call(world: &mut BatchWorld, id: String) {
    assert!(!world.results.is_empty());
    for result in &world.results {
        assert_eq!(result.as_ref().expect("the call is served").id, id);
    }
}

// ---- Then: groups ---------------------------------------------------------------------

#[then(expr = "group {int} is for {string} and lists the parts {string}")]
async fn then_group(world: &mut BatchWorld, position: usize, parent: String, parts: String) {
    assert_eq!(world.groups[position - 1].parent_id, parent);
    let (items, error) = match world.group_calls[position - 1].take() {
        Some(call) => drain_parts(call.items()).await,
        None => (
            world.groups[position - 1].items.clone(),
            world.groups[position - 1].error.clone(),
        ),
    };
    assert!(error.is_none(), "group {position} failed: {error:?}");
    assert_eq!(items, list_of(&parts));
    world.groups[position - 1].items = items;
    world.groups[position - 1].error = error;
}

#[then(expr = "group {int} fails with {string}")]
fn then_group_fails(world: &mut BatchWorld, position: usize, code: String) {
    assert_eq!(
        world.groups[position - 1]
            .error
            .as_ref()
            .expect("the group failed")
            .code,
        code_named(&code)
    );
    assert!(
        world.groups[position - 1].items.is_empty(),
        "a failed group lists nothing"
    );
}

#[then(expr = "groups {int} and {int} fail identically")]
fn then_groups_identical(world: &mut BatchWorld, first: usize, second: usize) {
    assert_eq!(
        shape(
            world.groups[first - 1]
                .error
                .as_ref()
                .expect("the group failed")
        ),
        shape(
            world.groups[second - 1]
                .error
                .as_ref()
                .expect("the group failed")
        )
    );
}

// ---- Then: what the server received -----------------------------------------------------

#[then(expr = "the server received {int} {string} request(s)")]
fn then_received(world: &mut BatchWorld, count: usize, rpc: String) {
    assert_eq!(seen_rpc(world, &rpc).len(), count);
}

#[then("the server received no request")]
fn then_received_none(world: &mut BatchWorld) {
    assert!(
        seen(world).is_empty(),
        "the server was asked: {:?}",
        seen(world)
    );
}

#[then(expr = "the batch requests carried the id counts {string}")]
fn then_counts(world: &mut BatchWorld, counts: String) {
    let carried: Vec<usize> = seen_rpc(world, "BatchGetWidgets")
        .iter()
        .map(|request| request.ids.len())
        .collect();
    let expected: Vec<usize> = list_of(&counts)
        .iter()
        .map(|count| count.parse().expect("a count"))
        .collect();
    assert_eq!(carried, expected);
}

#[then(expr = "the batch requests carried the id counts {string} in any order")]
fn then_counts_any_order(world: &mut BatchWorld, counts: String) {
    let mut carried: Vec<usize> = seen_rpc(world, "BatchGetWidgets")
        .iter()
        .map(|request| request.ids.len())
        .collect();
    let mut expected: Vec<usize> = list_of(&counts)
        .iter()
        .map(|count| count.parse().expect("a count"))
        .collect();
    carried.sort_unstable();
    expected.sort_unstable();
    assert_eq!(carried, expected);
}

#[then(expr = "the requests carried the ids {string}")]
fn then_ids(world: &mut BatchWorld, ids: String) {
    let mut carried: Vec<String> = seen(world)
        .into_iter()
        .flat_map(|request| request.ids)
        .collect();
    let mut expected = list_of(&ids);
    carried.sort();
    expected.sort();
    assert_eq!(carried, expected);
}

#[then(expr = "the first batch request carried fewer than {int} ids")]
fn then_first_fewer(world: &mut BatchWorld, limit: usize) {
    let first = seen_rpc(world, "BatchGetWidgets");
    let carried = first.first().expect("a batch request was sent").ids.len();
    assert!((1..limit).contains(&carried), "carried {carried} ids");
}

#[then("no request was larger than 1 MiB")]
fn then_size(world: &mut BatchWorld) {
    for request in seen(world) {
        assert!(request.encoded_len <= MIB, "{} bytes", request.encoded_len);
    }
}

#[then(expr = "the server never had more than {int} batch requests in flight")]
fn then_in_flight(world: &mut BatchWorld, limit: usize) {
    let peak = lock(&world.data).max_in_flight;
    assert!(peak <= limit, "{peak} requests were in flight at once");
    assert!(peak > 1, "the requests did not overlap");
}

#[then(expr = "the requests carried the x-trace values {string}")]
fn then_traces(world: &mut BatchWorld, traces: String) {
    let mut carried: Vec<String> = seen_rpc(world, "GetWidget")
        .into_iter()
        .map(|request| request.trace)
        .collect();
    let mut expected = list_of(&traces);
    carried.sort();
    expected.sort();
    assert_eq!(carried, expected);
}

#[then("the server received one batch request for each call that was served")]
fn then_one_per_served(world: &mut BatchWorld) {
    let served = world.results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(seen_rpc(world, "BatchGetWidgets").len(), served);
}

#[then(expr = "the single list requests carried the cursors {string}")]
fn then_cursors(world: &mut BatchWorld, cursors: String) {
    let carried: Vec<String> = seen_rpc(world, "ListWidgetParts")
        .into_iter()
        .map(|request| request.cursor)
        .collect();
    assert_eq!(carried, list_of(&cursors));
}

#[then("group continuation requests omit revision cache headers and preserve ordinary metadata")]
fn then_continuation_headers(world: &mut BatchWorld) {
    let requests = seen_rpc(world, "ListWidgetParts");
    assert!(
        !requests.is_empty(),
        "the group continued through the generated SDK"
    );
    for request in requests {
        assert!(request.if_none_match.is_empty());
        assert!(request.list_cache.is_empty());
        assert_eq!(request.trace, "preserved");
    }
}

#[then("the second token's requests were admitted before the first token's backlog drained")]
fn then_fair(world: &mut BatchWorld) {
    let requests = seen_rpc(world, "BatchGetWidgets");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.lane == "t2")
            .count(),
        2
    );
    let last_second = requests
        .iter()
        .rposition(|request| request.lane == "t2")
        .expect("the second token's calls were sent");
    assert!(
        last_second < requests.len() - 2,
        "the second token's last request was number {} of {}",
        last_second + 1,
        requests.len()
    );
}

#[then(expr = "one bones.batch record names {string} with {int} items and {int} calls")]
fn then_record(world: &mut BatchWorld, rpc: String, items: usize, calls: usize) {
    then_telemetry(world, "ok".into());
    let data = lock(&TELEMETRY);
    assert_eq!(data.spans.len(), 1);
    assert_eq!(data.spans[0]["rpc.method"], rpc);
    assert_eq!(data.spans[0]["bones.batch.items"], items);
    assert_eq!(data.spans[0]["bones.batch.calls"], calls);
    assert_eq!(data.spans[0]["bones.batch.requested"], items);
    assert_eq!(data.spans[0]["bones.batch.chunk_index"], 0);
    assert_eq!(data.spans[0]["bones.batch.chunks"], 1);
}

#[then("every result is a widget")]
fn then_every_served(world: &mut BatchWorld) {
    assert!(!world.results.is_empty());
    for position in 1..=world.results.len() {
        widget_at(world, position);
    }
}

#[given(expr = "the metadata producer attaches the x-trace value {string}")]
fn given_metadata(world: &mut BatchWorld, value: String) {
    *lock(&world.metadata) = value;
}

#[when(
    expr = "the callers get {string} while the metadata producer changes to {string} after enqueue"
)]
async fn when_metadata_changes(world: &mut BatchWorld, ids: String, next: String) {
    let batched = client(world).await;
    let mut members: Vec<Member> = list_of(&ids)
        .into_iter()
        .map(|id| {
            let captured = lock(&world.metadata).clone();
            Member::Get(
                GetWidgetRequest {
                    id,
                    ..Default::default()
                },
                CallHeaders::new().header("x-trace", captured.into_bytes()),
            )
        })
        .collect();
    let metadata = world.metadata.clone();
    members.push(Member::Do(Box::new(move || *lock(&metadata) = next)));
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

#[then(expr = "every request carried the captured x-trace value {string}")]
fn then_captured_metadata(world: &mut BatchWorld, expected: String) {
    let requests = seen(world);
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|request| request.trace == expected));
}

#[then("the single requests carried distinct timeout headers")]
fn then_distinct_deadlines(world: &mut BatchWorld) {
    let requests = seen_rpc(world, "GetWidget");
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| !request.timeout_ms.is_empty())
    );
    assert_ne!(requests[0].timeout_ms, requests[1].timeout_ms);
}

#[given("the session source fails while the server handles a batch request")]
fn given_source_fails_on_batch(world: &mut BatchWorld) {
    lock(&world.data).source_fails_on_batch = true;
}

#[when(expr = "the callers get {string} and the session source fails before flush")]
async fn when_source_fails_before_flush(world: &mut BatchWorld, ids: String) {
    let batched = client(world).await;
    let failed = world.source_failed.clone();
    let mut members = gets(&ids);
    members.push(Member::Do(Box::new(move || {
        failed.store(true, std::sync::atomic::Ordering::SeqCst)
    })));
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

#[when(expr = "the callers get {string} and the source returns {string} before flush")]
async fn when_source_fault_before_flush(world: &mut BatchWorld, ids: String, fault: String) {
    let batched = client(world).await;
    let source = world.credential.clone();
    let mut members = gets(&ids);
    members.push(Member::Do(Box::new(move || {
        corrupt_credential(&mut lock(&source), &fault)
    })));
    let (first, headers, others) = split_lead(members);
    world.results = concurrent_members(
        async { batched.get_widget_with_options(first, headers).await },
        &batched,
        others,
    )
    .await;
}

#[when(expr = "the caller batch-gets {string} with a per-call {string} credential override")]
async fn when_per_call_credential(world: &mut BatchWorld, ids: String, name: String) {
    assert!(matches!(
        name.as_str(),
        "authorization" | "brefwiz-relayed-credential"
    ));
    let batched = client(world).await;
    let bytes = lock(&world.credential)
        .headers
        .iter()
        .find(|(header, _)| header == "authorization")
        .expect("caller credential")
        .1
        .clone();
    world.results = batched
        .batch_get_widgets_with_options(
            BatchGetWidgetsRequest {
                ids: list_of(&ids),
                ..Default::default()
            },
            CallHeaders::new().header(&name, bytes),
        )
        .await
        .into_vec();
}

#[then(expr = "batch telemetry has exactly the wire outcomes {string}")]
fn then_telemetry(world: &mut BatchWorld, outcomes: String) {
    let data = lock(&TELEMETRY);
    assert_eq!(
        data.spans.len(),
        data.completions.len(),
        "one unsampled completion per span"
    );
    let sent = seen(world)
        .iter()
        .filter(|request| request.rpc.starts_with("Batch"))
        .count();
    assert_eq!(data.spans.len(), sent, "one span per wire batch");
    let mut actual: Vec<_> = data
        .spans
        .iter()
        .map(|span| {
            span["bones.batch.outcome"]
                .as_str()
                .expect("outcome")
                .to_string()
        })
        .collect();
    let mut expected = list_of(&outcomes);
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    let fields = [
        "rpc.method",
        "bones.batch.single_method",
        "bones.batch.requested",
        "bones.batch.items",
        "bones.batch.calls",
        "bones.batch.chunk_index",
        "bones.batch.chunks",
        "bones.batch.bytes",
        "bones.batch.wait_ms",
        "bones.batch.item_errors",
        "bones.batch.outcome",
    ];
    let mut measured_items: Vec<_> = data
        .spans
        .iter()
        .map(|span| span["bones.batch.items"].as_u64().expect("items"))
        .collect();
    let mut wire_items: Vec<_> = seen(world)
        .iter()
        .filter(|request| request.rpc.starts_with("Batch"))
        .map(|request| request.ids.len() as u64)
        .collect();
    measured_items.sort();
    wire_items.sort();
    assert_eq!(measured_items, wire_items);
    let mut measured_bytes: Vec<_> = data
        .spans
        .iter()
        .map(|span| span["bones.batch.bytes"].as_u64().expect("bytes"))
        .collect();
    let mut wire_bytes: Vec<_> = seen(world)
        .iter()
        .filter(|request| request.rpc.starts_with("Batch"))
        .map(|request| request.encoded_len as u64)
        .collect();
    measured_bytes.sort();
    wire_bytes.sort();
    assert_eq!(measured_bytes, wire_bytes);
    for span in &data.spans {
        let completions: Vec<_> = data
            .completions
            .iter()
            .filter(|event| {
                fields
                    .iter()
                    .all(|field| span.get(*field) == event.get(*field))
            })
            .collect();
        assert_eq!(
            completions.len(),
            1,
            "completion fields match exact span: {span:?}"
        );
        for field in &fields {
            assert!(span.contains_key(*field), "missing {field}");
        }
        for field in &fields[2..10] {
            assert!(
                span[*field].as_u64().is_some(),
                "nonnegative numeric {field}"
            );
        }
        assert!(span["bones.batch.bytes"].as_u64().expect("encoded bytes") > 0);
        assert_eq!(span["bones.batch.item_errors"], 0);
        assert!(span["bones.batch.requested"].as_u64() >= span["bones.batch.items"].as_u64());
        assert!(span["bones.batch.calls"].as_u64().expect("calls") >= 1);
        assert!(span["bones.batch.chunk_index"].as_u64() < span["bones.batch.chunks"].as_u64());
        let single = match span["rpc.method"].as_str().expect("RPC method") {
            "contract.v1.Widgets/BatchGetWidgets" => "contract.v1.Widgets/GetWidget",
            "contract.v1.Widgets/BatchListWidgetParts" => "contract.v1.Widgets/ListWidgetParts",
            method => panic!("unexpected batch method {method}"),
        };
        assert_eq!(span["bones.batch.single_method"], single);
    }
}
#[then("no batch span or completion is emitted")]
fn then_no_batch_telemetry(_: &mut BatchWorld) {
    let data = lock(&TELEMETRY);
    assert!(data.open.is_empty());
    assert!(data.spans.is_empty());
    assert!(data.completions.is_empty());
}
