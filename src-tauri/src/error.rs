//! The single error type that crosses the IPC boundary.
//!
//! Commands never panic into the webview: `run_command` catches unwinds and
//! converts them here (docs/pre-release/PLAN.md §Stability).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The library is open read-only (rekordbox is running, or the schema is unknown).
    ReadOnly,
    /// Rekordbox's database or a required file could not be found.
    NotFound,
    /// A file exists but could not be parsed.
    Malformed,
    /// The user cancelled a long-running job.
    Cancelled,
    /// A bug in our code; the detail is a crash-log reference, not a message for the user.
    Internal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub kind: ErrorKind,
    /// Shown to the user. Says what went wrong and what to do about it.
    pub message: String,
    /// Developer detail; surfaced only in the log pane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), detail: None }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        tracing::error!(error.detail = %detail, "internal error");
        crate::sentry::capture_internal_error();
        Self::new(ErrorKind::Internal, "Something went wrong inside rbxport.").with_detail(detail)
    }
}

impl core::fmt::Display for AppError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

/// Runs a command body, converting a panic into an `AppError` instead of
/// tearing down the app. Requires `panic = "unwind"` (see the root Cargo.toml).
pub fn run_command<T, F>(name: &str, f: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T> + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let what = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            tracing::error!(command = name, panic = %what, "command panicked");
            Err(AppError::internal(format!("{name} panicked: {what}")))
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_panicking_command_becomes_an_error() {
        let err = run_command("boom", || -> AppResult<()> { panic!("kaboom") }).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Internal);
        assert!(err.detail.unwrap().contains("kaboom"));
    }

    #[test]
    fn a_normal_command_passes_its_value_through() {
        assert_eq!(run_command("ok", || Ok(7)).unwrap(), 7);
    }

    #[test]
    fn an_internal_error_keeps_its_explanation() {
        let err = AppError::internal("the database was unavailable");
        assert_eq!(err.message, "Something went wrong inside rbxport.");
        assert_eq!(err.detail.as_deref(), Some("the database was unavailable"));
    }
}
