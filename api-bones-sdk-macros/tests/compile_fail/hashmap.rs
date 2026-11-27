// SPDX-License-Identifier: MIT
use std::collections::HashMap;

use api_bones_sdk::Error;
use api_bones_sdk_macros::client;

struct C;

#[client(surface = "s")]
impl C {
    #[export]
    pub async fn f(&self) -> Result<HashMap<String, u32>, Error> {
        std::future::pending().await
    }
}

fn main() {
    let _ = C;
}
