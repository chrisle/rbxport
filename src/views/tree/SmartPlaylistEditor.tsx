/**
 * The intelligent playlist editor: a name and a rule.
 *
 * rekordbox 7.2.11's dialog, captured 2026-09-18
 * (`docs/screenshots/intelligent-playlist-*-7.2.11@2x.png`): "List Name"
 * with "Untitled Intelligent List" selected, then "Match the following
 * condition:" over one row — "Match [all of the ▾] following conditions:"
 * once there are two — a ⊕ at the right of that line, one row per
 * condition (a property popup, an operator popup, the value, a ⊖), and
 * OK / Cancel. A new row is Artist = (empty). The operators are drawn as
 * `=`, `≠`, `>`, `<` and words for the rest; a text property offers
 * `=`, `≠`, contains, does not contain, starts with, ends with; a number
 * `=`, `≠`, `>`, `<`, is in the range; a date those plus is in the last
 * and is not in the last. The dialog has no Update automatically
 * control; the rule is always live.
 *
 * The property list is rekordbox's, in its order, each under the name
 * `SmartList` holds for it. My Tag picks one of the library's tags and
 * stores its `djmdMyTag.ID`; rekordbox answers it only with "contains" and
 * "does not contain" (operators 8 and 9) [OBS: static, rekordbox 7.2.19
 * `db::operate`], so those are the two offered [ASSUME: the words its own
 * dialog uses for them]. A property this list does not know — one a newer
 * rekordbox wrote — is shown as it is and cannot be chosen again.
 */
import { useEffect, useRef, useState } from "react";

import type { SmartCondition, SmartRule, TrackLookups } from "@/ipc/types";
import styles from "./SmartPlaylistEditor.module.css";

export interface SmartPlaylistEditorProps {
  /** "Create New Intelligent Playlist" or "Edit the Intelligent Playlist". */
  title: string;
  name: string;
  rule: SmartRule;
  /** The library's My Tags by category, for a My Tag condition's value. */
  myTags?: TrackLookups["myTagCategories"];
  onSave: (name: string, rule: SmartRule) => void;
  onCancel: () => void;
}

type Kind = "text" | "number" | "date" | "tag";

/**
 * The properties, in rekordbox's order and words [OBS 7.2.11]. `value` is
 * the name `SmartList` holds [OBS: static, the names rekordbox 7.2.19's
 * `db::getSmartlistCondition` compares against].
 */
const PROPERTIES: readonly { value: string; label: string; kind: Kind }[] = [
  { value: "album", label: "Album", kind: "text" },
  { value: "albumArtist", label: "Album artist", kind: "text" },
  { value: "artist", label: "Artist", kind: "text" },
  { value: "bpm", label: "BPM", kind: "number" },
  { value: "grouping", label: "Color", kind: "text" },
  { value: "comments", label: "Comments", kind: "text" },
  { value: "producer", label: "Composer", kind: "text" },
  { value: "stockDate", label: "Date Added", kind: "date" },
  { value: "dateCreated", label: "Date Created", kind: "date" },
  { value: "counter", label: "DJ play count", kind: "number" },
  { value: "fileName", label: "File name", kind: "text" },
  { value: "genre", label: "Genre", kind: "text" },
  { value: "key", label: "Key", kind: "text" },
  { value: "label", label: "Label", kind: "text" },
  { value: "mixName", label: "Mix name", kind: "text" },
  { value: "myTag", label: "My Tag", kind: "tag" },
  { value: "originalArtist", label: "Original artist", kind: "text" },
  { value: "rating", label: "Rating", kind: "number" },
  { value: "dateReleased", label: "Release Date", kind: "date" },
  { value: "remixedBy", label: "Remixer", kind: "text" },
  { value: "duration", label: "Time", kind: "number" },
  { value: "name", label: "Track Title", kind: "text" },
  { value: "year", label: "Year", kind: "number" },
];

/** The operators, numbered as rekordbox numbers them and drawn as it draws them [OBS 7.2.11]. */
const OPERATORS: readonly { value: string; label: string; for: readonly Kind[] }[] = [
  { value: "1", label: "=", for: ["text", "number", "date"] },
  { value: "2", label: "≠", for: ["text", "number", "date"] },
  { value: "3", label: ">", for: ["number", "date"] },
  { value: "4", label: "<", for: ["number", "date"] },
  { value: "6", label: "is in the last", for: ["date"] },
  { value: "7", label: "is not in the last", for: ["date"] },
  { value: "5", label: "is in the range", for: ["number", "date"] },
  { value: "8", label: "contains", for: ["text", "tag"] },
  { value: "9", label: "does not contain", for: ["text", "tag"] },
  { value: "10", label: "starts with", for: ["text"] },
  { value: "11", label: "ends with", for: ["text"] },
];

const UNITS: readonly { value: string; label: string }[] = [
  { value: "day", label: "day(s)" },
  { value: "week", label: "week(s)" },
  { value: "month", label: "month(s)" },
  { value: "year", label: "year(s)" },
];

function kindOf(property: string): Kind {
  return PROPERTIES.find((p) => p.value === property)?.kind ?? "text";
}

/**
 * A My Tag id as rekordbox compares it: a 32-bit signed integer, so a rule
 * value rekordbox rewrote into that form still names its tag. Mirrors
 * `rbl_index::smart::my_tag_key`.
 */
function tagKey(id: string): number {
  const match = /^\s*([+-]?\d+)/.exec(id);
  return match?.[1] ? Number(BigInt.asIntN(32, BigInt(match[1]))) : 0;
}

/** A fresh condition: Artist = (empty), as rekordbox's new row is [OBS 7.2.11]. */
export function emptyCondition(): SmartCondition {
  return { property: "artist", operator: "1", left: "", right: "", unit: "" };
}

export function SmartPlaylistEditor({ title, name: initialName, rule: initialRule, myTags = [], onSave, onCancel }: SmartPlaylistEditorProps) {
  const [name, setName] = useState(initialName);
  const [logic, setLogic] = useState<"all" | "any">(initialRule.logic === "any" ? "any" : "all");
  const [conditions, setConditions] = useState<SmartCondition[]>(
    initialRule.conditions.length === 0 ? [emptyCondition()] : initialRule.conditions.map((c) => ({ ...c })),
  );
  const nameField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    nameField.current?.focus();
    nameField.current?.select();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const update = (at: number, patch: Partial<SmartCondition>) =>
    setConditions((current) => current.map((c, i) => (i === at ? { ...c, ...patch } : c)));

  const changeProperty = (at: number, property: string) => {
    // An operator the new property cannot take falls back to its first.
    const kind = kindOf(property);
    const current = conditions[at];
    const keep = current && OPERATORS.some((o) => o.value === current.operator && o.for.includes(kind));
    const operator = keep ? current.operator : (OPERATORS.find((o) => o.for.includes(kind))?.value ?? "1");
    // A tag is picked, not typed: text carried into it would name no tag,
    // and a tag id carried out of it is not a word anyone wrote.
    const crossesTag = current !== undefined && (kind === "tag") !== (kindOf(current.property) === "tag");
    update(at, {
      property,
      operator,
      unit: operator === "6" || operator === "7" ? "day" : "",
      ...(crossesTag ? { left: "", right: "" } : {}),
    });
  };

  const changeOperator = (at: number, operator: string) => {
    const relative = operator === "6" || operator === "7";
    update(at, { operator, unit: relative ? (conditions[at]?.unit || "day") : "", right: operator === "5" ? (conditions[at]?.right ?? "") : "" });
  };

  const canSave = name.trim() !== "" && conditions.every((c) => c.left.trim() !== "");

  return (
    <div className={styles.backdrop} onMouseDown={onCancel} role="presentation">
      <div
        className={styles.window}
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <header className={styles.titlebar}>{title}</header>
        <div className={styles.body}>
          <label className={styles.nameRow}>
            <span>List name</span>
            <input
              ref={nameField}
              className={styles.input}
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-label="List name"
            />
          </label>
          <div className={styles.matchRow}>
            {conditions.length > 1 ? (
              <>
                <span>Match</span>
                <select
                  className={styles.select}
                  aria-label="Match"
                  value={logic}
                  onChange={(e) => setLogic(e.target.value === "any" ? "any" : "all")}
                >
                  <option value="all">all of the</option>
                  <option value="any">any of the</option>
                </select>
                <span>following conditions:</span>
              </>
            ) : (
              <span>Match the following condition:</span>
            )}
            <span className={styles.grow} />
            <button
              type="button"
              className={styles.small}
              aria-label="Add condition"
              onClick={() => setConditions((current) => [...current, emptyCondition()])}
            >
              +
            </button>
          </div>
          <div className={styles.conditions}>
            {conditions.map((condition, at) => {
              const kind = kindOf(condition.property);
              const relative = condition.operator === "6" || condition.operator === "7";
              const known = PROPERTIES.some((p) => p.value === condition.property);
              const pickedTag =
                kind === "tag" && condition.left !== ""
                  ? myTags.flatMap((c) => c.tags).find((t) => tagKey(t.id) === tagKey(condition.left))
                  : undefined;
              return (
                // Rows have no identity of their own; their place is what tells them apart.
                <div key={at} className={styles.condition} role="group" aria-label={`Condition ${at + 1}`}>
                  <select
                    className={styles.select}
                    aria-label="Property"
                    value={condition.property}
                    onChange={(e) => changeProperty(at, e.target.value)}
                  >
                    {known ? null : (
                      <option value={condition.property} disabled>
                        {condition.property || "—"}
                      </option>
                    )}
                    {PROPERTIES.map((p) => (
                      <option key={p.label} value={p.value}>{p.label}</option>
                    ))}
                  </select>
                  <select
                    className={styles.select}
                    aria-label="Operator"
                    value={condition.operator}
                    onChange={(e) => changeOperator(at, e.target.value)}
                  >
                    {OPERATORS.filter((o) => o.for.includes(kind)).map((o) => (
                      <option key={o.value} value={o.value}>{o.label}</option>
                    ))}
                  </select>
                  {kind === "tag" ? (
                    <select
                      className={styles.select}
                      aria-label="Value"
                      value={pickedTag?.id ?? condition.left}
                      onChange={(e) => update(at, { left: e.target.value })}
                    >
                      <option value="" disabled />
                      {condition.left !== "" && pickedTag === undefined ? (
                        // A tag the library no longer has: kept as it is
                        // rather than silently swapped for another.
                        <option value={condition.left} disabled>
                          {condition.left}
                        </option>
                      ) : null}
                      {myTags.map((category, c) => (
                        // Category names repeat ("Empty Category"), so their place keys them.
                        <optgroup key={c} label={category.name}>
                          {category.tags.map((tag) => (
                            <option key={tag.id} value={tag.id}>{tag.name}</option>
                          ))}
                        </optgroup>
                      ))}
                    </select>
                  ) : (
                    <input
                      className={styles.input}
                      aria-label="Value"
                      type={kind === "number" || relative ? "number" : kind === "date" ? "date" : "text"}
                      value={condition.left}
                      onChange={(e) => update(at, { left: e.target.value })}
                    />
                  )}
                  {condition.operator === "5" ? (
                    <>
                      <span>to</span>
                      <input
                        className={styles.input}
                        aria-label="Upper value"
                        type={kind === "date" ? "date" : "number"}
                        value={condition.right}
                        onChange={(e) => update(at, { right: e.target.value })}
                      />
                    </>
                  ) : null}
                  {relative ? (
                    <select
                      className={styles.select}
                      aria-label="Unit"
                      value={condition.unit || "day"}
                      onChange={(e) => update(at, { unit: e.target.value })}
                    >
                      {UNITS.map((u) => (
                        <option key={u.value} value={u.value}>{u.label}</option>
                      ))}
                    </select>
                  ) : null}
                  <button
                    type="button"
                    className={styles.small}
                    aria-label="Remove condition"
                    disabled={conditions.length === 1}
                    onClick={() => setConditions((current) => current.filter((_, i) => i !== at))}
                  >
                    −
                  </button>
                </div>
              );
            })}
          </div>
        </div>
        <footer className={styles.footer}>
          <button
            type="button"
            className={styles.button}
            disabled={!canSave}
            onClick={() => onSave(name.trim(), { logic, conditions: conditions.map((c) => ({ ...c, left: c.left.trim(), right: c.right.trim() })) })}
          >
            OK
          </button>
          <button type="button" className={styles.button} onClick={onCancel}>
            Cancel
          </button>
        </footer>
      </div>
    </div>
  );
}
