//! Host registration point for compiled analysis extensions.
//!
//! Keep the public host generic. A downstream distribution can register its own
//! stages here without making rbl-analysis depend on its implementation crate.
use crate::analysis::AnalysisSettings;
use rbl_analysis::stages::{StageError, StageRegistry};
#[allow(
    clippy::unnecessary_wraps,
    reason = "Downstream registrations are fallible"
)]
pub(crate) fn register(
    _registry: &mut StageRegistry,
    _settings: &AnalysisSettings,
) -> Result<(), StageError> {
    Ok(())
}
