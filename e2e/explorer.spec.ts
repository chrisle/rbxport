/**
 * The Explorer: the disk as a section of the tree, a folder as a track list.
 *
 * Against the mock's fake disk — four roots, a few folders, one folder of
 * files — which has the shape of the 9.10.55 PM capture.
 */
import { expect, test } from "@playwright/test";
import process from "node:process";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("grid")).toBeVisible();
  // The library, not just the window: its arrival selects the first playlist,
  // and a click on the rail before that would be selected over.
  await expect(page.getByRole("treeitem", { name: /Melodic Vox/ })).toBeVisible();
});

/** The tree, the rail, and the Explorer's own rows. */
function parts(page: import("@playwright/test").Page) {
  const tree = page.getByRole("navigation", { name: "Library" });
  return {
    tree,
    rail: page.getByRole("tablist", { name: "Library sources" }),
    // A row's accessible name ends with its label; a branch's begins with its
    // twisty's "Expand …" or "Collapse …".
    item: (name: string) => tree.getByRole("treeitem", { name: new RegExp(`(^|\\s)${name}$`) }),
    open: (name: string) => tree.getByRole("button", { name: `Expand ${name}` }),
    rows: page.getByRole("row").filter({ has: page.getByRole("gridcell") }),
    title: page.getByTestId("browser-title"),
  };
}

test("the rail's Explorer button opens the section on an empty Explorer", async ({ page }) => {
  const { rail, item, rows, title } = parts(page);
  const button = rail.getByRole("tab", { name: "Explorer" });
  await expect(button).not.toHaveAttribute("data-empty", "true");
  await button.click();
  await expect(button).toHaveAttribute("aria-selected", "true");

  // The heading is selected and the list is empty, as the capture shows —
  // and the heading's row is at the top of the pane, the playlists above it
  // scrolled out of sight.
  await expect(item("Explorer")).toHaveAttribute("aria-selected", "true");
  await expect(title).toContainText("Explorer (0 Tracks)");
  await expect(rows).toHaveCount(0);
  const list = page.getByRole("tree");
  const scrollable = await list.evaluate((el) => el.scrollHeight > el.clientHeight);
  const heading = await item("Explorer").boundingBox();
  const pane = await list.boundingBox();
  // Only where there is anything to scroll: the mock's tree fits a tall
  // window whole, and a row that cannot move is not a row out of place.
  if (scrollable) expect(heading && pane && Math.abs(heading.y - pane.y) < 2).toBe(true);
  else expect(heading && pane && heading.y >= pane.y).toBe(true);

  // The roots of the capture, closed, under it.
  for (const root of ["Music", "mock", "Macintosh HD", "SD"]) {
    await expect(item(root)).toBeVisible();
    await expect(item(root)).toHaveAttribute("aria-expanded", "false");
  }
});

test("the Explorer's columns are the FolderTracks header, in the capture's order", async ({ page }) => {
  const { rail } = parts(page);
  await rail.getByRole("tab", { name: "Explorer" }).click();
  const headers = page.getByRole("columnheader");
  await expect(headers).toHaveText([
    /#/, /Preview/, /Artwork/, /Track Title/, /Artist/, /Album/, /Genre/, /BPM/, /Rating/, /Time/,
    /Key/, /File Name/,
  ]);
  // And the collection keeps its own: the two are separate layouts.
  await rail.getByRole("tab", { name: "Playlists" }).click();
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  await expect(page.getByRole("columnheader", { name: /File Name/ })).toHaveCount(0);
});

test("a root opens into the real folder tree one level at a time", async ({ page }) => {
  const { rail, item, open } = parts(page);
  await rail.getByRole("tab", { name: "Explorer" }).click();

  await open("Macintosh HD").click();
  await expect(item("Macintosh HD")).toHaveAttribute("aria-expanded", "true");
  for (const name of ["Applications", "Library", "System", "Users"]) {
    await expect(item(name)).toBeVisible();
    // Closed until opened: nothing under them has been read.
    await expect(item(name)).toHaveAttribute("aria-expanded", "false");
  }
  await open("Users").click();
  await expect(item("Shared")).toBeVisible();
  // The home folder again, under Users, as the capture has it — a second row
  // with its own identity, not the root moved.
  await expect(item("mock")).toHaveCount(2);

  // Closing hides the branch and reopening costs nothing: it is still there.
  await page.getByRole("button", { name: "Collapse Macintosh HD" }).click();
  await expect(item("Users")).toHaveCount(0);
  await open("Macintosh HD").click();
  await expect(item("Shared")).toBeVisible();
});

test("library and Explorer folder expansion survives a restart", async ({ page }) => {
  const { rail, item, open } = parts(page);

  // A library folder that starts open is explicitly closed.
  await page.getByRole("button", { name: "Collapse CURRENT" }).click();
  await expect(item("CURRENT")).toHaveAttribute("aria-expanded", "false");

  // Explorer descendants are lazy, so restoring this state must reopen each
  // ancestor before the next remembered folder can even be reconstructed.
  await rail.getByRole("tab", { name: "Explorer" }).click();
  await open("Music").click();
  await open("Rekordbox").click();
  await expect(item("Sets")).toBeVisible();

  await page.reload();
  await expect(page.getByRole("grid")).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /CURRENT$/ })).toHaveAttribute("aria-expanded", "false");
  await expect(page.getByRole("treeitem", { name: /Melodic Vox/ })).toHaveCount(0);

  const restored = parts(page);
  await restored.rail.getByRole("tab", { name: "Explorer" }).click();
  await expect(restored.item("Music")).toHaveAttribute("aria-expanded", "true");
  await expect(restored.item("Rekordbox")).toHaveAttribute("aria-expanded", "true");
  await expect(restored.item("Sets")).toBeVisible();
});

test("selecting a folder lists its audio files, the library's rows first among them", async ({ page }) => {
  const { rail, item, open, rows, title } = parts(page);
  await rail.getByRole("tab", { name: "Explorer" }).click();
  await open("Music").click();
  await item("Downloads").click();

  await expect(title).toContainText("Downloads (12 Tracks)");
  await expect(rows).toHaveCount(12);
  // A file the library holds shows the library's row; one it does not shows
  // its own name and nothing the library would know.
  const known = rows.nth(0);
  await expect(known.getByRole("gridcell").nth(3)).not.toHaveText("");
  const loose = rows.nth(6);
  await expect(loose).toContainText("Untitled Bounce 1");
  await expect(loose).toContainText("Untitled Bounce 1.wav");

  // Searching narrows it the way it narrows a playlist. The tree grew its
  // own search field too (2026-09-20); this one is the track list's.
  await page.getByRole("searchbox", { name: "Search within this track list" }).fill("bounce");
  await expect(rows).toHaveCount(2);
});

test("a folder with nothing under it, and one that cannot be read, open empty", async ({ page }) => {
  const { rail, item, open, rows, title } = parts(page);
  await rail.getByRole("tab", { name: "Explorer" }).click();
  await open("Music").click();
  await item("Rekordbox").click();
  await expect(title).toContainText("Rekordbox (0 Tracks)");
  await expect(rows).toHaveCount(0);
  // Opening it reads nothing, and it stays a branch: a folder without a
  // twisty could never be asked again.
  await open("Rekordbox").click();
  await expect(item("Rekordbox")).toHaveAttribute("aria-expanded", "true");
  await expect(item("Sets")).toBeVisible();
});

test("a loose file cannot be rated: the library does not hold it", async ({ page }) => {
  // Writable, so the rating reaches the "not in the collection" refusal
  // rather than stopping at the mock's read-only default first.
  await page.goto("/?writable=1");
  await expect(page.getByRole("treeitem", { name: /Melodic Vox/ })).toBeVisible();
  const { rail, item, open, rows } = parts(page);
  await rail.getByRole("tab", { name: "Explorer" }).click();
  await open("Music").click();
  await item("Downloads").click();
  await expect(rows).toHaveCount(12);
  await rows.nth(7).getByRole("radio", { name: "3 of 5" }).click();
  await expect(page.getByRole("alert")).toContainText("not in the collection");
});

test("a folder cut at the backend's cap says how many folders were left out", async ({ page }) => {
  const { rail, item, open, tree } = parts(page);
  await rail.getByRole("tab", { name: "Explorer" }).click();
  await open("SD").click();
  await open("PIONEER").click();
  await expect(item("rekordbox")).toBeVisible();
  const note = tree.getByRole("treeitem", { name: /14,501 more folders not shown/ });
  await expect(note).toBeVisible();
  // A line of information, not a place: clicking it selects nothing.
  await note.click();
  await expect(note).toHaveAttribute("aria-selected", "false");
  await expect(item("Explorer")).toHaveAttribute("aria-selected", "true");
});

test("the track menu follows each file's import state, as rekordbox's Explorer menu does", async ({ page }, info) => {
  // rekordbox 7 [OBS Winrig 2026-10-08, issue #105]: over a file the
  // collection holds, Import To Collection is greyed and Analyze Track and
  // Remove from Collection are live; over one it does not, the reverse, with
  // Add To Playlist live either way. Neither draws Convert Memory Cues.
  await page.goto("/?writable=1");
  await expect(page.getByRole("treeitem", { name: /Melodic Vox/ })).toBeVisible();
  const { rail, item, open, rows } = parts(page);
  await rail.getByRole("tab", { name: "Explorer" }).click();
  await open("Music").click();
  await item("Downloads").click();
  await expect(rows).toHaveCount(12);

  const menu = page.getByRole("menu", { name: "Track" });
  const entry = (name: string) => menu.getByRole("menuitem", { name, exact: true });
  const shots = process.env.RBX_PARITY_DIR;

  await rows.nth(0).click({ button: "right" });
  await expect(entry("Import To Collection")).toBeDisabled();
  await expect(entry("Analyze Track")).toBeEnabled();
  await expect(entry("Remove from Collection")).toBeEnabled();
  await expect(entry("Add To Playlist")).toBeEnabled();
  await expect(entry("Convert Memory Cues to Hot Cues")).toHaveCount(0);
  if (shots) await page.screenshot({ path: `${shots}/rbx-after-menu-imported-${info.project.name}.png` });
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);

  await rows.nth(6).click({ button: "right" });
  await expect(entry("Import To Collection")).toBeEnabled();
  await expect(entry("Analyze Track")).toBeDisabled();
  await expect(entry("Remove from Collection")).toBeDisabled();
  await expect(entry("Add To Playlist")).toBeEnabled();
  if (shots) await page.screenshot({ path: `${shots}/rbx-after-menu-not-imported-${info.project.name}.png` });
});
