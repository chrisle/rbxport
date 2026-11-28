import { expect, test, type Page } from "@playwright/test";

const tree = (page: Page, side: "main" | "sub") => page.locator(`[data-browser-pane="tree"][data-browser-side="${side}"]`);
const row = (page: Page, side: "main" | "sub", name: string) => tree(page, side).getByRole("treeitem", { name, exact: true });
const create = (page: Page) => page.evaluate(() => {
  (window as unknown as { __menu: (id: string) => void }).__menu("create-playlist");
});

type HeldCreate = { __createdId?: string; __releaseCreate?: () => void; __libraryReady?: () => void };

async function holdCreate(page: Page) {
  await page.evaluate(async (path) => {
    const backend = await (await import(path) as typeof import("../src/ipc/client")).getBackend();
    const original = backend.edits.createPlaylistWithResult.bind(backend.edits);
    const w = window as unknown as HeldCreate;
    const held = new Promise<void>((resolve) => { w.__releaseCreate = resolve; });
    backend.edits.createPlaylistWithResult = async (...args) => {
      const made = await original(...args);
      w.__createdId = made.id;
      await held;
      return made;
    };
  }, "/src/ipc/client.ts");
}

async function createdId(page: Page) {
  await expect.poll(() => page.evaluate(() => (window as unknown as HeldCreate).__createdId)).toBeTruthy();
  return page.evaluate(() => (window as unknown as HeldCreate).__createdId as string);
}

const release = (page: Page) => page.evaluate(() => (window as unknown as HeldCreate).__releaseCreate?.());

for (const side of ["main", "sub"] as const) {
  test(`ordinary create selects its exact new playlist in ${side}`, async ({ page }) => {
    await page.goto("/?writable=1");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await row(page, "main", "Hardstyle").click();
    if (side === "sub") {
      await page.getByRole("button", { name: "Sub-Browser Window", exact: true }).click();
      await expect(tree(page, "sub")).toBeVisible();
    }
    await row(page, side, "Melodic Vox").click();
    await create(page);
    await expect(page.getByRole("contentinfo")).toContainText("Created New playlist.");
    await expect(row(page, side, "New playlist")).toHaveAttribute("aria-selected", "true");
    await expect(row(page, side, "Melodic Vox")).toHaveAttribute("aria-selected", "false");
    if (side === "sub") await expect(row(page, "main", "Hardstyle")).toHaveAttribute("aria-selected", "true");
  });

  test(`delayed ${side} create preserves later navigation and keyboard focus`, async ({ page }) => {
    await page.goto("/?writable=1");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    if (side === "sub") await page.getByRole("button", { name: "Sub-Browser Window", exact: true }).click();
    await row(page, side, "Melodic Vox").click();
    await holdCreate(page);
    await create(page);
    const id = await createdId(page);
    await row(page, side, "Hardstyle").click();
    const search = tree(page, side).getByRole("searchbox").first();
    await search.focus();
    await release(page);
    await expect(page.getByRole("contentinfo")).toContainText("Created New playlist.");
    await expect(row(page, side, "Hardstyle")).toHaveAttribute("aria-selected", "true");
    await expect(tree(page, side).locator(`[data-file-drop-playlist="${id}"]`)).toHaveAttribute("aria-selected", "false");
    await expect(search).toBeFocused();
  });
}

test("same-name concurrent writes do not replace the creator's exact ID", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await row(page, "main", "Melodic Vox").click();
  await holdCreate(page);
  await create(page);
  const id = await createdId(page);
  const other = await page.evaluate(async (path) => {
    const backend = await (await import(path) as typeof import("../src/ipc/client")).getBackend();
    const before = new Set((await backend.playlistTree()).map((node) => node.id));
    const generation = await backend.edits.createPlaylist("New playlist", "root");
    if (typeof generation !== "number") throw new Error("legacy create no longer returns its generation");
    return (await backend.playlistTree()).find((node) => !before.has(node.id))?.id;
  }, "/src/ipc/client.ts");
  expect(other).toBeTruthy();
  expect(other).not.toBe(id);
  await release(page);
  await expect(tree(page, "main").locator(`[data-file-drop-playlist="${id}"]`)).toHaveAttribute("aria-selected", "true");
  await expect(tree(page, "main").locator(`[data-file-drop-playlist="${other}"]`)).toHaveAttribute("aria-selected", "false");
});

test("a retired secondary pane cannot select into its replacement", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const toggle = page.getByRole("button", { name: "Sub-Browser Window", exact: true });
  await toggle.click();
  await row(page, "sub", "Melodic Vox").click();
  await holdCreate(page);
  await create(page);
  const id = await createdId(page);
  await toggle.click();
  await expect(tree(page, "sub")).toHaveCount(0);
  await toggle.click();
  await row(page, "sub", "Hardstyle").click();
  await release(page);
  await expect(page.getByRole("contentinfo")).toContainText("Created New playlist.");
  await expect(row(page, "sub", "Hardstyle")).toHaveAttribute("aria-selected", "true");
  await expect(tree(page, "sub").locator(`[data-file-drop-playlist="${id}"]`)).toHaveAttribute("aria-selected", "false");
});

test("a ready event retires pending selection even when IDs and selection match", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await row(page, "main", "Melodic Vox").click();
  await holdCreate(page);
  await create(page);
  const id = await createdId(page);
  // The mock's existing startup event retains the same tree IDs; no input
  // or pane navigation is involved in this ownership change.
  await page.evaluate(() => (window as unknown as HeldCreate).__libraryReady?.());
  await expect(row(page, "main", "Melodic Vox")).toHaveAttribute("aria-selected", "true");
  await release(page);
  await expect(page.getByRole("contentinfo")).toContainText("Created New playlist.");
  await expect(row(page, "main", "Melodic Vox")).toHaveAttribute("aria-selected", "true");
  await expect(tree(page, "main").locator(`[data-file-drop-playlist="${id}"]`)).toHaveAttribute("aria-selected", "false");
});

test("a refused ordinary create leaves the original playlist selected", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await row(page, "main", "Melodic Vox").click();
  await page.evaluate(async (path) => {
    const backend = await (await import(path) as typeof import("../src/ipc/client")).getBackend();
    backend.edits.createPlaylistWithResult = () => Promise.reject(new Error("Controlled refused write"));
  }, "/src/ipc/client.ts");
  await create(page);
  await expect(page.getByRole("contentinfo")).toContainText("Controlled refused write");
  await expect(row(page, "main", "Melodic Vox")).toHaveAttribute("aria-selected", "true");
  await expect(row(page, "main", "New playlist")).toHaveCount(0);
});
