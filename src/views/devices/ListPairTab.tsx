/**
 * Category and Sort: an Inactive list and an Active list with → and ←
 * between them, and Up / Down under the Active one. Captures:
 * docs/screenshots 9.08.32 PM (Category) and 9.08.35 PM (Sort).
 *
 * One selection across both lists, as in rekordbox: clicking an item in
 * either list picks it, and the arrows and Up / Down act on it. Greyed
 * items are the ones a player always has: they can be picked and moved
 * Up / Down, but never taken out of the Active list.
 *
 * Every move is a write to the stick, through `onChange`.
 */
import { useMemo, useState } from "react";

import type { MenuSlot } from "@/ipc/types";
import {
  activate, activeSlots, deactivate, displayName, inactiveSlots, isFixed, shift, type ListKind,
} from "@/lib/deviceSettings";
import styles from "./DevicePanel.module.css";

const HEADINGS: Record<ListKind, { inactive: string; active: string }> = {
  category: { inactive: "Inactive Categories", active: "Active Categories" },
  sort: { inactive: "Inactive Sort Options", active: "Active Sort Options" },
};

export interface ListPairTabProps {
  kind: ListKind;
  slots: readonly MenuSlot[];
  /** No library on the stick to write to: drawn, not editable. */
  disabled: boolean;
  onChange: (slots: MenuSlot[]) => void;
}

export function ListPairTab({ kind, slots, disabled, onChange }: ListPairTabProps) {
  // At most 22 rows; the split and order are decided here, not by the stick.
  const inactive = useMemo(() => inactiveSlots(slots), [slots]);
  const active = useMemo(() => activeSlots(slots), [slots]);
  const [selected, setSelected] = useState<number | null>(null);
  const picked = slots.find((s) => s.id === selected) ?? null;
  const pickedFixed = picked !== null && isFixed(kind, picked.menuItem);
  const headings = HEADINGS[kind];

  const canActivate = !disabled && picked !== null && !picked.visible;
  const canDeactivate = !disabled && picked !== null && picked.visible && !pickedFixed;
  const at = picked ? active.findIndex((s) => s.id === picked.id) : -1;
  const canUp = !disabled && picked !== null && picked.visible && at > 0;
  const canDown = !disabled && picked !== null && picked.visible && at >= 0 && at < active.length - 1;

  const list = (items: MenuSlot[], side: "inactive" | "active") => (
    <>
      <span className={styles.caption} id={`device-list-${kind}-${side}`}>
        {headings[side]}
      </span>
      <ul
        className={styles.list}
        data-tall={side === "inactive" || undefined}
        role="listbox"
        aria-labelledby={`device-list-${kind}-${side}`}
        aria-disabled={disabled || undefined}
      >
        {items.map((slot) => {
          const fixed = isFixed(kind, slot.menuItem);
          return (
            <li
              key={slot.id}
              role="option"
              aria-selected={slot.id === selected}
              aria-disabled={disabled || undefined}
              className={styles.item}
              data-fixed={fixed || undefined}
              data-on={slot.id === selected || undefined}
              onClick={() => {
                if (!disabled) setSelected(slot.id);
              }}
            >
              {displayName(slot)}
            </li>
          );
        })}
      </ul>
    </>
  );

  return (
    <div className={styles.pair} data-kind={kind}>
      <div className={styles.listColumn}>{list(inactive, "inactive")}</div>
      <div className={styles.arrows}>
        <button
          type="button"
          className={styles.arrow}
          aria-label={kind === "category" ? "Add to Active Categories" : "Add to Active Sort Options"}
          disabled={!canActivate}
          onClick={() => {
            if (picked) onChange(activate(slots, picked.id));
          }}
        >
          <span className={styles.arrowRight} aria-hidden />
        </button>
        <button
          type="button"
          className={styles.arrow}
          aria-label={
            kind === "category" ? "Remove from Active Categories" : "Remove from Active Sort Options"
          }
          disabled={!canDeactivate}
          onClick={() => {
            if (picked) onChange(deactivate(kind, slots, picked.id));
          }}
        >
          <span className={styles.arrowLeft} aria-hidden />
        </button>
      </div>
      <div className={styles.listColumn}>
        {list(active, "active")}
        <div className={styles.moves}>
          <button
            type="button"
            className={styles.move}
            disabled={!canUp}
            onClick={() => {
              if (picked) onChange(shift(slots, picked.id, -1));
            }}
          >
            Up
          </button>
          <button
            type="button"
            className={styles.move}
            disabled={!canDown}
            onClick={() => {
              if (picked) onChange(shift(slots, picked.id, 1));
            }}
          >
            Down
          </button>
        </div>
      </div>
    </div>
  );
}
