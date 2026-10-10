//! The main window's configuration as Tauri builds it on each platform.
//!
//! Tauri reads `tauri.conf.json`, then merges `tauri.<platform>.conf.json`
//! over it as an RFC 7396 merge patch. Arrays are replaced, not merged, so a
//! platform file that lists `app.windows` replaces the whole base window. A
//! bare entry there loses the title, the size and `visible: false`, and Tauri
//! fills those in with its defaults: "Tauri App", 800x600, shown before the
//! page has painted. This is how Linux showed "Tauri App" as its title.
//!
//! These checks run on every platform, so the Linux merge is tested on any
//! machine that runs the Rust suite.

#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use serde_json::{json, Value};

const BASE: &str = include_str!("../tauri.conf.json");
const LINUX: &str = include_str!("../tauri.linux.conf.json");

/// The window fields a platform file is allowed to change. Anything else the
/// base file sets must survive the merge unchanged.
const PLATFORM_OVERRIDES: [&str; 4] = [
    "titleBarStyle",
    "hiddenTitle",
    "decorations",
    "dragDropEnabled",
];

/// RFC 7396: objects merge key by key, `null` removes a key, and anything
/// else (including arrays) replaces the target.
fn merge(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = json!({});
    }
    let target = target.as_object_mut().unwrap();
    for (key, value) in patch {
        if value.is_null() {
            target.remove(key);
        } else {
            merge(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

/// The configuration Tauri ends up with on a platform whose file is `platform`.
fn merged(platform: &str) -> Value {
    let mut config: Value = serde_json::from_str(BASE).unwrap();
    let patch: Value = serde_json::from_str(platform).unwrap();
    merge(&mut config, &patch);
    config
}

fn windows(config: &Value) -> Vec<Value> {
    config
        .pointer("/app/windows")
        .and_then(Value::as_array)
        .cloned()
        .expect("app.windows is an array")
}

#[test]
fn the_base_window_is_the_one_titled_rbxport() {
    let base = windows(&merged("{}"));
    assert_eq!(base.len(), 1);
    assert_eq!(base[0].get("title"), Some(&json!("rbxport")));
}

#[test]
fn every_platform_window_keeps_the_title_and_the_base_fields() {
    let base = windows(&merged("{}"))[0].clone();
    let base_fields = base.as_object().unwrap();
    for (name, platform) in [("linux", LINUX)] {
        let merged_windows = windows(&merged(platform));
        assert_eq!(merged_windows.len(), 1, "{name}: one main window");
        let window = merged_windows[0].as_object().unwrap();

        assert_eq!(
            window.get("title"),
            Some(&json!("rbxport")),
            "{name}: window title"
        );
        for (key, value) in base_fields {
            if PLATFORM_OVERRIDES.contains(&key.as_str()) {
                continue;
            }
            assert_eq!(
                window.get(key),
                Some(value),
                "{name}: window field `{key}` is lost or changed by the platform file"
            );
        }
    }
}

#[test]
fn the_linux_window_still_takes_the_linux_overrides() {
    let window = windows(&merged(LINUX))[0].clone();
    assert_eq!(window.get("titleBarStyle"), Some(&json!("Visible")));
    assert_eq!(window.get("hiddenTitle"), Some(&json!(false)));
    assert_eq!(window.get("decorations"), Some(&json!(true)));
    assert_eq!(window.get("dragDropEnabled"), Some(&json!(true)));
}
