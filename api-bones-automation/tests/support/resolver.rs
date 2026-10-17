// SPDX-License-Identifier: MIT
//! Protox file resolver shared by the compiler tests and the BDD lane: serves
//! one in-memory contract at `PATH`, the vendored bones protos, and the
//! well-known Google protos.

use protox::{
    Error,
    file::{File, FileResolver, GoogleFileResolver},
};

pub const PATH: &str = "orders/v1/orders.proto";

pub struct Resolver(pub String);

impl FileResolver for Resolver {
    fn open_file(&self, name: &str) -> Result<File, Error> {
        if name == PATH {
            return File::from_source(name, &self.0);
        }
        if let Some((_, bytes)) = api_bones_protos::files().find(|(path, _)| *path == name) {
            return File::from_source(name, std::str::from_utf8(bytes).expect("utf-8 proto"));
        }
        GoogleFileResolver::new().open_file(name)
    }
}
