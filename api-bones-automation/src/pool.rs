// SPDX-License-Identifier: MIT
//! The descriptor pool of the shipped `bones/v1` files, which the manifest
//! and its signed envelope are built in.

use std::sync::OnceLock;

use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, prost::Message};
use protox::{
    Compiler, Error,
    file::{File, FileResolver, GoogleFileResolver},
};

const MANIFEST_PROTO: &str = "bones/v1/automation_manifest.proto";

struct Shipped;

impl FileResolver for Shipped {
    fn open_file(&self, name: &str) -> Result<File, Error> {
        if let Some((_, bytes)) = api_bones_protos::files().find(|(path, _)| *path == name) {
            let source = std::str::from_utf8(bytes).expect("shipped protos are utf8");
            return File::from_source(name, source);
        }
        GoogleFileResolver::new().open_file(name)
    }
}

/// The pool holding `bones/v1/automation_manifest.proto` and everything it
/// imports.
pub(crate) fn shipped_pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let mut compiler = Compiler::with_file_resolver(Shipped);
        compiler
            .open_file(MANIFEST_PROTO)
            .expect("the shipped protos compile");
        compiler.descriptor_pool()
    })
}

/// A shipped message by full name.
pub(crate) fn shipped(name: &str) -> MessageDescriptor {
    shipped_pool()
        .get_message_by_name(name)
        .expect("the shipped protos declare it")
}

/// `message`, re-read as the shipped message `name`: the same bytes, so any
/// extension the other pool did not know stays carried.
pub(crate) fn transcode(message: &DynamicMessage, name: &str) -> DynamicMessage {
    DynamicMessage::decode(shipped(name), message.encode_to_vec().as_slice())
        .expect("a message re-read as its own type decodes")
}
