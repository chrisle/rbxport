//! Binding the two TCP ports a player connects to.
//!
//! A player first asks the port-query service which port the database server
//! is on, then opens a second connection there and speaks the message
//! protocol. Both are blocking threads: a session is one player, and a link
//! network has at most a handful.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};

use crate::{kind, Message, GREETING, PORT_QUERY_REQUEST};

/// A player opens its session the moment it finds us and keeps it, idle
/// between one touch of the browser and the next (captured on a CDJ-3000).
/// rekordbox never hangs up on it; when we did after 30 s of quiet, the
/// deck tore the session down and the list it was showing went with it. So
/// a session lasts until the player ends it, and a player that vanished is
/// caught by TCP keepalive: a probe after this long with nothing heard, then
/// one every `KEEPALIVE_INTERVAL` until the stack gives up.
const KEEPALIVE_AFTER: Duration = Duration::from_secs(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
/// How long a single read may block. Short enough to notice a shutdown.
const READ_TIMEOUT: Duration = Duration::from_millis(200);
/// A session's reassembly buffer never legitimately grows past this.
const MAX_PENDING: usize = 64 * 1024;

/// Opens a session per connection. Implemented over the library index; kept
/// as a trait so the codec and the socket layer can be tested without one.
pub trait Handler: Send + Sync {
    fn open(&self) -> Box<dyn Session>;

    /// Whether the server is up for players: rekordbox starts its database
    /// server after its link is up and its exports are in, so before that
    /// a port query gets no answer. Serving from the start by default.
    fn serving(&self) -> bool {
        true
    }
}

/// One player's conversation. A menu request is answered with a count and
/// the rows come on a later render request, so a session remembers what was
/// asked last.
pub trait Session: Send {
    /// Returns the messages to send back, which may be none.
    fn handle(&mut self, message: &Message) -> Vec<Message>;
}

/// Serves one already-accepted session until the peer closes it or `stop` is set.
pub fn serve_session(
    handler: &Arc<dyn Handler>,
    stream: &mut TcpStream,
    stop: &Arc<AtomicBool>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_nodelay(true)?;
    SockRef::from(&*stream).set_tcp_keepalive(
        &TcpKeepalive::new()
            .with_time(KEEPALIVE_AFTER)
            .with_interval(KEEPALIVE_INTERVAL),
    )?;
    let peer = stream
        .peer_addr()
        .map_or_else(|_| "?".to_owned(), |a| a.to_string());
    tracing::debug!(%peer, "player connected to the database server");

    let mut session = handler.open();
    let mut pending: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    // Both sides open with the same five bytes before any message (measured);
    // ours goes out as soon as the player's has arrived.
    let mut greeted = false;

    while !stop.load(Ordering::Relaxed) {
        match stream.read(&mut chunk) {
            Ok(0) => {
                tracing::debug!(%peer, "player closed its database session");
                return Ok(());
            }
            Ok(len) => {
                tracing::trace!(%peer, len, "database bytes received");
                pending.extend_from_slice(chunk.get(..len).unwrap_or(&[]));
            }
            Err(error) if is_timeout(&error) => continue,
            Err(error) => {
                // A keepalive that went unanswered ends here too, as a
                // timed-out or reset connection.
                return Err(error);
            }
        }

        // A buffer that keeps growing without yielding a message means the
        // peer is not speaking this protocol; drop it rather than grow forever.
        if pending.len() > MAX_PENDING {
            tracing::warn!(%peer, pending = pending.len(), "no message in the bytes received; not this protocol, dropped");
            return Ok(());
        }

        if !greeted {
            if pending.len() < GREETING.len() {
                continue;
            }
            if !pending.starts_with(GREETING) {
                // Not a player; say nothing rather than guess.
                tracing::warn!(%peer, first = %hex(pending.get(..GREETING.len()).unwrap_or(&[])), "not the database greeting; dropped");
                return Ok(());
            }
            pending.drain(..GREETING.len());
            stream.write_all(GREETING)?;
            greeted = true;
            tracing::debug!(%peer, "database greeting exchanged");
        }

        let (messages, used) = Message::decode_all(&pending);
        pending.drain(..used);
        if !pending.is_empty() {
            tracing::trace!(%peer, pending = pending.len(), "partial message held for the next read");
        }
        for message in messages {
            tracing::trace!(
                %peer,
                tx = message.transaction,
                kind = %kind::name(message.kind),
                args = %describe(&message),
                "database request"
            );
            let replies = session.handle(&message);
            tracing::trace!(%peer, tx = message.transaction, replies = replies.len(), "database reply");
            for reply in &replies {
                let bytes = reply.encode();
                tracing::trace!(
                    %peer,
                    tx = reply.transaction,
                    kind = %kind::name(reply.kind),
                    args = %describe(reply),
                    len = bytes.len(),
                    "database message sent"
                );
                stream.write_all(&bytes)?;
            }
        }
    }
    tracing::debug!(%peer, "database session stopped with the server");
    Ok(())
}

/// A message's arguments, one after another, for a log line.
fn describe(message: &Message) -> String {
    let parts: Vec<String> = message
        .arguments
        .iter()
        .map(crate::Argument::describe)
        .collect();
    format!("[{}]", parts.join(", "))
}

/// Bytes as hex pairs, for a log line.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn is_disconnect(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::UnexpectedEof
    )
}

/// The port-query service and the database server, both listening.
#[derive(Debug)]
pub struct Bound {
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
    query: SocketAddr,
    database: SocketAddr,
}

impl Bound {
    /// Binds both listeners and starts accepting.
    ///
    /// Ports are arguments so a test can use ephemeral loopback ports: 12523
    /// belongs to rekordbox whenever it is running.
    pub fn start(
        handler: Arc<dyn Handler>,
        address: IpAddr,
        query_port: u16,
        database_port: u16,
    ) -> io::Result<Self> {
        let query_listener = TcpListener::bind(SocketAddr::new(address, query_port))?;
        let database_listener = TcpListener::bind(SocketAddr::new(address, database_port))?;
        let (query, database) = (
            query_listener.local_addr()?,
            database_listener.local_addr()?,
        );
        tracing::debug!(%query, %database, "database server bound");

        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::with_capacity(2);

        // The port-query service: read the fixed request, answer with the port.
        {
            let stop = Arc::clone(&stop);
            let port = database.port();
            let gate = Arc::clone(&handler);
            threads.push(std::thread::spawn(move || {
                accept_loop(&query_listener, &stop, move |stream| {
                    if !gate.serving() {
                        // rekordbox has no listener at all before its link is
                        // up; the nearest thing here is a silent close.
                        tracing::debug!("port query before the link is up; closed unanswered");
                        return Ok(());
                    }
                    answer_port_query(stream, port)
                });
            }));
        }
        {
            let stop = Arc::clone(&stop);
            let inner = Arc::clone(&stop);
            threads.push(std::thread::spawn(move || {
                accept_loop(&database_listener, &stop, move |stream| {
                    serve_session(&handler, stream, &inner)
                });
            }));
        }

        Ok(Self {
            stop,
            threads,
            query,
            database,
        })
    }

    pub const fn query_address(&self) -> SocketAddr {
        self.query
    }

    pub const fn database_address(&self) -> SocketAddr {
        self.database
    }

    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Unblock the accept loops by connecting to each once.
        for address in [self.query, self.database] {
            drop(TcpStream::connect_timeout(
                &address,
                Duration::from_millis(200),
            ));
        }
        for thread in self.threads.drain(..) {
            drop(thread.join());
        }
        tracing::debug!("database server stopped");
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn accept_loop<F>(listener: &TcpListener, stop: &Arc<AtomicBool>, session: F)
where
    F: Fn(&mut TcpStream) -> io::Result<()> + Send + Clone + 'static,
{
    while !stop.load(Ordering::Relaxed) {
        let (mut stream, peer) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "accept failed on the database server");
                continue;
            }
        };
        if stop.load(Ordering::Relaxed) {
            return;
        }
        tracing::trace!(%peer, "connection accepted");
        let session = session.clone();
        // One thread per player. A link network has at most a handful, and a
        // slow session must not stall the others.
        drop(std::thread::spawn(move || {
            if let Err(error) = session(&mut stream) {
                if is_disconnect(&error) {
                    tracing::debug!(%peer, %error, "peer ended its database session");
                } else {
                    tracing::warn!(%peer, %error, "database session failed");
                }
            }
        }));
    }
}

/// Reads the fixed port-query request and answers with a two-byte port.
fn answer_port_query(stream: &mut TcpStream, port: u16) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let peer = stream
        .peer_addr()
        .map_or_else(|_| "?".to_owned(), |a| a.to_string());
    let mut request = vec![0_u8; PORT_QUERY_REQUEST.len()];
    stream.read_exact(&mut request)?;
    if request != PORT_QUERY_REQUEST {
        // Not the request we know; say nothing rather than guess.
        tracing::warn!(%peer, request = %hex(&request), "not the port query; unanswered");
        return Ok(());
    }
    tracing::debug!(%peer, port, "port query answered");
    stream.write_all(&port.to_be_bytes())
}
