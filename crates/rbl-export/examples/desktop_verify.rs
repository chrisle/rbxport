//! Independent read-back for disposable desktop-test export destinations.
#![allow(clippy::print_stdout)]
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args_os().nth(1).ok_or("usage: desktop_verify <destination>")?);
    let verified = rbl_export::verify(&root)?;
    if !verified.is_ok() { return Err(format!("export verification failed: {verified:?}").into()); }
    let snapshot = rbl_export::snapshot::Snapshot::read(&root)?;
    println!("{}", serde_json::json!({ "verified": true, "snapshot": snapshot }));
    Ok(())
}
