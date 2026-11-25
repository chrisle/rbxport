/**
 * Column: one menu, "Default right column" — the item a CDJ shows beside the
 * track name. Capture: docs/screenshots 9.08.37 PM, which shows "Not
 * Specified".
 *
 * On the stick this is `sort.isSelectedAsSubColumn`, one row at most. The
 * choices offered are the sort options, since that is the table the flag
 * lives in [ASSUME]; the capture's menu was closed, so what rekordbox lists
 * has not been seen.
 */
import { useMemo } from "react";

import type { DeviceSettings } from "@/ipc/types";
import { displayName, isFixed } from "@/lib/deviceSettings";
import styles from "./DevicePanel.module.css";

export interface ColumnTabProps {
  settings: DeviceSettings;
  onChange: (next: DeviceSettings) => void;
}

export function ColumnTab({ settings, onChange }: ColumnTabProps) {
  // DEFAULT and ALPHABET are how a list is ordered, not something to show
  // beside a title, so they are left out.
  const choices = useMemo(
    () => settings.sorts.filter((s) => !isFixed("sort", s.menuItem)),
    [settings.sorts],
  );
  return (
    <div className={styles.column}>
      <span className={styles.caption}>Default right column</span>
      <p className={styles.columnHint}>Select item which is shown next to track name on CDJ/XDJ</p>
      <select
        className={styles.columnSelect}
        aria-label="Default right column"
        value={settings.subColumn ?? ""}
        disabled={!settings.hasMenuSettings}
        onChange={(e) => {
          const value = e.target.value;
          onChange({ ...settings, subColumn: value === "" ? null : Number(value) });
        }}
      >
        <option value="">Not Specified</option>
        {choices.map((slot) => (
          <option key={slot.id} value={slot.menuItem}>
            {displayName(slot)}
          </option>
        ))}
      </select>
    </div>
  );
}
