// SPDX-License-Identifier: MIT
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use api_bones_sdk::{Wire, from_json_str, to_json_string};
use api_bones_sdk_macros::{enumeration, record};

#[enumeration]
#[derive(Debug, PartialEq)]
pub enum Kind {
    Exact,
    LowerBound,
}

#[record]
#[derive(Debug, PartialEq)]
pub struct Inner {
    pub kind: Kind,
}

#[record]
#[derive(Debug, PartialEq)]
pub struct Everything {
    pub flag: bool,
    pub small: i32,
    pub count: u32,
    pub ratio: f64,
    pub label: String,
    pub big: i64,
    pub huge: u64,
    pub bytes: Vec<u8>,
    pub at: SystemTime,
    pub within: Duration,
    pub maybe: Option<String>,
    pub many: Vec<u32>,
    pub by_name: BTreeMap<String, Inner>,
    pub nested: Option<Vec<Inner>>,
}

fn assert_wire<T: Wire>() {}

fn main() {
    assert_wire::<Everything>();
    assert_wire::<Kind>();
    let kind = Kind::LowerBound;
    let text = to_json_string(&kind).expect("encodes");
    assert_eq!(text, "\"lower_bound\"");
    assert_eq!(from_json_str::<Kind>(&text).expect("decodes"), Kind::LowerBound);
    assert!(from_json_str::<Kind>("\"other\"").is_err());
    let inner = Inner { kind: Kind::Exact };
    let text = to_json_string(&inner).expect("encodes");
    assert_eq!(text, "{\"kind\":\"exact\"}");
    assert_eq!(from_json_str::<Inner>(&text).expect("decodes"), inner);
}
