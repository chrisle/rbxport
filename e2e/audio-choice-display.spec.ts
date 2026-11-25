import { expect, test, type Page } from "@playwright/test";

type AudioFixture = Window & { appliedOutput: string | null };

async function openAudio(page: Page, missing = false) {
  if (missing) await page.addInitScript(() => {
    localStorage.setItem("rbl.preferences", JSON.stringify({ audio: { outputDevice: "fixture-missing" } }));
  });
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as audioFixtureBackend } from "/src/ipc/client.ts";
      const audioBackend = await audioFixtureBackend();
      window.appliedOutput = null;
      audioBackend.audioDevices = async () => ({
        devices: [{ id: "fixture-built-in", name: "Fixture speakers" }, { id: "fixture-headphones", name: "Fixture headphones" }],
        default: "fixture-built-in", chosen: window.appliedOutput
      });
      audioBackend.setAudioDevice = async device => { window.appliedOutput = device; };
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?writable");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "Audio", exact: true }).click();
  const select = dialog.getByRole("combobox", { name: "Audio output device" });
  await expect(select).toBeVisible();
  return { dialog, select };
}

const applied = (page: Page) => page.evaluate(() => (window as unknown as AudioFixture).appliedOutput);
const saved = (page: Page) => page.evaluate(() => JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}").audio.outputDevice as string | null);

for (const change of ["reset", "external"] as const) {
  test(`audio output choice follows ${change} preferences and the applied command`, async ({ page }) => {
    const { dialog, select } = await openAudio(page);
    await select.selectOption("fixture-headphones");
    await expect(select).toHaveValue("fixture-headphones");
    await expect.poll(() => saved(page)).toBe("fixture-headphones");
    await expect.poll(() => applied(page)).toBe("fixture-headphones");
    if (change === "reset") await dialog.getByRole("button", { name: "Reset to defaults" }).click();
    else await page.evaluate(() => {
      const prefs = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
      prefs.audio.outputDevice = "fixture-built-in";
      localStorage.setItem("rbl.preferences", JSON.stringify(prefs));
      window.dispatchEvent(new StorageEvent("storage", { key: "rbl.preferences" }));
    });
    const expected = change === "reset" ? null : "fixture-built-in";
    await expect.poll(() => saved(page)).toBe(expected);
    await expect.poll(() => applied(page)).toBe(expected);
    await expect(select).toHaveValue(expected ?? "");
  });
}

test("an unavailable output keeps its saved choice until a listed device is selected", async ({ page }) => {
  const { dialog, select } = await openAudio(page, true);
  await expect(select).toHaveValue("");
  await expect(dialog.getByText(/The saved output device is not connected/)).toBeVisible();
  await expect.poll(() => saved(page)).toBe("fixture-missing");
  await expect.poll(() => applied(page)).toBe("fixture-missing");
  await select.selectOption("fixture-headphones");
  await expect(select).toHaveValue("fixture-headphones");
  await expect(dialog.getByText(/The saved output device is not connected/)).toHaveCount(0);
  await expect.poll(() => saved(page)).toBe("fixture-headphones");
  await expect.poll(() => applied(page)).toBe("fixture-headphones");
});
