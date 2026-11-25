// SPDX-License-Identifier: MIT
use std::time::SystemTime;

use api_bones_sdk::{Batch, Error, ListCall, Stream};
use api_bones_sdk_macros::{client, enumeration, error, record};

#[record]
pub struct Number {
    pub value: i64,
}

#[enumeration]
pub enum Kind {
    Exact,
    Estimate,
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

#[client(surface = "numbers")]
impl NumbersClient {
    #[export]
    pub async fn list(&self, limit: u32, since: Option<SystemTime>) -> ListCall<Number> {
        std::future::pending().await
    }

    #[export]
    pub async fn watch(&self, kind: Kind) -> Stream<Number> {
        std::future::pending().await
    }

    #[export]
    pub async fn get_all(&self, ids: Vec<String>) -> Batch<Number> {
        std::future::pending().await
    }

    #[export(surface = "numbers-write")]
    pub async fn get(&self, id: String) -> Result<Number, NumbersError> {
        std::future::pending().await
    }

    #[export]
    pub async fn clear(&self) -> Result<(), Error> {
        std::future::pending().await
    }

    #[export(sync)]
    pub fn label(&self, number: Number) -> String {
        number.value.to_string()
    }

    #[export(sync)]
    pub fn reset(&self) {}

    #[export(sync)]
    pub fn check(&self, number: Option<Vec<Number>>) -> Result<(), NumbersError> {
        match number {
            Some(_) => Ok(()),
            None => Err(NumbersError::Busy),
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
    assert_eq!(NumbersClient.label(Number { value: 7 }), "7");
    assert!(NumbersClient.check(None).is_err());
}
