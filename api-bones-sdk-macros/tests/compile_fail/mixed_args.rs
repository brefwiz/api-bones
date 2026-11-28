// SPDX-License-Identifier: MIT
use api_bones_sdk::Error;
use api_bones_sdk_macros::client;

struct C;
struct Req;

#[client(surface = "s")]
impl C {
    #[export(args = generated::p::v1::Req)]
    pub async fn f(&self, _request: Req, _extra: u32) -> Result<(), Error> {
        std::future::pending().await
    }
}

fn main() {
    let _ = (C, Req);
}
