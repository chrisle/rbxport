# Extending analysis

`rbl-analysis::stages` is a public Rust registration API. A host registers
compiled stages; the runner executes exactly the user's selected order. It
never silently adds prerequisites or moves a stage. For example, independent
key analysis can run before BPM/grid, while a stage consuming the beat grid
must run after its producer. The default key rules consume only audio; a
`KeyStage` configured with beat-aware rules declares the additional dependency.

## Write and register a stage

Implement `AnalysisStage` in your own crate. Return a `StageDescriptor` with a
unique namespaced ID, human-readable label/description, required artifact keys,
produced keys, and an implementation/settings revision. In `run`, read PCM and
sample rate from `StageContext`, read typed prerequisites with
`context.artifacts.get::<T>(key)`, and return all declared outputs. Use
`StageOutput::new(key, typed_value).with_json(value)` for results the UI or JSON
consumers need. Set `requires_full_track` when analysing only an initial buffer
would produce an incorrect result.

```rust,ignore
let mut registry = rbl_analysis::stages::standard_registry(options)?;
registry.register(MyMetricsStage::new(settings))?;
let plan = vec!["key".into(), "grid".into(), "example.metrics".into()];
let mut results = rbl_analysis::stages::ArtifactStore::default();
registry.run(&plan, &samples, sample_rate, &audio_revision, &mut results)?;
let metrics = results.get::<MyMetrics>("example.metrics")?;
```

The host owns the audio revision: change it whenever the PCM or sample rate
changes. Stage revisions must change with algorithm or effective settings.
Replacing a result invalidates downstream results; unrelated results stay
available. Failed stages publish nothing, and earlier successful stages remain
in the store. Typed outputs are in memory, and only explicitly supplied JSON
outputs are exported. This API does not discover or load dynamic libraries.

## Register an option in the desktop UI

Add the extension crate as a dependency of the **host**, then register its stage
in `src-tauri/src/analysis_extensions.rs`. The core analysis crate remains
independent of extension implementations. The hook receives `AnalysisSettings`;
use `stage_settings[stage_id]` for extension configuration, validating it before
constructing the stage.

The `analysis_stages` IPC command returns descriptors. The Analysis Setting
dialog automatically displays registered extension checkboxes and an ordered
list with move buttons. `default_enabled` supplies the first-run selection;
a remembered explicit selection wins. Dependency errors disable confirmation.
Labels/descriptions pass through the host's translation function.

Requests carry `extensionStages`, `stageOrder`, and optional `stageSettings`.
The backend validates the entire plan before decoding or writing library data.
It returns `completedStages` and namespaced `analysisArtifacts` JSON in the
analysis result; extension artifacts are not written into rekordbox ANLZ tags.
The existing BPM/grid and key persistence paths continue to own those writes.
Extensions that need persistent storage should provide a separate host storage
adapter rather than changing rekordbox's file format.

Calls omitting explicit order and extensions retain the existing optimized
parallel analysis path. Ordered calls run stages sequentially. Desktop decoding
retains a bounded head for analysis while drawing the entire track; a selected
full-track extension refuses a track exceeding that buffer rather than
reporting an incomplete result as complete.

See `crates/rbl-analysis/tests/stages.rs` for registration, execution order,
transaction, revision invalidation and grid compatibility examples.
