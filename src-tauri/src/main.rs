// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rbxport_lib::appimage_wayland::reexec_with_host_wayland_if_needed();
    if let Some(code) = rbxport_lib::run_elevated_update_helper_if_requested() {
        std::process::exit(code);
    }
    rbxport_lib::run();
}
