//! Decodes a recorded database-server session into readable messages.
//!
//! Input: either one text file with a line per TCP segment, `C <hex>` for
//! the player's bytes and `S <hex>` for the server's (from `tshark -z
//! follow,tcp,raw`), or two binary files, the player's stream and the
//! server's. Each direction is reassembled whole before decoding, since a
//! reply spans many segments.
//!
//! `cargo run -q -p rbl-dbserver --example decode_capture -- <stream.txt>`
//! `cargo run -q -p rbl-dbserver --example decode_capture -- <player.bin> <server.bin>`
#![allow(
    clippy::pedantic,
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use rbl_dbserver::{Argument, Message};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn show(a: &Argument) -> String {
    match a {
        Argument::Number(n) => format!("{n:#x}"),
        Argument::String(s) => format!("{s:?}"),
        Argument::Blob(b) => {
            if b.len() <= 24 {
                format!(
                    "blob[{}]={}",
                    b.len(),
                    b.iter().map(|x| format!("{x:02x}")).collect::<String>()
                )
            } else {
                format!(
                    "blob[{}]={}…",
                    b.len(),
                    b[..24]
                        .iter()
                        .map(|x| format!("{x:02x}"))
                        .collect::<String>()
                )
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Each direction is one byte stream; a reply spans many segments and
    // requests interleave with it, so the two are reassembled whole and the
    // messages are then ordered by transaction, which is how they pair up.
    let mut streams: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    match args.as_slice() {
        [text] => {
            for line in std::fs::read_to_string(text).unwrap().lines() {
                let (d, h) = line.split_once(' ').unwrap();
                streams[usize::from(d != "C")].extend(hex(h.trim()));
            }
        }
        [player, server] => {
            streams[0] = std::fs::read(player).unwrap();
            streams[1] = std::fs::read(server).unwrap();
        }
        _ => panic!("usage: <stream.txt> | <player.bin> <server.bin>"),
    }
    let mut lines: Vec<(u64, u8, String)> = Vec::new();
    for (slot, buf) in streams.iter().enumerate() {
        let d = if slot == 0 { 'C' } else { 'S' };
        let mut at = 0;
        // The 5-byte greeting has no magic.
        if buf.len() >= 5
            && buf[0] == 0x11
            && buf.get(1..5) != Some(&rbl_dbserver::MAGIC.to_be_bytes())
        {
            lines.push((
                0,
                slot as u8,
                format!(
                    "{d} raw {}",
                    buf[..5]
                        .iter()
                        .map(|x| format!("{x:02x}"))
                        .collect::<String>()
                ),
            ));
            at = 5;
        }
        let mut seq = 0_u64;
        while at < buf.len() {
            match Message::decode(&buf[at..]) {
                Ok((m, used)) => {
                    seq += 1;
                    let order = if m.transaction == 0xffff_fffe {
                        1
                    } else {
                        u64::from(m.transaction) * 4 + u64::from(slot as u8) * 2
                    };
                    lines.push((
                        order * 1_000_000 + seq,
                        slot as u8,
                        format!(
                            "{d} tx={:#x} kind={:#06x} [{}]",
                            m.transaction,
                            m.kind,
                            m.arguments.iter().map(show).collect::<Vec<_>>().join(", ")
                        ),
                    ));
                    at += used;
                }
                Err(e) => {
                    lines.push((
                        u64::MAX,
                        slot as u8,
                        format!(
                            "{d} ERR {e:?} at {at} of {}: {}",
                            buf.len(),
                            buf[at..(at + 32).min(buf.len())]
                                .iter()
                                .map(|x| format!("{x:02x}"))
                                .collect::<String>()
                        ),
                    ));
                    break;
                }
            }
        }
    }
    lines.sort();
    for (_, _, l) in lines {
        println!("{l}");
    }
}
