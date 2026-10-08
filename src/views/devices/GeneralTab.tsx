/**
 * General: the device name, the two background-colour menus, four radio
 * groups about the player's display, the on-jog image, and the space table.
 * Capture: docs/screenshots 9.08.22 PM.
 *
 * What is real and what is drawn:
 * - Device Name, Waveform color, Waveform Current Position, Key display
 *   format: read from and written to the stick (`exportLibrary.db` and
 *   `DEVSETTING.DAT`).
 * - Type of the Overview Waveform: read from `DEVSETTING.DAT`; greyed in the
 *   capture too, so shown and not offered.
 * - Waveform Divisions: drawn, inert. Its byte is `[UNKNOWN]` — see
 *   `crates/rbl-devices/src/settings.rs`.
 * - Background Color : OneLibrary: `property.backGroundColorType` in
 *   `exportLibrary.db`. Background Color : Device Library: byte 9 of the
 *   `property` row in `export.pdb`. Both use the track-colour order, 0
 *   Default to 8 Purple [OBS rekordbox 7.2.14]. Each is offered when the
 *   stick has the database that holds it.
 * - Image On-Jog Display: drawn, inert. Where it is kept on the stick is
 *   `[UNKNOWN]`.
 * - The space table is real, and the sync control that the panel had before
 *   the captures lives beneath it, since that is the only place rekordbox's
 *   layout leaves for it.
 */
import { useCallback, useEffect, useMemo, useState } from "react";

import type {
  Device, DeviceSettings, KeyDisplay, OverviewWaveform, TreeNode, WaveformColor,
  WaveformPosition,
} from "@/ipc/types";
import { contentsText, formatSpace } from "@/lib/devices";
import styles from "./DevicePanel.module.css";
import { useTooltip } from "@/store/usePreferences";

interface RadioGroupProps<T extends string> {
  label: string;
  name: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange?: (value: T) => void;
  /** Greyed, as the capture greys Type of the Overview Waveform. */
  disabled?: boolean;
  /** Why it is greyed, for the tooltip. */
  reason?: string;
}

function RadioGroup<T extends string>({
  label,
  name,
  value,
  options,
  onChange,
  disabled = false,
  reason,
}: RadioGroupProps<T>) {
  const tip = useTooltip();
  return (
    <fieldset className={styles.group} disabled={disabled} title={tip(reason)}>
      <legend className={styles.caption}>{label}</legend>
      <div className={styles.radios}>
        {options.map((option) => (
          <label key={option.value} className={styles.radio}>
            <input
              type="radio"
              name={name}
              value={option.value}
              checked={value === option.value}
              onChange={() => onChange?.(option.value)}
            />
            {option.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

const WAVEFORM_COLORS: readonly { value: WaveformColor; label: string }[] = [
  { value: "blue", label: "BLUE" },
  { value: "rgb", label: "RGB" },
  { value: "3band", label: "3Band" },
];
const POSITIONS: readonly { value: WaveformPosition; label: string }[] = [
  { value: "left", label: "LEFT" },
  { value: "center", label: "CENTER" },
];
const DIVISIONS = [
  { value: "timescale", label: "TIMESCALE" },
  { value: "phrase", label: "PHRASE" },
] as const;
const OVERVIEWS: readonly { value: OverviewWaveform; label: string }[] = [
  { value: "half", label: "Half Waveform" },
  { value: "full", label: "Full Waveform" },
];
const KEY_DISPLAYS: readonly { value: KeyDisplay; label: string }[] = [
  { value: "classic", label: "Classic" },
  { value: "alphanumeric", label: "Alphanumeric" },
];
const JOG_IMAGES = [
  { value: "artwork", label: "ARTWORK" },
  { value: "custom", label: "CUSTOMIMAGE" },
] as const;

const UNKNOWN_DIVISIONS =
  "Not written yet: which byte of DEVSETTING.DAT holds Waveform Divisions has not been recorded.";
/** The background colours, by the value the stick stores: the track colours after Default. */
const BACKGROUND_COLORS = [
  { value: 0, label: "Default Color" },
  { value: 1, label: "Pink" },
  { value: 2, label: "Red" },
  { value: 3, label: "Orange" },
  { value: 4, label: "Yellow" },
  { value: 5, label: "Green" },
  { value: 6, label: "Aqua" },
  { value: 7, label: "Blue" },
  { value: 8, label: "Purple" },
] as const;
const NO_BACKGROUND_HINT = "Export to this device first to set its background color.";
const UNKNOWN_IMAGE =
  "Not written yet: where the stick keeps the on-jog image has not been recorded.";
const NO_LIBRARY = "This device has no library to hold a name; export something to it first.";

interface BackgroundSelectProps {
  label: string;
  /** The stored value, or null when the stick has no database to hold it. */
  value: number | null;
  onChange: (value: number) => void;
}

function BackgroundSelect({ label, value, onChange }: BackgroundSelectProps) {
  const tip = useTooltip();
  const disabled = value === null;
  const current = value ?? 0;
  // A value from a newer rekordbox is shown as its number and kept.
  const unknown = current < 0 || current >= BACKGROUND_COLORS.length;
  return (
    <label className={styles.field} title={tip(disabled ? NO_BACKGROUND_HINT : undefined)}>
      <span className={styles.caption}>{label}</span>
      <select
        className={styles.select}
        value={current}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
      >
        {BACKGROUND_COLORS.map((color) => (
          <option key={color.value} value={color.value}>{color.label}</option>
        ))}
        {unknown && <option value={current}>{current}</option>}
      </select>
    </label>
  );
}

export interface GeneralTabProps {
  device: Device;
  settings: DeviceSettings;
  onChange: (next: DeviceSettings) => void;
  playlists: readonly TreeNode[];
  onSync: (playlistId: string) => Promise<void>;
  onRefresh: () => void;
  busy: boolean;
}

export function GeneralTab({
  device,
  settings,
  onChange,
  playlists,
  onSync,
  onRefresh,
  busy,
}: GeneralTabProps) {
  // The name is committed on Enter or blur, not per keystroke: each write
  // goes to the stick.
  const [name, setName] = useState(settings.deviceName);
  useEffect(() => setName(settings.deviceName), [settings.deviceName]);
  const commitName = useCallback(() => {
    const trimmed = name.trim();
    if (trimmed === settings.deviceName) return;
    if (trimmed === "") {
      setName(settings.deviceName);
      return;
    }
    onChange({ ...settings, deviceName: trimmed });
  }, [name, onChange, settings]);

  const choices = useMemo(() => playlists.filter((node) => node.kind === "playlist"), [playlists]);
  const [chosen, setChosen] = useState("");
  const playlistId = chosen || choices[0]?.id || "";
  const sync = useCallback(() => {
    if (playlistId) void onSync(playlistId);
  }, [onSync, playlistId]);

  const libraries = [
    settings.hasDeviceLibrary ? "Device Library" : "",
    settings.hasOneLibrary ? "OneLibrary" : "",
  ]
    .filter(Boolean)
    .join(", ");

  const tip = useTooltip();
  return (
    <div className={styles.general}>
      <label className={styles.field}>
        <span className={styles.caption}>Device Name</span>
        <input
          className={styles.text}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={commitName}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
          }}
          disabled={!settings.hasLibrarySettings}
          title={tip(settings.hasLibrarySettings ? undefined : NO_LIBRARY)}
          maxLength={64}
        />
      </label>

      <BackgroundSelect
        label="Background Color : OneLibrary"
        value={settings.hasLibrarySettings ? settings.backgroundColorType : null}
        onChange={(backgroundColorType) => onChange({ ...settings, backgroundColorType })}
      />
      <BackgroundSelect
        label="Background Color : Device Library"
        value={settings.deviceLibraryBackgroundColorType}
        onChange={(deviceLibraryBackgroundColorType) =>
          onChange({ ...settings, deviceLibraryBackgroundColorType })}
      />

      <RadioGroup
        label="Waveform color"
        name="waveform-color"
        value={settings.waveformColor}
        options={WAVEFORM_COLORS}
        onChange={(waveformColor) => onChange({ ...settings, waveformColor })}
      />
      <RadioGroup
        label="Waveform Current Position"
        name="waveform-position"
        value={settings.waveformPosition}
        options={POSITIONS}
        onChange={(waveformPosition) => onChange({ ...settings, waveformPosition })}
      />
      <RadioGroup
        label="Waveform Divisions"
        name="waveform-divisions"
        value="timescale"
        options={DIVISIONS}
        disabled
        reason={UNKNOWN_DIVISIONS}
      />
      <RadioGroup
        label="Type of the Overview Waveform"
        name="overview-waveform"
        value={settings.overviewWaveform}
        options={OVERVIEWS}
        disabled
        reason="Shown as the stick has it; rekordbox does not offer it here either."
      />
      <RadioGroup
        label="Key display format"
        name="key-display"
        value={settings.keyDisplay}
        options={KEY_DISPLAYS}
        onChange={(keyDisplay) => onChange({ ...settings, keyDisplay })}
      />

      <fieldset className={styles.group} title={tip(UNKNOWN_IMAGE)} disabled>
        <legend className={styles.caption}>Image On-Jog Display</legend>
        <div className={styles.radios}>
          {JOG_IMAGES.map((option, i) => (
            <label key={option.value} className={styles.radio} data-dim={i === 1 || undefined}>
              <input type="radio" name="jog-image" value={option.value} checked={i === 0} readOnly />
              {option.label}
            </label>
          ))}
        </div>
        <div className={styles.jog}>
          <div className={styles.disc}>
            <span className={styles.discRing} aria-hidden />
            <span className={styles.discText}>No image</span>
          </div>
          <div className={styles.imageActions}>
            {["Copy from [Preferences]", "Import image file", "Delete image"].map((label) => (
              <button key={label} type="button" className={styles.imageButton} disabled>
                <span className={styles.imageGlyph} aria-hidden />
                {label}
              </button>
            ))}
          </div>
        </div>
      </fieldset>

      <table className={styles.space}>
        <tbody>
          <tr>
            <th scope="row">Total Space</th>
            <td>{formatSpace(device.totalBytes)}</td>
          </tr>
          <tr>
            <th scope="row">Available Space</th>
            <td>{formatSpace(device.freeBytes)}</td>
          </tr>
          <tr>
            <th scope="row">Device Library</th>
            <td>{libraries}</td>
          </tr>
        </tbody>
      </table>

      <div className={styles.sync}>
        <p className={styles.contents}>{contentsText(device)}</p>
        <div className={styles.actions}>
          <label className={styles.caption} htmlFor="device-playlist">
            Playlist
          </label>
          <select
            id="device-playlist"
            className={styles.select}
            value={playlistId}
            onChange={(e) => setChosen(e.target.value)}
            disabled={choices.length === 0 || busy}
          >
            {choices.map((node) => (
              <option key={node.id} value={node.id}>
                {node.name}
              </option>
            ))}
          </select>
          <button
            type="button"
            className={styles.button}
            onClick={sync}
            disabled={playlistId === "" || busy}
          >
            {busy ? "Writing…" : device.export?.ours === true ? "Sync" : "Export"}
          </button>
          <button type="button" className={styles.button} onClick={onRefresh}>
            Refresh
          </button>
        </div>
        {choices.length === 0 ? (
          <p className={styles.contents}>There are no playlists to write yet.</p>
        ) : null}
      </div>
    </div>
  );
}
