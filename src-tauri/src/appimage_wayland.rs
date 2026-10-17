//! Linux `AppImage` launch workaround for a blank window on Wayland (#337).
//!
//! The `AppImage` bundles its build host's `libwayland-client`. On rolling-release
//! distributions (Manjaro, `CachyOS`, Arch) the host's Mesa/EGL needs the host's
//! newer copy, so `WebKitGTK` aborts with
//! `Could not create default EGL display: EGL_BAD_PARAMETER` and shows a blank
//! window. [OBS] Both reporters on #337 start the app with
//! `LD_PRELOAD=/usr/lib/libwayland-client.so`; the same fix is documented for
//! other Tauri `AppImages` (gitbutlerapp/gitbutler#5282).
//!
//! `LD_PRELOAD` only takes effect at process start, so when running from an
//! `AppImage` we re-exec ourselves once with the host's
//! library preloaded. Anything unexpected falls through to a normal start.
//! [UNKNOWN] Not verifiable off Linux; see the PR's Hardware/OS check.

#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
const GUARD: &str = "RBXPORT_WAYLAND_PRELOAD";
#[cfg(target_os = "linux")]
const CANDIDATES: &[&str] = &[
    "/usr/lib/libwayland-client.so.0",
    "/usr/lib64/libwayland-client.so.0",
    "/usr/lib/x86_64-linux-gnu/libwayland-client.so.0",
    "/usr/lib/aarch64-linux-gnu/libwayland-client.so.0",
    "/usr/lib/libwayland-client.so",
];

/// What the environment says about how we were launched.
pub struct LaunchEnv<'a> {
    pub appimage: Option<&'a str>,
    pub desktop_integration: Option<&'a str>,
    pub guard: Option<&'a str>,
    pub ld_preload: Option<&'a str>,
}

/// The `LD_PRELOAD` value to re-exec with, or `None` to start normally.
pub fn preload_value(env: &LaunchEnv, host_lib: Option<&str>) -> Option<String> {
    let set = |v: Option<&str>| v.is_some_and(|s| !s.is_empty());
    if !set(env.appimage) || set(env.guard) {
        return None;
    }
    let lib = host_lib?;
    match env.ld_preload.filter(|s| !s.is_empty()) {
        Some(existing) if existing.contains("libwayland-client") => None,
        Some(existing) => Some(format!("{lib}:{existing}")),
        None => Some(lib.to_owned()),
    }
}

/// Whether to set `DESKTOPINTEGRATION=1` on the re-exec. A value the user
/// already set (even `0`) is kept. [OBS] @schmic on #337: without it the mouse
/// and focus stay broken even with the preload.
pub fn wants_desktop_integration(env: &LaunchEnv) -> bool {
    let set = |v: Option<&str>| v.is_some_and(|s| !s.is_empty());
    set(env.appimage) && !set(env.guard) && env.desktop_integration.is_none()
}

/// Re-exec with the host's `libwayland-client` preloaded when needed. Returns
/// only if no re-exec was needed or it failed.
#[cfg(target_os = "linux")]
pub fn reexec_with_host_wayland_if_needed() {
    use std::os::unix::process::CommandExt;

    let get = |k: &str| std::env::var(k).ok();
    let (appimage, integration, guard, preload) = (
        get("APPIMAGE"),
        get("DESKTOPINTEGRATION"),
        get(GUARD),
        get("LD_PRELOAD"),
    );
    let env = LaunchEnv {
        appimage: appimage.as_deref(),
        desktop_integration: integration.as_deref(),
        guard: guard.as_deref(),
        ld_preload: preload.as_deref(),
    };
    let host_lib = CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned());
    let value = preload_value(&env, host_lib.as_deref());
    let integration = wants_desktop_integration(&env);
    if value.is_none() && !integration {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1)).env(GUARD, "1");
    if let Some(value) = value {
        cmd.env("LD_PRELOAD", value);
    }
    if integration {
        cmd.env("DESKTOPINTEGRATION", "1");
    }
    let err = cmd.exec();
    eprintln!("could not restart with the host libwayland-client: {err}");
}

#[cfg(not(target_os = "linux"))]
pub fn reexec_with_host_wayland_if_needed() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> LaunchEnv<'static> {
        LaunchEnv {
            appimage: Some("/x/rbxport.AppImage"),
            desktop_integration: None,
            guard: None,
            ld_preload: None,
        }
    }
    const LIB: Option<&str> = Some("/usr/lib/libwayland-client.so.0");

    #[test]
    fn preloads_host_library_in_appimage() {
        assert_eq!(preload_value(&env(), LIB).as_deref(), LIB);
    }

    #[test]
    fn preloads_on_x11_too() {
        // #337: Xorg/i3 fails the same way, so no session-type gate.
        assert!(preload_value(&env(), LIB).is_some());
    }

    #[test]
    fn sets_desktop_integration_unless_user_chose() {
        assert!(wants_desktop_integration(&env()));
        let user = LaunchEnv { desktop_integration: Some("0"), ..env() };
        assert!(!wants_desktop_integration(&user));
        let again = LaunchEnv { guard: Some("1"), ..env() };
        assert!(!wants_desktop_integration(&again));
        let plain = LaunchEnv { appimage: None, ..env() };
        assert!(!wants_desktop_integration(&plain));
    }

    #[test]
    fn leaves_other_launches_alone() {
        let not_appimage = LaunchEnv { appimage: None, ..env() };
        let again = LaunchEnv { guard: Some("1"), ..env() };
        let already = LaunchEnv { ld_preload: Some("/usr/lib/libwayland-client.so"), ..env() };
        assert!(preload_value(&not_appimage, LIB).is_none());
        assert!(preload_value(&again, LIB).is_none());
        assert!(preload_value(&already, LIB).is_none());
        assert!(preload_value(&env(), None).is_none());
    }

    #[test]
    fn keeps_an_existing_preload() {
        let e = LaunchEnv { ld_preload: Some("/opt/x.so"), ..env() };
        assert_eq!(
            preload_value(&e, LIB).as_deref(),
            Some("/usr/lib/libwayland-client.so.0:/opt/x.so")
        );
    }
}
