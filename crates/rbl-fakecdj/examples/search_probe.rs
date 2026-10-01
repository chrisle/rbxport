//! READ-ONLY: sends one bounded Link Export search and prints every row.
//! `cargo run -q -p rbl-fakecdj --example search_probe -- <host> <text>`
#![allow(clippy::expect_used, clippy::pedantic, clippy::print_stdout)]

use std::error::Error;
use std::net::{IpAddr, SocketAddr};

use rbl_dbserver::{kind, Argument};
use rbl_fakecdj::{database_port, Database};
use serde_json::json;

const CONTEXT: u32 = 0x0101_0301;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let host: IpAddr = args.next().expect("host").parse()?;
    let text = args.next().expect("search text").to_uppercase();
    let port = database_port(SocketAddr::new(host, rbl_dbserver::PORT_QUERY))?;
    let mut db = Database::connect(SocketAddr::new(host, port), 1)?;
    let text_bytes = u32::try_from(
        text.encode_utf16()
            .count()
            .saturating_add(1)
            .saturating_mul(2),
    )?;
    let request_kind = kind::SEARCH;
    let header = db.request(
        request_kind,
        vec![
            Argument::Number(CONTEXT),
            Argument::Number(0),
            Argument::Number(text_bytes),
            Argument::String(text.clone()),
            Argument::Number(0),
        ],
    )?;
    let total = header
        .iter()
        .find(|message| message.kind == kind::MENU_HEADER)
        .and_then(|message| match message.arguments.get(1) {
            Some(Argument::Number(value)) => Some(*value),
            _ => None,
        })
        .ok_or_else(|| format!("search returned no menu header: {header:?}"))?;
    let take = total.min(64);
    let mut messages = db.request(
        kind::RENDER,
        [CONTEXT, 0, take, 0, total, 0x0c, 1, 0]
            .into_iter()
            .map(Argument::Number)
            .collect(),
    )?;
    let mut rows = Vec::new();
    loop {
        let mut footer = false;
        for message in messages.drain(..) {
            if message.kind == kind::MENU_ITEM {
                rows.push(message.arguments);
            } else if message.kind == kind::MENU_FOOTER {
                footer = true;
            }
        }
        if footer {
            break;
        }
        messages = db.receive()?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "host": host.to_string(),
            "database_port": port,
            "request_kind": format!("{request_kind:#06x}"),
            "request_arguments": [
                json!({"number": CONTEXT}),
                json!({"number": 0}),
                json!({"number": text_bytes}),
                json!({"string": text}),
                json!({"number": 0}),
            ],
            "total_rows": total,
            "captured_rows": rows.len(),
            "rows": rows.iter().map(|row| row.iter().map(|argument| match argument {
                Argument::Number(value) => json!({"type": "number", "value": value}),
                Argument::String(value) => json!({"type": "string", "value": value}),
                Argument::Blob(value) => json!({"type": "blob", "length": value.len()}),
            }).collect::<Vec<_>>()).collect::<Vec<_>>(),
        }))?
    );
    Ok(())
}
