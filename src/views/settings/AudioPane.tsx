import { VuMeterFill } from "@/components/VuMeterFill";
import type { VuDisplay } from "@/lib/vuMeter";
/**
 * Audio › Configuration. Capture docs/screenshots 9.47.23 PM: the output
 * device, Sample Rate, Buffer size and the Metronome, and then a section of
 * ours — the master limiter, which rekordbox's pane does not have: two
 * decks at full level sum past what the output can carry, and this is where
 * the switch and its two numbers live. Input/Output, the channel routing
 * tab, is not here: there is one stereo output and nothing to route.
 */
import { useEffect, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { AudioDevices, Limiter } from "@/ipc/types";
import { BUFFER_SIZES, SAMPLE_RATES, type SampleRate } from "@/lib/preferences";
import { CEILING_DB, DEFAULT_LIMITER, INPUT_GAIN_DB, RELEASE_MS } from "@/store/useLimiter";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import limiterStyles from "./MasterLimiter.module.css";
import { Button, Note, Radios, Section, Select, Separator, Slider, Sub, Toggle } from "./controls";

export type AudioTab = "configuration";

export const AUDIO_TABS: readonly { id: AudioTab; label: string }[] = [
  { id: "configuration", label: "Configuration" },
];

export interface AudioPaneProps {
  tab: AudioTab;
  limiter: Limiter;
  onLimiterChange: (change: Partial<Limiter>) => void;
  /** How far the limiter is turning the sum down right now, in dB. */
  reduction: number;
  /** The loudest sample the device was given, per channel, 0 to 1. */
  vu?: VuDisplay | undefined;
  peakLeft?: number;
  peakRight?: number;
}

/** `512 samples (10.7 ms)`, as the capture prints the buffer size. */
export function bufferCaption(frames: number, sampleRate: number): string {
  const ms = sampleRate > 0 ? (frames / sampleRate) * 1000 : 0;
  return `${frames} samples (${ms.toFixed(1)} ms)`;
}

/** A level as the meter's own scale: −∞ for silence, else decibels below full. */
function decibels(peak: number): string {
  if (!(peak > 0)) return "−∞ dB";
  return `${(20 * Math.log10(Math.min(peak, 1))).toFixed(1)} dB`;
}

/** How much of a meter a level fills: a sixty-decibel scale. */
function fillOf(peak: number): number {
  if (!(peak > 0)) return 0;
  return Math.min(Math.max(1 + (20 * Math.log10(Math.min(peak, 1))) / 60, 0), 1);
}

/** The reduction meters' scale: twelve decibels of gain reduction is full. */
const REDUCTION_FULL_DB = 12;

export function AudioPane({ limiter, onLimiterChange, reduction, vu, peakLeft = 0, peakRight = 0 }: AudioPaneProps) {
  const [audio, setAudio] = useState<AudioDevices | null>(null);
  const { preferences, update } = usePreferencesContext();
  const prefs = preferences.audio;
  const gainReduction = limiter.enabled && Number.isFinite(reduction) ? Math.max(0, reduction) : 0;
  const limiterStatus = !limiter.enabled ? "Off" : gainReduction >= 0.1 ? "Limiting" : "Ready";
  const set = (patch: Partial<typeof prefs>) => update("audio", patch);
  // The slider moves over the stops; the stop is what is stored.
  const bufferStop = Math.max(0, BUFFER_SIZES.indexOf(prefs.bufferSize));

  // A chosen device that is not in the list is one that has been unplugged:
  // the engine is on the system default, and the select says so.
  const chosenListed = audio === null || audio.chosen === null || audio.devices.some((d) => d.id === audio.chosen);

  // Read when the pane opens rather than held: an interface is plugged in
  // while the app is running more often than not, and a list from launch
  // would be missing whatever somebody just connected.
  useEffect(() => {
    void (async () => {
      try {
        const backend = await getBackend();
        setAudio(await backend.audioDevices());
      } catch {
        // A build with no engine behind it has no devices to offer, and the
        // pane says so rather than showing an error for something nobody
        // asked about.
      }
    })();
  }, []);

  return (
    <>
    <Section title="Audio" label="Audio output">
      {audio && audio.devices.length > 0 ? (
        <>
          <div className={styles.actions}>
            <select
              className={styles.select}
              aria-label="Audio output device"
              value={chosenListed ? audio.chosen ?? "" : ""}
              onChange={(event) => {
                const chosen = event.target.value === "" ? null : event.target.value;
                setAudio({ ...audio, chosen });
                // Remembered first, so the choice survives a failed push and
                // the next launch; the engine is told directly as well, since
                // this pane can be in a window with no App behind it.
                set({ outputDevice: chosen });
                void (async () => {
                  try {
                    const backend = await getBackend();
                    await backend.setAudioDevice(chosen);
                  } catch {
                    // No engine behind this build; the choice is still kept.
                  }
                })();
              }}
            >
              <option value="">
                System default
                {audio.devices.find((d) => d.id === audio.default)?.name
                  ? ` — ${audio.devices.find((d) => d.id === audio.default)?.name}`
                  : ""}
              </option>
              {audio.devices.map((device) => (
                <option key={device.id} value={device.id}>
                  {device.name}
                </option>
              ))}
            </select>
          </div>
          {!chosenListed && (
            <Note>
              The saved output device is not connected, so the system default
              is in use. The choice is kept and used again when it returns.
            </Note>
          )}
        </>
      ) : (
        <Note>
          No output devices to choose from — this build has no audio engine
          behind it.
        </Note>
      )}
    </Section>

    <Section title="Sample Rate">
      <Select
        label="Sample Rate"
        value={String(prefs.sampleRate)}
        choices={SAMPLE_RATES.map((rate) => ({ value: String(rate), label: `${rate} Hz` }))}
        onChange={(value) => set({ sampleRate: Number(value) as SampleRate })}
      />
    </Section>

    <Section title="Buffer size">
      <div className={styles.row} data-nested="">
        <span data-testid="buffer-size">{bufferCaption(prefs.bufferSize, prefs.sampleRate)}</span>
      </div>
      <Slider
        label="Buffer size"
        value={bufferStop}
        steps={BUFFER_SIZES.length}
        ends={[`${BUFFER_SIZES[0]}`, `${BUFFER_SIZES[BUFFER_SIZES.length - 1]}`]}
        onChange={(stop) => set({ bufferSize: BUFFER_SIZES[stop] ?? prefs.bufferSize })}
      />
    </Section>

    <Section title="Metronome">
      <Radios
        label="Metronome"
        value={String(prefs.metronomeSound)}
        choices={[
          { value: "1", label: "Click Sound 01" },
          { value: "2", label: "Click Sound 02" },
          { value: "3", label: "Click Sound 03" },
        ]}
        onChange={(value) => set({ metronomeSound: Number(value) as 1 | 2 | 3 })}
      />
      <Separator />
      <Sub>Volume</Sub>
      <Radios
        label="Metronome volume"
        nested
        value={prefs.metronomeVolume}
        choices={[
          { value: "small", label: "Small" },
          { value: "middle", label: "Middle" },
          { value: "large", label: "Large" },
        ]}
        onChange={(metronomeVolume) => set({ metronomeVolume })}
      />
    </Section>

    <Section title="RBXport Master Limiter">
      <div className={limiterStyles.panel}>
      <div className={limiterStyles.header}>
        <Toggle label="Enable limiter" checked={limiter.enabled} onChange={(enabled) => onLimiterChange({ enabled })} />
        <span className={limiterStyles.status} data-state={limiterStatus}>{limiterStatus}</span>
      </div>
      <p className={limiterStyles.help}>Keeps the combined deck output below the ceiling by turning down peaks.</p>
      <div className={limiterStyles.monitor}>
      <div className={styles.meters} role="group" aria-label="Master output">
        <span className={styles.meterCaption}>Output</span>
        {([["L", peakLeft], ["R", peakRight]] as const).map(([channel, peak]) => (
          <div key={channel} className={styles.meterRow}>
            <span className={styles.meterLabel}>{channel}</span>
            <div
              className={styles.meterBar}
              role="meter"
              aria-label={`Output ${channel}`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round((vu?.[channel === "L" ? "left" : "right"].peak ?? fillOf(peak)) * 100)}
              aria-valuetext={decibels(peak)}
              data-mode={vu?.mode ?? "normal"}
            >
              <VuMeterFill mode={vu?.mode ?? "normal"} channel={vu?.[channel === "L" ? "left" : "right"] ?? { peak: fillOf(peak), rms: 0, marker: 0 }} />
            </div>
            <span className={styles.meterDb}>{decibels(peak)}</span>
          </div>
        ))}
      </div>
      <div className={styles.meters} role="group" aria-label="Limiter reduction">
        <div className={limiterStyles.readout}>
          <span className={styles.meterCaption}>Gain reduction</span>
          <span className={limiterStyles.value}>{gainReduction > 0 ? `−${gainReduction.toFixed(1)}` : "0.0"} dB</span>
        </div>
        <div className={styles.meterBar} role="meter" aria-label="Gain reduction"
          aria-valuemin={0} aria-valuemax={REDUCTION_FULL_DB}
          aria-valuenow={Math.min(gainReduction, REDUCTION_FULL_DB)}
          aria-valuetext={limiter.enabled ? `${gainReduction.toFixed(1)} dB reduction` : "Limiter off"}>
          <span className={styles.meterFill} data-reduction style={{ width: `${Math.min(gainReduction / REDUCTION_FULL_DB, 1) * 100}%` }} />
        </div>
      </div>
      </div>
      <div className={limiterStyles.controls}>
        <div className={limiterStyles.field}>
          <div className={limiterStyles.readout}>
            <strong>Input gain</strong>{" "}<span className={limiterStyles.value}>{limiter.inputGainDb > 0 ? "+" : ""}{limiter.inputGainDb.toFixed(1)} dB</span>
          </div>
          <p className={limiterStyles.help}>Level before limiting.</p>
          <Slider label="Limiter input gain" value={limiter.inputGainDb} range={INPUT_GAIN_DB}
            ends={[`${INPUT_GAIN_DB.min} dB`, `+${INPUT_GAIN_DB.max} dB`]}
            disabled={!limiter.enabled} onChange={(inputGainDb) => onLimiterChange({ inputGainDb })} />
        </div>
        <div className={limiterStyles.field}>
          <div className={limiterStyles.readout}>
            <strong>Ceiling</strong>{" "}<span className={limiterStyles.value}>{limiter.ceilingDb.toFixed(1)} dBFS</span>
          </div>
          <p className={limiterStyles.help}>Maximum output level.</p>
          <Slider label="Limiter ceiling" value={limiter.ceilingDb} range={CEILING_DB}
            ends={[`${CEILING_DB.min} dBFS`, `${CEILING_DB.max} dBFS`]}
            disabled={!limiter.enabled} onChange={(ceilingDb) => onLimiterChange({ ceilingDb })} />
        </div>
        <div className={limiterStyles.field}>
          <div className={limiterStyles.readout}>
            <strong>Release</strong>{" "}<span className={limiterStyles.value}>{Math.round(limiter.releaseMs)} ms</span>
          </div>
          <p className={limiterStyles.help}>Recovery after a peak. Longer is smoother.</p>
          <Slider label="Limiter release" value={limiter.releaseMs} range={RELEASE_MS}
            ends={[`${RELEASE_MS.min} ms · Fast`, `${RELEASE_MS.max} ms · Slow`]}
            disabled={!limiter.enabled} onChange={(releaseMs) => onLimiterChange({ releaseMs })} />
        </div>
      </div>
      <div className={limiterStyles.footer}>
        <Button disabled={!limiter.enabled || (limiter.inputGainDb === DEFAULT_LIMITER.inputGainDb && limiter.ceilingDb === DEFAULT_LIMITER.ceilingDb && limiter.releaseMs === DEFAULT_LIMITER.releaseMs)}
          onClick={() => onLimiterChange({ inputGainDb: DEFAULT_LIMITER.inputGainDb, ceilingDb: DEFAULT_LIMITER.ceilingDb, releaseMs: DEFAULT_LIMITER.releaseMs })}>Reset settings</Button>
      </div>
      </div>
    </Section>
    </>
  );
}
