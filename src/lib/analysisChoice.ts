import type { AnalysisChoice } from "@/views/analysis/AnalysisDialog";

const KEY = "rbxport.analysisChoice";

/**
 * The Analysis Setting window's own boxes. Add memory cue at first beat is
 * not one: it lives in Preferences only, so it is neither remembered here
 * nor restored from an entry an older version saved with it (#343).
 */
export type RememberedAnalysisChoice = Omit<AnalysisChoice, "firstBeatCue" | "stageOrder">;

/** What the Analysis Setting window was last confirmed with, if anything. */
function readAnalysisChoice(): Partial<RememberedAnalysisChoice> & Pick<AnalysisChoice, "stageOrder"> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (typeof raw !== "object" || raw === null) return {};
    const saved = raw as Record<string, unknown>;
    const out: ReturnType<typeof readAnalysisChoice> = {};
    for (const flag of ["bpmGrid", "key", "highPrecision"] as const) {
      if (typeof saved[flag] === "boolean") out[flag] = saved[flag];
    }
    if (saved.mode === "rekordbox" || saved.mode === "rbxport") out.mode = saved.mode;
    if (typeof saved.minBpm === "number" && typeof saved.maxBpm === "number") {
      out.minBpm = saved.minBpm;
      out.maxBpm = saved.maxBpm;
    }
    for (const key of ["stageOrder", "extensionStages"] as const) {
      const value = saved[key];
      if (Array.isArray(value) && value.every(item => typeof item === "string") && new Set(value).size === value.length) out[key] = value;
    }
    return out;
  } catch {
    return {};
  }
}

export function lastAnalysisChoice(): Partial<RememberedAnalysisChoice> {
  const { stageOrder: _legacyOrder, ...remembered } = readAnalysisChoice();
  return remembered;
}

export function legacyAnalysisOrder(): string[] {
  return readAnalysisChoice().stageOrder ?? [];
}

export function rememberAnalysisChoice(choice: AnalysisChoice): void {
  const { firstBeatCue: _preference, stageOrder: _order, ...remembered } = choice;
  // Keep the old order available until Preferences has saved its own copy.
  const stageOrder = readAnalysisChoice().stageOrder;
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...remembered, stageOrder }));
  } catch {
    // Not remembering is only a convenience lost.
  }
}
