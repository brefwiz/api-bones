// SPDX-License-Identifier: MIT
use api_bones_sdk_macros::export;

struct C;

impl C {
    #[export]
    pub fn f(&self) {}
}

fn main() {
    let _ = C;
}
