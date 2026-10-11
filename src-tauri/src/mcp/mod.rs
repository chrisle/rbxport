//! A Model Context Protocol server, so an AI assistant can search the
//! library and edit playlists while the app is open.
//!
//! Off by default. Advanced › Browse › MCP server turns it on, and the window
//! mirrors that choice here through `script_preferences`, as it mirrors
//! Library Protection. While on, it listens on loopback only
//! (`http://127.0.0.1:<port>/mcp`, Streamable HTTP) and refuses any request
//! that names another host or carries a browser `Origin`, so a web page
//! cannot reach it through DNS rebinding or a cross-site request.
//!
//! The tools (`tools.rs`) are the window's own commands: a search opens an
//! index view as the track list does, and an edit goes through the same
//! guarded edit path, honours Library Protection and the rekordbox-running
//! lock, lands in Undo, and brings the window up to date with
//! `library:changed`.

mod query;
mod tools;

use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use parking_lot::Mutex;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde::Serialize;
use serde_json::Value as Json;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

/// The port listened on unless `RBXPORT_MCP_PORT` names another. Fixed, so
/// an assistant's configuration keeps working from one launch to the next.
pub const DEFAULT_PORT: u16 = 47_200;
/// Overrides [`DEFAULT_PORT`], for a second copy of the app or a port taken.
pub const PORT_ENV: &str = "RBXPORT_MCP_PORT";
/// The path the endpoint is served at; anything else is a 404.
const PATH: &str = "/mcp";
/// Sent to every listener when the server starts, stops or fails to start.
pub const CHANGED_EVENT: &str = "mcp:changed";

/// Whether the server is on, and where.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    /// The preference is on.
    pub enabled: bool,
    /// The port is open.
    pub running: bool,
    /// The endpoint to give an assistant.
    pub url: String,
    /// Why it is not running although it is enabled: the port is taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct Running {
    cancel: CancellationToken,
}

/// The server's lifecycle. Managed on every launch; listens only once the
/// preference says so.
#[derive(Default)]
pub struct McpServer {
    running: Mutex<Option<Running>>,
    status: Mutex<McpStatusDto>,
}

/// The port to listen on.
pub fn port() -> u16 {
    std::env::var(PORT_ENV).ok().and_then(|p| p.parse().ok()).filter(|&p| p != 0).unwrap_or(DEFAULT_PORT)
}

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{PATH}")
}

/// Whether the mirrored preferences turn the server on. Absent is off.
pub fn enabled_in(preferences: &Json) -> bool {
    preferences["advanced"]["mcpServer"].as_bool() == Some(true)
}

pub fn install(app: &AppHandle) {
    let server = Arc::new(McpServer::default());
    *server.status.lock() = McpStatusDto { url: url(port()), ..McpStatusDto::default() };
    app.manage(server);
}

impl McpServer {
    pub fn status(&self) -> McpStatusDto {
        self.status.lock().clone()
    }

    fn set_status<R: Runtime>(&self, app: &AppHandle<R>, status: McpStatusDto) {
        *self.status.lock() = status.clone();
        let _ = app.emit(CHANGED_EVENT, status);
    }

    /// Starts or stops the server to match the preferences. Cheap when
    /// nothing changes, which is nearly every mirror.
    pub fn apply<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>, preferences: &Json) {
        let wanted = enabled_in(preferences);
        let mut running = self.running.lock();
        if wanted == running.is_some() {
            return;
        }
        let port = port();
        if !wanted {
            if let Some(stopping) = running.take() {
                stopping.cancel.cancel();
            }
            drop(running);
            tracing::info!("MCP server stopped");
            self.set_status(app, McpStatusDto { url: url(port), ..McpStatusDto::default() });
            return;
        }
        let cancel = CancellationToken::new();
        *running = Some(Running { cancel: cancel.clone() });
        drop(running);
        self.set_status(app, McpStatusDto { enabled: true, url: url(port), ..McpStatusDto::default() });
        let server = Arc::clone(self);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
            match tokio::net::TcpListener::bind(address).await {
                Ok(listener) => {
                    tracing::info!(%address, "MCP server listening");
                    // Only if this start is still the current one: the
                    // preference may have gone off while the port opened.
                    if !cancel.is_cancelled() {
                        server.set_status(&app, McpStatusDto { enabled: true, running: true, url: url(port), error: None });
                    }
                    serve(listener, &app, cancel).await;
                }
                Err(e) => {
                    tracing::warn!(%address, error = %e, "the MCP server could not open its port");
                    let mut running = server.running.lock();
                    if running.as_ref().is_some_and(|r| r.cancel == cancel) {
                        *running = None;
                    }
                    drop(running);
                    if !cancel.is_cancelled() {
                        server.set_status(
                            &app,
                            McpStatusDto {
                                enabled: true,
                                running: false,
                                url: url(port),
                                error: Some(format!("Port {port} could not be opened: {e}")),
                            },
                        );
                    }
                }
            }
        });
    }
}

/// The transport's settings.
///
/// - Loopback `Host`s only (rmcp's default), and no browser `Origin` at all:
///   an assistant is not a web page, and a page is the one thing that could
///   otherwise reach a loopback port from outside.
/// - Stateless, answering each request with plain JSON: every tool is one
///   request and one answer, so there is nothing a session would carry, and
///   the app keeps no per-client state or open event streams.
fn transport_config(cancel: CancellationToken) -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default()
        .enforce_origin_validation()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_cancellation_token(cancel)
}

async fn serve<R: Runtime>(listener: tokio::net::TcpListener, app: &AppHandle<R>, cancel: CancellationToken) {
    let handler_app = app.clone();
    let service: StreamableHttpService<tools::Tools<R>, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(tools::Tools::new(handler_app.clone())),
        Arc::default(),
        transport_config(cancel.child_token()),
    );
    loop {
        let accepted = tokio::select! {
            () = cancel.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let stream = match accepted {
            Ok((stream, _)) => stream,
            Err(e) => {
                // Out of descriptors, most likely: back off rather than spin.
                tracing::warn!(error = %e, "the MCP server could not accept a connection");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let service = service.clone();
        let cancel = cancel.clone();
        tauri::async_runtime::spawn(async move {
            let respond = hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                let service = service.clone();
                async move {
                    if request.uri().path() != PATH {
                        return Ok::<_, Infallible>(not_found());
                    }
                    Ok(service.handle(request).await)
                }
            });
            let connection = http1::Builder::new().serve_connection(TokioIo::new(stream), respond);
            tokio::pin!(connection);
            tokio::select! {
                result = connection.as_mut() => {
                    if let Err(e) = result {
                        tracing::debug!(error = %e, "an MCP connection ended with an error");
                    }
                }
                () = cancel.cancelled() => {
                    connection.as_mut().graceful_shutdown();
                    let _ = connection.await;
                }
            }
        });
    }
}

type Body = http_body_util::combinators::BoxBody<hyper::body::Bytes, Infallible>;

fn not_found() -> hyper::Response<Body> {
    use http_body_util::BodyExt;
    let mut response = hyper::Response::new(http_body_util::Full::new(hyper::body::Bytes::from_static(b"Not Found")).boxed());
    *response.status_mut() = hyper::StatusCode::NOT_FOUND;
    response
}

/// Whether the server is on, and where, for Preferences.
#[tauri::command]
pub async fn mcp_status(server: State<'_, Arc<McpServer>>) -> Result<McpStatusDto, AppError> {
    Ok(server.status())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_an_explicit_true_turns_the_server_on() {
        assert!(!enabled_in(&json!({})));
        assert!(!enabled_in(&json!({ "advanced": {} })));
        assert!(!enabled_in(&json!({ "advanced": { "mcpServer": "yes" } })));
        assert!(!enabled_in(&json!({ "advanced": { "mcpServer": false } })));
        assert!(enabled_in(&json!({ "advanced": { "mcpServer": true } })));
    }

    #[test]
    fn the_endpoint_is_on_loopback() {
        assert_eq!(url(47_200), "http://127.0.0.1:47200/mcp");
    }

    // ------------------------------------------------- end to end, over HTTP

    use std::io::{Read, Write};
    use tauri::test::MockRuntime;
    use tauri::Listener;

    /// A mock app over a fixture library, as `tests/commands.rs` builds one,
    /// with the server listening on a free loopback port.
    struct Served {
        _dir: tempfile::TempDir,
        app: tauri::App<MockRuntime>,
        address: SocketAddr,
        cancel: CancellationToken,
        changes: Arc<std::sync::Mutex<usize>>,
    }

    fn served() -> Served {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let state = crate::state::AppState::with_backups(dir.path().join("backups"));
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        state.set_library(library, false, db.schema().db_version, 0, location);
        let app = tauri::test::mock_app();
        app.manage(Arc::new(state));
        let bridge = Arc::new(crate::scripting::Bridge::default());
        bridge.mirror_preferences(json!({ "advanced": { "protectLibrary": false, "mcpServer": true } }));
        app.manage(bridge);
        let changes = Arc::new(std::sync::Mutex::new(0));
        let seen = Arc::clone(&changes);
        app.listen("library:changed", move |_| *seen.lock().unwrap() += 1);

        let cancel = CancellationToken::new();
        let handle = app.handle().clone();
        let stop = cancel.clone();
        let listener = tauri::async_runtime::block_on(tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let address = listener.local_addr().unwrap();
        tauri::async_runtime::spawn(async move { serve(listener, &handle, stop).await });
        Served { _dir: dir, app, address, cancel, changes }
    }

    impl Drop for Served {
        fn drop(&mut self) {
            self.cancel.cancel();
        }
    }

    /// One HTTP/1.1 exchange: the status and the body.
    fn exchange(address: SocketAddr, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let mut request = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
            address.port(),
            body.len()
        );
        request.extend(headers.iter().map(|(name, value)| format!("{name}: {value}\r\n")));
        request.push_str("\r\n");
        request.push_str(body);
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let raw = String::from_utf8(raw).unwrap();
        let (head, rest) = raw.split_once("\r\n\r\n").unwrap();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { unchunk(rest) } else { rest.to_owned() };
        (status, body)
    }

    fn unchunk(mut rest: &str) -> String {
        let mut out = String::new();
        while let Some((size, after)) = rest.split_once("\r\n") {
            let size = usize::from_str_radix(size.trim(), 16).unwrap();
            if size == 0 {
                break;
            }
            out.push_str(&after[..size]);
            rest = &after[size + 2..];
        }
        out
    }

    const PROTOCOL: (&str, &str) = ("MCP-Protocol-Version", "2025-06-18");

    fn rpc(served: &Served, method: &str, params: &Json) -> Json {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
        let (status, body) = exchange(served.address, PATH, &[PROTOCOL], &body);
        assert_eq!(status, 200, "{method}: {body}");
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{method}: {e}: {body}"))
    }

    /// A tool's answer: its JSON, or the error text it set `isError` with.
    fn call(served: &Served, tool: &str, arguments: &Json) -> Result<Json, String> {
        let answer = rpc(served, "tools/call", &json!({ "name": tool, "arguments": arguments }));
        let result = &answer["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{tool}: {answer}")).to_owned();
        if result["isError"].as_bool() == Some(true) {
            Err(text)
        } else {
            Ok(serde_json::from_str(&text).unwrap())
        }
    }

    #[test]
    fn an_assistant_initializes_lists_tools_and_searches() {
        let served = served();
        let init = rpc(
            &served,
            "initialize",
            &json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "rbxport", "{init}");
        assert!(init["result"]["instructions"].as_str().is_some_and(|i| i.contains("Camelot")));

        let listed = rpc(&served, "tools/list", &json!({}));
        let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        for wanted in ["search_tracks", "list_playlists", "create_playlist", "update_tracks", "undo_last_edit"] {
            assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
        }

        let page = call(&served, "search_tracks", &json!({ "limit": 3 })).unwrap();
        assert_eq!(page["total"], 40);
        assert_eq!(page["tracks"].as_array().unwrap().len(), 3);
        let stats = call(&served, "get_library_stats", &json!({})).unwrap();
        assert_eq!(stats["tracks"], 40);
        let sessions = call(&served, "list_history_sessions", &json!({})).unwrap();
        assert_eq!(sessions["total"], 2);
        let bad = call(&served, "search_tracks", &json!({ "keys": ["H"] })).unwrap_err();
        assert!(bad.contains("not a key"), "{bad}");
    }

    #[test]
    fn an_edit_lands_in_the_library_and_tells_the_window() {
        let served = served();
        let made = call(&served, "create_playlist", &json!({ "name": "From MCP" })).unwrap();
        let id = made["id"].as_str().unwrap().to_owned();
        assert_ne!(id, "");
        assert!(*served.changes.lock().unwrap() >= 1, "no library:changed");

        let tracks = call(&served, "search_tracks", &json!({ "limit": 2 })).unwrap();
        let ids: Vec<String> = tracks["tracks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect();
        call(&served, "add_tracks_to_playlist", &json!({ "playlist_id": id, "track_ids": ids })).unwrap();
        let listed = call(&served, "get_playlist_tracks", &json!({ "id": id })).unwrap();
        assert_eq!(listed["total"], 2);

        call(&served, "update_tracks", &json!({ "track_ids": [ids[0]], "rating": 4, "color": "Blue", "genre": "Techno" })).unwrap();
        let details = call(&served, "get_track_details", &json!({ "track_ids": [ids[0]] })).unwrap();
        let track = &details["tracks"][0];
        assert_eq!((track["rating"].as_u64(), track["color"].as_str(), track["genre"].as_str()), (Some(4), Some("Blue"), Some("Techno")));

        call(&served, "delete_playlist", &json!({ "playlist_id": id })).unwrap();
        let after = call(&served, "list_playlists", &json!({})).unwrap();
        assert!(after.as_array().unwrap().iter().all(|p| p["id"] != id.as_str()));
        call(&served, "undo_last_edit", &json!({})).unwrap();
        let restored = call(&served, "list_playlists", &json!({})).unwrap();
        assert!(restored.as_array().unwrap().iter().any(|p| p["name"] == "From MCP"));
    }

    #[test]
    fn library_protection_refuses_an_edit_and_allows_a_read() {
        let served = served();
        served
            .app
            .state::<Arc<crate::scripting::Bridge>>()
            .mirror_preferences(json!({ "advanced": { "protectLibrary": true } }));
        let refused = call(&served, "create_playlist", &json!({ "name": "Nope" })).unwrap_err();
        assert!(refused.contains("Library Protection"), "{refused}");
        let status = call(&served, "library_status", &json!({})).unwrap();
        assert_eq!(status["editable"], false);
        assert!(call(&served, "list_playlists", &json!({})).unwrap().as_array().unwrap().iter().all(|p| p["name"] != "Nope"));
    }

    #[test]
    fn a_web_page_or_another_host_or_path_is_turned_away() {
        let served = served();
        let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }).to_string();
        let (status, _) = exchange(served.address, PATH, &[PROTOCOL, ("Origin", "https://evil.example")], &ping);
        assert_eq!(status, 403, "a browser origin was let in");
        let (status, _) = exchange(served.address, "/other", &[PROTOCOL], &ping);
        assert_eq!(status, 404);
        // DNS rebinding: a page's own host name pointed at 127.0.0.1.
        let mut stream = std::net::TcpStream::connect(served.address).unwrap();
        let request = format!(
            "POST {PATH} HTTP/1.1\r\nHost: evil.example\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
             MCP-Protocol-Version: 2025-06-18\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ping}",
            ping.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 403"), "{raw}");
    }
}

