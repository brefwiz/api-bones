// SPDX-License-Identifier: MIT
use api_bones_sdk::Error;
use api_bones_sdk_macros::client;

struct C;

#[client(surface = "s")]
impl C {
    #[export]
    pub async fn f(&self) -> Result<&String, Error> {
        std::future::pending().await
    }
}

fn main() {
    let _ = C;
}
