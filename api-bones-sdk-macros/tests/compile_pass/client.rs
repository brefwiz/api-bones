// SPDX-License-Identifier: MIT
use std::time::SystemTime;

use api_bones_sdk::{Batch, Error, ListCall, Stream};
use api_bones_sdk_macros::{client, error};

/// Stand-in for the module the proto build generates; the macros read the
/// messages by their full path.
mod generated {
    pub mod contract {
        pub mod sdk {
            pub mod v1 {
                pub struct Number {
                    pub value: i64,
                }

                pub enum Kind {
                    Exact,
                    Estimate,
                }
            }
        }
    }
}

#[error(emitter = "numbers")]
#[derive(Debug)]
pub enum NumbersError {
    #[error(code = "NUMBER_GONE", platform = "NOT_FOUND")]
    Gone { id: String, at: SystemTime, size: i64, ratio: f64 },
    #[error(code = "NUMBER_BUSY", platform = "CONFLICT")]
    Busy,
}

pub struct NumbersClient;

#[client(surface = "numbers", proto = generated::contract::sdk::v1)]
impl NumbersClient {
    #[export]
    pub async fn list(
        &self,
        limit: u32,
        since: Option<SystemTime>,
    ) -> ListCall<generated::contract::sdk::v1::Number> {
        std::future::pending().await
    }

    #[export]
    pub async fn watch(
        &self,
        kind: generated::contract::sdk::v1::Kind,
    ) -> Stream<generated::contract::sdk::v1::Number> {
        std::future::pending().await
    }

    #[export]
    pub async fn get_all(&self, ids: Vec<String>) -> Batch<generated::contract::sdk::v1::Number> {
        std::future::pending().await
    }

    #[export(surface = "numbers-write")]
    pub async fn get(
        &self,
        id: String,
    ) -> Result<generated::contract::sdk::v1::Number, NumbersError> {
        std::future::pending().await
    }

    #[export]
    pub async fn clear(&self) -> Result<(), Error> {
        std::future::pending().await
    }

    #[export(sync)]
    pub fn label(&self, number: generated::contract::sdk::v1::Number) -> String {
        number.value.to_string()
    }

    #[export(sync)]
    pub fn reset(&self) {}

    #[export(sync)]
    pub fn check(
        &self,
        number: &generated::contract::sdk::v1::Number,
    ) -> Result<(), NumbersError> {
        if number.value < 0 {
            Err(NumbersError::Busy)
        } else {
            Ok(())
        }
    }

    pub fn plain(&self) -> usize {
        0
    }
}

fn main() {
    let error: Error = NumbersError::Busy.into();
    assert_eq!(error.code, "NUMBER_BUSY");
    assert_eq!(NumbersError::Busy.to_string(), "NUMBER_BUSY");
    assert_eq!(NumbersClient.label(generated::contract::sdk::v1::Number { value: 7 }), "7");
    assert!(NumbersClient.check(&generated::contract::sdk::v1::Number { value: -1 }).is_err());
}
