import { describe, expect, it } from "vitest";

import {
  browseListVars,
  browseScale,
  BROWSE_SCALE_DEFAULT,
  DEFAULT_PREFERENCES,
  formatKey,
  quantizeFraction,
  sanitisePreferences,
} from "./preferences";

describe("sanitisePreferences", () => {
  it("enables rekordbox browse sync by default and remembers an explicit off choice", () => {
    expect(DEFAULT_PREFERENCES.rekordbox.syncBrowseSettings).toBe(true);
    expect(sanitisePreferences({ view: {} }).rekordbox.syncBrowseSettings).toBe(true);
    expect(sanitisePreferences({ rekordbox: { syncBrowseSettings: "no" } }).rekordbox.syncBrowseSettings).toBe(true);
    expect(sanitisePreferences({ rekordbox: { syncBrowseSettings: false } }).rekordbox.syncBrowseSettings).toBe(false);
  });
  it("protects a new library without making existing users read-only", () => {
    expect(DEFAULT_PREFERENCES.advanced.protectLibrary).toBe(true);
    expect(sanitisePreferences({ view: { tooltips: true } }).advanced.protectLibrary).toBe(false);
    expect(sanitisePreferences({ advanced: { protectLibrary: false } }).advanced.protectLibrary).toBe(false);
    expect(sanitisePreferences({ advanced: { protectLibrary: true } }).advanced.protectLibrary).toBe(true);
  });

  it("preserves the BPM-change visibility preference and enables it for older settings", () => {
    expect(sanitisePreferences({view: {showBpmChanges: false}}).view.showBpmChanges).toBe(false);
    expect(sanitisePreferences({view: {}}).view.showBpmChanges).toBe(true);
  });
  it("keeps a supported locale and rejects unknown ones", () => {
    expect(sanitisePreferences({ view: { locale: "ja" } }).view.locale).toBe("ja");
    expect(sanitisePreferences({ view: { locale: "xx" } }).view.locale).toBe("en");
  });
  it("keeps the LINK key-sort choice and defaults invalid or older settings to musical", () => {
    expect(sanitisePreferences({djSystem: {linkKeySort: "alphabetical"}}).djSystem.linkKeySort).toBe("alphabetical");
    expect(sanitisePreferences({djSystem: {linkKeySort: "invalid"}}).djSystem.linkKeySort).toBe("musical");
  });
  it("leaves automatic LINK joining off unless it is explicitly enabled", () => {
    expect(DEFAULT_PREFERENCES.djSystem.autoJoinLink).toBe(false);
    expect(sanitisePreferences({ djSystem: {} }).djSystem.autoJoinLink).toBe(false);
    expect(sanitisePreferences({ djSystem: { autoJoinLink: "yes" } }).djSystem.autoJoinLink).toBe(false);
    expect(sanitisePreferences({ djSystem: { autoJoinLink: true } }).djSystem.autoJoinLink).toBe(true);
  });
  it("leaves automatic analysis off unless it is explicitly enabled", () => {
    expect(DEFAULT_PREFERENCES.analysis.auto).toBe(false);
    expect(sanitisePreferences({ analysis: {} }).analysis.auto).toBe(false);
    expect(sanitisePreferences({ analysis: { auto: "yes" } }).analysis.auto).toBe(false);
    expect(sanitisePreferences({ analysis: { auto: true } }).analysis.auto).toBe(true);
  });

  it("leaves the first-beat memory cue off unless it is explicitly enabled", () => {
    expect(DEFAULT_PREFERENCES.analysis.firstBeatCue).toBe(false);
    expect(sanitisePreferences({ analysis: { firstBeatCue: "yes" } }).analysis.firstBeatCue).toBe(false);
    expect(sanitisePreferences({ analysis: { firstBeatCue: true } }).analysis.firstBeatCue).toBe(true);
  });

  it("keeps the browser key-sort choice and preserves the old display-based ordering", () => {
    expect(sanitisePreferences({view: {keySort: "musical"}}).view.keySort).toBe("musical");
    expect(sanitisePreferences({view: {keySort: "invalid"}}).view.keySort).toBe("alphabetical");
    expect(sanitisePreferences({view: {keyDisplay: "classic"}}).view.keySort).toBe("alphabetical");
    expect(sanitisePreferences({view: {keyDisplay: "alphanumeric"}}).view.keySort).toBe("musical");
  });
  it("gives the defaults for nothing, garbage, and a wrong shape", () => {
    expect(sanitisePreferences(undefined)).toEqual(DEFAULT_PREFERENCES);
    expect(sanitisePreferences("view")).toEqual(DEFAULT_PREFERENCES);
    expect(sanitisePreferences({ view: 3, advanced: [] })).toEqual(DEFAULT_PREFERENCES);
  });

  it("keeps every valid choice and replaces each bad one on its own", () => {
    const stored = {
      view: {
        tooltips: false,
        browseFontSize: 4,
        browseLineSpace: 9,
        keyDisplay: "alphanumeric",
        keySort: "alphabetical",
        overviewWaveform: "full",
        explorer: "yes",
      },
      analysis: { auto: false },
      djSystem: { waveformColor: "rgb", subColumn: 5.5, categories: [{ id: 1 }], linkInterface: "" },
      advanced: {
        relocateFolders: ["/a", "", 3, "/b"],
        syncType: "bpm",
        quantizeBeat: "1/16",
        protectLibrary: true,
      },
    };
    const out = sanitisePreferences(stored);
    expect(out.view.tooltips).toBe(false);
    expect(out.view.browseFontSize).toBe(4);
    expect(out.view.browseLineSpace).toBe(BROWSE_SCALE_DEFAULT);
    expect(out.view.keyDisplay).toBe("alphanumeric");
    expect(out.view.keySort).toBe("alphabetical");
    expect(out.view.overviewWaveform).toBe("full");
    expect(out.view.explorer).toBe(true);
    expect(out.analysis.auto).toBe(false);
    expect(out.djSystem.waveformColor).toBe("rgb");
    expect(out.djSystem.subColumn).toBeNull();
    // A row that is not a row means the reference rows, not a broken list.
    expect(out.djSystem.categories).toBeNull();
    // An empty interface name is no choice: LINK picks for itself.
    expect(out.djSystem.linkInterface).toBeNull();
    expect(sanitisePreferences({ djSystem: { linkInterface: "en11" } }).djSystem.linkInterface).toBe("en11");
    expect(out.advanced.relocateFolders).toEqual(["/a", "/b"]);
    expect(out.advanced.syncType).toBe("bpm");
    expect(out.advanced.quantizeBeat).toBe("1/1");
    expect(out.advanced.protectLibrary).toBe(true);
  });

  it("checks for updates unless the store plainly says not to", () => {
    // A store written before the switch existed has no such key.
    expect(sanitisePreferences({ advanced: {} }).advanced.checkUpdates).toBe(true);
    expect(sanitisePreferences({ advanced: { checkUpdates: "no" } }).advanced.checkUpdates).toBe(true);
    expect(sanitisePreferences({ advanced: { checkUpdates: 0 } }).advanced.checkUpdates).toBe(true);
    expect(sanitisePreferences({ advanced: { checkUpdates: false } }).advanced.checkUpdates).toBe(false);
    expect(sanitisePreferences({ advanced: {} }).advanced.updateFrequency).toBe("start");
  });



  it("creates database folders on a blank drive unless the store plainly says not to", () => {
    expect(sanitisePreferences({ djSystem: {} }).djSystem.createDatabaseFolders).toBe(true);
    expect(sanitisePreferences({ djSystem: { createDatabaseFolders: "no" } }).djSystem.createDatabaseFolders).toBe(true);
    expect(sanitisePreferences({ djSystem: { createDatabaseFolders: false } }).djSystem.createDatabaseFolders).toBe(false);
  });

  it("keeps stored rows when every one is a row", () => {
    const rows = [{ id: 1, menuItem: 1, name: "GENRE", seq: 1, visible: true }];
    expect(sanitisePreferences({ djSystem: { categories: rows } }).djSystem.categories).toEqual(rows);
  });
});

describe("formatKey", () => {
  it("shows the library's name, or the Camelot code when asked", () => {
    expect(formatKey("Ebm", "classic")).toBe("Ebm");
    expect(formatKey("Ebm", "alphanumeric")).toBe("2A");
    expect(formatKey("C", "alphanumeric")).toBe("8B");
    // A key the wheel does not know is shown as it is, not hidden.
    expect(formatKey("Unknown", "alphanumeric")).toBe("Unknown");
    expect(formatKey("", "alphanumeric")).toBe("");
  });
});

describe("the sliders and the quantize value", () => {
  it("scale from the measured size in the middle", () => {
    expect(browseScale(BROWSE_SCALE_DEFAULT)).toBe(1);
    expect(browseScale(0)).toBeLessThan(1);
    expect(browseScale(4)).toBeGreaterThan(1);
    expect(browseScale(99)).toBe(1);
  });

  it("turn a beat value into a fraction of a beat", () => {
    expect(quantizeFraction("1/1")).toBe(1);
    expect(quantizeFraction("1/2")).toBe(0.5);
    expect(quantizeFraction("1/8")).toBe(0.125);
  });
});

 it("defaults USB imports to history only and preserves saved choices", () => {
  expect(sanitisePreferences({}).usbExport).toEqual({ importSettings: false, importHistory: true, deleteUnlistedMusic: false, maximumCompatibility: false, conversionFormat: "wav", importButtonCues: true, importButtonHistory: true, importButtonSettings: false });
  expect(sanitisePreferences({ usbExport: { importSettings: true, importHistory: false } }).usbExport).toEqual({ importSettings: true, importHistory: false, deleteUnlistedMusic: false, maximumCompatibility: false, conversionFormat: "wav", importButtonCues: true, importButtonHistory: true, importButtonSettings: false });
  expect(sanitisePreferences({ usbExport: { importButtonCues: false, importButtonSettings: true } }).usbExport).toMatchObject({ importButtonCues: false, importButtonHistory: true, importButtonSettings: true });
});

it("requires an explicit boolean to enable USB music cleanup", () => {
  expect(sanitisePreferences({ usbExport: { deleteUnlistedMusic: true } }).usbExport.deleteUnlistedMusic).toBe(true);
  expect(sanitisePreferences({ usbExport: { deleteUnlistedMusic: "true" } }).usbExport.deleteUnlistedMusic).toBe(false);
});

it("defaults compatibility conversion to off and WAV, and preserves AIFF and MP3 selections", () => {
  const defaults = sanitisePreferences({}).usbExport;
  expect(defaults.maximumCompatibility).toBe(false);
  expect(defaults.conversionFormat).toBe("wav");
  expect(sanitisePreferences({ usbExport: { maximumCompatibility: true, conversionFormat: "mp3" } }).usbExport)
    .toMatchObject({ maximumCompatibility: true, conversionFormat: "mp3" });
  expect(sanitisePreferences({ usbExport: { maximumCompatibility: true, conversionFormat: "aiff" } }).usbExport)
    .toMatchObject({ maximumCompatibility: true, conversionFormat: "aiff" });
  expect(sanitisePreferences({ usbExport: { maximumCompatibility: "yes", conversionFormat: "flac" } }).usbExport)
    .toMatchObject({ maximumCompatibility: false, conversionFormat: "wav", importButtonCues: true, importButtonHistory: true, importButtonSettings: false });
});

describe("browseListVars", () => {
  it("scales row height and font size from the Browse sliders", () => {
    const v = { browseFontSize: 4, browseLineSpace: 0, browseBold: true };
    const vars = browseListVars(v, 25);
    expect(vars["--s-row-height"]).toBe("20px");
    expect(vars["--f-size-ui"]).toBe("calc(1.3 * var(--f-size-ui-base))");
    expect(vars["--browse-weight"]).toBe(700);
  });
  it("is the measured size at the default stops", () => {
    const vars = browseListVars(
      { browseFontSize: BROWSE_SCALE_DEFAULT, browseLineSpace: BROWSE_SCALE_DEFAULT, browseBold: false },
      25,
    );
    expect(vars["--s-row-height"]).toBe("25px");
    expect(vars["--browse-weight"]).toBe(400);
  });
});
