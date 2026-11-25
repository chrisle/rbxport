import { expect, test } from "@playwright/test";

test("the existing panel edits legacy menus while SQL-owned controls stay disabled", async ({ page }) => {
  await page.goto("/");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    const backend = await getBackend();
    const read = backend.deviceSettings.bind(backend);
    let saved: Awaited<ReturnType<typeof read>> | null = null;
    backend.deviceSettings = async (path) => saved ?? {
      ...await read(path), hasOneLibrary: false, hasLibrarySettings: false, hasMenuSettings: true,
    };
    backend.saveDeviceSettings = (_path, settings) => {
      saved = settings;
      return Promise.resolve(settings);
    };
  });
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("treeitem", { name: /TEST/ }).click();
  const panel = page.getByRole("region", { name: "Device TEST" });
  await expect(panel.getByLabel("Device Name")).toBeDisabled();
  await panel.getByRole("tab", { name: "Column" }).click();
  const column = panel.getByRole("combobox", { name: "Default right column" });
  await expect(column).toBeEnabled();
  await column.selectOption("21");
  await expect(column).toHaveValue("21");
  await panel.getByRole("tab", { name: "Category" }).click();
  const inactive = panel.getByRole("listbox", { name: "Inactive Categories" });
  await inactive.getByRole("option", { name: "GENRE", exact: true }).click();
  await panel.getByRole("button", { name: "Add to Active Categories" }).click();
  await expect(panel.getByRole("listbox", { name: "Active Categories", exact: true }).getByRole("option").last()).toHaveText("GENRE");
  await panel.getByRole("tab", { name: "Sort" }).click();
  const sorts = panel.getByRole("listbox", { name: "Active Sort Options", exact: true });
  await sorts.getByRole("option", { name: "KEY", exact: true }).click();
  await panel.getByRole("button", { name: "Up", exact: true }).click();
  await expect(sorts.getByRole("option").nth(5)).toHaveText("KEY");
  await panel.getByRole("tab", { name: "Color" }).click();
  await expect(panel.getByLabel("Color comment 3")).toBeDisabled();
  await panel.getByRole("tab", { name: "Column" }).click();
  await expect(column).toHaveValue("21");
});
