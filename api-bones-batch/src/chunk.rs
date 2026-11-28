// SPDX-License-Identifier: MIT
//! Greedy chunking of batch elements into requests of at most 200 elements
//! and 768 KiB.

use std::collections::HashMap;

/// Most elements one batch request carries.
pub const MAX_ITEMS: usize = 200;
/// Most bytes one encoded batch request carries: the strictest request limit a
/// provider declares, under the fleet's 1 MiB body limit. A smaller chunk is
/// always accepted by a larger server.
pub const MAX_REQUEST_BYTES: usize = 786_432;

/// The distinct ids in first-occurrence order and, for each input position,
/// the index of its distinct id.
#[must_use]
pub fn dedup(ids: &[String]) -> (Vec<String>, Vec<usize>) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut distinct = Vec::new();
    let mut positions = Vec::with_capacity(ids.len());
    for id in ids {
        let next = distinct.len();
        let index = *seen.entry(id.as_str()).or_insert(next);
        if index == next {
            distinct.push(id.clone());
        }
        positions.push(index);
    }
    (distinct, positions)
}

/// The encoded size of one `repeated string` element with a one-byte tag.
#[must_use]
pub fn element_bytes(id: &str) -> usize {
    1 + varint_len(id.len()) + id.len()
}

/// The encoded size of one length-delimited element of `body_len` bytes behind
/// a tag of `tag_len` bytes, as a repeated message field carries it.
#[must_use]
pub fn delimited_bytes(tag_len: usize, body_len: usize) -> usize {
    tag_len + varint_len(body_len) + body_len
}

fn varint_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

/// How elements split into requests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Indexes into the elements, one list per request, in order.
    pub chunks: Vec<Vec<usize>>,
    /// Indexes of elements whose request alone exceeds [`MAX_REQUEST_BYTES`].
    /// They are never sent.
    pub too_large: Vec<usize>,
}

/// Split elements into requests. `template_bytes` is the encoded size of the
/// request with no elements; each of `sizes` is one element's encoded bytes,
/// tag and length included. A chunk closes at [`MAX_ITEMS`] elements, or when
/// the next element would push the request over [`MAX_REQUEST_BYTES`].
#[must_use]
pub fn plan(template_bytes: usize, sizes: impl IntoIterator<Item = usize>) -> Plan {
    let mut plan = Plan::default();
    let mut current: Vec<usize> = Vec::new();
    let mut bytes = template_bytes;
    for (index, cost) in sizes.into_iter().enumerate() {
        if template_bytes.saturating_add(cost) > MAX_REQUEST_BYTES {
            plan.too_large.push(index);
            continue;
        }
        if current.len() == MAX_ITEMS || bytes + cost > MAX_REQUEST_BYTES {
            plan.chunks.push(std::mem::take(&mut current));
            bytes = template_bytes;
        }
        current.push(index);
        bytes += cost;
    }
    if !current.is_empty() {
        plan.chunks.push(current);
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{i:08}")).collect()
    }

    fn id_sizes(ids: &[String]) -> Vec<usize> {
        ids.iter().map(|id| element_bytes(id)).collect()
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let input: Vec<String> = ["b", "a", "b", "c", "a"].map(String::from).to_vec();
        let (distinct, positions) = dedup(&input);
        assert_eq!(distinct, ["b", "a", "c"]);
        assert_eq!(positions, [0, 1, 0, 2, 1]);
    }

    #[test]
    fn four_hundred_fifty_ids_split_200_200_50() {
        let plan = plan(0, id_sizes(&ids(450)));
        let sizes: Vec<usize> = plan.chunks.iter().map(Vec::len).collect();
        assert_eq!(sizes, [200, 200, 50]);
        assert_eq!(plan.chunks[1][0], 200);
    }

    #[test]
    fn two_hundred_one_elements_split_into_two_requests() {
        let plan = plan(0, vec![10; 201]);
        let sizes: Vec<usize> = plan.chunks.iter().map(Vec::len).collect();
        assert_eq!(sizes, [200, 1]);
    }

    #[test]
    fn the_request_limit_is_768_kibibytes() {
        assert_eq!(MAX_REQUEST_BYTES, 768 * 1024);
    }

    #[test]
    fn a_chunk_closes_at_the_byte_limit_before_two_hundred_elements() {
        let plan = plan(0, vec![300_000; 4]);
        let sizes: Vec<usize> = plan.chunks.iter().map(Vec::len).collect();
        assert_eq!(sizes, [2, 2]);
    }

    #[test]
    fn a_chunk_may_fill_the_limit_exactly() {
        let plan = plan(100, [MAX_REQUEST_BYTES - 100, 1]);
        assert_eq!(plan.chunks, [vec![0], vec![1]]);
        let plan = super::plan(100, [MAX_REQUEST_BYTES - 101, 1]);
        assert_eq!(plan.chunks, [vec![0, 1]]);
    }

    #[test]
    fn an_element_that_alone_exceeds_the_limit_is_never_chunked() {
        let plan = plan(0, [10, MAX_REQUEST_BYTES + 1, 10]);
        assert_eq!(plan.too_large, [1]);
        assert_eq!(plan.chunks, [vec![0, 2]]);
    }

    #[test]
    fn the_template_counts_against_the_limit() {
        let plan = plan(MAX_REQUEST_BYTES - 5, [11]);
        assert_eq!(plan.too_large, [0]);
        assert_eq!(plan.chunks, [] as [Vec<usize>; 0]);
    }

    #[test]
    fn no_elements_make_no_chunks() {
        assert_eq!(plan(10, []), Plan::default());
    }

    #[test]
    fn a_delimited_element_counts_tag_length_and_body() {
        assert_eq!(delimited_bytes(1, 5), 7);
        assert_eq!(delimited_bytes(2, 200), 2 + 2 + 200);
        assert_eq!(delimited_bytes(1, 0), 2);
    }
}
