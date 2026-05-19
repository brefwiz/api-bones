use connectrpc::ConnectError;

mod private {
    pub trait Sealed {}
    impl<T, E> Sealed for Result<Option<T>, E> {}
}

/// Extension trait on `Result<Option<T>, E>` for ergonomic not-found shaping.
///
/// Sealed — only `api-bones` provides an implementation. Reimplementing in
/// service code produces a different trait that will not satisfy imports using
/// this path, making the canonical combinator mandatory at call sites that
/// import it (ADR-0096 §Enforcement Level 1).
///
/// # Example
///
/// ```rust
/// use api_bones::connect::ConnectOptionExt as _;
/// use connectrpc::ConnectError;
///
/// async fn get_record(id: u64) -> Result<String, ConnectError> {
///     let row: Option<String> = None; // e.g. from a DB lookup
///     Ok(Ok::<_, ConnectError>(row).or_not_found("record not found")?)
/// }
/// ```
pub trait ConnectOptionExt<T>: private::Sealed {
    /// Return `ConnectError::not_found(msg)` if the inner value is `None`,
    /// or propagate the `Err` variant mapped through `Into<ConnectError>`.
    ///
    /// # Errors
    ///
    /// Returns `Err(ConnectError::not_found(msg))` when `self` is `Ok(None)`.
    /// Returns `Err(e.into())` when `self` is `Err(e)`.
    fn or_not_found(self, msg: &str) -> Result<T, ConnectError>
    where
        Self: Sized;
}

impl<T, E> ConnectOptionExt<T> for Result<Option<T>, E>
where
    E: Into<ConnectError>,
{
    fn or_not_found(self, msg: &str) -> Result<T, ConnectError> {
        match self {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(ConnectError::not_found(msg.to_owned())),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn some_returns_value() {
        let res: Result<Option<u32>, ConnectError> = Ok(Some(42));
        assert_eq!(res.or_not_found("gone").unwrap(), 42);
    }

    #[test]
    fn none_returns_not_found() {
        let res: Result<Option<u32>, ConnectError> = Ok(None);
        let err = res.or_not_found("thing not found").unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not_found") || msg.contains("NotFound") || msg.contains("thing not found"));
    }

    #[test]
    fn err_propagates() {
        let res: Result<Option<u32>, ConnectError> = Err(ConnectError::internal("boom"));
        assert!(res.or_not_found("whatever").is_err());
    }
}
