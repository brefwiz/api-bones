//! `HasId` — universal "this resource has an identifier" trait.
//!
//! Transport-agnostic. Lets helpers like `created_under` (in socle) compose
//! a Location header from a route prefix + the resource's id without
//! coupling DTOs to HTTP paths.

use core::fmt::Display;

pub trait HasId {
    type Id: Display;
    fn id(&self) -> &Self::Id;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Thing {
        id: u64,
    }

    impl HasId for Thing {
        type Id = u64;
        fn id(&self) -> &u64 {
            &self.id
        }
    }

    #[test]
    fn id_is_displayable() {
        let t = Thing { id: 42 };
        assert_eq!(format!("{}", t.id()), "42");
    }
}
