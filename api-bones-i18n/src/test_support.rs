// SPDX-License-Identifier: MIT
//! Scratch directories the unit tests write catalog files into.

use std::{env, fs, path::PathBuf};

/// A fresh directory under the system temp directory holding `files`, named by
/// the test module, the test and the process so parallel tests never share one.
pub(crate) fn scratch_dir(module: &str, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = env::temp_dir().join(format!(
        "api-bones-i18n-{module}-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (file, body) in files {
        fs::write(dir.join(file), body).unwrap();
    }
    dir
}
