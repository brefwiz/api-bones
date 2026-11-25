// SPDX-License-Identifier: MIT
use api_bones_sdk_macros::client;

struct C;

#[client(surface = "s")]
impl C {
    #[export(sync)]
    pub async fn f(&self) {}
}

fn main() {
    let _ = C;
}
