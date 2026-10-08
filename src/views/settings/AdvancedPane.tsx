/**
 * Advanced: Database, Browse and Others. Captures docs/screenshots 9.49.32
 * to 9.49.51 PM.
 *
 * Database holds the library's own facts, rekordbox's Database management
 * (the one place rekordbox chooses which library it works on), and the
 * missing-file manager, with rekordbox's Auto Relocate Search Folders
 * feeding it. Browse holds
 * Library Protection and Edit Library. Others holds BEAT/BPM SYNC, the
 * quantize beat value and play history.
 *
 * Not here: iTunes and rekordbox xml (neither is read), Auto Export and
 * Database management's Move Database (neither is built), My Tag, colour names, display
 * speed, the long-press menu and the Tag List (none exist here), the
 * export name (link export is not built), hot cue GATE, loop export,
 * Recordings, and every streaming service.
 */
import { useEffect, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { DatabaseDrive, Duplicates, LibrarySummary, MissingTracks, RelocateReport } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import { QUANTIZE_BEATS } from "@/lib/preferences";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import { Button, Note, Radios, Section, Select, Sub, Toggle } from "./controls";

/**
 * Whether Database shows the missing-file manager and Auto Relocate Search
 * Folders. Hidden from users for now; the code stays so it can be turned
 * back on. The File menu's Missing File Manager item is gated to match in
 * `src-tauri/src/menu.rs`.
 */
export const MISSING_FILES_ENABLED = false;

export type AdvancedTab = "database" | "browse" | "others";

export const ADVANCED_TABS: readonly { id: AdvancedTab; label: string }[] = [
  { id: "database", label: "Database" },
  { id: "browse", label: "Browse" },
  { id: "others", label: "Others" },
];

/** How many missing tracks to list. The count above it is exact. */
const MISSING_SHOWN = 20;

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
      {MISSING_FILES_ENABLED ? (
        <RelocateSection
          folders={advanced.relocateFolders}
          onFolders={(relocateFolders) => set({ relocateFolders })}
          readOnly={(summary?.readOnly ?? false) || advanced.protectLibrary}
        />
      ) : null}
      <DuplicatesSection readOnly={(summary?.readOnly ?? false) || advanced.protectLibrary} />
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

/** How many duplicate groups to list. The counts above are exact. */
const DUPLICATE_GROUPS_SHOWN = 20;

/** `m:ss`, for telling two copies apart by length. */
function minutes(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/**
 * Duplicates: tracks that share a title and an artist. Nothing is changed
 * by looking; a copy is removed from the collection one at a time, after
 * asking, and the file stays where it is.
 */
function DuplicatesSection({ readOnly }: { readOnly: boolean }) {
  const [found, setFound] = useState<Duplicates | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const scan = async () => {
    const backend = await getBackend();
    setFound(await backend.findDuplicates(DUPLICATE_GROUPS_SHOWN));
  };

  return (
    <Section title="Duplicates">
      {found === null ? (
        <>
          <div className={styles.actions}>
            <Button
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void scan().finally(() => setBusy(false));
              }}
            >
              {busy ? "Looking…" : "Find duplicates"}
            </Button>
          </div>
        </>
      ) : found.groups === 0 ? (
        <Note>No two tracks share a title and an artist.</Note>
      ) : (
        <>
          <Note>
            {found.groups.toLocaleString()} title{found.groups === 1 ? "" : "s"} with more than one copy,{" "}
            {found.extra.toLocaleString()} extra cop{found.extra === 1 ? "y" : "ies"} in all.
          </Note>
          <ul className={styles.list} aria-label="Duplicates">
            {found.shown.map((group) => (
              <li key={`${group.title}\u0000${group.artist}`}>
                <span className={styles.listTitle}>
                  {group.title}
                  {group.artist ? ` — ${group.artist}` : ""}
                </span>
                {group.tracks.map((track) => (
                  <span key={track.id} className={styles.listPath}>
                    {minutes(track.durationSec)} · {track.path}
                    {track.present ? "" : " (file missing)"}
                    <button
                      type="button"
                      className={styles.listAction}
                      disabled={readOnly || busy}
                      onClick={() => {
                        setBusy(true);
                        void (async () => {
                          try {
                            const backend = await getBackend();
                            const sure = await backend.confirm(
                              `Remove this copy of ${group.title} from the collection? This can’t be undone. The file stays where it is.`,
                            );
                            if (!sure) return;
                            await backend.edits.removeFromCollection([track.id]);
                            setNote(`Removed a copy of ${group.title}.`);
                            await scan();
                          } catch (e) {
                            setNote(e instanceof Error ? e.message : "That copy could not be removed.");
                          } finally {
                            setBusy(false);
                          }
                        })();
                      }}
                    >
                      Remove
                    </button>
                  </span>
                ))}
              </li>
            ))}
          </ul>
          {found.groups > found.shown.length ? <Note>Showing the first {found.shown.length}.</Note> : null}
          {note ? <Note>{note}</Note> : null}
        </>
      )}
    </Section>
  );
}

function RelocateSection({ folders, onFolders, readOnly }: {
  folders: readonly string[];
  onFolders: (folders: string[]) => void;
  readOnly: boolean;
}) {
  const [missing, setMissing] = useState<MissingTracks | null>(null);
  const [scanning, setScanning] = useState(false);
  const [picked, setPicked] = useState<string>(folders[0] ?? "");
  const [report, setReport] = useState<RelocateReport | null>(null);
  const current = folders.includes(picked) ? picked : (folders[0] ?? "");

  const rescan = async () => {
    const backend = await getBackend();
    setMissing(await backend.missingTracks(MISSING_SHOWN));
  };

  return (
    <>
      <Section title="Auto Relocate Search Folders">
        <Sub>Specified user folders</Sub>
        <div className={styles.actions}>
          <select
            className={styles.select}
            data-plain
            aria-label="Search folders"
            value={current}
            onChange={(e) => setPicked(e.target.value)}
          >
            {folders.length === 0 ? <option value="">No folders</option> : null}
            {folders.map((folder) => (
              <option key={folder} value={folder}>{folder}</option>
            ))}
          </select>
          <Button
            onClick={() => {
              void (async () => {
                const backend = await getBackend();
                const folder = await backend.pickFolder("Choose a folder to search for moved files");
                if (folder === null || folders.includes(folder)) return;
                onFolders([...folders, folder]);
                setPicked(folder);
              })();
            }}
          >
            Add
          </Button>
          <Button disabled={current === ""} onClick={() => onFolders(folders.filter((f) => f !== current))}>
            Del
          </Button>
        </div>
      </Section>

      <Section title="Missing files">
        {missing === null ? (
          <>
            <div className={styles.actions}>
              <Button
                disabled={scanning}
                onClick={() => {
                  setScanning(true);
                  void rescan().finally(() => setScanning(false));
                }}
              >
                {scanning ? "Checking…" : "Check for missing files"}
              </Button>
            </div>
          </>
        ) : missing.total === 0 ? (
          <Note>Every track&rsquo;s file is where the library expects it.</Note>
        ) : (
          <>
            <Note>
              {missing.total.toLocaleString()} track{missing.total === 1 ? "" : "s"} cannot be found.
            </Note>
            <div className={styles.actions}>
              <Button
                disabled={readOnly || folders.length === 0 || scanning}
                onClick={() => {
                  setScanning(true);
                  void (async () => {
                    try {
                      const backend = await getBackend();
                      setReport(await backend.autoRelocate([...folders]));
                      await rescan();
                    } finally {
                      setScanning(false);
                    }
                  })();
                }}
              >
                {scanning ? "Searching…" : "Auto Relocate"}
              </Button>
            </div>
            {report ? (
              <Note>
                {report.relocated} relocated
                {report.unresolved > 0 ? `, ${report.unresolved} not found in the search folders.` : "."}
              </Note>
            ) : folders.length === 0 ? (
              <Note>Add a search folder above to relocate them automatically.</Note>
            ) : null}
            <ul className={styles.list}>
              {missing.tracks.map((track) => (
                <li key={track.id}>
                  <span className={styles.listTitle}>
                    {track.title}
                    {track.artist ? ` — ${track.artist}` : ""}
                  </span>
                  <span className={styles.listPath}>{track.path}</span>
                  <button
                    type="button"
                    className={styles.listAction}
                    disabled={readOnly}
                    onClick={() => {
                      void (async () => {
                        const backend = await getBackend();
                        const chosen = await backend.relocateTrack(track.id);
                        // Cancelling leaves the list alone; a successful
                        // relocate means the track is no longer missing.
                        if (chosen === null) return;
                        await rescan();
                      })();
                    }}
                  >
                    Locate&hellip;
                  </button>
                </li>
              ))}
            </ul>
            {missing.total > missing.tracks.length ? (
              <Note>Showing the first {missing.tracks.length}.</Note>
            ) : null}
          </>
        )}
      </Section>
    </>
  );
}
