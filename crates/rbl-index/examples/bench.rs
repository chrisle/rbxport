//! READ-ONLY benchmark against the installed library.
//! `cargo run --release -p rbl-index --example bench`
#![allow(clippy::pedantic, clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;
use rbl_index::{BpmFilter, SortColumn, TrackFilter, TrackSource, ViewSpec};

fn main() {
    let budget_open_ms = 1000u128;
    let budget_sort_ms = 50u128;
    let budget_search_ms = 30u128;

    let t0 = Instant::now();
    let db = match rbl_db::Library::open_installed_read_only() {
        Ok(db) => db,
        Err(e) => { println!("cannot open library: {e}"); return; }
    };
    let open_ms = t0.elapsed().as_millis();

    let (lib, stats) = rbl_index::load(&db).expect("load");
    let total_ms = t0.elapsed().as_millis();

    println!("== load ==");
    println!("  open+key      {open_ms:>5} ms");
    println!("  read rows     {:>5} ms", stats.read_ms);
    println!("  build index   {:>5} ms", stats.index_ms);
    println!("  TOTAL         {total_ms:>5} ms   (budget {budget_open_ms} ms)  {}",
             if total_ms <= budget_open_ms { "PASS" } else { "FAIL" });
    println!("  tracks {}  playlists {}  memberships {}", stats.tracks, stats.playlists, stats.memberships);
    println!("  histories {}  plays {}", stats.histories, stats.plays);
    println!("  heap  {:.1} MB", stats.heap_bytes as f64 / 1_048_576.0);
    println!("  interners: artists {} albums {} genres {} labels {} keys {}",
             lib.artists.len(), lib.albums.len(), lib.genres.len(), lib.labels.len(), lib.keys.len());

    println!("== sort (whole collection) ==");
    let mut worst_sort = 0u128;
    for (name, col) in [("title", SortColumn::Title), ("artist", SortColumn::Artist),
                        ("bpm", SortColumn::Bpm), ("dateAdded", SortColumn::DateAdded)] {
        for desc in [false, true] {
            let spec = ViewSpec { source: TrackSource::Collection, sort: col, descending: desc, query: String::new(), filter: Default::default() };
            let t = Instant::now();
            let view = lib.open_view(&spec);
            let ms = t.elapsed().as_millis();
            worst_sort = worst_sort.max(ms);
            println!("  {name:<10} {:<5} {ms:>4} ms  ({} rows)", if desc { "desc" } else { "asc" }, view.len());
        }
    }
    println!("  worst {worst_sort} ms (budget {budget_sort_ms} ms)  {}",
             if worst_sort <= budget_sort_ms { "PASS" } else { "FAIL" });

    println!("== search ==");
    let mut worst_search = 0u128;
    for q in ["a", "art", "artbat", "extended mix", "zzzznomatch"] {
        let spec = ViewSpec { source: TrackSource::Collection, sort: SortColumn::Title, descending: false, query: q.to_owned(), filter: Default::default() };
        let t = Instant::now();
        let view = lib.open_view(&spec);
        let ms = t.elapsed().as_millis();
        worst_search = worst_search.max(ms);
        println!("  {q:<14} {ms:>4} ms  ({} hits)", view.len());
    }
    println!("  worst {worst_search} ms (budget {budget_search_ms} ms)  {}",
             if worst_search <= budget_search_ms { "PASS" } else { "FAIL" });

    println!("== filter (whole collection, sorted by title) ==");
    let mut worst_filter = 0u128;
    let keys = lib.key_ids_named(&["Fm".to_owned(), "Abm".to_owned()]);
    let cases: [(&str, TrackFilter, &str); 6] = [
        ("bpm 128", TrackFilter { bpm: Some(BpmFilter { values: vec![128], tolerance_pct: 0, master_bpm_x100: None }), ..Default::default() }, ""),
        ("master ±2%", TrackFilter { bpm: Some(BpmFilter { values: vec![], tolerance_pct: 2, master_bpm_x100: Some(12800) }), ..Default::default() }, ""),
        ("keys Fm Abm", TrackFilter { keys: Some(keys.clone()), ..Default::default() }, ""),
        ("rating 0", TrackFilter { ratings: Some(vec![0]), ..Default::default() }, ""),
        ("colour any", TrackFilter { colors: Some(vec![1, 2, 3, 4, 5, 6, 7, 8]), ..Default::default() }, ""),
        ("all four + query", TrackFilter {
            bpm: Some(BpmFilter { values: vec![126, 128, 130], tolerance_pct: 1, master_bpm_x100: None }),
            keys: Some(keys),
            ratings: Some(vec![0, 1, 2, 3, 4, 5]),
            colors: Some(vec![0]),
            ..Default::default()
        }, "mix"),
    ];
    for (name, filter, query) in cases {
        let spec = ViewSpec { source: TrackSource::Collection, sort: SortColumn::Title, descending: false, query: query.to_owned(), filter };
        let t = Instant::now();
        let view = lib.open_view(&spec);
        let ms = t.elapsed().as_millis();
        worst_filter = worst_filter.max(ms);
        println!("  {name:<18} {ms:>4} ms  ({} hits)", view.len());
    }
    let spec = ViewSpec { source: TrackSource::Collection, sort: SortColumn::Title, descending: false, query: String::new(), filter: Default::default() };
    let t = Instant::now();
    let values = lib.filter_values(&spec);
    let ms = t.elapsed().as_millis();
    worst_filter = worst_filter.max(ms);
    println!("  filter_values     {ms:>4} ms  ({} bpms, {} keys, {} tag categories)", values.bpms.len(), values.keys.len(), values.tags.len());
    println!("  worst {worst_filter} ms (budget {budget_search_ms} ms)  {}",
             if worst_filter <= budget_search_ms { "PASS" } else { "FAIL" });

    println!("== fetch window ==");
    let spec = ViewSpec { source: TrackSource::Collection, sort: SortColumn::Title, descending: false, query: String::new(), filter: Default::default() };
    let view = lib.open_view(&spec);
    let t = Instant::now();
    let mut checksum = 0u64;
    for page in 0..100 {
        for &row in view.window(page * 64, 64) {
            checksum = checksum.wrapping_add(u64::from(row));
        }
    }
    let us = t.elapsed().as_micros();
    println!("  100 pages of 64 rows: {us} us total, {:.1} us/page (budget 5000 us/page)  [checksum {checksum}]",
             us as f64 / 100.0);

    println!("== playlists ==");
    let biggest = (0..lib.playlists().len())
        .max_by_key(|&i| lib.playlists().members.get(i).map_or(0, Vec::len))
        .unwrap_or(0);
    let spec = ViewSpec { source: TrackSource::Playlist(biggest), sort: SortColumn::Title, descending: false, query: String::new(), filter: Default::default() };
    let t = Instant::now();
    let view = lib.open_view(&spec);
    println!("  largest playlist \"{}\": {} tracks, sorted in {} ms",
             lib.playlists().name(biggest), view.len(), t.elapsed().as_millis());
}
