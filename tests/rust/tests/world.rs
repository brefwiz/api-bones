// SPDX-License-Identifier: MIT
//! The Cucumber World for the shared contract lane.
//!
//! A sibling of `steps/` rather than a member of it: step modules hold
//! glue, and world state shared across them is its own thing.

use connectrpc::ConnectError;
use cucumber::World;

#[derive(Debug, Default, World)]
pub struct RetryWorld {
    pub failure: Option<ConnectError>,
}

impl RetryWorld {
    /// The failure under test, or a panic naming the missing Given.
    pub fn failure(&self) -> &ConnectError {
        self.failure.as_ref().expect("no failure given")
    }
}
