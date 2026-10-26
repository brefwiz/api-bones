// SPDX-License-Identifier: MIT
//! Compiles one scenario-declared contract against the shipped `bones/v1`
//! files, shared by every lane that reads the vocabulary back by reflection.

use protox::{
    Compiler, Error,
    file::{File, FileResolver, GoogleFileResolver},
};

/// The one contract file each scenario declares.
pub const CONTRACT: &str = "contract/v1/contract.proto";

/// The shipped files as `(path, utf8 source)` pairs.
type Shipped = Vec<(&'static str, &'static [u8])>;

struct ShippedAndContract {
    contract: String,
    shipped: Shipped,
}

impl FileResolver for ShippedAndContract {
    fn open_file(&self, name: &str) -> Result<File, Error> {
        if name == CONTRACT {
            return File::from_source(name, &self.contract);
        }
        if let Some((_, bytes)) = self.shipped.iter().find(|(path, _)| *path == name) {
            let source = std::str::from_utf8(bytes).expect("shipped protos are utf8");
            return File::from_source(name, source);
        }
        GoogleFileResolver::new().open_file(name)
    }
}

/// A compiler that resolves the shipped files, the Google well-known files and
/// `contract` as [`CONTRACT`].
pub fn compiler(
    contract: String,
    shipped: impl Iterator<Item = (&'static str, &'static [u8])>,
) -> Compiler {
    Compiler::with_file_resolver(ShippedAndContract {
        contract,
        shipped: shipped.collect(),
    })
}
