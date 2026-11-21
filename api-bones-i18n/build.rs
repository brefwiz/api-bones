// SPDX-License-Identifier: MIT
//! Embeds this crate's own platform catalogs the way an emitter embeds its
//! own: through the same scanner.

#[path = "src/build.rs"]
mod scan;

fn main() {
    scan::emit_error_catalogs("catalogs/errors");
}
