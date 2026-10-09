//! The host-side activation lease for an XDJ-RX3 Link Export connection.
//!
//! The RX3's rear USB-B connection supplies the network interface and the
//! stock PC-mounted transition. rekordbox additionally sends command `0x50`
//! over the USB-MIDI output about every 200 ms; the firmware gives that
//! command a one-second PC-control lease. This module keeps that lease for
//! exactly as long as LINK is on. It does not manufacture the mounted event:
//! that belongs to the RX3 USB connection driver.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use std::time::Duration;

/// Pioneer command `0x50`, captured from a working RX3 Link Export session.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
const ACTIVATE: [u8; 12] = [
    0xf0, 0x00, 0x40, 0x05, 0x00, 0x00, 0x03, 0x0d, 0x00, 0x50, 0x01, 0xf7,
];
/// Safely inside the firmware's one-second certification expiry.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
const REFRESH_EVERY: Duration = Duration::from_millis(200);

/// An RX3 MIDI output found before the network services are bound.
///
/// Detection and activation are separate so the caller can select and bind
/// the USB network interface before the first activation command is sent.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub struct Detected {
    output: midir::MidiOutput,
    port: midir::MidiOutputPort,
    name: String,
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
pub struct Detected;

/// The refresh worker, owned by the running LINK session.
pub struct Activation {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

/// Finds the RX3's output endpoint without sending anything.
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub fn detect() -> Result<Option<Detected>, String> {
    use midir::MidiOutput;

    let output = MidiOutput::new("rbxport RX3 Link Export")
        .map_err(|error| format!("Could not inspect MIDI outputs for an XDJ-RX3: {error}"))?;
    let mut seen = Vec::new();
    let found = output.ports().into_iter().find_map(|port| {
        let name = output.port_name(&port).ok()?;
        if is_rx3_port_name(&name) {
            return Some((port, name));
        }
        seen.push(name);
        None
    });
    if found.is_none() {
        // What to compare against when an RX3 is plugged in but not found:
        // its output is named by the OS driver, not by this application.
        tracing::debug!(outputs = ?seen, "no XDJ-RX3 MIDI output");
    }
    Ok(found.map(|(port, name)| Detected { output, port, name }))
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
#[allow(clippy::unnecessary_wraps)]
pub fn detect() -> Result<Option<Detected>, String> {
    Ok(None)
}

impl Detected {
    /// Opens the captured USB-MIDI output and starts refreshing its lease.
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    pub fn activate(self) -> Result<Activation, String> {
        let name = self.name;
        let mut connection = self
            .output
            .connect(&self.port, "rbxport RX3 Link Export activation")
            .map_err(|error| format!("Could not open the XDJ-RX3 MIDI output {name:?}: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::Builder::new()
            .name("rx3-link-activation".to_owned())
            .spawn(move || {
                tracing::info!(midi_output = %name, "XDJ-RX3 Link Export activation started");
                while !worker_stop.load(Ordering::Relaxed) {
                    if let Err(error) = connection.send(&ACTIVATE) {
                        tracing::warn!(%error, "XDJ-RX3 Link Export activation stopped: MIDI send failed");
                        return;
                    }
                    std::thread::sleep(REFRESH_EVERY);
                }
                tracing::debug!("XDJ-RX3 Link Export activation stopped");
            })
            .map_err(|error| format!("Could not start the XDJ-RX3 activation worker: {error}"))?;
        Ok(Activation {
            stop,
            worker: Some(worker),
        })
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    pub fn activate(self) -> Result<Activation, String> {
        let _ = self;
        unreachable!("RX3 detection is unavailable on this platform")
    }
}

impl Drop for Activation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            drop(worker.join());
        }
    }
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn is_rx3_port_name(name: &str) -> bool {
    let compact: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_uppercase)
        .collect();
    compact.contains("XDJRX3")
}

#[cfg(all(test, any(target_os = "macos", windows, target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn captured_activation_command_and_interval_stay_exact() {
        assert_eq!(
            ACTIVATE,
            [0xf0, 0x00, 0x40, 0x05, 0x00, 0x00, 0x03, 0x0d, 0x00, 0x50, 0x01, 0xf7]
        );
        assert_eq!(REFRESH_EVERY, Duration::from_millis(200));
    }

    #[test]
    fn rx3_output_names_tolerate_platform_punctuation() {
        assert!(is_rx3_port_name("XDJ-RX3"));
        assert!(is_rx3_port_name("PIONEER DJ XDJ RX3 MIDI"));
        assert!(is_rx3_port_name("XDJRX3 Port 1"));
        // midir's ALSA names are "client:port client-id:port-id".
        assert!(is_rx3_port_name("XDJ-RX3:XDJ-RX3 MIDI 1 24:0"));
        assert!(is_rx3_port_name("Pioneer DJ XDJ-RX3:XDJ-RX3 MIDI 1 20:0"));
        assert!(!is_rx3_port_name("Midi Through:Midi Through Port-0 14:0"));
        assert!(!is_rx3_port_name("XDJ-RX2:XDJ-RX2 MIDI 1 24:0"));
        assert!(!is_rx3_port_name("XDJ-RX2"));
        assert!(!is_rx3_port_name("XDJ-XZ"));
        assert!(!is_rx3_port_name("XDJ-AZ"));
        assert!(!is_rx3_port_name("OPUS-QUAD"));
        assert!(!is_rx3_port_name("CDJ-3000"));
    }
}
