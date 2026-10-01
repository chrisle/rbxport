//! READ-ONLY: serves the installed library over the link, as the app does
//! with LINK on, and prints the players until Ctrl-C.
//! `cargo run --release -p rbl-link --example serve [interface]`
//!
//! rekordbox must not be running: it holds the ports. Verify from another
//! terminal with `tcpdump -i <interface> udp port 50000` (a keep-alive every
//! 2 s), or with a player on the network.
//!
//! Typing `load <player> <track id>` on stdin tells that player to load the
//! track from us, as dropping it onto the player's deck in the app does.
#![allow(
    clippy::pedantic,
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
// perf-ok: a read-only tool run by hand, not shipped code; printing is its point.

use std::sync::Arc;
use std::time::Duration;

fn main() {
    let wanted = std::env::args().nth(1);
    let interfaces = rbl_link::interfaces();
    let interface = match &wanted {
        Some(name) => interfaces.iter().find(|i| &i.name == name).cloned(),
        None => interfaces.first().cloned(),
    }
    .unwrap_or_else(|| {
        // perf-ok: a tool run by hand.
        eprintln!(
            "interfaces: {:?}",
            interfaces
                .iter()
                .map(|i| format!("{} {}", i.name, i.address))
                .collect::<Vec<_>>()
        );
        panic!("no such interface");
    });

    // perf-ok: a read-only tool run by hand.
    let db = rbl_db::Library::open_installed_read_only().expect("open");
    let (library, _) = rbl_index::load(&db).expect("index"); // perf-ok: a tool run by hand.
    let share_root = db.location().share_root.clone();
    // perf-ok: printing is the point of a tool.
    println!(
        "{} tracks; {} playlists; share {}",
        library.len(),
        library.playlists().len(),
        share_root.display()
    );

    let source = Arc::new(rbl_link::StaticSource {
        library: Arc::new(library),
        share_root,
    });
    let started = std::time::Instant::now();
    // perf-ok: a tool run by hand; failing to bind is its answer.
    let link =
        rbl_link::LinkExport::start(source, interface, rbl_link::Ports::REKORDBOX).expect("start");
    let snapshot = link.snapshot();
    println!(
        "serving as rekordbox on {} ({}) in {:?}; database port {}, query {}, portmap {}",
        snapshot.interface.name,
        snapshot.interface.address,
        started.elapsed(),
        snapshot.database_port,
        link.query_address(),
        link.portmap_address()
    );
    let link = Arc::new(link);
    {
        let link = Arc::clone(&link);
        std::thread::spawn(move || {
            for line in std::io::stdin().lines().map_while(Result::ok) {
                let words: Vec<&str> = line.split_whitespace().collect();
                // perf-ok: a hand tool; printing what each command did is the point.
                match words.as_slice() {
                    // perf-ok: a hand tool; the prints below are its output.
                    ["load", player, track] => match (player.parse::<u8>(), track.parse::<u32>()) {
                        (Ok(player), Ok(track)) => match link.load_track(player, track) {
                            Ok(()) => println!("told player {player} to load {track}"), // perf-ok: tool output
                            Err(error) => println!("load refused: {error}"), // perf-ok: tool output
                        },
                        _ => println!("usage: load <player> <track id>"), // perf-ok: tool output
                    },
                    ["master", "on"] => {
                        link.set_master(true);
                        println!(
                            "master on at {:.2} BPM",
                            f64::from(link.snapshot().master.bpm_x100) / 100.0
                        ); // perf-ok: tool output
                    }
                    ["master", "off"] => {
                        link.set_master(false);
                        println!("master off"); // perf-ok: tool output
                    }
                    ["bpm", value] => match value.parse::<f64>() {
                        Ok(bpm) => {
                            let now = link.snapshot().master.bpm_x100;
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let target = (bpm * 100.0).round() as i32;
                            link.nudge_master(target - i32::from(now));
                            println!(
                                "bpm {:.2}",
                                f64::from(link.snapshot().master.bpm_x100) / 100.0
                            ); // perf-ok: tool output
                        }
                        Err(_) => println!("usage: bpm <value>"), // perf-ok: tool output
                    },
                    ["recycle"] => {
                        if link.take_master_tempo() {
                            println!(
                                "took the master's tempo: {:.2}",
                                f64::from(link.snapshot().master.bpm_x100) / 100.0
                            ); // perf-ok: tool output
                        } else {
                            println!("no player is master"); // perf-ok: tool output
                        }
                    }
                    [] => {}
                    _ => println!(
                        "usage: load <player> <track id> | master on|off | bpm <value> | recycle"
                    ), // perf-ok: tool output
                }
            }
        });
    }
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let players = link.snapshot().players;
        // perf-ok: a hand tool; the periodic print of the players is its point.
        if players.is_empty() {
            println!("no players");
        }
        for p in players {
            // perf-ok: tool output.
            println!(
                "{:?} {} #{} at {}: loaded {:?} playing={} master={} bpm={}",
                p.kind, p.name, p.number, p.address, p.loaded, p.playing, p.master, p.bpm_x100
            );
        }
    }
}
