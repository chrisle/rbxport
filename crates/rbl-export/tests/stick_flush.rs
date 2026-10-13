//! Every file flush an export makes goes through `rbl_core::durable::flush`.
//!
//! On macOS `File::sync_all` is `fcntl(F_FULLFSYNC)` with no fallback, and
//! exFAT does not implement it [OBS `man 2 fcntl`, macOS 26.6.2]: a
//! `sync_all` on a file staged on an exFAT stick fails with ENOTSUP (os
//! error 45) on an affected Mac (#283). The disk images the removable-media
//! tests mount accept `F_FULLFSYNC`, so they cannot catch a `sync_all` that
//! slips back in; this scans the crates an export runs through instead.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

/// Crates whose code writes files into an export's staging area on the stick.
const CRATES: &[&str] = &["rbl-export", "rbl-onelibrary", "rbl-audio", "rbl-pdb", "rbl-anlz", "rbl-core"];

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn export_code_flushes_files_only_through_durable_flush() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut offenders = Vec::new();
    for name in CRATES {
        let mut files = Vec::new();
        rust_files(&crates.join(name).join("src"), &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for (number, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if code.contains(".sync_all(") || code.contains(".sync_data(") {
                    offenders.push(format!("{}:{}: {}", file.display(), number + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "flush with rbl_core::durable::flush, which falls back to fsync where F_FULLFSYNC is unsupported:\n{}",
        offenders.join("\n")
    );
}
