//! Throwaway: make a library the app's way under a given folder.
#![allow(clippy::pedantic, clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)]
fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let mut sources = rbl_db::locate::Sources::under(&root);
    sources.default_dir = root.join("rekordbox");
    let plan = rbl_db::new_library::plan_with(&sources).unwrap().unwrap();
    let made = rbl_db::new_library::create(&plan).unwrap();
    println!("{}", made.master_db.display());
}
