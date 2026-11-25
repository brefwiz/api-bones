// SPDX-License-Identifier: MIT
use api_bones_sdk_macros::client;

struct C;

struct NotAnError;

#[client(surface = "s")]
impl C {
    #[export]
    pub async fn f(&self) -> Result<(), NotAnError> {
        std::future::pending().await
    }
}

fn main() {
    let _ = (C, NotAnError);
}
