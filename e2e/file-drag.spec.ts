import { expect, test } from "@playwright/test";

const trackRows = (page: import("@playwright/test").Page) =>
  page.getByRole("row").filter({ has: page.getByRole("gridcell") });

test("an internal track drag carrying native files adds tracks instead of importing", async ({ page }) => {
  await page.goto("/?writable=1");
  const source = trackRows(page).nth(1);
  await expect(source).toBeVisible();
  // Native file sources carry Files even when the tracks came from this app.
  // Keep the app's internal drag state while supplying that OS payload.
  await source.dispatchEvent("dragstart", { dataTransfer: await page.evaluateHandle(() => new DataTransfer()) });
  const target = page.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first();
  const files = await page.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(["audio"], "track.mp3", { type: "audio/mpeg" }));
    return transfer;
  });
  await target.dispatchEvent("dragover", { dataTransfer: files });
  await target.dispatchEvent("drop", { dataTransfer: files });
  await expect(page.getByRole("contentinfo")).toContainText(/Added \d+ track/);
});

test("Finder files dropped onto an existing playlist row reach the import handler", async ({ page }) => {
  await page.goto("/?writable=1");
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const row = trackRows(page).first();
  await expect(row).toBeVisible();
  const files = await page.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(["audio"], "track.mp3", { type: "audio/mpeg" }));
    return transfer;
  });
  await row.dispatchEvent("dragover", { dataTransfer: files });
  await row.dispatchEvent("drop", { dataTransfer: files });
  // This browser has no native filesystem paths. A readable refusal proves
  // the row let the external drop reach the playlist's import callback.
  await expect(page.getByRole("contentinfo")).toContainText("Drop files in the desktop app to import them.");
});

for (const target of ["playlist header", "outside the playlist"] as const) {
  test(`an internal file drop on ${target} cannot navigate to the audio`, async ({ page }) => {
    await page.goto("/?writable=1");
    await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
    const source = trackRows(page).first();
    await expect(source).toBeVisible();
    const before = await trackRows(page).allTextContents();
    await source.dispatchEvent("dragstart", {
      dataTransfer: await page.evaluateHandle(() => new DataTransfer()),
    });
    const cancelled = await page.evaluate((target) => {
      const transfer = new DataTransfer();
      transfer.items.add(new File(["audio"], "track.mp3", { type: "audio/mpeg" }));
      const destination = target === "playlist header"
        ? document.querySelector("[data-testid=browser-title]")!.parentElement!.parentElement!
        : document.body;
      // Synthetic drops cannot navigate, so check the default action itself
      // was cancelled. Merely asserting the URL would miss this regression.
      const drop = new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer });
      destination.dispatchEvent(drop);
      return drop.defaultPrevented;
    }, target);
    expect(cancelled).toBe(true);
    expect(await trackRows(page).allTextContents()).toEqual(before);
    await expect(page.getByRole("contentinfo")).not.toContainText("Imported");
    await source.dispatchEvent("dragend");
  });
}

for (const count of [1, 2]) {
  test(`dropping ${count} tracks below the final row appends them in playlist order`, async ({ page }) => {
    await page.setViewportSize({ width: 1800, height: 1600 });
    await page.goto("/?writable=1");
    await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
    const rows = trackRows(page);
    const titles = () => rows.locator('[data-col="title"]').allTextContents();
    await expect(rows.first()).toBeVisible();
    const before = await titles();
    expect(before.length).toBeGreaterThan(2);
    await rows.first().click();
    if (count > 1) await rows.nth(count - 1).click({ modifiers: ["Shift"] });
    const scroller = page.getByTestId("track-scroll");
    const box = await scroller.boundingBox();
    const last = await rows.last().boundingBox();
    if (!box || !last) throw new Error("Playlist is not visible");
    const y = last.y + last.height + 40;
    expect(y).toBeLessThan(box.y + box.height);
    await rows.first().dragTo(scroller, { targetPosition: { x: 300, y: y - box.y } });
    await expect(page.getByRole("contentinfo")).toContainText("Playlist reordered.");
    await expect.poll(titles).toEqual([...before.slice(count), ...before.slice(0, count)]);
  });
}

test("a native file drag below the last row is accepted and appends the track", async ({ page }) => {
  await page.setViewportSize({ width: 1800, height: 1600 });
  await page.goto("/?writable=1");
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const rows = trackRows(page);
  const titles = () => rows.locator('[data-col="title"]').allTextContents();
  await expect(rows.first()).toBeVisible();
  const before = await titles();
  await rows.first().dispatchEvent("dragstart", {
    dataTransfer: await page.evaluateHandle(() => new DataTransfer()),
  });
  const accepted = await page.getByTestId("track-scroll").evaluate((scroll) => {
    const rows = scroll.querySelectorAll('[role="row"][data-selected], [role="row"][data-even]');
    const last = rows[rows.length - 1]!.getBoundingClientRect();
    const transfer = new DataTransfer();
    transfer.items.add(new File(["audio"], "track.mp3", { type: "audio/mpeg" }));
    const options = { bubbles: true, cancelable: true, dataTransfer: transfer, clientY: last.bottom + 40 };
    const over = new DragEvent("dragover", options);
    scroll.dispatchEvent(over);
    const drop = new DragEvent("drop", options);
    scroll.dispatchEvent(drop);
    return { over: over.defaultPrevented, drop: drop.defaultPrevented };
  });
  expect(accepted).toEqual({ over: true, drop: true });
  await expect.poll(titles).toEqual([...before.slice(1), before[0]]);
});

/**
 * A folder from Finder dropped onto the Playlists root or a playlist folder
 * becomes a playlist named after it, as in rekordbox 7
 * (`TreeViewer::treeMessageImportExternalFoldersToList`). The browser has no
 * file system, so the drop carries paths the way other desktop hosts do, and
 * the mock makes the playlist empty.
 */
const folderDrop = (page: import("@playwright/test").Page, paths: string[]) =>
  page.evaluateHandle((paths) => {
    const transfer = new DataTransfer();
    for (const path of paths) {
      const name = path.split("/").pop() ?? path;
      transfer.items.add(new File([], name));
    }
    // `dataTransfer.files` hands back the File objects added; give each the
    // path a desktop host would carry.
    Array.from(transfer.files).forEach((file, i) => Object.defineProperty(file, "path", { value: paths[i] }));
    return transfer;
  }, paths);

test("a folder dropped on the Playlists root becomes a playlist named after it", async ({ page }) => {
  await page.goto("/?writable=1");
  const root = page.getByRole("treeitem").filter({ hasText: /^Playlists/ }).first();
  await expect(root).toHaveAttribute("data-file-drop-folder", "root");
  const drop = await folderDrop(page, ["/Music/Friday Set"]);
  await root.dispatchEvent("dragover", { dataTransfer: drop });
  await root.dispatchEvent("drop", { dataTransfer: drop });
  await expect(page.getByRole("contentinfo")).toContainText("Made playlists: Friday Set.");
  const made = page.getByRole("treeitem").filter({ hasText: "Friday Set" });
  await expect(made).toHaveCount(1);
  await expect(made).toHaveAttribute("data-kind", "playlist");

  // Again: rekordbox's question, answered yes by the mock, replaces it.
  const again = await folderDrop(page, ["/Music/Friday Set"]);
  await root.dispatchEvent("drop", { dataTransfer: again });
  await expect(page.getByRole("contentinfo")).toContainText("Made playlists: Friday Set.");
  await expect(page.getByRole("treeitem").filter({ hasText: "Friday Set" })).toHaveCount(1);
});

test("a folder dropped on a playlist folder lands inside it; loose files are ignored", async ({ page }) => {
  await page.goto("/?writable=1");
  const folder = page.locator('[role="treeitem"][data-kind="folder"]').first();
  const id = await folder.getAttribute("data-file-drop-folder");
  expect(id).toBeTruthy();
  const drop = await folderDrop(page, ["/Music/Warm Up"]);
  await folder.dispatchEvent("drop", { dataTransfer: drop });
  await expect(page.getByRole("contentinfo")).toContainText("Made playlists: Warm Up.");
  const made = page.getByRole("treeitem").filter({ hasText: "Warm Up" });
  const depth = async (row: import("@playwright/test").Locator) =>
    row.evaluate((el) => Number.parseFloat((el as HTMLElement).style.paddingLeft));
  expect(await depth(made)).toBeGreaterThan(await depth(folder));

  const loose = await folderDrop(page, ["/Music/track.mp3"]);
  await folder.dispatchEvent("drop", { dataTransfer: loose });
  await expect(page.getByRole("contentinfo")).toContainText(
    "Drop folders onto Playlists or a playlist folder to make playlists of them.",
  );
});
