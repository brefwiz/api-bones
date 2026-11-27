// SPDX-License-Identifier: MIT
use api_bones_sdk::ListCall;
use api_bones_sdk_macros::client;

pub struct Number {
    pub value: i64,
}

struct C;

#[client(surface = "s")]
impl C {
    #[export(sync)]
    pub fn f(&self) -> ListCall<Number> {
        loop {}
    }
}

fn main() {
    let number = Number { value: 0 };
    let _ = (C, number.value);
}
