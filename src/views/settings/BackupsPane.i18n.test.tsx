// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BackupsPane } from "./BackupsPane";

// The German catalog through the real lookup, without the DOM localizer, so
// only text passed through useTranslation() comes out translated.
vi.mock("@/i18n", async () => {
  const { translate } = await vi.importActual<typeof import("@/i18n")>("@/i18n");
  const de = (await import("../../../public/locales/de.json")).default;
  return {
    useTranslation: () => (text: string, values: Readonly<Record<string, string | number>> = {}) => {
      let result = translate(text, de);
      for (const [name, value] of Object.entries(values)) result = result.replaceAll(`{${name}}`, String(value));
      return result;
    },
  };
});

const held = vi.hoisted(() => ({
  backend: { backupSizes: vi.fn(), startBackup: vi.fn(), cancelBackup: vi.fn(), backupProgress: vi.fn(), backupDirectory: vi.fn(), listBackups: vi.fn(), setBackupDirectory: vi.fn(), pickFolder: vi.fn(), deleteBackup: vi.fn(), confirm: vi.fn() },
}));
vi.mock("@/ipc/client", () => ({ getBackend: () => Promise.resolve(held.backend) }));
let host: HTMLDivElement;
let root: Root;
const button = (name: string) => [...host.querySelectorAll("button")].find(b => b.textContent === name);
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.resetAllMocks();
  held.backend.listBackups.mockResolvedValue([{ path: "/backups/rbexport-test.zip", name: "rbexport-test.zip", bytes: 1048576, createdAt: 1700000000000, includesAnalysis: true }]);
  held.backend.backupDirectory.mockResolvedValue("/backups");
  held.backend.confirm.mockResolvedValue(false);
  held.backend.backupSizes.mockResolvedValue({ updatedAt: Date.now() - 3 * 24 * 60 * 60 * 1000, trackCount: 1, artwork: 32, vocals: 64, database: 1024, waveforms: 4096, cues: 256, beatGrids: 512, phrases: 128, other: 128 });
  held.backend.backupProgress.mockResolvedValue({ running: false, phase: "", copiedBytes: 0, totalBytes: 0, path: null, error: null });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

it("shows the Backups preferences in the chosen language, including composed and dialog text", async () => {
  await act(async () => { root.render(<BackupsPane />); await Promise.resolve(); });
  await act(async () => { await Promise.resolve(); });
  expect(button("Backup erstellen")).toBeDefined();
  expect(host.textContent).toContain("Rekordbox-Daten");
  expect(host.textContent).toContain("insgesamt · 1 Titel");
  expect(host.querySelector("time")?.textContent).toBe("vor 3 Tagen");
  expect(host.textContent).toContain("Wellenformvorschauen");

  await act(async () => { button("Löschen")?.click(); await Promise.resolve(); });
  expect(held.backend.confirm).toHaveBeenCalledWith(expect.stringMatching(/^Sicherung vom .+ löschen\? Dies kann nicht rückgängig gemacht werden\.$/));

  held.backend.pickFolder.mockResolvedValue(null);
  await act(async () => { button("Ordner wechseln…")?.click(); await Promise.resolve(); });
  expect(held.backend.pickFolder).toHaveBeenCalledWith("Wählen Sie den Standard-Sicherungsordner");
});
