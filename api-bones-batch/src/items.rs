// SPDX-License-Identifier: MIT
//! Explicit batches of message items: a batch request that repeats the
//! single's request message instead of an id.

use std::sync::Arc;

use crate::key::CallHeaders;
use crate::results::BatchResults;
use crate::wire::{BatchFn, Explicit, Wire};

/// How one message-item batch method is sent.
pub struct ItemsSpec<Tmpl, I, T> {
    /// Full name of the batch RPC.
    pub rpc: &'static str,
    /// Full name of the single RPC.
    pub single: &'static str,
    /// Sends one batch request.
    pub batch: BatchFn<Tmpl, I, T>,
    /// Encoded size of the template, with no items.
    pub template_bytes: usize,
    /// The encoded size of one item as an element of the batch request: its
    /// field tag, length prefix and body.
    pub item_bytes: fn(&I) -> usize,
}

/// An explicit batch of message items: one outcome per input item, in input
/// order.
///
/// Items are never deduplicated and never coalesced: every position is sent
/// and answered on its own, because the single is a write or an item may
/// repeat on purpose. Items are split into requests by count and bytes and sent
/// one request after the other; an item that alone exceeds the request limit
/// answers its own `invalid_argument` and is not sent.
pub async fn run_items<Tmpl, I, T>(
    wire: &Arc<Wire>,
    spec: &ItemsSpec<Tmpl, I, T>,
    template: Tmpl,
    items: Vec<I>,
    headers: &CallHeaders,
) -> BatchResults<T>
where
    Tmpl: Clone,
    I: Clone + PartialEq,
    T: Clone,
{
    let sizes: Vec<usize> = items.iter().map(spec.item_bytes).collect();
    let explicit = Explicit {
        rpc: spec.rpc,
        single: spec.single,
        batch: &spec.batch,
        template_bytes: spec.template_bytes,
    };
    BatchResults::new(
        wire.run_items(&explicit, template, items, &sizes, headers)
            .await
            .0,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use connectrpc::ConnectError;

    use super::*;
    use crate::chunk::{MAX_ITEMS, MAX_REQUEST_BYTES};
    use crate::key::Anonymous;

    /// A recording batch over `(name, size)` items: it answers each item with
    /// its own name, or refuses the item named "refused".
    type Item = (String, usize);

    fn spec(sent: Arc<Mutex<Vec<Vec<Item>>>>) -> ItemsSpec<(), Item, String> {
        ItemsSpec {
            rpc: "/contract.Widgets/BatchFindWidgets",
            single: "/contract.Widgets/FindWidget",
            batch: Arc::new(move |(), items: Vec<Item>, _| {
                sent.lock().expect("sent").push(items.clone());
                Box::pin(async move {
                    Ok(items
                        .into_iter()
                        .map(|item| {
                            let outcome = if item.0 == "refused" {
                                Err(ConnectError::not_found("no such widget"))
                            } else {
                                Ok(item.0.clone())
                            };
                            (item, outcome)
                        })
                        .collect())
                })
            }),
            template_bytes: 0,
            item_bytes: |item| item.1,
        }
    }

    async fn run(items: Vec<Item>) -> (Vec<Vec<Item>>, Vec<Result<String, ConnectError>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let spec = spec(sent.clone());
        let wire = Arc::new(Wire::new(Arc::new(Anonymous), None));
        let results = run_items(&wire, &spec, (), items, &CallHeaders::new())
            .await
            .into_vec();
        let sent = sent.lock().expect("sent").clone();
        (sent, results)
    }

    fn named(name: &str, size: usize) -> Item {
        (name.to_owned(), size)
    }

    #[tokio::test]
    async fn no_items_send_no_request() {
        let (sent, results) = run(Vec::new()).await;
        assert_eq!(sent, Vec::<Vec<Item>>::new());
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn two_hundred_one_items_are_two_requests_answered_in_order() {
        let items: Vec<Item> = (0..201).map(|i| named(&format!("w{i}"), 10)).collect();
        let (sent, results) = run(items).await;
        let counts: Vec<usize> = sent.iter().map(Vec::len).collect();
        assert_eq!(counts, [MAX_ITEMS, 1]);
        let answered: Vec<String> = results.into_iter().map(|r| r.expect("item")).collect();
        let expected: Vec<String> = (0..201).map(|i| format!("w{i}")).collect();
        assert_eq!(answered, expected);
    }

    #[tokio::test]
    async fn a_request_closes_at_the_byte_limit_before_the_count_limit() {
        let half = MAX_REQUEST_BYTES / 2;
        let items = vec![
            named("a", half),
            named("b", half),
            named("c", 1),
            named("d", 1),
        ];
        let (sent, results) = run(items).await;
        let counts: Vec<usize> = sent.iter().map(Vec::len).collect();
        assert_eq!(counts, [2, 2]);
        let answered: Vec<String> = results.into_iter().map(|r| r.expect("item")).collect();
        assert_eq!(answered, ["a", "b", "c", "d"]);
    }

    #[tokio::test]
    async fn repeated_items_are_each_sent_and_answered_at_their_position() {
        let items = vec![named("a", 5), named("a", 5), named("b", 5), named("a", 5)];
        let (sent, results) = run(items).await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].len(), 4);
        let answered: Vec<String> = results.into_iter().map(|r| r.expect("item")).collect();
        assert_eq!(answered, ["a", "a", "b", "a"]);
    }

    #[tokio::test]
    async fn an_oversize_item_answers_its_own_error_and_is_not_sent() {
        let items = vec![
            named("a", 5),
            named("big", MAX_REQUEST_BYTES + 1),
            named("c", 5),
        ];
        let (sent, results) = run(items).await;
        assert_eq!(sent, [vec![named("a", 5), named("c", 5)]]);
        assert_eq!(results[0].as_ref().expect("first"), "a");
        let error = results[1].as_ref().expect_err("oversize");
        assert_eq!(error.code, connectrpc::ErrorCode::InvalidArgument);
        assert_eq!(results[2].as_ref().expect("last"), "c");
    }

    #[tokio::test]
    async fn an_item_refusal_stays_at_its_position() {
        let items = vec![named("a", 5), named("refused", 5), named("c", 5)];
        let (_, results) = run(items).await;
        assert_eq!(results[0].as_ref().expect("first"), "a");
        assert_eq!(
            results[1].as_ref().expect_err("refused").code,
            connectrpc::ErrorCode::NotFound
        );
        assert_eq!(results[2].as_ref().expect("last"), "c");
    }
}
