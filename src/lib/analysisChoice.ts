import type { AnalysisChoice } from "@/views/analysis/AnalysisDialog";

const KEY = "rbxport.analysisChoice";

/**
 * The Analysis Setting window's own boxes. Add memory cue at first beat is
 * not one: it lives in Preferences only, so it is neither remembered here
 * nor restored from an entry an older version saved with it (#343).
 */
export type RememberedAnalysisChoice = Omit<AnalysisChoice, "firstBeatCue">;

/** What the Analysis Setting window was last confirmed with, if anything. */
export function lastAnalysisChoice(): Partial<RememberedAnalysisChoice> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (typeof raw !== "object" || raw === null) return {};
    const saved = raw as Record<string, unknown>;
    const out: Partial<RememberedAnalysisChoice> = {};
    for (const flag of ["bpmGrid", "key", "highPrecision"] as const) {
      if (typeof saved[flag] === "boolean") out[flag] = saved[flag];
    }
    if (saved.mode === "rekordbox" || saved.mode === "rbxport") out.mode = saved.mode;
    if (typeof saved.minBpm === "number" && typeof saved.maxBpm === "number") {
      out.minBpm = saved.minBpm;
      out.maxBpm = saved.maxBpm;
    }
    return out;
  } catch {
    return {};
  }
}

export function rememberAnalysisChoice(choice: AnalysisChoice): void {
  const { firstBeatCue: _preference, ...remembered } = choice;
  try {
    localStorage.setItem(KEY, JSON.stringify(remembered));
  } catch {
    // Not remembering is only a convenience lost.
  }
}
