// SPDX-License-Identifier: MIT
//! What an explicit batch returns.

use connectrpc::{ConnectError, ErrorCode};

/// One item's outcome.
pub type ItemResult<T> = Result<T, ConnectError>;

/// The outcomes of an explicit batch: one per input id, in input order.
#[derive(Debug, Clone)]
pub struct BatchResults<T> {
    results: Vec<ItemResult<T>>,
}

impl<T> BatchResults<T> {
    /// Serve one provider request, enforcing the platform cap before fetching
    /// or authorizing any item.
    ///
    /// # Errors
    /// More than [`crate::MAX_ITEMS`] ids, or an unexpected item failure.
    pub async fn serve<'a, I, F>(
        ids: &'a [I],
        mut fetch: impl FnMut(&'a I) -> F,
        authorize: impl FnMut(&T) -> Result<(), ConnectError>,
    ) -> Result<Self, ConnectError>
    where
        F: std::future::Future<Output = ItemResult<T>>,
    {
        if ids.len() > crate::MAX_ITEMS {
            return Err(api_bones::connect::with_error_info(
                ConnectError::invalid_argument("too many items in one batch request"),
                &api_bones::connect::ErrorInfo::from_code(crate::BATCH_TOO_LARGE),
            ));
        }
        let mut outcomes = Vec::with_capacity(ids.len());
        for id in ids {
            outcomes.push(fetch(id).await);
        }
        Self::authorize(outcomes, authorize)
    }

    /// Authorize every successful server-side item with the single RPC's
    /// object authorization function before returning batch outcomes.
    ///
    /// # Errors
    /// Unexpected failures fail the whole call for service-layer redaction.
    pub fn authorize(
        outcomes: Vec<ItemResult<T>>,
        mut authorize: impl FnMut(&T) -> Result<(), ConnectError>,
    ) -> Result<Self, ConnectError> {
        let mut results = Vec::with_capacity(outcomes.len());
        for outcome in outcomes {
            let result = outcome.and_then(|item| authorize(&item).map(|()| item));
            if let Err(error) = result {
                if !matches!(
                    error.code,
                    ErrorCode::NotFound
                        | ErrorCode::PermissionDenied
                        | ErrorCode::InvalidArgument
                        | ErrorCode::FailedPrecondition
                ) {
                    return Err(error);
                }
                results.push(Err(error));
            } else {
                results.push(result);
            }
        }
        Ok(Self { results })
    }

    pub(crate) fn new(results: Vec<ItemResult<T>>) -> Self {
        Self { results }
    }

    /// How many ids were asked, duplicates included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Whether no id was asked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// The outcomes in input order.
    pub fn iter(&self) -> std::slice::Iter<'_, ItemResult<T>> {
        self.results.iter()
    }

    /// The outcomes in input order.
    #[must_use]
    pub fn into_vec(self) -> Vec<ItemResult<T>> {
        self.results
    }

    /// Every value, or the first error in input order.
    ///
    /// # Errors
    /// The first failed position's error.
    pub fn into_all(self) -> Result<Vec<T>, ConnectError> {
        self.results.into_iter().collect()
    }
}

impl<'a, T> IntoIterator for &'a BatchResults<T> {
    type Item = &'a ItemResult<T>;
    type IntoIter = std::slice::Iter<'a, ItemResult<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.results.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_all_reports_the_first_error_in_input_order() {
        let results = BatchResults::new(vec![
            Ok(1),
            Err(ConnectError::not_found("second")),
            Err(ConnectError::internal("third")),
        ]);
        assert_eq!(results.len(), 3);
        let error = results.into_all().expect_err("a failed position");
        assert_eq!(error.message.as_deref(), Some("second"));
    }

    #[test]
    fn into_all_returns_every_value_when_none_failed() {
        let results = BatchResults::new(vec![Ok(1), Ok(2)]);
        assert_eq!(results.into_all().expect("all ok"), [1, 2]);
    }

    #[test]
    fn authorization_preserves_existing_errors_and_refuses_successful_items() {
        let mut authorized = Vec::new();
        let results = BatchResults::authorize(
            vec![Ok(1), Err(ConnectError::not_found("absent")), Ok(2)],
            |item| {
                authorized.push(*item);
                if *item == 2 {
                    Err(ConnectError::permission_denied("not owned"))
                } else {
                    Ok(())
                }
            },
        )
        .expect("item refusals")
        .into_vec();
        assert_eq!(authorized, [1, 2]);
        assert_eq!(results[0].as_ref().expect("authorized"), &1);
        assert_eq!(
            results[1].as_ref().expect_err("missing").code,
            connectrpc::ErrorCode::NotFound
        );
        assert_eq!(
            results[2].as_ref().expect_err("denied").code,
            connectrpc::ErrorCode::PermissionDenied
        );
    }

    #[tokio::test]
    async fn provider_cap_refuses_before_any_item_work() {
        let ids = vec![(); crate::MAX_ITEMS + 1];
        let error = BatchResults::serve(
            &ids,
            |()| async { panic!("fetch must not run") },
            |(): &()| panic!("authorization must not run"),
        )
        .await
        .expect_err("over-cap request");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(
            api_bones::connect::error_info(&error)
                .expect("ErrorInfo")
                .code,
            crate::BATCH_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn provider_serves_the_full_cap_in_request_order() {
        let ids: Vec<_> = (0..crate::MAX_ITEMS).collect();
        let outcomes = BatchResults::serve(&ids, |id| async move { Ok(*id) }, |_| Ok(()))
            .await
            .expect("at-cap request")
            .into_all()
            .expect("every item");
        assert_eq!(outcomes, ids);
    }

    #[test]
    fn only_expected_item_error_codes_are_retained() {
        for code in [
            ErrorCode::NotFound,
            ErrorCode::PermissionDenied,
            ErrorCode::InvalidArgument,
            ErrorCode::FailedPrecondition,
        ] {
            let outcomes = BatchResults::<()>::authorize(
                vec![Err(ConnectError::new(code, "expected refusal"))],
                |()| Ok(()),
            )
            .expect("expected per-item refusal")
            .into_vec();
            assert_eq!(outcomes[0].as_ref().expect_err("item refused").code, code);
        }
        for code in [
            ErrorCode::Unknown,
            ErrorCode::Unavailable,
            ErrorCode::Unauthenticated,
            ErrorCode::ResourceExhausted,
            ErrorCode::Internal,
        ] {
            let error = BatchResults::<()>::authorize(
                vec![Err(ConnectError::new(code, "private fault"))],
                |()| Ok(()),
            )
            .expect_err("unexpected failure escapes whole call");
            assert_eq!(error.code, code);
        }
    }

    #[test]
    fn unexpected_item_and_authorization_errors_fail_the_whole_call() {
        for authorization in [false, true] {
            let outcomes = if authorization {
                vec![Ok(1)]
            } else {
                vec![Err(ConnectError::internal("private fault"))]
            };
            let error =
                BatchResults::authorize(outcomes, |_| Err(ConnectError::internal("private fault")))
                    .expect_err("server fault must escape item serialization");
            assert_eq!(error.code, ErrorCode::Internal);
            assert_eq!(error.message.as_deref(), Some("private fault"));
        }
    }
}
