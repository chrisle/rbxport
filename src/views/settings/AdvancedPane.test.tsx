/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { AdvancedPane } from "./AdvancedPane";

let host: HTMLDivElement;
let root: Root;
let update: ReturnType<typeof vi.fn>;
let listBackups: ReturnType<typeof vi.fn>;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  update = vi.fn();
  listBackups = vi.fn().mockResolvedValue([]);
  __setBackend({ listBackups } as unknown as Backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

function mount(protectLibrary = true) {
  const preferences = {
    ...DEFAULT_PREFERENCES,
    advanced: { ...DEFAULT_PREFERENCES.advanced, protectLibrary },
  };
  act(() => root.render(
    <PreferencesProvider value={{ preferences, update, reset: vi.fn() }}>
      <AdvancedPane tab="browse" summary={null} />
    </PreferencesProvider>,
  ));
}

async function toggleProtection() {
  const toggle = host.querySelector<HTMLInputElement>('input[role="switch"]');
  if (!toggle) throw new Error("Library Protection toggle missing");
  await act(async () => { toggle.click(); await Promise.resolve(); await Promise.resolve(); });
}

it("unlocks immediately when a backup exists", async () => {
  listBackups.mockResolvedValue([{ path: "/backups/library.zip" }]);
  mount();

  await toggleProtection();

  expect(update).toHaveBeenCalledWith("advanced", { protectLibrary: false });
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});

it("gently recommends a backup and offers Unlock anyway or Cancel", async () => {
  mount();

  await toggleProtection();

  const dialog = host.querySelector('[role="dialog"]');
  expect(dialog?.textContent).toContain("It looks like you haven’t made a backup yet. We strongly recommend creating one before using RBXport.");
  expect([...dialog!.querySelectorAll("button")].map(button => button.textContent)).toEqual([
    "Unlock anyway", "Cancel",
  ]);
  expect(update).not.toHaveBeenCalled();
  await act(async () => { [...dialog!.querySelectorAll("button")][1]?.click(); await Promise.resolve(); });
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  expect(update).not.toHaveBeenCalled();
});

it("unlocks without a backup only after explicit confirmation", async () => {
  mount();

  await toggleProtection();
  const unlock = [...host.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')]
    .find(button => button.textContent === "Unlock anyway");
  await act(async () => { unlock?.click(); await Promise.resolve(); });

  expect(update).toHaveBeenCalledWith("advanced", { protectLibrary: false });
});

it("turns protection on without checking backups", async () => {
  mount(false);

  await toggleProtection();

  expect(update).toHaveBeenCalledWith("advanced", { protectLibrary: true });
  expect(listBackups).not.toHaveBeenCalled();
});

it("offers the four same-name folder choices, Import as a copy first and chosen by default (#355)", () => {
  mount();
  const select = host.querySelector<HTMLSelectElement>('select[aria-label="When a dropped folder has the same name as a list"]');
  if (!select) throw new Error("same-name folder choices missing");
  const options = [...select.options];
  expect(options.map((option) => option.value)).toEqual(["copy", "replace", "keep", "ask"]);
  expect(options.map((option) => option.textContent))
    .toEqual(["Import as a copy", "Replace existing", "Keep existing", "Ask me"]);
  expect(select.value).toBe("copy");
  expect(host.textContent).toContain(
    "Replace existing deletes the list with that name, and every list inside it if it is a folder, without asking. " +
      "Edit > Undo puts it back.",
  );

  act(() => {
    select.value = "ask";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(update).toHaveBeenCalledWith("advanced", { folderDropClash: "ask" });
});

describe("Database management", () => {
  const local = { name: "Macintosh HD", masterDb: "/Users/x/Library/Pioneer/rekordbox/master.db", current: true };
  const drive = { name: "DJ SSD", masterDb: "/Volumes/DJ SSD/PIONEER/Master/master.db", current: false };
  const drives = [local, drive];

  async function mountDatabase(backend: Partial<Backend>, readOnly = false) {
    __setBackend({ listBackups, findDuplicates: vi.fn(), ...backend } as unknown as Backend);
    act(() => root.render(
      <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update, reset: vi.fn() }}>
        <AdvancedPane tab="database" summary={{ trackCount: 1, playlistCount: 1, dbVersion: "6000", readOnly } as never} />
      </PreferencesProvider>,
    ));
    await act(async () => { for (let i = 0; i < 4; i++) await Promise.resolve(); });
    return host.querySelector<HTMLSelectElement>('select[aria-label="Select a drive"]');
  }

  async function choose(select: HTMLSelectElement, value: string) {
    await act(async () => {
      select.value = value;
      select.dispatchEvent(new Event("change", { bubbles: true }));
      for (let i = 0; i < 4; i++) await Promise.resolve();
    });
  }

  it("lists the drives by name with the open library selected, as rekordbox does", async () => {
    const select = await mountDatabase({ databaseDrives: vi.fn().mockResolvedValue(drives) });
    expect(host.textContent).toContain("Database management");
    expect(host.textContent).toContain("Select a drive");
    expect([...select!.options].map((option) => option.textContent)).toEqual(["Macintosh HD", "DJ SSD"]);
    expect(select!.value).toBe(local.masterDb);
    expect(select!.disabled).toBe(false);
  });

  it("is greyed out with only one drive, and while rekordbox runs", async () => {
    expect((await mountDatabase({ databaseDrives: vi.fn().mockResolvedValue(drives.slice(0, 1)) }))!.disabled).toBe(true);
    expect((await mountDatabase({ databaseDrives: vi.fn().mockResolvedValue(drives) }, true))!.disabled).toBe(true);
  });

  it("asks before switching, and switches only on OK", async () => {
    const switchLibrary = vi.fn().mockResolvedValue(undefined);
    const confirm = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    const select = await mountDatabase({ databaseDrives: vi.fn().mockResolvedValue(drives), confirm, switchLibrary });

    await choose(select!, drive.masterDb);
    expect(confirm).toHaveBeenCalledWith(
      "Are you sure you want to switch Master Database?\nThis operation may require long time.",
      { yes: "OK", no: "Cancel" },
    );
    expect(switchLibrary).not.toHaveBeenCalled();
    expect(select!.value).toBe(local.masterDb);

    await choose(select!, drive.masterDb);
    expect(switchLibrary).toHaveBeenCalledWith(drive.masterDb);
  });

  it("says rekordbox's words when the switch fails", async () => {
    const select = await mountDatabase({
      databaseDrives: vi.fn().mockResolvedValue(drives),
      confirm: vi.fn().mockResolvedValue(true),
      switchLibrary: vi.fn().mockRejectedValue({ kind: "internal", message: "Failed to switch Master Database." }),
    });
    await choose(select!, drive.masterDb);
    expect(host.textContent).toContain("Failed to switch Master Database.");
    expect(select!.value).toBe(local.masterDb);
  });
});

describe("MCP server", () => {
  const running = { enabled: true, running: true, url: "http://127.0.0.1:47200/mcp" };

  async function mountMcp(mcpServer: boolean, status: object) {
    let changed: ((status: object) => void) | undefined;
    __setBackend({
      listBackups,
      mcpStatus: vi.fn().mockResolvedValue(status),
      onMcpChanged: vi.fn((listener: (status: object) => void) => { changed = listener; return () => undefined; }),
    } as unknown as Backend);
    const preferences = { ...DEFAULT_PREFERENCES, advanced: { ...DEFAULT_PREFERENCES.advanced, mcpServer } };
    act(() => root.render(
      <PreferencesProvider value={{ preferences, update, reset: vi.fn() }}>
        <AdvancedPane tab="browse" summary={null} />
      </PreferencesProvider>,
    ));
    await act(async () => { for (let i = 0; i < 4; i++) await Promise.resolve(); });
    return (next: object) => act(() => changed?.(next));
  }

  function mcpSwitch() {
    return host.querySelector<HTMLInputElement>('section[aria-label="MCP server"] input[role="switch"]');
  }

  it("is off by default and turns on through the preference", async () => {
    expect(DEFAULT_PREFERENCES.advanced.mcpServer).toBe(false);
    await mountMcp(false, { enabled: false, running: false, url: running.url });
    expect(mcpSwitch()?.checked).toBe(false);
    expect(host.textContent).not.toContain("Listening on");
    await act(async () => { mcpSwitch()?.click(); await Promise.resolve(); });
    expect(update).toHaveBeenCalledWith("advanced", { mcpServer: true });
  });

  it("shows where it listens", async () => {
    await mountMcp(true, running);
    expect(host.textContent).toContain("Listening on http://127.0.0.1:47200/mcp");
  });

  it("follows the backend's changes and shows why it could not start", async () => {
    const change = await mountMcp(true, { enabled: true, running: false, url: running.url });
    change({ ...running, running: false, error: "Port 47200 could not be opened: in use" });
    expect(host.textContent).toContain("Port 47200 could not be opened: in use");
    expect(host.textContent).not.toContain("Listening on");
    change(running);
    expect(host.textContent).toContain("Listening on http://127.0.0.1:47200/mcp");
  });
});
