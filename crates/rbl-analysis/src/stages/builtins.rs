//! Public built-ins; registration is the same API third-party stages use.
use super::{
    AnalysisStage, StageContext, StageDescriptor, StageError, StageOutput, StageRegistry, AUDIO,
};
use crate::{key, waveform, AnalysisOptions, TempoResult};
pub const GRID: &str = "core.beat_grid";
pub const KEY: &str = "core.key";
pub const WAVEFORM: &str = "core.waveform";
pub const GRID_STAGE: &str = "grid";
pub const KEY_STAGE: &str = "key";
pub const WAVEFORM_STAGE: &str = "waveform";
pub struct GridStage {
    pub options: AnalysisOptions,
}
impl AnalysisStage for GridStage {
    fn descriptor(&self) -> StageDescriptor {
        let mut d = StageDescriptor::new(GRID_STAGE, "BPM / Grid", &[AUDIO], &[GRID]);
        d.default_enabled = true;
        d.revision = format!("1:{:?}", self.options);
        d
    }
    fn run(&self, c: &StageContext<'_>) -> Result<Vec<StageOutput>, StageError> {
        Ok(vec![StageOutput::new(
            GRID,
            crate::analyse_grid_with(c.samples, c.sample_rate, self.options),
        )])
    }
}
pub struct WaveformStage;
impl AnalysisStage for WaveformStage {
    fn descriptor(&self) -> StageDescriptor {
        let mut d = StageDescriptor::new(WAVEFORM_STAGE, "Waveform", &[AUDIO], &[WAVEFORM]);
        d.default_enabled = true;
        d
    }
    fn run(&self, c: &StageContext<'_>) -> Result<Vec<StageOutput>, StageError> {
        Ok(vec![StageOutput::new(
            WAVEFORM,
            waveform::compute(c.samples, c.sample_rate),
        )])
    }
}
pub struct KeyStage {
    pub options: key::KeyOptions,
    pub rules: Vec<key::Rule>,
}
impl AnalysisStage for KeyStage {
    fn descriptor(&self) -> StageDescriptor {
        let mut required = vec![AUDIO];
        if key::wants_grid(&self.rules) {
            required.push(GRID);
        }
        let mut d = StageDescriptor::new(KEY_STAGE, "KEY", &required, &[KEY]);
        d.default_enabled = true;
        d.revision = format!("1:{:?}:{:?}", self.options, self.rules);
        d
    }
    fn run(&self, c: &StageContext<'_>) -> Result<Vec<StageOutput>, StageError> {
        let mut grid = key::KeyGrid::default();
        if key::wants_grid(&self.rules) {
            let tempo = c.artifacts.get::<TempoResult>(GRID)?;
            grid.beats = tempo
                .beats
                .iter()
                .map(|b| (f64::from(b.time_ms) / 1000., b.beat_number))
                .collect();
            let starts: Vec<_> = grid.beats.iter().map(|b| b.0).collect();
            let frames = crate::downbeat::band_frames(c.samples, c.sample_rate);
            grid.phrase_starts =
                crate::downbeat::grid_phase_from(&frames, c.sample_rate, &starts).phrase_starts;
        }
        let result =
            key::detect_key_with(c.samples, c.sample_rate, self.options, &self.rules, &grid)
                .map(|r| r.key);
        Ok(vec![StageOutput::new(KEY, result)])
    }
}
pub fn standard_registry(options: AnalysisOptions) -> Result<StageRegistry, StageError> {
    let mut r = StageRegistry::default();
    r.register(GridStage { options })?;
    r.register(KeyStage {
        options: options.key,
        rules: key::DEFAULT_RULES.to_vec(),
    })?;
    r.register(WaveformStage)?;
    Ok(r)
}
