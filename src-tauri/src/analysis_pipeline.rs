//! Registry-backed bridge between the stage API and the existing authoring path.
use crate::{
    analysis::AnalysisSettings,
    error::{AppError, AppResult, ErrorKind},
};
use rbl_analysis::{
    key::MusicalKey,
    stages::{self, ArtifactStore, StageRegistry},
    waveform::Waveform,
    Analysis, AnalysisOptions, TempoResult,
};
use std::collections::{BTreeMap, BTreeSet};
fn error(error: impl std::fmt::Display) -> AppError {
    AppError::new(
        ErrorKind::Malformed,
        "The selected analysis order cannot run.",
    )
    .with_detail(error.to_string())
}
pub(crate) fn registry(
    options: &AnalysisOptions,
    settings: &AnalysisSettings,
) -> AppResult<StageRegistry> {
    let mut r = stages::standard_registry(*options).map_err(error)?;
    crate::analysis_extensions::register(&mut r, settings).map_err(error)?;
    Ok(r)
}
pub(crate) fn plan(
    registry: &StageRegistry,
    settings: &AnalysisSettings,
) -> AppResult<Vec<String>> {
    let mut selected = Vec::new();
    if settings.bpm_grid {
        selected.extend(["waveform".into(), "grid".into()]);
    }
    if settings.key {
        selected.push("key".into());
    }
    selected.extend(settings.extension_stages.clone());
    let order = settings
        .stage_order
        .clone()
        .unwrap_or_else(|| selected.clone());
    let expected: BTreeSet<_> = selected.iter().collect();
    let actual: BTreeSet<_> = order.iter().collect();
    if expected != actual || order.len() != actual.len() || selected.len() != expected.len() {
        return Err(error(
            "Selected stages and the execution order must match, without duplicates.",
        ));
    }
    if order.is_empty() {
        return Err(error("Select at least one analysis stage."));
    }
    registry
        .validate(&order, &ArtifactStore::default(), "decoded-track")
        .map_err(error)?;
    Ok(order)
}
pub(crate) struct Completed {
    pub analysis: Analysis,
    pub artifacts: BTreeMap<String, serde_json::Value>,
    pub order: Vec<String>,
}
pub(crate) fn execute(
    registry: &StageRegistry,
    plan: &[String],
    samples: &[f32],
    rate: u32,
    complete: bool,
) -> AppResult<Completed> {
    if !complete
        && registry
            .descriptors()
            .iter()
            .any(|d| d.requires_full_track && plan.contains(&d.id))
    {
        return Err(error("A selected stage requires the complete track; the retained analysis head is too short."));
    }
    let mut store = ArtifactStore::default();
    let order = registry
        .run(plan, samples, rate, "decoded-track", &mut store)
        .map_err(error)?;
    let tempo = store
        .get::<TempoResult>(stages::GRID)
        .cloned()
        .unwrap_or_else(|_| TempoResult::empty());
    let waveform = store
        .get::<Waveform>(stages::WAVEFORM)
        .cloned()
        .unwrap_or_default();
    let key = store
        .get::<Option<MusicalKey>>(stages::KEY)
        .cloned()
        .unwrap_or_default();
    let peak = samples.iter().fold(0_f32, |a, b| a.max(b.abs()));
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "The decoded buffer length fits f64 for an RMS calculation"
    )]
    let rms = if samples.is_empty() {
        0.
    } else {
        (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
            as f32
    };
    Ok(Completed {
        analysis: Analysis {
            tempo,
            key,
            waveform,
            peak,
            rms,
        },
        artifacts: store.json_outputs(),
        order,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "Fixture-only assertions")]
mod tests {
    use super::*;
    use rbl_analysis::stages::{
        AnalysisStage, StageContext, StageDescriptor, StageError, StageOutput, AUDIO, GRID,
    };
    struct Extension;
    impl AnalysisStage for Extension {
        fn descriptor(&self) -> StageDescriptor {
            StageDescriptor::new(
                "example.metrics",
                "Metrics",
                &[AUDIO, GRID],
                &["example.metrics"],
            )
        }
        fn run(&self, _: &StageContext<'_>) -> Result<Vec<StageOutput>, StageError> {
            Ok(vec![StageOutput::new("example.metrics", ())])
        }
    }
    #[test]
    fn selected_order_is_validated_without_reordering() {
        let settings = AnalysisSettings {
            stage_order: Some(vec![
                "key".into(),
                "grid".into(),
                "example.metrics".into(),
                "waveform".into(),
            ]),
            extension_stages: vec!["example.metrics".into()],
            ..AnalysisSettings::default()
        };
        let mut registry =
            registry(&rbl_analysis::AnalysisPreset::Rbxport.options(), &settings).unwrap();
        registry.register(Extension).unwrap();
        assert_eq!(
            plan(&registry, &settings).unwrap(),
            settings.stage_order.clone().unwrap()
        );
        let mut invalid = settings;
        invalid.stage_order.as_mut().unwrap().swap(1, 2);
        assert!(plan(&registry, &invalid).is_err());
    }
    #[test]
    fn full_track_extensions_refuse_truncated_audio() {
        struct FullTrack;
        impl AnalysisStage for FullTrack {
            fn descriptor(&self) -> StageDescriptor {
                let mut d =
                    StageDescriptor::new("example.full", "Full track", &[AUDIO], &["example.full"]);
                d.requires_full_track = true;
                d
            }
            fn run(&self, _: &StageContext<'_>) -> Result<Vec<StageOutput>, StageError> {
                unreachable!("must fail before any stage runs")
            }
        }
        let mut r = StageRegistry::default();
        r.register(FullTrack).unwrap();
        assert!(execute(&r, &["example.full".into()], &[0.; 100], 100, false).is_err());
    }
}
