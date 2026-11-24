/** Analysis presets retain high-precision beat placement and key detection. */
import { useEffect, useRef, useState, type PointerEvent } from "react";
import { GripVertical } from "lucide-react";
import { getBackend } from "@/ipc/client";
import { analysisDependencyError, analysisStageOrder } from "@/lib/analysisStages";
import type { AnalysisStageDescriptor } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import { BPM_RANGES } from "@/lib/preferences";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import { Section, Select, Toggle } from "./controls";

export type AnalysisTab = "track" | "key" | "advanced";

export const ANALYSIS_TABS: readonly { id: AnalysisTab; label: string }[] = [
  { id: "track", label: "Track Analysis" },
  { id: "key", label: "Key Analysis" },
  { id: "advanced", label: "Advanced" },
];

export function AnalysisPane({ tab }: { tab: AnalysisTab }) {
  const { preferences, update } = usePreferencesContext();
  const t = useTranslation();
  if (tab === "key") {
    return (
      <Section title={t("Key Analysis")}>
        <div className={styles.analysisSwitch}>
          <Toggle label={t("Analyze key")} checked={preferences.analysis.detectKey}
            onChange={(detectKey) => update("analysis", { detectKey })} />
          <p>{t("Turn off to keep keys from Mixed In Key or another source. RBXport will not overwrite them.")}</p>
        </div>
        <div className={styles.analysisSwitch}>
          <Toggle label={t("Write the value to the ID3 tag")} checked={preferences.analysis.writeKeyTag}
            onChange={(writeKeyTag) => update("analysis", { writeKeyTag })} />
          <p>{t("Store the detected key in the ID3 tag.")}</p>
        </div>
      </Section>
    );
  }
  if (tab === "advanced") return <AnalysisOrder />;
  const auto = preferences.analysis.auto;
  return (
    <Section title={t("Track Analysis")}>
      <p className={styles.analysisIntro}>Choose how tracks are analysed and how much processing power to use.</p>
      <div className={styles.analysisField}>
        <Select label="Analysis mode" caption="Analysis mode" plain value={preferences.analysis.mode}
          choices={[{ value: "rekordbox", label: "Rekordbox · Normal" },
            { value: "rbxport", label: "RBXport (for Electronic Music)" }]}
          onChange={mode => update("analysis", { mode })} />
        <p aria-live="polite">{preferences.analysis.mode === "rekordbox"
          ? "Normal mode: one constant tempo for the whole track, with high-precision beat placement and key detection."
          : "Aligns beats to kick drums, uses musical phrase changes to find the first beat of each bar, and detects tempo changes and key."}</p>
      </div>
      <div className={styles.analysisField}>
        <Select label="BPM Range" caption="BPM Range" plain preserveChoiceLabels value={preferences.analysis.bpmRange}
          choices={BPM_RANGES.map(range => ({ value: range, label: range }))}
          onChange={bpmRange => update("analysis", { bpmRange })} />
      </div>
      <div className={styles.analysisField}>
        <Select label="Tracks analysed at once" caption="Tracks analysed at once" plain value={String(preferences.analysis.concurrentTracks)}
          choices={[1, 2, 3, 4].map(n => ({ value: String(n), label: `${n} ${n === 1 ? "track" : "tracks"}${n === 3 ? " (default)" : ""}` }))}
          onChange={value => update("analysis", { concurrentTracks: Number(value) })} />
        <p>Lower this if analysis affects playback. More tracks at once uses more processing power.</p>
      </div>
      <div className={styles.analysisSwitch}>
        <Toggle label="Automatic analysis" checked={auto} onChange={(enabled) => update("analysis", { auto: enabled })} />
        <p>Analyse tracks as soon as they are imported into the collection.</p>
      </div>
      <div className={styles.analysisSwitch}>
        <Toggle label="Add memory cue at first beat" checked={preferences.analysis.firstBeatCue}
          onChange={(firstBeatCue) => update("analysis", { firstBeatCue })} />
        <p>Adds a memory cue on the first beat of the beat grid, unless the track already has one there.</p>
      </div>
    </Section>
  );
}

function AnalysisOrder() {
  const { preferences, update } = usePreferencesContext();
  const t = useTranslation();
  const [stages, setStages] = useState<AnalysisStageDescriptor[]>([]);
  const [failed, setFailed] = useState(false);
  const drag = useRef<{ pointerId: number; offsetX: number; offsetY: number; width: number } | null>(null);
  const [preview, setPreview] = useState<{ id: string; label: string; x: number; y: number; width: number } | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    void getBackend().then(backend => backend.analysisStages()).then(descriptors => {
      if (active) setStages(descriptors);
    }).catch(() => {
      if (active) setFailed(true);
    });
    return () => { active = false; };
  }, []);

  const savedOrder = analysisStageOrder(preferences.analysis.stageOrder, stages);
  const order = savedOrder.filter(id => stages.some(stage => stage.id === id));
  const dependency = analysisDependencyError(order, stages);

  function move(id: string, target: string) {
    const index = savedOrder.indexOf(target);
    if (id === target || index < 0) return;
    const next = savedOrder.filter(stage => stage !== id);
    next.splice(index, 0, id);
    update("analysis", { stageOrder: next });
  }

  function stageAtPointer(event: PointerEvent<HTMLLIElement>): string | undefined {
    // Capture keeps events on the grabbed row. Hit-test the pointer itself
    // to find the row underneath it.
    const row = document.elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>("[data-analysis-stage]");
    return row?.parentElement === event.currentTarget.parentElement
      ? row?.dataset.analysisStage
      : undefined;
  }

  function endDrag(event: PointerEvent<HTMLLIElement>) {
    if (event.pointerId !== drag.current?.pointerId) return;
    drag.current = null;
    setPreview(null);
    setDropTarget(null);
  }

  return (
    <Section title={t("Analysis order")}>
      {failed ? <p role="alert">{t("Analysis stages could not be loaded.")}</p> : null}
      <p className={styles.analysisIntro}>{t("Drag stages to change the order they run.")}</p>
      <ol className={styles.stageOrder} role="list">
        {order.map((id, index) => {
          const label = t(stages.find(stage => stage.id === id)?.label ?? id);
          return (
            <li
              key={id}
              tabIndex={0}
              aria-keyshortcuts="ArrowUp ArrowDown"
              data-analysis-stage={id}
              data-drag-source={preview?.id === id || undefined}
              data-drop-target={dropTarget === id || undefined}
              onPointerDown={event => {
                if (!event.isPrimary || event.button !== 0 || drag.current !== null) return;
                event.currentTarget.setPointerCapture(event.pointerId);
                const row = event.currentTarget.getBoundingClientRect();
                drag.current = {
                  pointerId: event.pointerId,
                  offsetX: event.clientX - row.left,
                  offsetY: event.clientY - row.top,
                  width: row.width,
                };
              }}
              onPointerMove={event => {
                const active = drag.current;
                if (event.pointerId !== active?.pointerId) return;
                setPreview({
                  id, label, width: active.width,
                  x: event.clientX - active.offsetX,
                  y: event.clientY - active.offsetY,
                });
                const target = stageAtPointer(event);
                setDropTarget(target && target !== id ? target : null);
              }}
              onPointerUp={event => {
                if (event.pointerId !== drag.current?.pointerId) return;
                const target = stageAtPointer(event);
                endDrag(event);
                if (target !== undefined) move(id, target);
              }}
              onPointerCancel={endDrag}
              onLostPointerCapture={endDrag}
              onKeyDown={event => {
                if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
                event.preventDefault();
                const target = order[index + (event.key === "ArrowUp" ? -1 : 1)];
                if (target !== undefined) move(id, target);
              }}
            >
              <GripVertical size={16} aria-hidden="true" />
              {label}
            </li>
          );
        })}
      </ol>
      {preview && (
        <div
          aria-hidden="true"
          className={styles.stageDragPreview}
          style={{ left: preview.x, top: preview.y, width: preview.width }}
        >
          <GripVertical size={16} />
          {preview.label}
        </div>
      )}
      {dependency && <p role="alert">{t("{stage} requires {requirement} earlier in the analysis order.", { stage: t(dependency.stage), requirement: t(dependency.requirement) })}</p>}
    </Section>
  );
}
