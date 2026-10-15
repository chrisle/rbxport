//! Measures the repeated browse windows a LINK player requests while scrolling.
//!
//! `cargo run --release -p rbl-link --example browsebench`
#![allow(clippy::expect_used, clippy::print_stdout, clippy::unwrap_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use rbl_dbserver::catalog::{Catalog, Query, Row, Sort, TrackScope};

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn main() {
    let db = match rbl_db::Library::open_installed_read_only() {
        Ok(db) => db,
        Err(error) => {
            println!("cannot open the library: {error}");
            return;
        }
    };
    let (library, _) = rbl_index::load(&db).expect("load");
    let source = rbl_link::StaticSource { library: Arc::new(library), share_root: db.location().share_root.clone() };
    let catalog = rbl_link::IndexCatalog::new(Arc::new(source), rbl_link::Played::default());
    let query = Query::Tracks { scope: TrackScope::All, sort: Sort::Default };

    let started = Instant::now();
    let rows = catalog.list(&query);
    let cold_menu = started.elapsed();
    let ids: Vec<u32> = rows.iter().filter_map(|row| match row { Row::Track { id, .. } => Some(*id), _ => None }).collect();

    let warm_menu = median((0..50).map(|_| {
        let started = Instant::now();
        let _ = catalog.list(&query);
        started.elapsed()
    }).collect());

    // The player asks small adjacent windows, then commonly scrolls back.
    let offsets: Vec<usize> = (0..40).chain((0..40).rev()).map(|page| page * 12).collect();
    let started = Instant::now();
    for &offset in &offsets {
        for &id in ids.iter().skip(offset).take(12) {
            let _ = catalog.track_row(id, None);
        }
    }
    let cold_rows = started.elapsed();
    let warm_rows = median((0..50).map(|_| {
        let started = Instant::now();
        for &offset in &offsets {
            for &id in ids.iter().skip(offset).take(12) {
                let _ = catalog.track_row(id, None);
            }
        }
        started.elapsed()
    }).collect());

    println!("{} tracks", ids.len());
    println!("menu: cold {cold_menu:?}; warm median {warm_menu:?}");
    println!("960 row requests (forward then reverse): cold median {cold_rows:?}; warm median {warm_rows:?}");
}
