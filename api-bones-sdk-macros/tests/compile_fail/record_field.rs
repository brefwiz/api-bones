// SPDX-License-Identifier: MIT
use api_bones_sdk_macros::record;

#[record]
pub struct Number {
    pub id: u8,
}

fn main() {
    let number = Number { id: 0 };
    let _ = number.id;
}
