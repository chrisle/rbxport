import type { AnalysisSettings, AnalysisStageDescriptor } from "@/ipc/types";

/** Safe initial built-ins while the host's extension registry is loading. */
export const BUILTIN_ANALYSIS_STAGES: AnalysisStageDescriptor[] = [
  { id: "grid", label: "BPM / Grid", description: "", requires: ["core.audio"], produces: ["core.beat_grid"], defaultEnabled: true, requiresFullTrack: false, revision: "1" },
  { id: "key", label: "KEY", description: "", requires: ["core.audio"], produces: ["core.key"], defaultEnabled: true, requiresFullTrack: false, revision: "1" },
  { id: "waveform", label: "Waveform", description: "", requires: ["core.audio"], produces: ["core.waveform"], defaultEnabled: true, requiresFullTrack: false, revision: "1" },
];
export function selectedStageOrder(settings: AnalysisSettings, descriptors: AnalysisStageDescriptor[]): string[] {
  const selected = new Set([
    ...(settings.bpmGrid ? ["waveform", "grid"] : []),
    ...(settings.key ? ["key"] : []),
    ...(settings.extensionStages ?? []),
  ]);
  const preference = settings.stageOrder ?? ["waveform", "grid", "key"];
  return [...new Set([...preference, ...descriptors.map(stage => stage.id), ...selected])].filter(id => selected.has(id));
}
/** Match the host's explicit-order dependency check; never reorder for the user. */
export function analysisDependencyError(order: string[], descriptors: AnalysisStageDescriptor[]): { stage: string; requirement: string } | null {
  const available = new Set(["core.audio"]);
  for (const id of order) {
    const stage = descriptors.find(stage => stage.id === id);
    if (!stage) return { stage: id, requirement: id };
    for (const required of stage.requires) {
      if (!available.has(required)) return { stage: stage.label, requirement: descriptors.find(stage => stage.produces.includes(required))?.label ?? required };
    }
    stage.produces.forEach(key => available.add(key));
  }
  return null;
}
