//! HATEOAS `Link` and `Links` types for hypermedia-driven API responses.
//!
//! A [`Link`] captures a single hypermedia relation (`rel`, `href`, optional
//! `method`). [`Links`] is an ordered collection of [`Link`] values with
//! helper factory methods for the most common rels.
//!
//! # Example
//!
//! ```rust
//! use api_bones::links::{Link, Links};
//!
//! let links = Links::new()
//!     .push(Link::self_link("/resources/42"))
//!     .push(Link::next("/resources?page=2"));
//!
//! assert_eq!(links.find("self").unwrap().href, "/resources/42");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

/// A single HATEOAS link with a relation type, target URL, and optional HTTP
/// method hint.
///
/// The `rel` field follows the
/// [IANA link relations registry](https://www.iana.org/assignments/link-relations/link-relations.xhtml)
/// where applicable (e.g. `"self"`, `"next"`, `"prev"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct Link {
    /// The link relation type (e.g. `"self"`, `"next"`, `"related"`).
    pub rel: String,

    /// The target URL.
    pub href: String,

    /// Optional HTTP method hint (e.g. `"GET"`, `"POST"`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub method: Option<String>,
}

impl Link {
    /// Create a new `Link` with the given relation and href.
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::new("related", "/other");
    /// assert_eq!(link.rel, "related");
    /// assert_eq!(link.href, "/other");
    /// assert!(link.method.is_none());
    /// ```
    pub fn new(rel: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            rel: rel.into(),
            href: href.into(),
            method: None,
        }
    }

    /// Set the optional HTTP method hint (builder-style).
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::new("create", "/items").method("POST");
    /// assert_eq!(link.method.as_deref(), Some("POST"));
    /// ```
    #[must_use]
    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Construct a `"self"` link.
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::self_link("/resources/42");
    /// assert_eq!(link.rel, "self");
    /// assert_eq!(link.href, "/resources/42");
    /// ```
    pub fn self_link(href: impl Into<String>) -> Self {
        Self::new("self", href)
    }

    /// Construct a `"next"` link (next page in a paginated response).
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::next("/resources?page=2");
    /// assert_eq!(link.rel, "next");
    /// assert_eq!(link.href, "/resources?page=2");
    /// ```
    pub fn next(href: impl Into<String>) -> Self {
        Self::new("next", href)
    }

    /// Construct a `"prev"` link (previous page in a paginated response).
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::prev("/resources?page=1");
    /// assert_eq!(link.rel, "prev");
    /// assert_eq!(link.href, "/resources?page=1");
    /// ```
    pub fn prev(href: impl Into<String>) -> Self {
        Self::new("prev", href)
    }

    /// Construct a `"related"` link.
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::related("/users/42");
    /// assert_eq!(link.rel, "related");
    /// assert_eq!(link.href, "/users/42");
    /// ```
    pub fn related(href: impl Into<String>) -> Self {
        Self::new("related", href)
    }

    /// Construct a `"first"` link (first page of a paginated response).
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::first("/resources?page=1");
    /// assert_eq!(link.rel, "first");
    /// assert_eq!(link.href, "/resources?page=1");
    /// ```
    pub fn first(href: impl Into<String>) -> Self {
        Self::new("first", href)
    }

    /// Construct a `"last"` link (last page of a paginated response).
    ///
    /// ```
    /// use api_bones::links::Link;
    ///
    /// let link = Link::last("/resources?page=10");
    /// assert_eq!(link.rel, "last");
    /// assert_eq!(link.href, "/resources?page=10");
    /// ```
    pub fn last(href: impl Into<String>) -> Self {
        Self::new("last", href)
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// An ordered collection of [`Link`] values.
///
/// Preserves insertion order; duplicate `rel` values are allowed (some APIs
/// return multiple `"related"` links).  Use [`Links::find`] to look up the
/// first link with a given `rel`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct Links(Vec<Link>);

impl Links {
    /// Create an empty `Links` collection.
    ///
    /// ```
    /// use api_bones::links::Links;
    ///
    /// let links = Links::new();
    /// assert!(links.is_empty());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a [`Link`] and return `self` (builder-style).
    ///
    /// ```
    /// use api_bones::links::{Link, Links};
    ///
    /// let links = Links::new()
    ///     .push(Link::self_link("/a"))
    ///     .push(Link::next("/b"));
    /// assert_eq!(links.len(), 2);
    /// ```
    #[must_use]
    pub fn push(mut self, link: Link) -> Self {
        self.0.push(link);
        self
    }

    /// Return the first [`Link`] whose `rel` matches `rel`, if any.
    ///
    /// ```
    /// use api_bones::links::{Link, Links};
    ///
    /// let links = Links::new()
    ///     .push(Link::self_link("/a"))
    ///     .push(Link::next("/b"));
    /// assert_eq!(links.find("next").unwrap().href, "/b");
    /// assert!(links.find("prev").is_none());
    /// ```
    #[must_use]
    pub fn find(&self, rel: &str) -> Option<&Link> {
        self.0.iter().find(|l| l.rel == rel)
    }

    /// Iterate over all contained links.
    pub fn iter(&self) -> impl Iterator<Item = &Link> {
        self.0.iter()
    }

    /// Return the number of links in the collection.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return `true` if the collection contains no links.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<Link>> for Links {
    fn from(v: Vec<Link>) -> Self {
        Self(v)
    }
}

impl IntoIterator for Links {
    type Item = Link;
    type IntoIter = <Vec<Link> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Link construction
    // -----------------------------------------------------------------------

    #[test]
    fn link_new() {
        let l = Link::new("self", "/foo");
        assert_eq!(l.rel, "self");
        assert_eq!(l.href, "/foo");
        assert!(l.method.is_none());
    }

    #[test]
    fn link_with_method() {
        let l = Link::new("create", "/items").method("POST");
        assert_eq!(l.method.as_deref(), Some("POST"));
    }

    #[test]
    fn link_self_link_factory() {
        let l = Link::self_link("/resources/1");
        assert_eq!(l.rel, "self");
        assert_eq!(l.href, "/resources/1");
    }

    #[test]
    fn link_next_factory() {
        let l = Link::next("/resources?page=2");
        assert_eq!(l.rel, "next");
    }

    #[test]
    fn link_prev_factory() {
        let l = Link::prev("/resources?page=0");
        assert_eq!(l.rel, "prev");
    }

    #[test]
    fn link_related_factory() {
        let l = Link::related("/other");
        assert_eq!(l.rel, "related");
    }

    #[test]
    fn link_first_factory() {
        let l = Link::first("/resources?page=1");
        assert_eq!(l.rel, "first");
    }

    #[test]
    fn link_last_factory() {
        let l = Link::last("/resources?page=10");
        assert_eq!(l.rel, "last");
    }

    // -----------------------------------------------------------------------
    // Links collection
    // -----------------------------------------------------------------------

    #[test]
    fn links_new_is_empty() {
        let links = Links::new();
        assert!(links.is_empty());
        assert_eq!(links.len(), 0);
    }

    #[test]
    fn links_push_and_len() {
        let links = Links::new()
            .push(Link::self_link("/a"))
            .push(Link::next("/b"));
        assert_eq!(links.len(), 2);
        assert!(!links.is_empty());
    }

    #[test]
    fn links_find_hit() {
        let links = Links::new()
            .push(Link::self_link("/a"))
            .push(Link::next("/b"));
        let found = links.find("next").unwrap();
        assert_eq!(found.href, "/b");
    }

    #[test]
    fn links_find_miss() {
        let links = Links::new().push(Link::self_link("/a"));
        assert!(links.find("prev").is_none());
    }

    #[test]
    fn links_find_returns_first_match() {
        let links = Links::new()
            .push(Link::related("/x"))
            .push(Link::related("/y"));
        assert_eq!(links.find("related").unwrap().href, "/x");
    }

    #[test]
    fn links_iter() {
        let links = Links::new()
            .push(Link::self_link("/a"))
            .push(Link::next("/b"));
        let hrefs: Vec<&str> = links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, vec!["/a", "/b"]);
    }

    #[test]
    fn links_into_iterator() {
        let links = Links::new().push(Link::self_link("/a"));
        assert_eq!(links.into_iter().count(), 1);
    }

    #[test]
    fn links_from_vec() {
        let v = vec![Link::self_link("/a"), Link::next("/b")];
        let links = Links::from(v);
        assert_eq!(links.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Serde round-trips
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn link_serde_round_trip_without_method() {
        let l = Link::self_link("/resources/1");
        let json = serde_json::to_value(&l).unwrap();
        assert_eq!(json["rel"], "self");
        assert_eq!(json["href"], "/resources/1");
        assert!(json.get("method").is_none());
        let back: Link = serde_json::from_value(json).unwrap();
        assert_eq!(back, l);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn link_serde_round_trip_with_method() {
        let l = Link::new("create", "/items").method("POST");
        let json = serde_json::to_value(&l).unwrap();
        assert_eq!(json["method"], "POST");
        let back: Link = serde_json::from_value(json).unwrap();
        assert_eq!(back, l);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn links_serde_round_trip() {
        let links = Links::new()
            .push(Link::self_link("/a"))
            .push(Link::next("/b"));
        let json = serde_json::to_value(&links).unwrap();
        // transparent: serializes as an array
        assert!(json.is_array());
        assert_eq!(json[0]["rel"], "self");
        assert_eq!(json[1]["rel"], "next");
        let back: Links = serde_json::from_value(json).unwrap();
        assert_eq!(back, links);
    }
}
