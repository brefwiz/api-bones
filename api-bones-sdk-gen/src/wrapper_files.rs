// SPDX-License-Identifier: MIT
//! Writing generated wrapper files: the files are written under the package
//! directory only after the snapshot already tracked there has been read back.

use std::path::Path;

use anyhow::{Context, anyhow};
use api_bones_sdk_model::L2Methods;

/// One generated file: its path under the package directory and its text.
pub(crate) type File = (String, String);

/// Writes `files` under `out`.
///
/// A snapshot already in `out` must be a readable document of the current
/// schema: a snapshot of another schema, or one missing a field, is refused
/// before anything is written, so a stale tracked file is never silently
/// replaced.
///
/// # Errors
///
/// An I/O error, or the tracked `l2-methods.json` is not a readable snapshot.
pub(crate) fn write(out: &Path, files: &[File]) -> anyhow::Result<()> {
    let tracked = out.join("l2-methods.json");
    if tracked.is_file() {
        let text = std::fs::read_to_string(&tracked)
            .with_context(|| format!("cannot read {}", tracked.display()))?;
        L2Methods::from_json(&text).map_err(|error| {
            anyhow!(
                "the tracked {} is unreadable: {error}; remove it to regenerate",
                tracked.display()
            )
        })?;
    }
    for (path, text) in files {
        let target = out.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, text)
            .with_context(|| format!("cannot write {}", target.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sdk-files-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        dir
    }

    #[test]
    fn files_are_written_under_their_nested_paths() {
        let out = scratch("nested");
        let files = vec![
            ("a.txt".to_owned(), "a".to_owned()),
            ("src/deep/b.txt".to_owned(), "b".to_owned()),
        ];
        write(&out, &files).expect("written");
        assert_eq!(std::fs::read_to_string(out.join("a.txt")).expect("a"), "a");
        assert_eq!(
            std::fs::read_to_string(out.join("src/deep/b.txt")).expect("b"),
            "b"
        );
    }

    #[test]
    fn an_unreadable_tracked_snapshot_is_refused_before_anything_is_written() {
        let out = scratch("stale");
        std::fs::write(out.join("l2-methods.json"), "{\"schema\":\"other\"}").expect("a snapshot");
        let error = write(&out, &[("new.txt".to_owned(), "x".to_owned())]).expect_err("refused");
        assert!(error.to_string().contains("is unreadable"), "{error}");
        assert!(!out.join("new.txt").exists());
    }

    #[test]
    fn a_readable_tracked_snapshot_is_replaced() {
        let out = scratch("fresh");
        let snapshot = L2Methods {
            schema: "api-bones.l2-methods/v1".to_owned(),
            service: "svc".to_owned(),
            crate_name: "krate".to_owned(),
            package: "pkg".to_owned(),
            methods: Vec::new(),
        }
        .to_json();
        std::fs::write(out.join("l2-methods.json"), &snapshot).expect("a snapshot");
        write(&out, &[("l2-methods.json".to_owned(), "{}".to_owned())]).expect("written");
        assert_eq!(
            std::fs::read_to_string(out.join("l2-methods.json")).expect("read"),
            "{}"
        );
    }
}
