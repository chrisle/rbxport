//! Composable analysis with explicit order, typed artifacts and revision tracking.
//!
//! Extensions are compiled/registered by the host; this does not load arbitrary
//! dynamic libraries. This public module has no dependencies on extension code.
mod builtins;
pub use builtins::{standard_registry, GridStage, KeyStage, WaveformStage, GRID, KEY, WAVEFORM};
use serde::Serialize;
use serde_json::Value;
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub const AUDIO: &str = "core.audio";
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageDescriptor {
    pub id: String,
    pub label: String,
    pub description: String,
    pub requires: Vec<String>,
    pub produces: Vec<String>,
    pub default_enabled: bool,
    pub requires_full_track: bool,
    /// Change whenever this stage's implementation or effective settings change.
    pub revision: String,
}
impl StageDescriptor {
    pub fn new(id: &str, label: &str, requires: &[&str], produces: &[&str]) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: String::new(),
            requires: requires.iter().map(|s| (*s).into()).collect(),
            produces: produces.iter().map(|s| (*s).into()).collect(),
            default_enabled: false,
            requires_full_track: false,
            revision: "1".into(),
        }
    }
}
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum StageError {
    #[error("Unknown analysis stage: {0}")]
    Unknown(String),
    #[error("Stage registration is invalid: {0}")]
    Registration(String),
    #[error("{stage} requires {artifact} before it can run")]
    Missing { stage: String, artifact: String },
    #[error("Artifact is missing, stale, or has a different type: {0}")]
    Artifact(String),
    #[error("Analysis stage {stage} failed: {message}")]
    Failed { stage: String, message: String },
    #[error("Invalid stage output: {0}")]
    Output(String),
}
/// A stage's outputs are immutable and published together only on success.
pub struct StageOutput {
    pub key: String,
    value: Arc<dyn Any + Send + Sync>,
    pub json: Option<Value>,
}
impl StageOutput {
    pub fn new<T: Any + Send + Sync>(key: &str, value: T) -> Self {
        Self {
            key: key.into(),
            value: Arc::new(value),
            json: None,
        }
    }
    #[must_use]
    pub fn with_json(mut self, value: Value) -> Self {
        self.json = Some(value);
        self
    }
}
pub struct StageContext<'a> {
    pub samples: &'a [f32],
    pub sample_rate: u32,
    pub artifacts: &'a ArtifactStore,
}
pub trait AnalysisStage: Send + Sync {
    fn descriptor(&self) -> StageDescriptor;
    fn run(&self, context: &StageContext<'_>) -> Result<Vec<StageOutput>, StageError>;
}
#[derive(Clone)]
struct Artifact {
    value: Arc<dyn Any + Send + Sync>,
    json: Option<Value>,
    revision: u64,
    inputs: BTreeMap<String, u64>,
    producers: BTreeMap<String, String>,
}
/// Reusable results. Changed input/configuration revisions invalidate consumers.
/// A new audio revision must be supplied whenever samples/rate change.
#[derive(Clone, Default)]
pub struct ArtifactStore {
    values: BTreeMap<String, Artifact>,
    clock: u64,
    audio_revision: Option<String>,
    stage_revisions: BTreeMap<String, String>,
}
impl ArtifactStore {
    pub fn get<T: Any>(&self, key: &str) -> Result<&T, StageError> {
        if !self.is_current(key) {
            return Err(StageError::Artifact(key.into()));
        }
        self.values
            .get(key)
            .and_then(|a| a.value.downcast_ref())
            .ok_or_else(|| StageError::Artifact(key.into()))
    }
    pub fn is_current(&self, key: &str) -> bool {
        self.current(key, &mut BTreeSet::new())
    }
    fn current(&self, key: &str, visiting: &mut BTreeSet<String>) -> bool {
        let Some(value) = self.values.get(key) else {
            return false;
        };
        if !visiting.insert(key.into()) {
            return false;
        }
        let valid = value
            .producers
            .iter()
            .all(|(id, revision)| self.stage_revisions.get(id) == Some(revision))
            && value.inputs.iter().all(|(key, revision)| {
                self.values
                    .get(key)
                    .is_some_and(|a| a.revision == *revision)
                    && self.current(key, visiting)
            });
        visiting.remove(key);
        valid
    }
    /// Seed an externally validated result; inserting it invalidates dependents.
    pub fn insert<T: Any + Send + Sync>(&mut self, key: &str, value: T) {
        self.clock += 1;
        self.values.insert(
            key.into(),
            Artifact {
                value: Arc::new(value),
                json: None,
                revision: self.clock,
                inputs: BTreeMap::new(),
                producers: BTreeMap::new(),
            },
        );
    }
    pub fn json_outputs(&self) -> BTreeMap<String, Value> {
        self.values
            .iter()
            .filter(|(k, _)| self.is_current(k))
            .filter_map(|(k, a)| a.json.clone().map(|v| (k.clone(), v)))
            .collect()
    }
    fn audio(&mut self, revision: &str) {
        if self.audio_revision.as_deref() != Some(revision) {
            self.insert(AUDIO, ());
            self.audio_revision = Some(revision.into());
        }
    }
    fn publish(
        &mut self,
        descriptor: &StageDescriptor,
        outputs: Vec<StageOutput>,
    ) -> Result<(), StageError> {
        let expected: BTreeSet<_> = descriptor.produces.iter().collect();
        let actual: BTreeSet<_> = outputs.iter().map(|o| &o.key).collect();
        if actual != expected || outputs.len() != expected.len() {
            return Err(StageError::Output(descriptor.id.clone()));
        }
        // Capture all dependencies before replacing anything. A transforming
        // stage can consume and replace one key without a self-dependency cycle.
        let mut inputs = BTreeMap::new();
        let mut producers = BTreeMap::new();
        for key in &descriptor.requires {
            let a = self
                .values
                .get(key)
                .ok_or_else(|| StageError::Artifact(key.clone()))?;
            inputs.insert(key.clone(), a.revision);
            producers.extend(a.producers.clone());
        }
        producers.insert(descriptor.id.clone(), descriptor.revision.clone());
        let mut batch = Vec::new();
        for output in outputs {
            let mut dependencies = inputs.clone();
            if dependencies.remove(&output.key).is_some() {
                if let Some(old) = self.values.get(&output.key) {
                    dependencies.extend(old.inputs.clone());
                }
            }
            self.clock += 1;
            batch.push((
                output.key,
                Artifact {
                    value: output.value,
                    json: output.json,
                    revision: self.clock,
                    inputs: dependencies,
                    producers: producers.clone(),
                },
            ));
        }
        self.values.extend(batch);
        Ok(())
    }
}
#[derive(Default)]
pub struct StageRegistry {
    stages: BTreeMap<String, Arc<dyn AnalysisStage>>,
    order: Vec<String>,
}
impl StageRegistry {
    pub fn register(&mut self, stage: impl AnalysisStage + 'static) -> Result<(), StageError> {
        let d = stage.descriptor();
        let outputs: BTreeSet<_> = d.produces.iter().collect();
        if d.id.is_empty()
            || d.revision.is_empty()
            || outputs.is_empty()
            || outputs.len() != d.produces.len()
            || self.stages.contains_key(&d.id)
            || d.produces.iter().any(|s| s.is_empty() || s == AUDIO)
        {
            return Err(StageError::Registration(d.id));
        }
        self.order.push(d.id.clone());
        self.stages.insert(d.id, Arc::new(stage));
        Ok(())
    }
    pub fn descriptors(&self) -> Vec<StageDescriptor> {
        self.order
            .iter()
            .filter_map(|id| self.stages.get(id).map(|s| s.descriptor()))
            .collect()
    }
    fn prepare(&self, store: &mut ArtifactStore, audio_revision: &str) {
        store.audio(audio_revision);
        store.stage_revisions = self
            .stages
            .iter()
            .map(|(id, s)| (id.clone(), s.descriptor().revision))
            .collect();
    }
    /// Preflight the whole requested order. Never silently insert/reorder stages.
    pub fn validate(
        &self,
        plan: &[String],
        store: &ArtifactStore,
        audio_revision: &str,
    ) -> Result<(), StageError> {
        let mut simulated = store.clone();
        self.prepare(&mut simulated, audio_revision);
        for id in plan {
            let stage = self
                .stages
                .get(id)
                .ok_or_else(|| StageError::Unknown(id.clone()))?;
            let d = stage.descriptor();
            for key in &d.requires {
                if !simulated.is_current(key) {
                    return Err(StageError::Missing {
                        stage: id.clone(),
                        artifact: key.clone(),
                    });
                }
            }
            let outputs = d
                .produces
                .iter()
                .map(|key| StageOutput::new(key, ()))
                .collect();
            simulated.publish(&d, outputs)?;
        }
        Ok(())
    }
    /// Executes exactly the requested order. A failure retains prior successful
    /// outputs and the failing stage cannot publish a partial result.
    pub fn run(
        &self,
        plan: &[String],
        samples: &[f32],
        sample_rate: u32,
        audio_revision: &str,
        store: &mut ArtifactStore,
    ) -> Result<Vec<String>, StageError> {
        if sample_rate == 0 || samples.iter().any(|s| !s.is_finite()) {
            return Err(StageError::Failed {
                stage: AUDIO.into(),
                message: "invalid audio input".into(),
            });
        }
        self.validate(plan, store, audio_revision)?;
        self.prepare(store, audio_revision);
        let mut completed = Vec::new();
        for id in plan {
            let stage = self
                .stages
                .get(id)
                .ok_or_else(|| StageError::Unknown(id.clone()))?;
            let d = stage.descriptor();
            let context = StageContext {
                samples,
                sample_rate,
                artifacts: store,
            };
            let output =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stage.run(&context)))
                    .map_err(|_| StageError::Failed {
                    stage: id.clone(),
                    message: "stage panicked".into(),
                })??;
            store.publish(&d, output)?;
            completed.push(id.clone());
        }
        Ok(completed)
    }
}
