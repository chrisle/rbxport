//! A port a test drives the webviews through, on macOS.
//!
//! Windows has `RBXPORT_DEVTOOLS_PORT` (`crate::browser_args`): `WebView2`
//! speaks the Chrome `DevTools` Protocol, and `scripts/e2e-win/` attaches
//! Playwright to it. `WKWebView` speaks nothing of the kind, so this is the
//! equivalent: with `RBXPORT_TEST_PORT` set, a debug build listens on
//! `127.0.0.1:<port>` and runs the JavaScript a test sends in the webview it
//! names, the way `scripts/e2e-mac/` drives the compiled app without
//! touching the mouse or the keyboard.
//!
//! The protocol is one JSON object per line, each answered with one:
//!
//! - `{"id":1,"window":"main","js":"<expression>"}` evaluates the expression
//!   (awaited, so a promise is fine) and answers
//!   `{"id":1,"ok":true,"value":<json>}` or
//!   `{"id":1,"ok":false,"error":"…"}`. The value comes back through the
//!   `test_eval_result` command, since `eval` itself returns nothing.
//! - `{"id":2,"windows":true}` answers with the labels of the open webview
//!   windows, so a test can wait for Preferences or the Sync Manager.
//! - `{"id":3,"menu":"settings"}` fires a native menu item by its id, which
//!   is what its accelerator does when a person presses it — the one thing a
//!   keystroke in the webview cannot reach on macOS.
//!
//! A release build ignores the variable: the gate is `cfg!(debug_assertions)`
//! as well as the variable being set, so nothing ships that listens.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// Names the port. Honoured by a debug build only.
pub const PORT_ENV: &str = "RBXPORT_TEST_PORT";

/// One isolated `WebKit` store shared by all windows of a desktop test run.
/// UUID stores require macOS 14+. Ordinary and release launches ignore this.
#[cfg(target_os = "macos")]
pub fn data_store() -> Option<[u8; 16]> {
    port()?;
    if !rbl_db::test_mode() { return None; }
    let id = std::env::var("RBXPORT_TEST_DATA_STORE").ok()?;
    Some(*uuid::Uuid::parse_str(&id).ok()?.as_bytes())
}

/// How long one evaluation may take before the request is answered with an
/// error. Long enough for a library to load behind an awaited promise.
const TIMEOUT: Duration = Duration::from_secs(30);

/// What one evaluation hands back: the value, or what the page threw.
type Outcome = Result<Value, String>;

/// The evaluations waiting for the page to answer, by the token the wrapper
/// carries. Managed on every launch so the command can always find it; only
/// a listening build ever puts anything in it.
#[derive(Default)]
pub struct TestPort {
    pending: Mutex<HashMap<u64, mpsc::Sender<Outcome>>>,
    next: AtomicU64,
}

/// The port asked for, if this build honours it.
pub fn port() -> Option<u16> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var(PORT_ENV).ok()?.parse().ok()
}

/// Starts listening, on a thread of its own, when a port is asked for.
pub fn start(app: &AppHandle) {
    let Some(port) = port() else { return };
    let app = app.clone();
    if let Err(e) = std::thread::Builder::new().name("test-port".into()).spawn(move || serve(&app, port)) {
        tracing::error!(error = %e, "the test port's thread could not start");
    }
}

fn serve(app: &AppHandle, port: u16) {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(port, error = %e, "the test port could not open");
            return;
        }
    };
    tracing::debug!(port, "test port open");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let app = app.clone();
                let spawned = std::thread::Builder::new()
                    .name("test-port-client".into())
                    .spawn(move || session(&app, stream));
                if let Err(e) = spawned {
                    tracing::error!(error = %e, "a test connection could not be served");
                }
            }
            Err(e) => tracing::warn!(error = %e, "a test connection failed"),
        }
    }
}

/// One request, as the line carries it.
#[derive(Deserialize)]
struct Request {
    id: Value,
    #[serde(default)]
    window: Option<String>,
    #[serde(default)]
    js: Option<String>,
    #[serde(default)]
    windows: bool,
    #[serde(default)]
    layout: bool,
    #[serde(default)]
    menu: Option<String>,
}

/// Answers one connection's requests, in order, until it hangs up.
fn session(app: &AppHandle, stream: TcpStream) {
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(e) => {
            tracing::warn!(error = %e, "a test connection could not be answered");
            return;
        }
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                let id = request.id.clone();
                match handle(app, request) {
                    Ok(value) => json!({ "id": id, "ok": true, "value": value }),
                    Err(error) => json!({ "id": id, "ok": false, "error": error }),
                }
            }
            Err(e) => json!({ "id": Value::Null, "ok": false, "error": format!("not a request: {e}") }),
        };
        if writeln!(writer, "{reply}").is_err() {
            break;
        }
    }
}

fn handle(app: &AppHandle, request: Request) -> Outcome {
    if request.layout {
        let window = app.get_webview_window(crate::MAIN_WINDOW)
            .ok_or_else(|| "no main window for desktop test layout".to_owned())?;
        window.set_size(tauri::LogicalSize::new(1200.0, 800.0)).map_err(|e| e.to_string())?;
        window.set_position(tauri::LogicalPosition::new(500.0, 50.0)).map_err(|e| e.to_string())?;
        return Ok(json!({ "width": 1200, "height": 800, "x": 500, "y": 50 }));
    }
    if request.windows {
        let mut labels: Vec<String> = app.webview_windows().into_keys().collect();
        labels.sort();
        return Ok(json!(labels));
    }
    if let Some(id) = request.menu {
        crate::menu::on_event(app, &id);
        return Ok(Value::Null);
    }
    let Some(js) = request.js else {
        return Err("a request names js, windows or menu".into());
    };
    let label = request.window.unwrap_or_else(|| crate::MAIN_WINDOW.to_owned());
    let Some(window) = app.get_webview_window(&label) else {
        return Err(format!("no window labelled {label}"));
    };
    let port = app.state::<TestPort>();
    let token = port.next.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    port.pending.lock().insert(token, tx);
    let outcome = match window.eval(wrap(token, &js)) {
        Ok(()) => rx
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|_| Err(format!("no answer from the page in {} s", TIMEOUT.as_secs()))),
        Err(e) => Err(format!("the page could not be asked: {e}")),
    };
    port.pending.lock().remove(&token);
    outcome
}

/// The script the page runs: the expression, awaited, its value or its
/// exception sent back through the command with the token.
///
/// `undefined` has no JSON, so it goes as `null`. The newline before the
/// closing bracket keeps a trailing `//` comment from swallowing it.
fn wrap(token: u64, js: &str) -> String {
    format!(
        "(async()=>{{const __id={token};try{{const v=await ({js}\n);\
         window.__TAURI_INTERNALS__.invoke('test_eval_result',{{id:__id,value:JSON.stringify(v===undefined?null:v)}})}}\
         catch(e){{window.__TAURI_INTERNALS__.invoke('test_eval_result',{{id:__id,error:e&&e.message?e.message+'\\n'+(e.stack||''):String(e)}})}}}})()"
    )
}

/// The page answering an evaluation. Test-only: it does nothing unless the
/// test port asked the question, and a token nobody is waiting on is dropped.
#[tauri::command]
// Tauri hands the state over by value; the command owns it.
#[allow(clippy::needless_pass_by_value)]
pub fn test_eval_result(port: tauri::State<'_, TestPort>, id: u64, value: Option<String>, error: Option<String>) {
    let Some(tx) = port.pending.lock().remove(&id) else {
        return;
    };
    let outcome = match (value, error) {
        (_, Some(error)) => Err(error),
        (Some(text), None) => serde_json::from_str(&text).map_err(|e| format!("the page's answer is not JSON: {e}")),
        (None, None) => Ok(Value::Null),
    };
    // The asker gave up (the timeout answered first); nothing to tell.
    let _ = tx.send(outcome);
}
