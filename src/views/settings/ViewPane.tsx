/**
 * View: Display Type, Layout and Color. Captures docs/screenshots 9.46.53
 * to 9.47.18 PM.
 *
 * Color's Appearance is drawn greyed on Dark: the light theme is not built,
 * and a radio that cannot be chosen still says the choice exists.
 */
import { getBackend } from "@/ipc/client";
import { BROWSE_SCALE_STEPS } from "@/lib/preferences";
import { LANGUAGE_CHOICES, useTranslation } from "@/i18n";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import {
  Button, Checkbox, Radios, Section, Select, Separator, Slider, Sub, Toggle,
} from "./controls";

export type ViewTab = "display" | "layout" | "color";

export const VIEW_TABS: readonly { id: ViewTab; label: string }[] = [
  { id: "display", label: "Display Type" },
  { id: "layout", label: "Layout" },
  { id: "color", label: "Color" },
];

export interface ViewPaneProps {
  tab: ViewTab;
  onResetColumns: () => void;
  onResetLayout: () => void;
}

/** Coloring of played tracks › RESET, once the question is answered OK. */
async function resetPlayedTracks(t: (text: string) => string): Promise<void> {
  const backend = await getBackend();
  if (await backend.confirm(t("Do you want to reset all coloring of played tracks?"), { yes: t("OK"), no: t("Cancel") })) {
    await backend.resetPlayedTracks();
  }
}

export function ViewPane({ tab, onResetColumns, onResetLayout }: ViewPaneProps) {
  const t = useTranslation();
  const { preferences, update } = usePreferencesContext();
  const view = preferences.view;
  const set = (patch: Partial<typeof view>) => update("view", patch);

  if (tab === "color") {
    return (
      <>
        <Section title="Appearance">
          <Radios
            label="Appearance"
            dim
            value="dark"
            choices={[
              { value: "dark", label: "Dark" },
              { value: "light", label: "Light" },
            ]}
            onChange={() => {}}
          />
        </Section>
        <Section title="Waveform color">
          {/* The three palettes rekordbox 7 draws, from the analysis files
              it wrote: BLUE from the mono waveforms, RGB from the colour
              ones, 3Band from the three-band ones. */}
          <Radios
            label="Waveform color"
            value={view.waveformColor}
            choices={[
              { value: "blue", label: "BLUE" },
              { value: "rgb", label: "RGB" },
              { value: "3band", label: "3Band" },
            ]}
            onChange={(waveformColor) => set({ waveformColor })}
          />
        </Section>
        <Section title="HOT CUE color">
          <Select
            label="HOT CUE color"
            value={view.hotCueColor}
            // rekordbox's ViewHotCueColor choices, in its order.
            choices={[
              { value: "cdj", label: "CDJ" },
              { value: "cold1", label: "COLD1" },
              { value: "cold2", label: "COLD2" },
              { value: "colorful", label: "COLORFUL" },
            ]}
            onChange={(hotCueColor) => set({ hotCueColor })}
          />
        </Section>
      </>
    );
  }

  if (tab === "layout") {
    return (
      <>
        <Section title="Layout">
          <Sub>Media Browser</Sub>
          {/* Of rekordbox's twelve sources only the Explorer exists here; the
              others are streaming services, iTunes and its own formats. */}
          <Checkbox
            label="Explorer"
            nested
            checked={view.explorer}
            onChange={(explorer) => set({ explorer })}
          />
          <Separator />
          <Sub>Browser panel</Sub>
          <Checkbox
            label="Display Cue Markers on Preview"
            nested
            checked={view.previewCueMarkers}
            onChange={(previewCueMarkers) => set({ previewCueMarkers })}
          />
          <Checkbox
            label="Display All Tracks in the Playlists"
            nested
            checked={view.allTracks}
            onChange={(allTracks) => set({ allTracks })}
          />
          <Checkbox
            label="Display the number of tracks in a playlist on the Tree View"
            nested
            checked={view.playlistCounts}
            onChange={(playlistCounts) => set({ playlistCounts })}
          />
          <Separator />
          <Sub>Waveform</Sub>
          <Checkbox
            label="Show BPM changes"
            nested
            checked={view.showBpmChanges}
            onChange={(showBpmChanges) => set({ showBpmChanges })}
          />
          <Separator />
          <Sub>Phrases</Sub>
          <Checkbox
            label="Phrase (Full Waveform)"
            nested
            checked={view.phraseFull}
            onChange={(phraseFull) => set({ phraseFull })}
          />
          <Toggle
            label="Always show types of phrases"
            nested
            checked={view.phraseLabels}
            disabled={!view.phraseFull}
            onChange={(phraseLabels) => set({ phraseLabels })}
          />
          <Separator />
          <Sub>Vocal</Sub>
          <Checkbox
            label="Vocal (Full Waveform)"
            nested
            checked={view.vocalFull}
            onChange={(vocalFull) => set({ vocalFull })}
          />
        </Section>
        {/* Ours: the columns and pane widths the browser remembers. */}
        <Section title="Browser">
          <div className={styles.actions}>
            <Button onClick={onResetColumns}>Reset columns</Button>
            <Button onClick={onResetLayout}>Reset panel sizes</Button>
          </div>
        </Section>
      </>
    );
  }

  return (
    <>
      <Section title="Language">
        <Select label="Language" value={view.locale} choices={LANGUAGE_CHOICES}
          preserveChoiceLabels onChange={(locale) => set({ locale })} />
      </Section>
      <Section title="Tooltips">
        <Toggle label="Show Tooltips" checked={view.tooltips} onChange={(tooltips) => set({ tooltips })} />
      </Section>
      <Section title="Browse">
        <Sub>FontSize</Sub>
        <Slider
          label="FontSize"
          value={view.browseFontSize}
          steps={BROWSE_SCALE_STEPS}
          onChange={(browseFontSize) => set({ browseFontSize })}
        />
        <Toggle label="Bold" nested checked={view.browseBold} onChange={(browseBold) => set({ browseBold })} />
        <Separator />
        <Sub>Line Space</Sub>
        <Slider
          label="Line Space"
          value={view.browseLineSpace}
          steps={BROWSE_SCALE_STEPS}
          onChange={(browseLineSpace) => set({ browseLineSpace })}
        />
        <Separator />
        {/* rekordbox 7.2.19's ViewBrowsePlayedColorComponent: RESET asks,
            then takes the played colour off every track
            (`buttonClicked` @0x101a79a7c). Its "Reset color when exiting"
            box is not here: it is ticked by default, which is what this
            does, keeping nothing past quitting (#540). */}
        <Sub>Coloring of played tracks</Sub>
        <div className={styles.actions}>
          <Button onClick={() => { void resetPlayedTracks(t); }}>RESET</Button>
        </div>
      </Section>
      <Section title="RBXport VU Meter">
        <Radios label="RBXport VU Meter" value={view.vuMeter}
          choices={[{ value: "normal", label: "Normal (shows signal peaks, like rekordbox)" }, { value: "fabulous", label: "Advanced (peak + RMS, inspired by FabFilter Pro-L 2)" }]}
          onChange={(vuMeter) => set({ vuMeter })} />
      </Section>
      <Section title="Key display format">
        <Radios
          label="Key display format"
          value={view.keyDisplay}
          choices={[
            { value: "classic", label: "Classic" },
            { value: "alphanumeric", label: "Alphanumeric" },
          ]}
          onChange={(keyDisplay) => set({ keyDisplay })}
        />
        <Separator />
        <Sub>Sort keys</Sub>
        <Radios
          label="Sort keys"
          nested
          value={view.keySort}
          choices={[
            { value: "alphabetical", label: "Alphabetically — A, Ab, B, …" },
            { value: "musical", label: "Musically — Abm, B, Ebm, F#, Bbm, …" },
          ]}
          onChange={(keySort) => set({ keySort })}
        />
      </Section>
      <Section title="Waveform">
        <Sub>Full/Preview Waveform</Sub>
        <Radios
          label="Full/Preview Waveform"
          nested
          value={view.overviewWaveform}
          choices={[
            { value: "half", label: "Half Waveform" },
            { value: "full", label: "Full Waveform" },
          ]}
          onChange={(overviewWaveform) => set({ overviewWaveform })}
        />
      </Section>
      <Section title="Beat Count Display">
        {/* The number beside the playhead on the enlarged waveform. */}
        <Radios
          label="Beat Count Display"
          value={view.beatCount}
          choices={[
            { value: "position", label: "Current Position (Bars)" },
            { value: "toMemoryBars", label: "Count to the next MEMORY CUE (Bars)" },
            { value: "toMemoryBeats", label: "Count to the next MEMORY CUE (Beats)" },
          ]}
          onChange={(beatCount) => set({ beatCount })}
        />
      </Section>
      <Section title="Click on the waveform for PLAY and CUE">
        {/* On to enable: a click on the enlarged waveform plays a stopped
            deck, and pauses a playing one and sets the cue at the playhead. */}
        <Toggle
          label="Enable"
          checked={view.waveformClick}
          onChange={(waveformClick) => set({ waveformClick })}
        />
      </Section>
      <Section title="Traffic Light">
        {/* rekordbox's own reaches, from its tooltip: for a track in 2A,
            Same Key lights 2A; Related Key 1 adds 2B; 2 adds 1A and 3A; 3
            adds 1B and 3B. */}
        <Select
          label="Traffic Light"
          value={view.trafficLight}
          choices={[
            { value: "same", label: "Same Key" },
            { value: "related1", label: "Related Key 1" },
            { value: "related2", label: "Related Key 2" },
            { value: "related3", label: "Related Key 3" },
          ]}
          onChange={(trafficLight) => set({ trafficLight })}
        />
      </Section>
    </>
  );
}
