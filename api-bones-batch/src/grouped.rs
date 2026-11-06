// SPDX-License-Identifier: MIT
//! Grouped `BatchList`: one group per parent, each continuing through the page
//! iterator with the single List RPC.
//!
//! A group that carries items seeds [`ListCall::from_first_page`] with the
//! page the batch returned and the group's `next_cursor`; the batch-sourced
//! first page never touches a revision cache. A group that carries an error
//! yields that error, mapped exactly like the single call's. A parent from
//! another organization is answered by the server like an unknown one, so it
//! arrives as a `not_found` group and the SDK adds nothing to tell them apart.

use std::sync::Arc;

use api_bones::connect::{Idempotency, error_info};
use api_bones_page::reason::CURSOR_INVALID;
use api_bones_page::{ListCall, Page, PagedRequest, PagedResponse};
use connectrpc::ConnectError;
use connectrpc::client::CallOptions;

use crate::key::{CallHeaders, CredentialGuard};
use crate::wire::{BatchFn, Boxed, Explicit, Wire};

/// Builds the single List request of one parent from the batch template.
///
/// The template arrives with its parent ids cleared; everything else the
/// batch request carries (filters, order, page size) belongs in the list
/// request too.
pub trait ListRequestFor<Tmpl>: Sized {
    /// The single List request for `parent_id`.
    fn for_parent(template: &Tmpl, parent_id: &str) -> Self;
}

/// What a batch answered for one parent: its first page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstPage<T> {
    /// The group's items.
    pub items: Vec<T>,
    /// Cursor of the next page; empty when the list ended there.
    pub next_cursor: String,
}

/// Why a group has no list call.
#[derive(Debug, Clone)]
pub enum GroupError {
    /// The server refused a cursor: it is tampered, or issued under another
    /// organization or principal. The list must restart from its first page.
    CursorInvalid(ConnectError),
    /// Any other failure of the group or of its chunk.
    Failed(ConnectError),
}

impl GroupError {
    /// Classify `error` by its `bones.v1.ErrorInfo` code.
    #[must_use]
    pub fn from_connect(error: ConnectError) -> Self {
        if error_info(&error).is_some_and(|info| info.code == CURSOR_INVALID) {
            Self::CursorInvalid(error)
        } else {
            Self::Failed(error)
        }
    }

    /// Whether the cursor was refused.
    #[must_use]
    pub fn is_cursor_invalid(&self) -> bool {
        matches!(self, Self::CursorInvalid(_))
    }

    /// The error the single call would raise.
    #[must_use]
    pub fn connect_error(&self) -> &ConnectError {
        match self {
            Self::CursorInvalid(error) | Self::Failed(error) => error,
        }
    }

    /// The error the single call would raise.
    #[must_use]
    pub fn into_connect_error(self) -> ConnectError {
        match self {
            Self::CursorInvalid(error) | Self::Failed(error) => error,
        }
    }
}

/// One parent's outcome.
pub struct Group<Req, Resp: PagedResponse> {
    /// The parent this group answers.
    pub parent_id: String,
    /// The list continuing from the batch's first page, or why there is none.
    pub outcome: Result<ListCall<Req, Resp>, GroupError>,
}

/// The groups of a `BatchList`, one per input parent in input order.
pub struct BatchGroups<Req, Resp: PagedResponse> {
    groups: Vec<Group<Req, Resp>>,
}

impl<Req, Resp: PagedResponse> BatchGroups<Req, Resp> {
    /// How many parents were asked, duplicates included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether no parent was asked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The groups in input order.
    #[must_use]
    pub fn groups(self) -> Vec<Group<Req, Resp>> {
        self.groups
    }
}

/// The single List RPC, called with the options of one page fetch.
pub type SingleList<Req, Resp> =
    Arc<dyn Fn(Req, CallOptions) -> Boxed<Result<Resp, ConnectError>> + Send + Sync>;

/// How one grouped method is sent.
pub struct GroupedSpec<Tmpl, Req, Resp: PagedResponse> {
    /// Full name of the batch RPC.
    pub rpc: &'static str,
    /// Full name of the single List RPC.
    pub single: &'static str,
    /// The single List RPC's declared contract.
    pub idempotency: Idempotency,
    /// Sends one batch request.
    pub batch: BatchFn<Tmpl, FirstPage<Resp::Item>>,
    /// Fetches one page of the single List RPC.
    pub single_call: SingleList<Req, Resp>,
    /// Encoded size of the template, reserve for the ids field included.
    pub template_bytes: usize,
}

/// Seeds the continuation of each group under the identity of batch time.
struct Seeder<'a, Tmpl, Req, Resp: PagedResponse> {
    wire: &'a Arc<Wire>,
    spec: &'a GroupedSpec<Tmpl, Req, Resp>,
    template: &'a Tmpl,
    headers: &'a CallHeaders,
    guard: &'a CredentialGuard,
}

impl<Tmpl, Req, Resp> Seeder<'_, Tmpl, Req, Resp>
where
    Req: PagedRequest + ListRequestFor<Tmpl>,
    Resp: PagedResponse,
    Resp::Item: Clone,
{
    fn seed(&self, parent_id: &str, first: FirstPage<Resp::Item>) -> ListCall<Req, Resp> {
        let request = Req::for_parent(self.template, parent_id);
        let wire = Arc::clone(self.wire);
        let single = Arc::clone(&self.spec.single_call);
        let guard = self.guard.clone();
        let headers = self.headers.continuation();
        let fetch: api_bones_page::Fetch<Req, Resp> =
            Arc::new(move |request: Req| -> Boxed<Result<Resp, ConnectError>> {
                let wire = Arc::clone(&wire);
                let single = Arc::clone(&single);
                let guard = guard.clone();
                let headers = headers.clone();
                Box::pin(async move {
                    let credential = match wire.verify(&guard) {
                        Ok(credential) => credential,
                        Err(error) => return Err(error),
                    };
                    headers.validate(&credential.headers)?;
                    let response =
                        single(request, headers.call_options(&credential.headers)?).await;
                    wire.verify(&guard)?;
                    response
                })
            });
        let page = Page {
            from_cache: false,
            has_next: !first.next_cursor.is_empty(),
            items: first.items.into(),
            revision: String::new(),
            total: None,
        };
        ListCall::from_first_page(
            self.spec.single,
            self.spec.idempotency,
            fetch,
            request,
            page,
            first.next_cursor,
        )
    }
}

/// A grouped batch: one group per input parent, in input order.
///
/// Parents are deduplicated, chunked and sent exactly like the ids of an
/// explicit get; each chunk failure lands in the groups of that chunk only.
pub async fn run_grouped<Tmpl, Req, Resp>(
    wire: &Arc<Wire>,
    spec: &GroupedSpec<Tmpl, Req, Resp>,
    template: Tmpl,
    parent_ids: &[String],
    headers: &CallHeaders,
) -> BatchGroups<Req, Resp>
where
    Tmpl: Clone,
    Req: PagedRequest + ListRequestFor<Tmpl>,
    Resp: PagedResponse,
    Resp::Item: Clone,
{
    let explicit = Explicit {
        rpc: spec.rpc,
        single: spec.single,
        batch: &spec.batch,
        template_bytes: spec.template_bytes,
    };
    let (outcomes, guard) = wire
        .run_explicit(&explicit, template.clone(), parent_ids, headers)
        .await;
    let groups = parent_ids
        .iter()
        .zip(outcomes)
        .map(|(parent_id, outcome)| {
            let outcome = match (&guard, outcome) {
                (_, Err(error)) => Err(GroupError::from_connect(error)),
                (None, Ok(_)) => Err(GroupError::from_connect(
                    crate::item_error::identity_changed(),
                )),
                (Some(guard), Ok(first)) => Ok(Seeder {
                    wire,
                    spec,
                    template: &template,
                    headers,
                    guard,
                }
                .seed(parent_id, first)),
            };
            Group {
                parent_id: parent_id.clone(),
                outcome,
            }
        })
        .collect();
    BatchGroups { groups }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Anonymous, Identity};
    use crate::wire::WireResults;
    use api_bones::connect::{ErrorInfo, with_error_info};
    use api_bones_page::PageInfo;
    use futures_util::StreamExt as _;

    #[derive(Clone)]
    struct Tmpl;

    #[derive(Clone)]
    struct Req {
        parent: String,
        cursor: String,
    }

    impl PagedRequest for Req {
        fn set_page(&mut self, cursor: &str, _page_size: Option<u32>) {
            cursor.clone_into(&mut self.cursor);
        }
    }

    impl ListRequestFor<Tmpl> for Req {
        fn for_parent(_template: &Tmpl, parent_id: &str) -> Self {
            Self {
                parent: parent_id.to_owned(),
                cursor: String::new(),
            }
        }
    }

    struct Resp {
        items: Vec<String>,
        next: String,
    }

    impl PagedResponse for Resp {
        type Item = String;

        fn split(self) -> (Vec<String>, PageInfo) {
            (
                self.items,
                PageInfo {
                    next_cursor: self.next,
                    ..PageInfo::default()
                },
            )
        }
    }

    fn cursor_invalid() -> ConnectError {
        with_error_info(
            ConnectError::invalid_argument("bad cursor"),
            &ErrorInfo::from_code(CURSOR_INVALID),
        )
    }

    fn spec(next_cursor: &'static str) -> GroupedSpec<Tmpl, Req, Resp> {
        GroupedSpec {
            rpc: "p.S/BatchList",
            single: "p.S/List",
            idempotency: Idempotency::NoSideEffects,
            batch: Arc::new(
                move |_template: Tmpl, ids: Vec<String>, _options: CallOptions| -> Boxed<Result<WireResults<FirstPage<String>>, ConnectError>> {
                    Box::pin(async move {
                        let answers: WireResults<FirstPage<String>> = ids
                            .into_iter()
                            .map(|id| {
                                let outcome = if id == "gone" {
                                    Err(ConnectError::not_found("no such parent"))
                                } else {
                                    Ok(FirstPage {
                                        items: vec![format!("{id}-1"), format!("{id}-2")],
                                        next_cursor: next_cursor.to_owned(),
                                    })
                                };
                                (id, outcome)
                            })
                            .collect();
                        Ok::<_, ConnectError>(answers)
                    })
                },
            ),
            single_call: Arc::new(
                |request: Req, _options: CallOptions| -> Boxed<Result<Resp, ConnectError>> {
                    Box::pin(async move {
                        if request.cursor == "stale" {
                            Err(cursor_invalid())
                        } else {
                            Ok(Resp {
                                items: vec![format!("{}-next", request.parent)],
                                next: String::new(),
                            })
                        }
                    })
                },
            ),
            template_bytes: 0,
        }
    }

    fn wire() -> Arc<Wire> {
        Arc::new(Wire::new(Arc::new(Anonymous), None))
    }

    fn parents(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[tokio::test]
    async fn one_group_per_parent_in_input_order_with_unknown_parents_as_not_found() {
        let wire = wire();
        let groups = run_grouped(
            &wire,
            &spec(""),
            Tmpl,
            &parents(&["a", "gone", "a"]),
            &CallHeaders::new(),
        )
        .await
        .groups();
        let names: Vec<&str> = groups.iter().map(|g| g.parent_id.as_str()).collect();
        assert_eq!(names, ["a", "gone", "a"]);
        let mut groups = groups.into_iter();
        let first = groups.next().expect("group").outcome.expect("a list call");
        let items: Vec<String> = first.items().map(|r| r.expect("item")).collect().await;
        assert_eq!(items, ["a-1", "a-2"]);
        let missing = groups
            .next()
            .expect("group")
            .outcome
            .err()
            .expect("an error");
        assert!(!missing.is_cursor_invalid());
        assert_eq!(
            missing.connect_error().code,
            connectrpc::ErrorCode::NotFound
        );
        assert!(
            groups.next().expect("group").outcome.is_ok(),
            "a duplicate parent is answered at every position"
        );
    }

    #[tokio::test]
    async fn a_group_continues_through_the_single_list() {
        let wire = wire();
        let mut groups = run_grouped(
            &wire,
            &spec("c1"),
            Tmpl,
            &parents(&["a"]),
            &CallHeaders::new(),
        )
        .await
        .groups();
        let call = groups.remove(0).outcome.expect("a list call");
        let items: Vec<String> = call.items().map(|r| r.expect("item")).collect().await;
        assert_eq!(items, ["a-1", "a-2", "a-next"]);
    }

    #[tokio::test]
    async fn a_group_discards_a_continuation_when_identity_changes_during_await() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let changed = Arc::new(AtomicBool::new(false));
        let source_changed = Arc::clone(&changed);
        let wire = Arc::new(Wire::new(
            Arc::new(move || {
                Ok(crate::key::Credential {
                    identity: Identity {
                        subject: source_changed.load(Ordering::SeqCst).to_string(),
                        org: String::new(),
                    },
                    headers: Vec::new(),
                })
            }),
            None,
        ));
        let mut method = spec("c1");
        method.single_call = Arc::new(move |_request, _options| {
            let changed = Arc::clone(&changed);
            Box::pin(async move {
                tokio::task::yield_now().await;
                changed.store(true, Ordering::SeqCst);
                Ok(Resp {
                    items: vec!["must-not-deliver".to_owned()],
                    next: String::new(),
                })
            })
        });
        let call = run_grouped(&wire, &method, Tmpl, &parents(&["a"]), &CallHeaders::new())
            .await
            .groups()
            .remove(0)
            .outcome
            .expect("seeded group");
        let results: Vec<_> = call.items().collect().await;
        assert_eq!(results.len(), 3);
        let error = results[2]
            .as_ref()
            .expect_err("discard changed-identity response");
        assert!(
            matches!(&error.kind, api_bones_page::PageErrorKind::Rpc(cause) if cause.code == connectrpc::ErrorCode::Unauthenticated)
        );
    }

    #[tokio::test]
    async fn a_refused_cursor_ends_that_group_with_the_typed_error() {
        let wire = wire();
        let mut groups = run_grouped(
            &wire,
            &spec("stale"),
            Tmpl,
            &parents(&["a"]),
            &CallHeaders::new(),
        )
        .await
        .groups();
        let call = groups.remove(0).outcome.expect("a list call");
        let results: Vec<_> = call.items().collect().await;
        assert_eq!(results.len(), 3);
        let failure = results
            .last()
            .expect("last")
            .as_ref()
            .expect_err("the cursor was refused");
        assert!(failure.is_cursor_rejected());
    }

    #[test]
    fn a_group_error_with_the_cursor_reason_is_typed() {
        assert!(GroupError::from_connect(cursor_invalid()).is_cursor_invalid());
        let other = GroupError::from_connect(ConnectError::not_found("x"));
        assert!(!other.is_cursor_invalid());
        assert_eq!(
            other.into_connect_error().code,
            connectrpc::ErrorCode::NotFound
        );
    }

    #[tokio::test]
    async fn no_parents_make_no_groups() {
        let wire = wire();
        let groups = run_grouped(&wire, &spec(""), Tmpl, &[], &CallHeaders::new()).await;
        assert!(groups.is_empty());
        assert_eq!(groups.len(), 0);
    }

    #[tokio::test]
    async fn duplicate_credential_headers_never_dispatch_a_grouped_batch() {
        for name in ["authorization", "brefwiz-relayed-credential"] {
            let mut method = spec("");
            method.batch = Arc::new(|_, _, _| panic!("ambiguous credentials must not dispatch"));
            for reverse in [false, true] {
                let mut headers = vec![
                    (name.to_owned(), digest_for_fixture(1)),
                    (name.to_ascii_uppercase(), digest_for_fixture(2)),
                ];
                if reverse {
                    headers.reverse();
                }
                let wire = Arc::new(Wire::new(
                    Arc::new(move || {
                        Ok(crate::key::Credential {
                            identity: Identity::default(),
                            headers: headers.clone(),
                        })
                    }),
                    None,
                ));
                let mut groups = run_grouped(
                    &wire,
                    &method,
                    Tmpl,
                    &parents(&["a", "b"]),
                    &CallHeaders::new(),
                )
                .await
                .groups();
                for group in groups.drain(..) {
                    let error = group.outcome.err().expect("duplicate credential rejected");
                    assert_eq!(
                        error.connect_error().code,
                        connectrpc::ErrorCode::InvalidArgument
                    );
                }
            }
        }
    }

    fn digest_for_fixture(value: u8) -> Vec<u8> {
        crate::key::digest(&[value]).to_vec()
    }
}
