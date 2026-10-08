/** Analysis presets retain high-precision beat placement and key detection. */
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import { Section, Select, Toggle } from "./controls";

export type AnalysisTab = "track";

export const ANALYSIS_TABS: readonly { id: AnalysisTab; label: string }[] = [
  { id: "track", label: "Track Analysis" },
];

export function AnalysisPane(_: { tab: AnalysisTab }) {
  const { preferences, update } = usePreferencesContext();
  const auto = preferences.analysis.auto;
  return (
    <Section title="Track Analysis">
      <p className={styles.analysisIntro}>Choose how tracks are analysed and how much processing power to use.</p>
      <div className={styles.analysisField}>
        <Select label="Analysis mode" caption="Analysis mode" plain value={preferences.analysis.mode}
          choices={[{ value: "rekordbox", label: "Rekordbox · Normal" },
            { value: "rbxport", label: "RBXport (for Electronic Music)" }]}
          onChange={mode => update("analysis", { mode })} />
        <p aria-live="polite">{preferences.analysis.mode === "rekordbox"
          ? "Normal mode with a 70–180 BPM range, high-precision beat placement and key detection."
          : "Aligns beats to kick drums, uses musical phrase changes to find the first beat of each bar, and detects tempo changes and key."}</p>
      </div>
      <div className={styles.analysisField}>
        <Select label="Tracks analysed at once" caption="Tracks analysed at once" plain value={String(preferences.analysis.concurrentTracks)}
          choices={[1, 2, 3, 4].map(n => ({ value: String(n), label: `${n} ${n === 1 ? "track" : "tracks"}${n === 3 ? " (default)" : ""}` }))}
          onChange={value => update("analysis", { concurrentTracks: Number(value) })} />
        <p>Lower this if analysis affects playback. More tracks at once uses more processing power.</p>
      </div>
      <div className={styles.analysisAuto}>
        <Toggle label="Automatic analysis" checked={auto} onChange={(enabled) => update("analysis", { auto: enabled })} />
      </div>
      <div className={styles.analysisAuto}>
        <Toggle label="Add memory cue at first beat" checked={preferences.analysis.firstBeatCue}
          onChange={(firstBeatCue) => update("analysis", { firstBeatCue })} />
      </div>
    </Section>
  );
}
