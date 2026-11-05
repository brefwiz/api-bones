// SPDX-License-Identifier: MIT
//! Greedy chunking of ids into requests of at most 200 ids and 1 MiB.

use std::collections::HashMap;

/// Most ids one batch request carries.
pub const MAX_ITEMS: usize = 200;
/// Most bytes one encoded batch request carries.
pub const MAX_REQUEST_BYTES: usize = 1_048_576;

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

fn varint_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

/// How distinct ids split into requests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Indexes into the distinct ids, one list per request, in order.
    pub chunks: Vec<Vec<usize>>,
    /// Indexes of ids whose request alone exceeds [`MAX_REQUEST_BYTES`]. They
    /// are never sent.
    pub too_large: Vec<usize>,
}

/// Split `ids` into requests. `template_bytes` is the encoded size of the
/// request with no ids. A chunk closes at [`MAX_ITEMS`] ids, or when the next
/// id would push the request over [`MAX_REQUEST_BYTES`].
#[must_use]
pub fn plan(template_bytes: usize, ids: &[String]) -> Plan {
    let mut plan = Plan::default();
    let mut current: Vec<usize> = Vec::new();
    let mut bytes = template_bytes;
    for (index, id) in ids.iter().enumerate() {
        let cost = element_bytes(id);
        if template_bytes + cost > MAX_REQUEST_BYTES {
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

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let input: Vec<String> = ["b", "a", "b", "c", "a"].map(String::from).to_vec();
        let (distinct, positions) = dedup(&input);
        assert_eq!(distinct, ["b", "a", "c"]);
        assert_eq!(positions, [0, 1, 0, 2, 1]);
    }

    #[test]
    fn four_hundred_fifty_ids_split_200_200_50() {
        let plan = plan(0, &ids(450));
        let sizes: Vec<usize> = plan.chunks.iter().map(Vec::len).collect();
        assert_eq!(sizes, [200, 200, 50]);
        assert_eq!(plan.chunks[1][0], 200);
    }

    #[test]
    fn a_chunk_closes_at_one_mebibyte_before_two_hundred_ids() {
        let big: Vec<String> = (0..4)
            .map(|i| format!("{i}{}", "x".repeat(400_000)))
            .collect();
        let plan = plan(0, &big);
        let sizes: Vec<usize> = plan.chunks.iter().map(Vec::len).collect();
        assert_eq!(sizes, [2, 2]);
    }

    #[test]
    fn an_id_that_alone_exceeds_the_limit_is_never_chunked() {
        let mut input = ids(2);
        input.insert(1, "y".repeat(MAX_REQUEST_BYTES));
        let plan = plan(0, &input);
        assert_eq!(plan.too_large, [1]);
        assert_eq!(plan.chunks, [vec![0, 2]]);
    }

    #[test]
    fn the_template_counts_against_the_limit() {
        let plan = plan(MAX_REQUEST_BYTES - 5, &["abcdefghij".to_owned()]);
        assert_eq!(plan.too_large, [0]);
        assert_eq!(plan.chunks, [] as [Vec<usize>; 0]);
    }

    #[test]
    fn no_ids_make_no_chunks() {
        assert_eq!(plan(10, &[]), Plan::default());
    }
}
