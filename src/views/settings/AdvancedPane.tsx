/**
 * Advanced: Database, Browse and Others. Captures docs/screenshots 9.49.32
 * to 9.49.51 PM.
 *
 * Database holds the library's own facts and rekordbox's Database management
 * (the one place rekordbox chooses which library it works on). Browse holds
 * Library Protection and Edit Library. Others holds BEAT/BPM SYNC, the
 * quantize beat value and play history.
 *
 * Not here: Auto Relocate Search Folders and Duplicates (hidden; Auto
 * Relocate searches the stored folders, Music, Movies and Desktop by
 * default), iTunes and rekordbox xml (neither is read), Auto Export and
 * Database management's Move Database (neither is built), My Tag, colour names, display
 * speed, the long-press menu and the Tag List (none exist here), the
 * export name (link export is not built), hot cue GATE, loop export,
 * Recordings, and every streaming service.
 */
import { useEffect, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { DatabaseDrive, LibrarySummary } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import { QUANTIZE_BEATS } from "@/lib/preferences";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import { Button, Note, Radios, Section, Select, Sub, Toggle } from "./controls";

export type AdvancedTab = "database" | "browse" | "others";

export const ADVANCED_TABS: readonly { id: AdvancedTab; label: string }[] = [
  { id: "database", label: "Database" },
  { id: "browse", label: "Browse" },
  { id: "others", label: "Others" },
];

export function AdvancedPane({ tab, summary }: {
  tab: AdvancedTab;
  summary: LibrarySummary | null;
}) {
  const t = useTranslation();
  const { preferences, update } = usePreferencesContext();
  const advanced = preferences.advanced;
  const set = (patch: Partial<typeof advanced>) => update("advanced", patch);
  const [checkingBackup, setCheckingBackup] = useState(false);
  const [unlockWarning, setUnlockWarning] = useState(false);
  const warningDialog = useRef<HTMLElement>(null);

  useEffect(() => {
    if (unlockWarning) warningDialog.current?.focus();
  }, [unlockWarning]);

  const setLibraryProtection = async (protectLibrary: boolean) => {
    if (protectLibrary) {
      set({ protectLibrary: true });
      return;
    }
    setCheckingBackup(true);
    try {
      const backend = await getBackend();
      const backups = await backend.listBackups().catch(() => null);
      if (backups && backups.length > 0) {
        set({ protectLibrary: false });
        return;
      }
      setUnlockWarning(true);
    } finally {
      setCheckingBackup(false);
    }
  };

  if (tab === "browse") {
    return (
      <>
        <Section title="Library Protection">
          <Toggle
            label="Protect library edit."
            checked={advanced.protectLibrary}
            disabled={checkingBackup}
            onChange={(protectLibrary) => { void setLibraryProtection(protectLibrary); }}
          />
        </Section>
        <Section title="Edit Library">
          <Toggle
            label="Double-click to edit"
            checked={advanced.doubleClickToEdit}
            onChange={(doubleClickToEdit) => set({ doubleClickToEdit })}
          />
        </Section>
        <Section title="Playlists">
          <Sub>When a track is already in the playlist</Sub>
          <Select
            label="When a track is already in the playlist"
            value={advanced.duplicateTracks}
            choices={[
              { value: "ask", label: "Ask every time" },
              { value: "skip", label: "Skip the duplicates" },
              { value: "add", label: "Add the duplicates" },
            ]}
            onChange={(duplicateTracks) => set({ duplicateTracks })}
          />
          <Sub>{t("When importing a playlist from a device")}</Sub>
          <Note>{t("Tracks the collection does not have are copied to PioneerDJ/Imported from Device in your Music folder. Files already there are never replaced.")}</Note>
          <Select
            label={t("When importing a playlist from a device")}
            value={advanced.deviceImportMissing}
            choices={[
              { value: "copy", label: t("Copy missing tracks") },
              { value: "ask", label: t("Ask each import") },
              { value: "matched", label: t("Import only already-matched tracks") },
              { value: "cancel", label: t("Cancel when any track is missing") },
            ]}
            onChange={(deviceImportMissing) => set({ deviceImportMissing })}
          />
          <Sub>{t("When a dropped folder has the same name as a list")}</Sub>
          <Select
            label={t("When a dropped folder has the same name as a list")}
            value={advanced.folderDropClash}
            choices={[
              { value: "copy", label: t("Import as a copy") },
              { value: "replace", label: t("Replace existing") },
              { value: "keep", label: t("Keep existing") },
              { value: "ask", label: t("Ask me") },
            ]}
            onChange={(folderDropClash) => set({ folderDropClash })}
          />
          <Note>{t("Replace existing deletes the list with that name, and every list inside it if it is a folder, without asking. Edit > Undo puts it back.")}</Note>
        </Section>
        {unlockWarning ? <div className={styles.warningBackdrop} role="presentation">
          <section
            ref={warningDialog}
            className={styles.warningDialog}
            role="dialog"
            aria-modal="true"
            aria-label={t("Library Protection")}
            tabIndex={-1}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.stopPropagation();
                setUnlockWarning(false);
              }
            }}
          >
            <h3>{t("Library Protection")}</h3>
            <p>{t("It looks like you haven’t made a backup yet. We strongly recommend creating one before using RBXport.")}</p>
            <div className={styles.warningActions}>
              <Button onClick={() => {
                setUnlockWarning(false);
                set({ protectLibrary: false });
              }}>{t("Unlock anyway")}</Button>
              <Button onClick={() => setUnlockWarning(false)}>{t("Cancel")}</Button>
            </div>
          </section>
        </div> : null}
      </>
    );
  }

  if (tab === "others") {
    return (
      <>
        <Section title="History">
          <Toggle
            label="Record play history"
            checked={advanced.recordHistory}
            onChange={(recordHistory) => set({ recordHistory })}
          />
        </Section>
        <Section title="QUANTIZE BEAT VALUE">
          <Select
            label="Quantize beat value"
            value={advanced.quantizeBeat}
            choices={QUANTIZE_BEATS.map((value) => ({ value, label: value }))}
            onChange={(quantizeBeat) => set({ quantizeBeat })}
          />
        </Section>
        <Section title="BEAT/BPM SYNC">
          <Sub>Sync Type</Sub>
          <Radios
            label="Sync Type"
            nested
            value={advanced.syncType}
            choices={[
              { value: "beat", label: "BEAT SYNC" },
              { value: "bpm", label: "BPM SYNC" },
            ]}
            onChange={(syncType) => set({ syncType })}
          />
          <Toggle
            label="Allow BEAT/BPM SYNC with double/half BPM."
            nested
            checked={advanced.syncDoubleHalf}
            onChange={(syncDoubleHalf) => set({ syncDoubleHalf })}
          />
        </Section>
      </>
    );
  }

  return (
    <>
      <Section title="Library">
        <dl className={styles.facts}>
          <dt>Tracks</dt>
          <dd>{summary ? summary.trackCount.toLocaleString() : "—"}</dd>
          <dt>Playlists</dt>
          <dd>{summary ? summary.playlistCount.toLocaleString() : "—"}</dd>
          <dt>Database version</dt>
          <dd>{summary?.dbVersion ?? "—"}</dd>
          <dt>Editing</dt>
          <dd>
            {summary?.readOnly
              ? "Read-only — rekordbox is running"
              : advanced.protectLibrary
                ? "Protected — see Browse"
                : "Available"}
          </dd>
        </dl>
      </Section>
      {/* Last, as in rekordbox, under the external-drive settings. */}
      <DatabaseManagementSection readOnly={summary?.readOnly ?? false} />
    </>
  );
}

/**
 * rekordbox's Database management: which drive's Master Database the
 * library is. The list is the default drive when it holds a library, then
 * every connected drive holding `PIONEER/Master/master.db` (or
 * `.PIONEER/Master` on HFS), named by volume label (`C:BOOTCAMP` on
 * Windows), and it is greyed out while there is only one; choosing one asks
 * "Are you sure you want to switch Master Database?" and switches
 * (`DetailDatabaseManagement::setup`, `comboBoxChanged`, `selectDrive`)
 * [OBS rekordbox 7.2.11, static analysis; Windows layout observed on
 * chris-win11 2026-10-08]. Its "?" help and Move Database are not built.
 * Here the switch sets rekordbox's
 * own `masterDbDirectory` and the app starts again on the chosen library.
 * Not while rekordbox runs: it puts its own setting back when it quits.
 */
function DatabaseManagementSection({ readOnly }: { readOnly: boolean }) {
  const t = useTranslation();
  const [drives, setDrives] = useState<DatabaseDrive[]>([]);
  const [switching, setSwitching] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    void getBackend()
      .then((backend) => backend.databaseDrives())
      .then((found) => { if (live) setDrives(found); })
      .catch(() => { if (live) setDrives([]); });
    return () => { live = false; };
  }, []);

  const current = drives.find((drive) => drive.current)?.masterDb ?? drives[0]?.masterDb ?? "";
  const choose = async (masterDb: string) => {
    if (masterDb === current) return;
    const backend = await getBackend();
    const sure = await backend.confirm(
      [t("Are you sure you want to switch Master Database?"), t("This operation may require long time.")].join("\n"),
      { yes: t("OK"), no: t("Cancel") },
    );
    if (!sure) return;
    setSwitching(true);
    setFailed(null);
    try {
      await backend.switchLibrary(masterDb);
      setDrives((all) => all.map((drive) => ({ ...drive, current: drive.masterDb === masterDb })));
    } catch {
      // rekordbox says only this; the reason is in the log.
      setFailed(t("Failed to switch Master Database."));
    } finally {
      setSwitching(false);
    }
  };

  return (
    <Section title="Database management">
      <Sub>Select a drive</Sub>
      <Select
        label="Select a drive"
        nested
        value={current}
        choices={drives.map((drive) => ({ value: drive.masterDb, label: drive.name }))}
        preserveChoiceLabels
        disabled={readOnly || switching || drives.length < 2}
        onChange={(masterDb) => { void choose(masterDb); }}
      />
      {failed ? <Note failed>{failed}</Note> : null}
    </Section>
  );
}
