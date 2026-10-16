/**
 * Keyboard: rekordbox's EXPORT mode key map, grouped as its pane groups it.
 * Capture docs/screenshots 9.48.54 PM.
 *
 * Every row is rekordbox's — the ten groups and the commands in each, with
 * the key the chosen preset gives it. The dropdown holds rekordbox's
 * presets (`keyPresets.ts`): Export (Preset), the one this app's layout
 * clones, and the two Performance presets and None, which rekordbox keeps
 * for PERFORMANCE mode; a Performance preset's keys land on the same
 * rekordbox commands here, Deck 1's on Player A and Deck 2's on Player B.
 * A row this app answers is drawn live and its key can be changed: click
 * the badge, press the new chord, and it is stored with the preferences and
 * read by every key handler through `keyMap`. Escape leaves the key as it
 * was; backspace takes it away. A chord another row already has moves to
 * the row that took it. One this app does not answer is greyed, the way
 * rekordbox greys what a selection cannot do, so the map reads the same
 * here as there. The menu's accelerators are the shell's and stay as they
 * are, in every preset; a preset key one of them holds is left off its row.
 * Reset puts the preset's keys back.
 */
import { useEffect, useMemo, useState } from "react";

import { TwistyIcon } from "@/components/icons";
import { useTranslation } from "@/i18n";
import {
  KEY_PRESETS, KEY_PRESET_NAMES, keyMap, performanceKeys, presetChord, type KeyPreset,
} from "@/lib/keyPresets";
import { KEYMAP, KEYMAP_GROUPS, keyForPlatform, type KeymapGroup } from "@/lib/keymap";
import {
  BINDINGS, chordFromEvent, describeChord, detectPlatform, sameChord, type KeyChord, type KeyOverrides,
} from "@/lib/shortcuts";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import { Button } from "./controls";

interface Row {
  id: string;
  label: string;
  /** The key as printed, or null for a row with none. */
  key: string | null;
  /** Whether this app answers it. */
  built: boolean;
  /** The binding whose key this row can change, when it can. */
  bindingId: string | null;
  /** Whether the key is the person's own rather than the preset's. */
  changed: boolean;
  /** Whether the preset's key is one the shell's menu holds here. */
  shell?: true;
}

/** The pane's rows for a group: rekordbox's, then this app's own. */
export function rowsFor(
  group: KeymapGroup, mac: boolean, overrides: KeyOverrides = {}, preset: KeyPreset = "export",
): Row[] {
  const platform = { mac };
  const keys = keyMap({ preset, overrides });
  const chordOf = (id: string, fallback: KeyChord) => keys[id] ?? fallback;
  const byCommand = new Map(BINDINGS.filter((b) => b.command !== undefined).map((b) => [b.command, b]));
  const rows: Row[] = KEYMAP[group].map((row) => {
    const binding = byCommand.get(row.id);
    if (binding?.action === undefined) {
      // The menu's keys are the shell's, the Export preset's in every
      // preset; any other row this app lacks shows the preset's key.
      const text = preset === "export" || binding !== undefined
        ? row.key
        : performanceKeys(preset, row.id).join(", ") || null;
      return {
        id: row.id,
        label: row.label,
        key: text === null ? null : keyForPlatform(text, mac),
        built: binding !== undefined,
        bindingId: null,
        changed: false,
      };
    }
    const chord = chordOf(binding.id, binding.chord);
    const changed = binding.id in overrides;
    const out: Row = {
      id: row.id,
      label: row.label,
      key: chord.key === "" ? null : describeChord(chord, platform),
      built: true,
      bindingId: binding.id,
      changed,
    };
    if (!changed && presetChord(binding, preset).shell) out.shell = true;
    return out;
  });
  for (const binding of BINDINGS) {
    if (binding.command !== undefined || binding.pane !== group || binding.alias) continue;
    const chord = chordOf(binding.id, binding.chord);
    rows.push({
      id: `ours:${binding.id}`,
      label: binding.label,
      key: chord.key === "" ? null : describeChord(chord, platform),
      built: true,
      bindingId: binding.action === undefined ? null : binding.id,
      changed: binding.id in overrides,
    });
  }
  return rows;
}

/**
 * The stored keys after one binding takes a chord: the binding gets it, a
 * preset key put back is forgotten rather than stored, and another binding
 * whose own saved key was that chord loses it. A row that has the chord only
 * from the preset is left unsaved: `keyMap` takes the key from it while the
 * clash lasts, and gives it back its key in a preset where there is none.
 */
export function assignChord(
  overrides: KeyOverrides, id: string, chord: KeyChord, preset: KeyPreset = "export",
): KeyOverrides {
  const next: Record<string, KeyChord> = { ...overrides };
  const target = BINDINGS.find((b) => b.id === id);
  if (!target) return overrides;
  if (chord.key !== "") {
    for (const [other, held] of Object.entries(overrides)) {
      if (other !== id && held.key !== "" && sameChord(held, chord)) next[other] = { key: "" };
    }
  }
  const fallback = presetChord(target, preset).chord;
  if (fallback.key === chord.key && sameChord(fallback, chord)) delete next[id];
  else next[id] = chord;
  return next;
}

export function KeyboardPane() {
  const t = useTranslation();
  const platform = useMemo(detectPlatform, []);
  const { preferences, update } = usePreferencesContext();
  const { overrides, preset } = preferences.keyboard;
  const [open, setOpen] = useState<Set<KeymapGroup>>(() => new Set());
  /** The binding waiting for its new key. */
  const [listening, setListening] = useState<string | null>(null);

  // The next chord goes to the row that asked, before any handler in the
  // window can act on it: the key the person is about to bind must not
  // also do what it does now.
  useEffect(() => {
    if (listening === null) return undefined;
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setListening(null);
        return;
      }
      const chord = event.key === "Backspace" || event.key === "Delete"
        ? { key: "" }
        : chordFromEvent(event, platform);
      if (chord === null) return;
      update("keyboard", { overrides: assignChord(overrides, listening, chord, preset) });
      setListening(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [listening, overrides, platform, preset, update]);

  const changed = Object.keys(overrides).length > 0;
  return (
    <section className={`${styles.section} ${styles.sectionFill}`} aria-label={t("Keyboard")}>
      <div className={styles.actions} data-spaced="">
        <select
          className={styles.select}
          aria-label={t("Keyboard preset")}
          value={preset}
          onChange={(event) => {
            setListening(null);
            update("keyboard", { preset: event.target.value as KeyPreset });
          }}
        >
          {/* rekordbox keeps its preset names in English in every language
              [OBS static: `PresetKeyMapping`'s names are never translated]. */}
          {KEY_PRESETS.map((choice) => (
            <option key={choice} value={choice} data-i18n-ignore="">{KEY_PRESET_NAMES[choice]}</option>
          ))}
        </select>
        <Button onClick={() => update("keyboard", { overrides: {} })} disabled={!changed}>
          {t("Reset to the preset")}
        </Button>
      </div>
      <div className={styles.keys}>
        {KEYMAP_GROUPS.map((group) => {
          const shown = open.has(group);
          const rows = shown ? rowsFor(group, platform.mac, overrides, preset) : [];
          return (
            <div key={group}>
              <button
                type="button"
                className={styles.keyGroup}
                aria-expanded={shown}
                onClick={() =>
                  setOpen((current) => {
                    const next = new Set(current);
                    if (next.has(group)) next.delete(group);
                    else next.add(group);
                    return next;
                  })
                }
              >
                <TwistyIcon />
                {group}
              </button>
              {shown && rows.length === 0 ? (
                <div className={styles.keyRow} data-dim="">
                  <span className={styles.keyName}>{t("Nothing is bound here in the Export preset.")}</span>
                </div>
              ) : null}
              {rows.map((row) => (
                <div
                  key={row.id}
                  className={styles.keyRow}
                  data-dim={row.built ? undefined : ""}
                  title={row.built ? undefined : t("rekordbox has this; it is not built here")}
                >
                  <span className={styles.keyName}>{row.label}</span>
                  {row.bindingId !== null ? (
                    <button
                      type="button"
                      className={styles.keyBadge}
                      data-listening={listening === row.bindingId ? "" : undefined}
                      data-changed={row.changed ? "" : undefined}
                      data-shell={row.shell ? "" : undefined}
                      aria-label={`${row.label} key`}
                      title={listening === row.bindingId
                        ? t("Press the new key; Escape keeps this one, Backspace takes it away")
                        : row.shell
                          ? t("The menu uses this key here. Click to choose another.")
                          : t("Click to change the key")}
                      onClick={() => setListening((current) => (current === row.bindingId ? null : row.bindingId))}
                    >
                      {listening === row.bindingId ? "…" : row.key ?? "+"}
                    </button>
                  ) : row.key !== null ? (
                    <span className={styles.keyBadge}>{row.key}</span>
                  ) : null}
                </div>
              ))}
            </div>
          );
        })}
      </div>
    </section>
  );
}
