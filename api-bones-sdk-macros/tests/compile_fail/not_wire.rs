// SPDX-License-Identifier: MIT
use api_bones_sdk::Error;
use api_bones_sdk_macros::client;

struct C;

struct Plain;

#[client(surface = "s")]
impl C {
    #[export]
    pub async fn f(&self, x: Plain) -> Result<(), Error> {
        let _ = x;
        std::future::pending().await
    }
}

fn main() {
    let _ = (C, Plain);
}
