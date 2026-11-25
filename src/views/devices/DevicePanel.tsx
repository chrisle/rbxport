/**
 * The panel a selected device opens: six tabs across the top — General,
 * Category, Sort, Column, Color, My Settings — and the tab's body beneath.
 *
 * Drawn from the captures docs/screenshots 9.08.22 to 9.08.41 PM (rekordbox
 * 7.2.11, the TEST stick). The strip and every measurement in the tabs come
 * from them; My Settings has no capture and says so.
 *
 * The settings are the stick's own files, read when the panel opens and
 * written on every change — there is no Apply button in the captures. Each
 * write resolves to what the stick now holds, which is what is shown, so a
 * write that did not take cannot leave the panel claiming it did.
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { Device, DeviceSettings, TreeNode } from "@/ipc/types";
import { errorMessage } from "@/lib/errorMessage";
import { usePreferences } from "@/store/usePreferences";
import { ColorTab } from "./ColorTab";
import { ColumnTab } from "./ColumnTab";
import styles from "./DevicePanel.module.css";
import { GeneralTab } from "./GeneralTab";
import { ListPairTab } from "./ListPairTab";
import { MySettingsTab } from "./MySettingsTab";

/** The six, in the order and with the wording of the capture [OBS]. */
const TABS = [
  { id: "general", label: "General" },
  { id: "category", label: "Category" },
  { id: "sort", label: "Sort" },
  { id: "column", label: "Column" },
  { id: "color", label: "Color" },
  { id: "mySettings", label: "My Settings" },
] as const;

export type DeviceTab = (typeof TABS)[number]["id"];

export interface DevicePanelProps {
  device: Device;
  /** Every playlist in the library, so one can be chosen to write. */
  playlists: readonly TreeNode[];
  /** Writes `playlistId` to the device. */
  onSync: (playlistId: string) => Promise<void>;
  /** Re-reads the connected volumes. */
  onRefresh: () => void;
  /** Something the stick refused; shown in the status bar. */
  onError?: (message: string) => void;
  busy?: boolean;
}

export function DevicePanel({
  device,
  playlists,
  onSync,
  onRefresh,
  onError,
  busy = false,
}: DevicePanelProps) {
  const [tab, setTab] = useState<DeviceTab>("general");
  const [settings, setSettings] = useState<DeviceSettings | null>(null);

  // Read when the device changes, and again after a sync: an export writes a
  // library, and with it the rows the Category, Sort and Color tabs edit.
  const exportStamp = device.export?.written ?? "";
  // Opening the panel on a stick that holds an export but no DEVSETTING.DAT
  // gives it the DJ System defaults, which is when rekordbox writes one too;
  // an export on its own leaves the stick without.
  const stickDefaults = usePreferences().djSystem;
  const hasExport = device.export !== null;
  const owner = useMemo(() => ({ path: device.path, volumeId: device.volumeId, exportStamp, hasExport, version: 0, confirmed: null as DeviceSettings | null, tail: Promise.resolve() }), [device.path, device.volumeId, exportStamp, hasExport]);
  const active = useRef<typeof owner | null>(null);
  useLayoutEffect(() => {
    active.current = owner;
    return () => { if (active.current === owner) active.current = null; };
  }, [owner]);
  useEffect(() => {
    let cancelled = false;
    setSettings(null);
    void getBackend()
      .then(async (backend) => {
        const read = await backend.deviceSettings(device.path);
        // A stick with an export but no DEVSETTING.DAT gets the defaults.
        if (cancelled || active.current !== owner || read.hasDevSetting || !hasExport) return read;
        return backend.writeDeviceDefaults(device.path, stickDefaults);
      })
      .then((read) => {
        if (!cancelled && active.current === owner) { owner.confirmed = read; setSettings(read); }
      })
      .catch((e: unknown) => {
        if (!cancelled && active.current === owner) onError?.(errorMessage(e));
      });
    return () => {
      cancelled = true;
    };
    // The defaults change with every keystroke in Preferences; the stick is
    // read again only when the device or its export changes. A preference
    // changed while the panel is open is for the next stick.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [device.path, exportStamp, hasExport, onError, owner]);

  // Every change is written at once and the panel shows what came back.
  const save = useCallback(
    (next: DeviceSettings) => {
      if (active.current !== owner || owner.confirmed === null) return;
      const version = ++owner.version;
      setSettings(next);
      const current = () => active.current === owner && owner.version === version;
      owner.tail = owner.tail.then(async () => {
        try {
          const backend = await getBackend();
          if (active.current !== owner) return;
          const saved = await backend.saveDeviceSettings(device.path, next);
          owner.confirmed = saved;
          if (current()) setSettings(saved);
        } catch (e: unknown) {
          if (current()) {
            setSettings(owner.confirmed);
            onError?.(errorMessage(e));
          }
        }
      });
    },
    [device.path, onError, owner],
  );

  return (
    <section className={styles.panel} aria-label={`Device ${device.name}`}>
      <div className={styles.strip} role="tablist" aria-label="Device settings">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            id={`device-tab-${t.id}`}
            aria-selected={tab === t.id}
            aria-controls={`device-tabpanel-${t.id}`}
            className={styles.tab}
            data-on={tab === t.id || undefined}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div
        className={styles.body}
        role="tabpanel"
        id={`device-tabpanel-${tab}`}
        aria-labelledby={`device-tab-${tab}`}
      >
        {settings === null ? null : tab === "general" ? (
          <GeneralTab
            device={device}
            settings={settings}
            onChange={save}
            playlists={playlists}
            onSync={onSync}
            onRefresh={onRefresh}
            busy={busy}
          />
        ) : tab === "category" ? (
          <ListPairTab
            kind="category"
            slots={settings.categories}
            disabled={!settings.hasMenuSettings}
            onChange={(categories) => save({ ...settings, categories })}
          />
        ) : tab === "sort" ? (
          <ListPairTab
            kind="sort"
            slots={settings.sorts}
            disabled={!settings.hasMenuSettings}
            onChange={(sorts) => save({ ...settings, sorts })}
          />
        ) : tab === "column" ? (
          <ColumnTab settings={settings} onChange={save} />
        ) : tab === "color" ? (
          <ColorTab settings={settings} onChange={save} />
        ) : (
          <MySettingsTab />
        )}
      </div>
    </section>
  );
}
