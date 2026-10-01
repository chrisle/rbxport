//! READ-ONLY: captures rows after the one-argument setup used by the RX3.
//! `cargo run -q -p rbl-fakecdj --example legacy_menu_probe -- <host>`
#![allow(clippy::expect_used, clippy::pedantic, clippy::print_stdout)]

use std::error::Error;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use rbl_dbserver::{kind, Argument, Message};
use rbl_fakecdj::database_port;
use serde_json::json;

const CONTEXT: u32 = 0x0101_0301;

struct Client {
    stream: TcpStream,
    pending: Vec<u8>,
    transaction: u32,
}

impl Client {
    fn connect(address: SocketAddr) -> Result<Self, Box<dyn Error>> {
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.write_all(rbl_dbserver::GREETING)?;
        let mut greeting = [0_u8; 5];
        stream.read_exact(&mut greeting)?;
        if greeting != rbl_dbserver::GREETING {
            return Err("unexpected greeting".into());
        }
        let mut client = Self {
            stream,
            pending: Vec::new(),
            transaction: 0,
        };
        client.send(Message::new(
            rbl_dbserver::SETUP_TXID,
            kind::SETUP,
            vec![Argument::Number(1)],
        ))?;
        let _ = client.receive()?;
        Ok(client)
    }

    fn request(&mut self, request: u16, numbers: &[u32]) -> Result<Vec<Message>, Box<dyn Error>> {
        self.transaction = self.transaction.wrapping_add(1);
        self.send(Message::new(
            self.transaction,
            request,
            numbers.iter().copied().map(Argument::Number).collect(),
        ))?;
        self.receive()
    }

    fn send(&mut self, message: Message) -> Result<(), Box<dyn Error>> {
        self.stream.write_all(&message.encode())?;
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<Message>, Box<dyn Error>> {
        let mut chunk = [0_u8; 8192];
        loop {
            let (messages, used) = Message::decode_all(&self.pending);
            if !messages.is_empty() {
                self.pending.drain(..used);
                return Ok(messages);
            }
            let len = self.stream.read(&mut chunk)?;
            if len == 0 {
                return Err("server closed the connection".into());
            }
            self.pending.extend_from_slice(&chunk[..len]);
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let host: IpAddr = std::env::args().nth(1).expect("host").parse()?;
    let port = database_port(SocketAddr::new(host, rbl_dbserver::PORT_QUERY))?;
    let mut client = Client::connect(SocketAddr::new(host, port))?;
    let header = client.request(kind::TRACK_MENU, &[CONTEXT, 0])?;
    let total = header
        .iter()
        .find(|message| message.kind == kind::MENU_HEADER)
        .and_then(|message| match message.arguments.get(1) {
            Some(Argument::Number(value)) => Some(*value),
            _ => None,
        })
        .ok_or("track menu returned no header")?;
    let mut messages = client.request(kind::RENDER, &[CONTEXT, 0, 3, 0, total, 0x0c, 1, 0])?;
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
        messages = client.receive()?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "host": host.to_string(),
            "database_port": port,
            "setup_arguments": 1,
            "total_rows": total,
            "rows": rows.iter().map(|row| json!({
                "argument_count": row.len(),
                "arguments": row.iter().map(|argument| match argument {
                    Argument::Number(value) => json!({"type": "number", "value": value}),
                    Argument::String(value) => json!({"type": "string", "value": value}),
                    Argument::Blob(value) => json!({"type": "blob", "length": value.len()}),
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }))?
    );
    Ok(())
}
