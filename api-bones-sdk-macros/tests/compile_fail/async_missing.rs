// SPDX-License-Identifier: MIT
use api_bones_sdk_macros::client;

struct C;

#[client(surface = "s")]
impl C {
    #[export]
    pub fn f(&self) {}
}

fn main() {
    let _ = C;
}
