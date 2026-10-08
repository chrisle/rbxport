/**
 * DJ System: what a stick gets when it is first exported to. Captures
 * docs/screenshots 9.47.58 to 9.48.44 PM.
 *
 * A stick that already carries settings keeps its own — its device panel
 * edits those — so these are the defaults, applied once, to a stick that
 * has none. The General tab is `DEVSETTING.DAT`; Category, Sort and Column
 * are `exportLibrary.db`'s rows.
 *
 * Not here: the account nickname (no account), the background colours
 * (where rekordbox keeps their defaults has not been recorded; each stick's
 * own are set on its General tab), the jog image (not written), My Settings
 * (`MYSETTING.DAT` is not written), and the Device tab (history import is
 * not built). PRO DJ LINK, in place of rekordbox's Others tab, holds the
 * LINK switch and the network interface it runs on.
 */
import { useEffect, useMemo, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { MenuSlot } from "@/ipc/types";
import { displayName, isFixed } from "@/lib/deviceSettings";
import { usePreferencesContext } from "@/store/usePreferences";
import { ListPairTab } from "@/views/devices/ListPairTab";
import styles from "./Preferences.module.css";
import { Radios, Section, Select, Sub } from "./controls";

export type DjSystemTab = "general" | "category" | "sort" | "column";

export const DJ_SYSTEM_TABS: readonly { id: DjSystemTab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "category", label: "Category" },
  { id: "sort", label: "Sort" },
  { id: "column", label: "Column" },
];

/** The reference rows, read once: they are what a stored null stands for. */
function useReferenceRows(): { categories: MenuSlot[]; sorts: MenuSlot[] } | null {
  const [rows, setRows] = useState<{ categories: MenuSlot[]; sorts: MenuSlot[] } | null>(null);
  useEffect(() => {
    let live = true;
    void getBackend()
      .then((backend) => backend.referenceStickSettings())
      .then((read) => {
        if (live) setRows(read);
      })
      .catch(() => {
        // Nothing to edit against; the tabs stay empty rather than inventing rows.
      });
    return () => {
      live = false;
    };
  }, []);
  return rows;
}

export function DjSystemPane({ tab }: { tab: DjSystemTab }) {
  const { preferences, update } = usePreferencesContext();
  const dj = preferences.djSystem;
  const set = (patch: Partial<typeof dj>) => update("djSystem", patch);
  const reference = useReferenceRows();
  const categories = dj.categories ?? reference?.categories ?? [];
  const sorts = dj.sorts ?? reference?.sorts ?? [];

  if (tab === "category" || tab === "sort") {
    return (
      <Section title={tab === "category" ? "Category" : "Sort"}>
        <div className={styles.pairHost}>
          <ListPairTab
            kind={tab}
            slots={tab === "category" ? categories : sorts}
            disabled={reference === null && (tab === "category" ? dj.categories : dj.sorts) === null}
            onChange={(slots) => set(tab === "category" ? { categories: slots } : { sorts: slots })}
          />
        </div>
      </Section>
    );
  }

  if (tab === "column") {
    return <ColumnSection sorts={sorts} subColumn={dj.subColumn} onChange={(subColumn) => set({ subColumn })} />;
  }


  return (
    <>
      <Section title="Waveform color">
        <Sub>Waveform color displayed on CDJ/XDJ</Sub>
        <Radios
          label="Waveform color displayed on CDJ/XDJ"
          nested
          value={dj.waveformColor}
          choices={[
            { value: "blue", label: "BLUE" },
            { value: "rgb", label: "RGB" },
            { value: "3band", label: "3Band" },
          ]}
          onChange={(waveformColor) => set({ waveformColor })}
        />
      </Section>
      <Section title="Waveform Current Position">
        <Sub>Waveform Current Position displayed on CDJ/XDJ</Sub>
        <Radios
          label="Waveform Current Position displayed on CDJ/XDJ"
          nested
          value={dj.waveformPosition}
          choices={[
            { value: "left", label: "LEFT" },
            { value: "center", label: "CENTER" },
          ]}
          onChange={(waveformPosition) => set({ waveformPosition })}
        />
      </Section>
      <Section title="Type of the Overview Waveform">
        <Sub>Type of the Overview Waveform displayed on CDJ/XDJ</Sub>
        <Radios
          label="Type of the Overview Waveform displayed on CDJ/XDJ"
          nested
          value={dj.overviewWaveform}
          choices={[
            { value: "half", label: "Half Waveform" },
            { value: "full", label: "Full Waveform" },
          ]}
          onChange={(overviewWaveform) => set({ overviewWaveform })}
        />
      </Section>
      <Section title="Key display format">
        <Sub>The key display format displayed on CDJ/XDJ.</Sub>
        <Radios
          label="The key display format displayed on CDJ/XDJ"
          nested
          value={dj.keyDisplay}
          choices={[
            { value: "classic", label: "Classic" },
            { value: "alphanumeric", label: "Alphanumeric" },
          ]}
          onChange={(keyDisplay) => set({ keyDisplay })}
        />
      </Section>
    </>
  );
}

function ColumnSection({ sorts, subColumn, onChange }: {
  sorts: readonly MenuSlot[];
  subColumn: number | null;
  onChange: (subColumn: number | null) => void;
}) {
  // DEFAULT and ALPHABET are how a list is ordered, not something to show
  // beside a title, so they are left out.
  const choices = useMemo(
    () => [
      { value: "", label: "Not Specified" },
      ...sorts.filter((s) => !isFixed("sort", s.menuItem)).map((s) => ({
        value: String(s.menuItem),
        label: displayName(s),
      })),
    ],
    [sorts],
  );
  return (
    <Section title="Column">
      <Sub>Select item which is shown next to track name on CDJ/XDJ</Sub>
      <Select
        label="Default right column"
        nested
        value={subColumn === null ? "" : String(subColumn)}
        choices={choices}
        onChange={(value) => onChange(value === "" ? null : Number(value))}
      />
    </Section>
  );
}
