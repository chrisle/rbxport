//! Keeps the computer and its display from going to sleep on their own while
//! a USB stick is being written.
//!
//! A long sync runs with nobody touching the computer. If the display or the
//! system idles into sleep part-way through, macOS and Windows may power the
//! USB port down or remount the volume under the writes still in flight, and
//! the stick answers "Input/output error" (#661, #662). rekordbox does not let
//! that happen: its start-up (`RekordboxApplication::initialise`, rekordbox
//! 7.2.19 for macOS at `0x1008407ac`) calls JUCE's
//! `Desktop::setScreenSaverEnabled(false)`, whose `ScreenSaverDefeater` holds
//! an `IOKit` `PreventUserIdleDisplaySleep` assertion while rekordbox is in
//! front (`timerCallback` at `0x1027069f0`). An export here holds the same
//! assertion for as long as it writes, whichever app is in front.
//!
//! Sleep the user asks for (closing the lid, the Sleep menu item) is never
//! blocked. Where the platform has no way to ask, or the request fails, the
//! export runs as it did before.

/// Holds the request until dropped.
#[must_use = "the computer may sleep again as soon as this is dropped"]
pub struct KeepAwake {
    inner: platform::Request,
}

impl KeepAwake {
    /// Asks the system to keep itself and the display awake, naming `reason`
    /// where the OS shows who is holding it (`pmset -g assertions`,
    /// `powercfg /requests`).
    pub fn new(reason: &str) -> Self {
        let inner = platform::Request::new(reason);
        if !inner.held() {
            tracing::debug!(reason, "could not keep the computer awake; the export goes ahead");
        }
        Self { inner }
    }

    /// Whether the system accepted the request.
    #[must_use]
    pub fn held(&self) -> bool {
        self.inner.held()
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "IOKit power assertions are C FFI; the assertion id is owned and released once")]
mod platform {
    use objc2_core_foundation::CFString;

    type IOPMAssertionID = u32;
    /// `kIOPMAssertionLevelOn`.
    const LEVEL_ON: u32 = 255;
    /// `kIOReturnSuccess`.
    const SUCCESS: i32 = 0;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: *const CFString,
            level: u32,
            name: *const CFString,
            id: *mut IOPMAssertionID,
        ) -> i32;
        fn IOPMAssertionRelease(id: IOPMAssertionID) -> i32;
    }

    pub(super) struct Request(Option<IOPMAssertionID>);

    impl Request {
        pub(super) fn new(reason: &str) -> Self {
            // `kIOPMAssertionTypePreventUserIdleDisplaySleep`, the assertion
            // rekordbox holds. Keeping the display on keeps the system from
            // idle sleep as well.
            let kind = CFString::from_static_str("PreventUserIdleDisplaySleep");
            let name = CFString::from_str(reason);
            let mut id: IOPMAssertionID = 0;
            // SAFETY: both strings are live CFStrings for the call, and `id`
            // is a valid out pointer.
            let result = unsafe { IOPMAssertionCreateWithName(&raw const *kind, LEVEL_ON, &raw const *name, &raw mut id) };
            Self((result == SUCCESS).then_some(id))
        }

        pub(super) fn held(&self) -> bool {
            self.0.is_some()
        }
    }

    impl Drop for Request {
        fn drop(&mut self) {
            if let Some(id) = self.0.take() {
                // SAFETY: `id` came from a successful create and is released
                // exactly once.
                unsafe { IOPMAssertionRelease(id) };
            }
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code, reason = "Win32 power requests are FFI; the request handle is owned and closed once")]
mod platform {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::{
            Power::{
                PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired,
                PowerRequestSystemRequired, PowerSetRequest, POWER_REQUEST_TYPE,
            },
            Threading::{POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0},
        },
    };

    /// `POWER_REQUEST_CONTEXT_VERSION`.
    const CONTEXT_VERSION: u32 = 0;
    const KINDS: [POWER_REQUEST_TYPE; 2] = [PowerRequestSystemRequired, PowerRequestDisplayRequired];

    pub(super) struct Request {
        handle: Option<OwnedHandle>,
        set: Vec<POWER_REQUEST_TYPE>,
    }

    impl Request {
        pub(super) fn new(reason: &str) -> Self {
            let mut wide: Vec<u16> = reason.encode_utf16().chain(std::iter::once(0)).collect();
            let context = REASON_CONTEXT {
                Version: CONTEXT_VERSION,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 { SimpleReasonString: wide.as_mut_ptr() },
            };
            // SAFETY: `context` and the string it points to outlive the call,
            // which copies the reason.
            let raw = unsafe { PowerCreateRequest(&raw const context) };
            if raw.is_null() || raw == INVALID_HANDLE_VALUE {
                return Self { handle: None, set: Vec::new() };
            }
            // SAFETY: a handle PowerCreateRequest returned, owned from here on.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            let set = KINDS
                .into_iter()
                // SAFETY: `handle` is a live power request.
                .filter(|kind| unsafe { PowerSetRequest(raw, *kind) } != 0)
                .collect();
            Self { handle: Some(handle), set }
        }

        pub(super) fn held(&self) -> bool {
            !self.set.is_empty()
        }
    }

    impl Drop for Request {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                use std::os::windows::io::AsRawHandle;
                for kind in self.set.drain(..) {
                    // SAFETY: `handle` is still open; each set request is
                    // cleared once before it is closed.
                    unsafe { PowerClearRequest(handle.as_raw_handle(), kind) };
                }
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    /// Linux desktops each have their own inhibitor service; none is asked.
    pub(super) struct Request;

    impl Request {
        pub(super) fn new(_: &str) -> Self {
            Self
        }

        pub(super) fn held(&self) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taking and dropping the request never fails the caller, two exports
    /// can each hold one, and macOS accepts it from any process. (A Windows
    /// service session may refuse the display request, so it is not asserted
    /// there.)
    #[test]
    fn the_request_is_taken_and_given_back() {
        let first = KeepAwake::new("rbxport test: keep awake");
        let second = KeepAwake::new("rbxport test: a second export at the same time");
        if cfg!(target_os = "macos") {
            assert!(first.held() && second.held());
        }
        if cfg!(not(any(target_os = "macos", windows))) {
            assert!(!first.held());
        }
        drop(first);
        drop(second);
    }
}
