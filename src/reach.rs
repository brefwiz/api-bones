// SPDX-License-Identifier: MIT
//! How far a failed call got, and whether sending it again is safe.
//!
//! A client that retries a call can run it twice. [`Reach`] names the three
//! things a failure can prove, and [`Reach::is_replayable`] states the one
//! rule every caller follows: only a call proven never to have reached the
//! server may be replayed. Anything ambiguous is [`Reach::Unanswered`].

use core::fmt;

/// How far a failed call got.
///
/// The variants map one to one onto a proto enum, so the class can cross a
/// process boundary unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reach {
    /// The call is proven never to have reached the server: the connection
    /// never opened, or the transport proves no server processed the request.
    Unreached,
    /// The call may have reached the server, but no answer came back (a reset
    /// or timeout after the request was sent). The server may have acted.
    Unanswered,
    /// The server answered with an error of its own.
    Answered,
}

impl Reach {
    /// Whether the call may be sent again without running it twice.
    ///
    /// True only for [`Reach::Unreached`]. An unanswered call may already
    /// have run, and an answered call was decided by the server.
    #[must_use]
    pub const fn is_replayable(self) -> bool {
        matches!(self, Self::Unreached)
    }

    /// The class's name as it appears in logs and events.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unreached => "unreached",
            Self::Unanswered => "unanswered",
            Self::Answered => "answered",
        }
    }
}

impl fmt::Display for Reach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_unreached_call_is_replayable() {
        assert!(Reach::Unreached.is_replayable());
        assert!(!Reach::Unanswered.is_replayable());
        assert!(!Reach::Answered.is_replayable());
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(Reach::Unreached.as_str(), "unreached");
        assert_eq!(Reach::Unanswered.as_str(), "unanswered");
        assert_eq!(Reach::Answered.as_str(), "answered");
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn display_is_the_name() {
        #[cfg(not(feature = "std"))]
        use alloc::string::ToString;
        assert_eq!(Reach::Unanswered.to_string(), "unanswered");
    }
}
