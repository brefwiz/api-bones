// SPDX-License-Identifier: MIT
use std::rc::Rc;

use api_bones_sdk::Error;
use api_bones_sdk_macros::client;

struct C;

#[client(surface = "s")]
impl C {
    #[export]
    pub async fn f(&self, x: Rc<String>) -> Result<(), Error> {
        let _ = x;
        std::future::pending().await
    }
}

fn main() {
    let _ = C;
}
