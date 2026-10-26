//! Registration/order/transaction/revision contracts, without any private stage.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::pedantic
)]
use rbl_analysis::stages::*;
use std::sync::{Arc, Mutex};
struct NumberStage {
    id: &'static str,
    required: Vec<&'static str>,
    output: &'static str,
    value: u32,
    revision: &'static str,
    fail: bool,
    trace: Arc<Mutex<Vec<String>>>,
}
impl AnalysisStage for NumberStage {
    fn descriptor(&self) -> StageDescriptor {
        let mut d = StageDescriptor::new(self.id, self.id, &self.required, &[self.output]);
        d.revision = self.revision.into();
        d
    }
    fn run(&self, c: &StageContext<'_>) -> Result<Vec<StageOutput>, StageError> {
        self.trace.lock().unwrap().push(self.id.into());
        if self.fail {
            return Err(StageError::Failed {
                stage: self.id.into(),
                message: "fixture".into(),
            });
        }
        let sum = self
            .required
            .iter()
            .filter(|k| **k != AUDIO)
            .map(|k| c.artifacts.get::<u32>(k).copied())
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .sum::<u32>();
        Ok(vec![StageOutput::new(self.output, self.value + sum)])
    }
}
fn stage(
    id: &'static str,
    requires: Vec<&'static str>,
    output: &'static str,
    value: u32,
    trace: Arc<Mutex<Vec<String>>>,
) -> NumberStage {
    NumberStage {
        id,
        required: requires,
        output,
        value,
        revision: "1",
        fail: false,
        trace,
    }
}
#[test]
fn requested_order_is_real_and_preflight_does_not_partially_run() {
    let trace = Arc::new(Mutex::new(vec![]));
    let mut r = StageRegistry::default();
    r.register(stage("producer", vec![AUDIO], "grid", 4, trace.clone()))
        .unwrap();
    r.register(stage("consumer", vec!["grid"], "output", 2, trace.clone()))
        .unwrap();
    let mut store = ArtifactStore::default();
    assert!(r
        .run(
            &["consumer".into(), "producer".into()],
            &[0.1],
            44100,
            "a",
            &mut store
        )
        .is_err());
    assert!(trace.lock().unwrap().is_empty());
    r.run(
        &["producer".into(), "consumer".into()],
        &[0.1],
        44100,
        "a",
        &mut store,
    )
    .unwrap();
    assert_eq!(*trace.lock().unwrap(), ["producer", "consumer"]);
    assert_eq!(*store.get::<u32>("output").unwrap(), 6);
}
#[test]
fn replacing_grid_invalidates_consumers_but_keeps_unrelated_results() {
    let trace = Arc::new(Mutex::new(vec![]));
    let mut r = StageRegistry::default();
    r.register(stage("grid", vec![AUDIO], "grid", 4, trace.clone()))
        .unwrap();
    r.register(stage("consumer", vec!["grid"], "output", 2, trace.clone()))
        .unwrap();
    r.register(stage("independent", vec![AUDIO], "key", 3, trace))
        .unwrap();
    let mut s = ArtifactStore::default();
    r.run(
        &["grid".into(), "consumer".into(), "independent".into()],
        &[0.1],
        44100,
        "a",
        &mut s,
    )
    .unwrap();
    r.run(&["grid".into()], &[0.1], 44100, "a", &mut s).unwrap();
    assert!(!s.is_current("output"));
    assert!(s.is_current("key"));
    r.run(&["consumer".into()], &[0.1], 44100, "a", &mut s)
        .unwrap();
    assert!(s.is_current("output"));
    assert!(r
        .validate(&["consumer".into()], &s, "different audio")
        .is_err());
}
#[test]
fn configuration_changes_invalidate_reused_results() {
    let trace = Arc::new(Mutex::new(vec![]));
    let mut r = StageRegistry::default();
    r.register(stage("producer", vec![AUDIO], "grid", 4, trace.clone()))
        .unwrap();
    r.register(stage("consumer", vec!["grid"], "output", 2, trace.clone()))
        .unwrap();
    let mut s = ArtifactStore::default();
    r.run(
        &["producer".into(), "consumer".into()],
        &[0.1],
        44100,
        "a",
        &mut s,
    )
    .unwrap();
    let mut changed = StageRegistry::default();
    let mut p = stage("producer", vec![AUDIO], "grid", 4, trace.clone());
    p.revision = "new settings";
    changed.register(p).unwrap();
    changed
        .register(stage("consumer", vec!["grid"], "output", 2, trace))
        .unwrap();
    assert!(changed.validate(&["consumer".into()], &s, "a").is_err());
}
#[test]
fn failed_stage_retains_prior_success() {
    let trace = Arc::new(Mutex::new(vec![]));
    let mut r = StageRegistry::default();
    r.register(stage("first", vec![AUDIO], "a", 1, trace.clone()))
        .unwrap();
    let mut fail = stage("fail", vec!["a"], "b", 2, trace);
    fail.fail = true;
    r.register(fail).unwrap();
    let mut s = ArtifactStore::default();
    assert!(r
        .run(&["first".into(), "fail".into()], &[0.1], 44100, "a", &mut s)
        .is_err());
    assert_eq!(*s.get::<u32>("a").unwrap(), 1);
    assert!(!s.is_current("b"));
}
#[test]
fn a_transform_can_replace_its_input_without_a_cycle() {
    let trace = Arc::new(Mutex::new(vec![]));
    let mut r = StageRegistry::default();
    r.register(stage("producer", vec![AUDIO], "grid", 4, trace.clone()))
        .unwrap();
    r.register(stage("transform", vec!["grid"], "grid", 2, trace))
        .unwrap();
    let mut s = ArtifactStore::default();
    r.run(
        &["producer".into(), "transform".into()],
        &[0.1],
        44100,
        "a",
        &mut s,
    )
    .unwrap();
    assert_eq!(*s.get::<u32>("grid").unwrap(), 6);
}
#[test]
fn key_dependencies_follow_the_selected_rules() {
    let normal = KeyStage {
        options: Default::default(),
        rules: rbl_analysis::key::DEFAULT_RULES.to_vec(),
    };
    assert!(!normal.descriptor().requires.contains(&GRID.into()));
    let aware = KeyStage {
        options: Default::default(),
        rules: vec![rbl_analysis::key::Rule::BassVote {
            weight: 1.,
            source: rbl_analysis::key::BassSource::Downbeats,
        }],
    };
    assert!(aware.descriptor().requires.contains(&GRID.into()));
}
#[test]
fn duplicate_registration_is_rejected() {
    let trace = Arc::new(Mutex::new(vec![]));
    let mut r = StageRegistry::default();
    r.register(stage("same", vec![AUDIO], "a", 1, trace.clone()))
        .unwrap();
    assert!(r
        .register(stage("same", vec![AUDIO], "b", 1, trace))
        .is_err());
}

#[test]
fn independent_grid_stage_matches_the_default_analyser() {
    let rate = 8000;
    let mut samples = vec![0_f32; 8 * rate];
    for beat in 0..16 {
        let start = beat * rate / 2;
        for i in 0..160 {
            samples[start + i] = (-(i as f32) / 35.).exp();
        }
    }
    let options = rbl_analysis::AnalysisPreset::Rbxport.options();
    let baseline = rbl_analysis::analyse_with(&samples, rate as u32, options);
    let grid = rbl_analysis::analyse_grid_with(&samples, rate as u32, options);
    assert_eq!(grid.beats.len(), baseline.tempo.beats.len());
    assert_eq!(grid.bpm, baseline.tempo.bpm);
    assert_eq!(grid.first_beat_secs, baseline.tempo.first_beat_secs);
    assert!(grid
        .beats
        .iter()
        .zip(&baseline.tempo.beats)
        .all(|(a, b)| a.time_ms == b.time_ms && a.beat_number == b.beat_number));
}
