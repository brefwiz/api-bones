// SPDX-License-Identifier: MIT
//! The declaration digest.

use sha2::{Digest, Sha256};

/// The declaration digest of `files`, each an `(import path, source bytes)`
/// pair of the declaring schema whose source is in the product's repository.
///
/// Files are ordered by import path (bytewise); each contributes its import
/// path, a NUL byte, its source bytes and a NUL byte. Files the build resolves
/// from outside the repository (toolchain-bundled well-known types,
/// registry-resolved dependencies such as `bones/v1/*.proto`) are not passed.
/// The result is `sha256:` followed by the lowercase hex SHA-256.
#[must_use]
pub fn declaration_digest(files: &[(String, Vec<u8>)]) -> String {
    let mut ordered: Vec<&(String, Vec<u8>)> = files.iter().collect();
    ordered.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Sha256::new();
    for (path, source) in ordered {
        hasher.update(path.as_bytes());
        hasher.update([0u8]);
        hasher.update(source);
        hasher.update([0u8]);
    }
    format!("sha256:{}", hex(&hasher.finalize()))
}

/// Lowercase hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0xf)]));
    }
    out
}
