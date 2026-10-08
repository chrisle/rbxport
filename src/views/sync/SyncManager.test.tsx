/**
 * @vitest-environment jsdom
 *
 * The Sync Manager's ticking and its SYNC: a folder ticks what is under it
 * and shows part-way when only some of that is ticked, a ticked stick brings
 * its last selection back, and SYNC sends the ticked playlists to every
 * ticked stick and reports on each.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, Device, DeviceSyncState, ItunesLibrary, SyncDeviceReport, SyncProgress, TreeNode, ExportProgress } from "@/ipc/types";
import { SyncManager } from "./SyncManager";
import { PreferencesProvider } from "@/store/usePreferences";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}
let exportProgress: ((progress: ExportProgress) => void) | undefined;
const cancelExport = vi.fn(() => Promise.resolve());

const TREE: TreeNode[] = [
  { id: "all", name: "All Tracks", kind: "allTracks", depth: 0 },
  { id: "playlists", name: "Playlists", kind: "collection", depth: 0, expanded: true },
  { id: "f1", name: "Sets", kind: "folder", depth: 1, expanded: true },
  { id: "p1", name: "Warm Up", kind: "playlist", depth: 2 },
  { id: "p2", name: "Main Set", kind: "playlist", depth: 2 },
  { id: "s1", name: "Peak Time", kind: "smartPlaylist", depth: 2 },
  { id: "p3", name: "Closing", kind: "playlist", depth: 1 },
  { id: "histories", name: "Histories", kind: "histories", depth: 0 },
];

const stick = (name: string): Device => ({
  name,
  path: `/Volumes/${name}`,
  totalBytes: 32 * 1024 ** 3,
  freeBytes: 24 * 1024 ** 3,
  fileSystem: "FAT32",
  removable: true,
  volumeId: "dev:1",
  export: null,
});
const DEVICES = [stick("USB A"), stick("USB B")];

const STATES: Record<string, DeviceSyncState> = {
  "/Volumes/USB A": { selected: [{ libraryId: "p3", name: "Closing" }], onDevice: ["Closing"], automatic: true },
  "/Volumes/USB B": { selected: [], onDevice: [], automatic: false },
};

const report = (path: string, tracks: number): SyncDeviceReport => ({
  path,
  report: { tracks, playlists: 1, bytesCopied: 0, analysisFiles: 0, reused: 0, removed: 0, playlistsAdded: 1, playlistsRemoved: 0, skipped: [], verified: true },
});

let host: HTMLDivElement;
let root: Root;
let devicesChanged: (() => void) | undefined;
let libraryChanged: (() => void) | undefined;
let tree: TreeNode[];
let readTree: () => Promise<TreeNode[]>;
let listDevices: ReturnType<typeof vi.fn>;
let importUsb: ReturnType<typeof vi.fn>;
let syncDevices: ReturnType<typeof vi.fn>;
let validateExportFiles: ReturnType<typeof vi.fn>;
let confirmExport: ReturnType<typeof vi.fn>;
let ejectDevice: ReturnType<typeof vi.fn>;
let progress: ((p: SyncProgress) => void) | null;
let onClose: ReturnType<typeof vi.fn>;
let rekordboxOpen: boolean;
let itunesLibrary: ItunesLibrary | null;
let importItunesSelected: ReturnType<typeof vi.fn>;

const settle = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });

const box = (label: string) => host.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);
const click = (el: HTMLElement | null | undefined) => {
  if (!el) throw new Error("nothing to click");
  act(() => el.click());
};
const status = () => host.querySelector('[role="status"]')?.textContent ?? "";

beforeEach(async () => {
  cancelExport.mockClear();
  devicesChanged = undefined;
  libraryChanged = undefined;
  tree = TREE;
  readTree = () => Promise.resolve(tree.map((n) => ({ ...n })));
  listDevices = vi.fn(() => Promise.resolve(DEVICES.map(d => ({ ...d }))));
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  progress = null;
  rekordboxOpen = false;
  onClose = vi.fn();
  importUsb = vi.fn(() => Promise.resolve({ tracks: 2, histories: 0, settings: 0, skipped: 0 }));
  ejectDevice = vi.fn(() => Promise.resolve());
  syncDevices = vi.fn((playlists: string[], destinations: string[]) =>
    Promise.resolve(destinations.map((path) => report(path, playlists.length * 10))),
  );
  validateExportFiles = vi.fn(() => Promise.resolve([]));
  confirmExport = vi.fn(() => Promise.resolve(true));
  itunesLibrary = null;
  importItunesSelected = vi.fn(() => Promise.resolve({ imported: 5, existing: 0, skipped: [], playlists: 1, cues: 0, tracks: [] }));
  __setBackend({
    librarySummary: () => Promise.resolve({ trackCount: 3, playlistCount: 3, readOnly: rekordboxOpen, dbVersion: 6000 }),
    playlistTree: () => readTree(),
    onLibraryChanged: (listener: () => void) => {
      libraryChanged = listener;
      return () => { libraryChanged = undefined; };
    },
    itunesDefaultLibrary: () => Promise.resolve(itunesLibrary),
    chooseItunesLibrary: () => Promise.resolve(null),
    importItunesSelected,
    listDevices,
    onDevicesChanged: (listener: () => void) => {
      devicesChanged = listener;
      return () => { devicesChanged = undefined; };
    },
    deviceSyncState: (path: string) => {
      const state = STATES[path];
      return state ? Promise.resolve(state) : Promise.reject(new Error("gone"));
    },
    syncDevices,
    validateExportFiles,
    cancelExport,
    ejectDevice,
    onExportProgress: (listener: (progress: ExportProgress) => void) => {
      exportProgress = listener;
      return () => { exportProgress = undefined; };
    },
    exportProgress: () => Promise.resolve([]),
    importUsb,
    confirm: confirmExport,
    onSyncProgress: (listener: (p: SyncProgress) => void) => {
      progress = listener;
      return () => {
        progress = null;
      };
    },
  } as unknown as Backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => {
    root.render(<SyncManager onClose={onClose} />);
  });
  await settle();
});

afterEach(() => {
  vi.useRealTimers();
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("SyncManager", () => {
  it("disables sync while rekordbox is open and enables it after rekordbox closes", async () => {
    act(() => root.unmount());
    vi.useFakeTimers();
    rekordboxOpen = true;
    root = createRoot(host);
    act(() => root.render(<SyncManager onClose={onClose} />));
    await settle();
    click(box("Closing"));
    click(box("USB B"));
    await settle();
    const sync = host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]');
    expect(sync?.disabled).toBe(true);
    expect(sync?.title).toBe("Quit rekordbox to enable synchronization.");
    expect(status()).toContain("Quit rekordbox to enable synchronization.");

    rekordboxOpen = false;
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    await settle();
    expect(sync?.disabled).toBe(false);
  });
  it("rechecks rekordbox when sync is clicked", async () => {
    click(box("Closing"));
    click(box("USB B"));
    await settle();
    const sync = host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]');
    expect(sync?.disabled).toBe(false);

    // rekordbox launches after the most recent background check but before
    // the user clicks the still-enabled button.
    rekordboxOpen = true;
    click(sync);
    await settle();
    expect(syncDevices).not.toHaveBeenCalled();
    expect(sync?.disabled).toBe(true);
    expect(status()).toContain("Quit rekordbox to enable synchronization.");
  });
  it("rechecks selected USB devices before importing or syncing", async () => {
    click(box("Closing"));
    click(box("USB B"));
    await settle();

    // The selected path disappeared after the last device refresh. SYNC must
    // not pass that stale mount point to either the import or export command.
    listDevices.mockResolvedValueOnce([stick("USB A")]);
    click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
    await settle();

    expect(importUsb).not.toHaveBeenCalled();
    expect(syncDevices).not.toHaveBeenCalled();
    expect(status()).toContain("selected USB device is no longer connected");
  });
  it("lists missing source files and requires confirmation before writing", async () => {
    validateExportFiles.mockResolvedValueOnce([
      { title: "Missing One", path: "/Music/missing-one.mp3" },
      { title: "Missing Two", path: "/Music/missing-two.wav" },
    ]);
    confirmExport.mockResolvedValueOnce(false);
    click(box("Closing"));
    click(box("USB B"));
    await settle();
    click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
    await settle();
    expect(validateExportFiles).toHaveBeenCalledWith(["p3"]);
    expect(confirmExport).toHaveBeenCalledWith(
      expect.stringContaining("2 selected tracks have missing audio files"),
      { yes: "Yes", no: "No" },
    );
    expect(confirmExport.mock.calls[0]?.[0]).toContain("Missing One\n  /Music/missing-one.mp3");
    expect(syncDevices).not.toHaveBeenCalled();
    expect(status()).toContain("Export cancelled because files are missing.");
  });
  it("stops an export started outside Sync Manager", async () => {
    const job: ExportProgress = { path: "/Volumes/USB B", state: "copying", done: 3, total: 10, title: "Track" };
    act(() => exportProgress?.(job));
    click(host.querySelector<HTMLButtonElement>('button[aria-label="Stop export to /Volumes/USB B"]'));
    await settle();
    expect(cancelExport).toHaveBeenCalledWith(job.path);
    expect(host.textContent).toContain("Stopping…");
    act(() => exportProgress?.({ ...job, state: "cancelled" }));
    expect(host.textContent).toContain("Export stopped");
    expect(host.querySelector('button[aria-label="Stop export to /Volumes/USB B"]')).toBeNull();
  });
  it("ejects only the chosen drive and clears it from the sync selection", async () => {
    click(box("USB A"));
    await settle();
    let finish = () => {};
    ejectDevice.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; }));
    const eject = host.querySelector<HTMLButtonElement>('button[aria-label="Eject USB A"]');
    click(eject);
    await settle();
    expect(ejectDevice).toHaveBeenCalledWith("/Volumes/USB A");
    expect(eject?.disabled).toBe(true);
    expect(status()).toBe("Ejecting USB A…");
    expect(host.querySelector('button[aria-label="SYNC"]')).toHaveProperty("disabled", true);
    act(() => finish());
    await settle();
    expect(box("USB A")).toBeNull();
    expect(box("USB B")).not.toBeNull();
    expect(box("USB B")?.checked).toBe(false);
    expect(host.textContent).toContain("0 of 1 selected");
    expect(status()).toBe("USB A: Safely ejected.");
  });

  it("keeps the drive available when safe ejection fails", async () => {
    ejectDevice.mockRejectedValueOnce({ kind: "internal", message: "Device is busy." });
    click(host.querySelector<HTMLButtonElement>('button[aria-label="Eject USB B"]'));
    await settle();
    expect(status()).toContain("Could not eject. Device is busy.");
    expect(box("USB B")).not.toBeNull();
    expect(host.querySelector('button[aria-label="Eject USB B"]')).toHaveProperty("disabled", false);
  });

  it("prevents manual ejection while an export is running in the background", () => {
    act(() => exportProgress?.({ path: "/Volumes/USB B", state: "copying", done: 1, total: 10, title: "Track" }));
    const button = host.querySelector<HTMLButtonElement>('button[aria-label="Eject USB B"]');
    expect(button?.disabled).toBe(true);
    click(button);
    expect(ejectDevice).not.toHaveBeenCalled();
  });

  it("shows per-device progress and lets the window close while exporting", () => {
    const job: ExportProgress = { path: "/Volumes/USB B", state: "copying", done: 3, total: 10, title: "Track" };
    act(() => exportProgress?.(job));
    const meter = host.querySelector<HTMLProgressElement>('progress[aria-label="Exporting USB B"]');
    expect(meter?.value).toBe(30);
    expect(host.textContent).toContain("Exporting — Track (30%)");
    const background = [...host.querySelectorAll("button")].find(button => button.textContent === "Run in background");
    expect(background?.disabled).toBe(false);
    click(background ?? null);
    expect(onClose).toHaveBeenCalled();
    act(() => exportProgress?.({ ...job, done: 10 }));
    expect(meter?.value).toBe(99);
    act(() => exportProgress?.({ ...job, state: "done", done: 10 }));
    expect(meter?.value).toBe(100);
    expect(meter?.dataset.state).toBe("done");
    act(() => exportProgress?.({ ...job, state: "failed", title: "Device disconnected" }));
    expect(meter?.value).toBe(30);
    expect(meter?.dataset.state).toBe("failed");
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Device disconnected");
  });
  it("passes cleanup and the chosen compatibility format to sync", async () => {
    const preferences = { ...DEFAULT_PREFERENCES, usbExport: {
      ...DEFAULT_PREFERENCES.usbExport, deleteUnlistedMusic: true,
      maximumCompatibility: true, conversionFormat: "mp3" as const,
    } };
    act(() => root.render(<PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
      <SyncManager onClose={onClose} />
    </PreferencesProvider>));
    await settle();
    click(box("Sets"));
    click(box("USB A"));
    await settle();
    click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
    await settle();
    expect(syncDevices.mock.calls[0]?.slice(5)).toEqual([true, "mp3"]);
  });

  it("lists the playlists and folders, not All Tracks or the heading", () => {
    const names = [...host.querySelectorAll('[aria-label="Playlists"] > [role="treeitem"]')].map((row) => row.textContent?.trim());
    expect(names).toEqual(["Sets", "Warm Up", "Main Set", "Peak Time", "Closing"]);
    expect(host.querySelector('button[aria-label="SYNC"]')).toHaveProperty("disabled", true);
  });

  it("selects an intelligent playlist and sends it to sync", async () => {
    click(box("Peak Time"));
    click(box("USB B"));
    await settle();
    click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
    await settle();
    expect(validateExportFiles).toHaveBeenCalledWith(["s1"]);
    expect(syncDevices.mock.calls[0]?.[0]).toEqual(["s1"]);
  });

  it("sends an intelligent playlist with the folder it is in", async () => {
    click(box("Sets"));
    expect(box("Peak Time")?.checked).toBe(true);
    click(box("USB B"));
    await settle();
    click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
    await settle();
    expect(syncDevices.mock.calls[0]?.[0]).toEqual(["p1", "p2", "s1"]);
  });

  it("follows library changes while open, keeping ticks on playlists still there", async () => {
    click(box("Warm Up"));
    click(box("Closing"));
    tree = [...TREE.filter((n) => n.id !== "p3").slice(0, 6), { id: "p4", name: "New Playlist", kind: "playlist", depth: 1 }, ...TREE.slice(7)];
    act(() => libraryChanged?.());
    await settle();

    const names = [...host.querySelectorAll('[aria-label="Playlists"] > [role="treeitem"]')].map((row) => row.textContent?.trim());
    expect(names).toEqual(["Sets", "Warm Up", "Main Set", "Peak Time", "New Playlist"]);
    expect(box("Warm Up")?.checked).toBe(true);
    expect(host.textContent).toContain("1 of 4 playlists selected");
  });

  it("keeps open and closed folders as they were across a re-read", async () => {
    act(() => root.unmount());
    const nested: TreeNode[] = [
      ...TREE.slice(0, 3),
      { id: "f2", name: "Archive", kind: "folder", depth: 2 },
      { id: "p5", name: "Old Set", kind: "playlist", depth: 3 },
      ...TREE.slice(3, 7),
      { id: "f3", name: "Gigs", kind: "folder", depth: 1 },
      { id: "p6", name: "Friday", kind: "playlist", depth: 2 },
      ...TREE.slice(7),
    ];
    tree = nested;
    root = createRoot(host);
    act(() => root.render(<SyncManager onClose={onClose} />));
    await settle();
    const names = () => [...host.querySelectorAll('[aria-label="Playlists"] > [role="treeitem"]')].map((row) => row.textContent?.trim());
    // Top folders open; deeper ones start closed.
    expect(names()).toEqual(["Sets", "Archive", "Warm Up", "Main Set", "Peak Time", "Closing", "Gigs", "Friday"]);

    click(host.querySelector<HTMLButtonElement>('button[aria-label="Expand Archive"]'));
    click(host.querySelector<HTMLButtonElement>('button[aria-label="Collapse Gigs"]'));
    expect(names()).toEqual(["Sets", "Archive", "Old Set", "Warm Up", "Main Set", "Peak Time", "Closing", "Gigs"]);

    // A new nested folder starts closed; the folders already seen stay as the user left them.
    tree = [
      ...nested.slice(0, 8),
      { id: "f4", name: "Later", kind: "folder", depth: 2 },
      { id: "p7", name: "Encore", kind: "playlist", depth: 3 },
      ...nested.slice(8),
    ];
    act(() => libraryChanged?.());
    await settle();
    expect(names()).toEqual(["Sets", "Archive", "Old Set", "Warm Up", "Main Set", "Peak Time", "Later", "Closing", "Gigs"]);
    expect(host.querySelector('[role="treeitem"][aria-expanded="true"] input[aria-label="Archive"]')).not.toBeNull();
    expect(host.querySelector('[role="treeitem"][aria-expanded="false"] input[aria-label="Gigs"]')).not.toBeNull();
    expect(host.querySelector('[role="treeitem"][aria-expanded="false"] input[aria-label="Later"]')).not.toBeNull();
  });

  it("ignores a playlist read that answers after a newer one", async () => {
    const held: { resolve: (tree: TreeNode[]) => void; reject: (error: Error) => void }[] = [];
    readTree = () => new Promise((resolve, reject) => { held.push({ resolve, reject }); });
    const stale = [...TREE.slice(0, 7), { id: "p4", name: "Stale Playlist", kind: "playlist" as const, depth: 1 }, ...TREE.slice(7)];
    const fresh = [...TREE.slice(0, 7), { id: "p5", name: "Fresh Playlist", kind: "playlist" as const, depth: 1 }, ...TREE.slice(7)];
    const names = () => [...host.querySelectorAll('[aria-label="Playlists"] > [role="treeitem"]')].map((row) => row.textContent?.trim());

    act(() => libraryChanged?.());
    await settle();
    act(() => libraryChanged?.());
    await settle();
    expect(held).toHaveLength(2);

    // The newer read answers first, then the older one: the older is dropped.
    act(() => { held[1]?.resolve(fresh); });
    await settle();
    act(() => { held[0]?.resolve(stale); });
    await settle();
    expect(names()).toEqual(["Sets", "Warm Up", "Main Set", "Peak Time", "Closing", "Fresh Playlist"]);

    // Nor does an older read that fails replace the list with an error.
    act(() => libraryChanged?.());
    await settle();
    act(() => libraryChanged?.());
    await settle();
    act(() => { held[3]?.resolve(fresh); });
    await settle();
    act(() => { held[2]?.reject(new Error("gone")); });
    await settle();
    expect(host.querySelector('[aria-label="Playlists"] [role="alert"]')).toBeNull();
    expect(names()).toEqual(["Sets", "Warm Up", "Main Set", "Peak Time", "Closing", "Fresh Playlist"]);
  });

  it("imports only the ticked iTunes playlists and refreshes the library column", async () => {
    act(() => root.unmount());
    itunesLibrary = {
      path: "/Users/dj/Music/Music/Library.xml",
      tree: [
        { id: "itunes:0", name: "Chill Folder", kind: "folder", depth: 1 },
        { id: "itunes:1", name: "Airplane Mix", kind: "playlist", depth: 2 },
        { id: "itunes:2", name: "Police Set", kind: "playlist", depth: 1 },
      ],
    };
    root = createRoot(host);
    act(() => root.render(<SyncManager onClose={onClose} />));
    await settle();

    const itunesSync = host.querySelector<HTMLButtonElement>('button[aria-label="Import selected iTunes playlists"]');
    expect(itunesSync?.disabled).toBe(true);
    // The iTunes column is its own tree, separate from the rekordbox one.
    expect(host.querySelector('[aria-label="iTunes playlists"]')?.textContent).toContain("Airplane Mix");

    click(box("Airplane Mix"));
    await settle();
    expect(host.textContent).toContain("1 of 2 playlists selected");
    expect(itunesSync?.disabled).toBe(false);

    click(itunesSync);
    await settle();
    expect(importItunesSelected).toHaveBeenCalledWith("/Users/dj/Music/Music/Library.xml", ["itunes:1"]);
    expect(status()).toContain("Imported 1 playlists from iTunes");
  });

  it("offers a file picker when no iTunes library is detected", () => {
    expect(host.textContent).toContain("No iTunes or Music library was found");
    const choose = [...host.querySelectorAll("button")].find((button) => button.textContent === "Choose…");
    expect(choose).toBeDefined();
    expect(host.querySelector('button[aria-label="Import selected iTunes playlists"]')).toHaveProperty("disabled", true);
  });

  it("shows used space as the filled portion and labels the remaining free space", () => {
    const meter = host.querySelector('[role="meter"][aria-label="USB A storage used"]');
    expect(meter?.getAttribute("aria-valuenow")).toBe("25");
    expect(meter?.getAttribute("aria-valuetext")).toBe("8.0 GB used; 24.0 GB free (75%)");
    expect(meter?.querySelector("span")?.style.width).toBe("25%");
  });

  it("warns beside a USB stick that is not FAT32", async () => {
    listDevices.mockResolvedValueOnce([{ ...stick("USB A"), fileSystem: "exFAT" }]);
    act(() => root.unmount());
    root = createRoot(host);
    act(() => root.render(<SyncManager onClose={onClose} />));
    await settle();

    const warning = host.querySelector('[title="Pioneer DJ recommends FAT32"]');
    expect(warning?.querySelector("svg")).not.toBeNull();
    expect(warning?.querySelector("svg")?.getAttribute("aria-label")).toBe("Pioneer DJ recommends FAT32");
  });

  it("does not warn when Linux reports a FAT32 stick through the vfat driver", async () => {
    listDevices.mockResolvedValueOnce([{ ...stick("USB A"), fileSystem: "vfat" }]);
    act(() => root.unmount());
    root = createRoot(host);
    act(() => root.render(<SyncManager onClose={onClose} />));
    await settle();

    expect(host.querySelector('[title="Pioneer DJ recommends FAT32"]')).toBeNull();
  });

  it("requests post-sync ejection and distinguishes eject errors from sync errors", async () => {
    syncDevices.mockResolvedValueOnce([
      { ...report("/Volumes/USB A", 5), ejected: true },
      { ...report("/Volumes/USB B", 5), ejectError: "Device is busy." },
    ]);
    expect(box("Eject after syncing")?.checked).toBe(false);
    click(box("USB A"));
    click(box("USB B"));
    click(box("Eject after syncing"));
    await settle();
    click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
    expect(box("Eject after syncing")?.disabled).toBe(true);
    await settle();
    expect(syncDevices.mock.calls[0]?.[4]).toBe(true);
    expect(host.querySelector('[aria-label="USB A export report"]')?.textContent).toContain("5 updated");
    expect(status()).toContain("Safely ejected.");
    expect(host.querySelector('[aria-label="USB B export report"]')?.textContent).toContain("5 updated");
    expect(status()).toContain("Not ejected: Device is busy.");
  });

  it("ticking a folder ticks every playlist under it, and part of it shows as mixed", () => {
    click(box("Sets"));
    expect(box("Warm Up")?.checked).toBe(true);
    expect(box("Main Set")?.checked).toBe(true);
    expect(box("Peak Time")?.checked).toBe(true);
    expect(box("Closing")?.checked).toBe(false);
    expect(box("Sets")?.getAttribute("aria-checked")).toBe("true");

    click(box("Warm Up"));
    expect(box("Sets")?.checked).toBe(false);
    expect(box("Sets")?.indeterminate).toBe(true);
    expect(box("Sets")?.getAttribute("aria-checked")).toBe("mixed");

    // A mixed folder ticks the rest on a click, as a native tri-state box
    // does; the click after that clears it.
    click(box("Sets"));
    expect(box("Warm Up")?.checked).toBe(true);
    expect(box("Sets")?.indeterminate).toBe(false);
    click(box("Sets"));
    expect(box("Warm Up")?.checked).toBe(false);
    expect(box("Main Set")?.checked).toBe(false);
    expect(box("Peak Time")?.checked).toBe(false);
  });

  it("ticking a device restores its selection without expanding it", async () => {
    expect(box("Closing")?.checked).toBe(false);
    click(box("USB A"));
    await settle();
    expect(box("Closing")?.checked).toBe(true);
    expect(host.querySelector('[aria-label="USB A library"]')).toBeNull();
    expect(box("USB A")?.closest('[role="treeitem"]')?.getAttribute("aria-expanded")).toBe("false");
    click(host.querySelector<HTMLButtonElement>('button[aria-label="Expand USB A"]'));
    await settle();
    const library = host.querySelector('[aria-label="USB A library"]');
    expect(library?.textContent).toContain("Device Library");
    expect(library?.textContent).toContain("Closing");
    expect(host.textContent).toContain("24.0 GB free (75%)");
    // A stick with nothing on it says so, and ticks nothing.
    click(box("USB B"));
    await settle();
    expect(host.querySelector('[aria-label="USB B library"]')).toBeNull();
    click(host.querySelector<HTMLButtonElement>('button[aria-label="Expand USB B"]'));
    await settle();
    expect(host.querySelector('[aria-label="USB B library"]')?.textContent).toContain("No playlists on this device yet.");
    expect(box("Warm Up")?.checked).toBe(false);
  });

  it("SYNC sends the ticked playlists to both ticked sticks and reports on each", async () => {
    // The write is held open so the line it shows while running can be read.
    let finish: (reports: SyncDeviceReport[]) => void = () => {};
    syncDevices.mockImplementationOnce(
      () =>
        new Promise<SyncDeviceReport[]>((resolve) => {
          finish = resolve;
        }),
    );
    click(box("Sets"));
    click(box("USB A"));
    click(box("USB B"));
    await settle();
    const sync = host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]');
    expect(sync?.disabled).toBe(false);
    click(sync);
    await settle();
    expect(sync?.disabled).toBe(true);
    // The run is announced stick by stick while it is going.
    act(() => progress?.({ path: "/Volumes/USB A", state: "writing" }));
    expect(status()).toBe("Writing to USB A…");
    act(() => finish([report("/Volumes/USB A", 30), report("/Volumes/USB B", 30)]));
    await settle();
    expect(syncDevices).toHaveBeenCalledTimes(1);
    const [playlists, destinations, , automatic] = syncDevices.mock.calls[0] as [string[], string[], unknown, boolean];
    expect(playlists).toEqual(["p1", "p2", "s1", "p3"]);
    expect(destinations).toEqual(["/Volumes/USB A", "/Volumes/USB B"]);
    expect(automatic).toBe(false);
    expect(syncDevices.mock.calls[0]?.[4]).toBe(false);
    expect(box("Automatic synchronization for USB A")).toBeNull();
    expect(box("Automatic synchronization for USB B")).toBeNull();
    expect(host.querySelector('[aria-label="USB A export report"]')?.textContent).toContain("30 updated");
    expect(host.querySelector('[aria-label="USB B export report"]')?.textContent).toContain("30 updated");
    expect(sync?.disabled).toBe(false);
  });

  it("a stick that failed says why, beside the ones that were written", async () => {
    syncDevices.mockImplementationOnce(() => Promise.resolve([
      report("/Volumes/USB A", 5),
      { path: "/Volumes/USB B", error: "That device is no longer connected." },
    ]));
    click(box("Closing"));
    click(box("USB A"));
    click(box("USB B"));
    await settle();
    click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
    await settle();
    expect(host.querySelector('[aria-label="USB A export report"]')?.textContent).toContain("5 updated");
    expect(status()).toContain("USB B: That device is no longer connected.");
  });

  it("Close and Escape both close it", () => {
    click([...host.querySelectorAll("button")].find((b) => b.textContent === "Close"));
    expect(onClose).toHaveBeenCalledTimes(1);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

const importTick = (label: string) =>
  [...host.querySelectorAll("label")].find(l => l.textContent === label)?.querySelector("input") ?? null;
const importButton = () => [...host.querySelectorAll("button")].find(b => b.textContent?.trim() === "Import")!;
const renderWith = async (usbExport: Partial<typeof DEFAULT_PREFERENCES.usbExport>, protectLibrary = false) => {
  const preferences = {
    ...DEFAULT_PREFERENCES,
    usbExport: { ...DEFAULT_PREFERENCES.usbExport, ...usbExport },
    advanced: { ...DEFAULT_PREFERENCES.advanced, protectLibrary },
  };
  act(() => root.render(<PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
    <SyncManager onClose={onClose} />
  </PreferencesProvider>));
  await settle();
};

it("imports cue/grid information from selected devices only", async () => {
  await renderWith({ importButtonCues: true, importButtonHistory: false, importButtonSettings: false });
  expect(importButton().disabled).toBe(true);
  click(box("USB A"));
  await settle();
  click(importButton());
  await settle();
  expect(importUsb).toHaveBeenCalledWith("/Volumes/USB A", true, false, false);
  expect(importUsb).toHaveBeenCalledTimes(1);
  expect(status()).toContain("updated 2 tracks");
});

it("starts Import's ticks at the Preferences defaults and imports each ticked kind", async () => {
  await renderWith({ importButtonCues: false, importButtonHistory: true, importButtonSettings: true });
  expect(importTick("Cues and beat grids")?.checked).toBe(false);
  expect(importTick("Play history")?.checked).toBe(true);
  expect(importTick("CDJ/mixer settings")?.checked).toBe(true);
  click(importTick("CDJ/mixer settings"));
  click(importTick("Cues and beat grids"));
  click(box("USB A"));
  await settle();
  click(importButton());
  await settle();
  expect(importUsb.mock.calls).toEqual([
    ["/Volumes/USB A", true, false, false],
    ["/Volumes/USB A", false, true, false],
  ]);
});

it("Import cannot be pressed with nothing ticked", async () => {
  await renderWith({ importButtonCues: false, importButtonHistory: false, importButtonSettings: false });
  click(box("USB A"));
  await settle();
  expect(importButton().disabled).toBe(true);
});

it("library protection leaves only settings to import", async () => {
  await renderWith({ importButtonCues: true, importButtonHistory: true, importButtonSettings: true }, true);
  expect(importTick("Cues and beat grids")?.disabled).toBe(true);
  expect(importTick("Play history")?.disabled).toBe(true);
  click(box("USB A"));
  await settle();
  click(importButton());
  await settle();
  expect(importUsb).toHaveBeenCalledExactlyOnceWith("/Volumes/USB A", false, false, true);
});

it("imports history explicitly and offers details and retry after a failure", async () => {
  await renderWith({ importButtonCues: false, importButtonHistory: true, importButtonSettings: false });
  click(box("USB A"));
  await settle();
  importUsb.mockRejectedValueOnce(new Error("Could not read play history: damaged database"));
  click(importButton());
  await settle();
  expect(importUsb).toHaveBeenCalledWith("/Volumes/USB A", false, true, false);
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("damaged database");
  expect([...host.querySelectorAll("button")].some(button => button.textContent?.includes("Show details"))).toBe(true);
  const retry = [...host.querySelectorAll("button")].find(button => button.textContent?.includes("Retry import"));
  expect(retry).toBeTruthy();
  click(retry);
  await settle();
  expect(importUsb).toHaveBeenCalledTimes(2);
  expect(importUsb).toHaveBeenLastCalledWith("/Volumes/USB A", false, true, false);
});

it("expanding a USB does not select it for synchronization", async () => {
  click(host.querySelector<HTMLButtonElement>('button[aria-label="Expand USB A"]'));
  await settle();
  expect(box("USB A")?.checked).toBe(false);
  expect(host.querySelector('[aria-label="USB A library"]')?.textContent).toContain("Closing");
});


it("connection changes refresh only the device list; imports wait for SYNC", async () => {
  expect(importUsb).not.toHaveBeenCalled();
  listDevices.mockResolvedValueOnce([...DEVICES, stick("USB C")]);
  act(() => { devicesChanged?.(); });
  await settle();
  listDevices.mockResolvedValue([...DEVICES, stick("USB C")]);
  expect(box("USB C")).not.toBeNull();
  expect(importUsb).not.toHaveBeenCalled();
  expect(syncDevices).not.toHaveBeenCalled();
  click(box("Closing"));
  click(box("USB C"));
  await settle();
  expect(importUsb).not.toHaveBeenCalled();
  click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
  await settle();
  expect(importUsb).toHaveBeenCalledExactlyOnceWith("/Volumes/USB C", false, true, false);
  expect(importUsb.mock.invocationCallOrder[0]).toBeLessThan(syncDevices.mock.invocationCallOrder[0]!);
});

it("does not export when the pre-sync import fails", async () => {
  importUsb.mockRejectedValueOnce(new Error("Missing recovery file"));
  click(box("Closing"));
  click(box("USB B"));
  await settle();
  click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
  await settle();
  expect(syncDevices).not.toHaveBeenCalled();
  expect(status()).toContain("USB B: Couldn’t import before syncing. Missing recovery file");
});

it("honors disabled imports when syncing", async () => {
  const preferences = { ...DEFAULT_PREFERENCES, usbExport: {
    ...DEFAULT_PREFERENCES.usbExport, importHistory: false, importSettings: false,
  } };
  act(() => root.render(<PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
    <SyncManager onClose={onClose} />
  </PreferencesProvider>));
  await settle();
  click(box("Closing"));
  click(box("USB B"));
  await settle();
  click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
  await settle();
  expect(importUsb).not.toHaveBeenCalled();
  expect(syncDevices).toHaveBeenCalledTimes(1);
});


it("imports settings during SYNC only when enabled", async () => {
  const preferences = { ...DEFAULT_PREFERENCES, usbExport: {
    ...DEFAULT_PREFERENCES.usbExport, importHistory: false, importSettings: true,
  } };
  act(() => root.render(<PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
    <SyncManager onClose={onClose} />
  </PreferencesProvider>));
  await settle();
  expect(importUsb).not.toHaveBeenCalled();
  click(box("Closing"));
  click(box("USB B"));
  await settle();
  click(host.querySelector<HTMLButtonElement>('button[aria-label="SYNC"]'));
  await settle();
  expect(importUsb).toHaveBeenCalledExactlyOnceWith("/Volumes/USB B", false, false, true);
  expect(importUsb.mock.invocationCallOrder[0]).toBeLessThan(syncDevices.mock.invocationCallOrder[0]!);
});
