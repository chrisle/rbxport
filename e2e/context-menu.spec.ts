import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * The right-click menus, measured against docs/screenshots
 * context-menu-tree@2x and context-menu-track@2x (rekordbox 7.2.11, 2x): a
 * 25pt row pitch in Arial 12.5, a point of panel above the first row and
 * below the last, an 11pt separator block, a 6x8 arrow 17pt in from the
 * right on the entries that open a submenu, and the greyed entries said to
 * be so.
 */

async function openTreeMenu(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const playlist = page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first();
  await playlist.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Playlist" });
  await expect(menu).toBeVisible();
  return menu;
}

/** The geometry of every entry, relative to the menu's outer edge. */
function rows(menu: Locator) {
  return menu.evaluate((m) => {
    const box = m.getBoundingClientRect();
    return [...m.querySelectorAll<HTMLElement>('[role="menuitem"]')].map((item) => {
      const r = item.getBoundingClientRect();
      const label = item.querySelector("span")!.getBoundingClientRect();
      const style = getComputedStyle(item);
      return {
        label: item.textContent ?? "",
        top: r.top - box.top,
        height: r.height,
        textLeft: label.left - box.left,
        fontSize: style.fontSize,
        fontWeight: style.fontWeight,
        family: style.fontFamily,
      };
    });
  });
}

test("the rows are on rekordbox's 25pt pitch in its 12.5px face", async ({ page }) => {
  const menu = await openTreeMenu(page);
  const items = await rows(menu);
  // Thirteen in the capture, less the three cloud rows and Collaborative
  // playlist, plus Rename.
  expect(items).toHaveLength(10);
  for (const item of items) {
    expect(item.height, item.label).toBe(25);
    expect(item.fontSize, item.label).toBe("12.5px");
    expect(item.fontWeight, item.label).toBe("400");
    expect(item.family, item.label).toMatch(/^Arial/);
    // 25pt past the hairline: 'Analyze Track' starts 52px in at 2x.
    expect(item.textLeft, item.label).toBe(26);
  }
  // Two entries in one group are a row apart; across a separator they are a
  // row and the 11pt separator block apart (1pt rule, 5pt above and below).
  const at = (label: string) => items.find((i) => i.label === label)!.top;
  expect(at("Create New Intelligent Playlist") - at("Create New Playlist")).toBe(25);
  expect(at("Create New Folder") - at("Create New Intelligent Playlist")).toBe(25);
  expect(at("Create New Playlist") - at("Export Playlist")).toBe(36);
  expect(at("Add To Shortcut") - at("Export a playlist to a file")).toBe(36);
});

test("the panel is a point of padding inside a one-point hairline", async ({ page }) => {
  const menu = await openTreeMenu(page);
  const panel = await menu.evaluate((m) => {
    const style = getComputedStyle(m);
    return {
      paddingTop: style.paddingTop,
      paddingBottom: style.paddingBottom,
      paddingLeft: style.paddingLeft,
      border: style.borderTopWidth,
      height: m.getBoundingClientRect().height,
    };
  });
  expect(panel.paddingTop).toBe("1px");
  expect(panel.paddingBottom).toBe("1px");
  expect(panel.paddingLeft).toBe("0px");
  expect(panel.border).toBe("1px");
  // Ten rows, six separators, the padding and the hairline. The capture's
  // panel is 812px tall at 2x: four cloud rows and a separator shorter, one
  // Rename taller.
  expect(panel.height).toBe(10 * 25 + 6 * 11 + 2 + 2);

  const separators = await menu.evaluate((m) => {
    const box = m.getBoundingClientRect();
    return [...m.querySelectorAll('[role="separator"]')].map((s) => {
      const r = s.getBoundingClientRect();
      return { height: r.height, left: r.left - box.left, right: box.right - r.right };
    });
  });
  expect(separators).toHaveLength(6);
  for (const s of separators) {
    expect(s.height).toBe(1);
    // 4pt in from the hairline on the left, 16pt on the right.
    expect(s.left).toBe(5);
    expect(s.right).toBe(17);
  }
});

test("entries that open a submenu carry the arrow, 17pt in from the right", async ({ page }) => {
  const menu = await openTreeMenu(page);
  const arrows = await menu.evaluate((m) => {
    const box = m.getBoundingClientRect();
    return [...m.querySelectorAll<HTMLElement>('[role="menuitem"]')].map((item) => {
      const arrow = item.querySelector<HTMLElement>("span:nth-child(2)");
      const r = arrow?.getBoundingClientRect();
      return {
        label: item.textContent ?? "",
        arrow: r ? { width: r.width, height: r.height, right: box.right - r.right } : null,
      };
    });
  });
  const withArrow = arrows.filter((a) => a.arrow).map((a) => a.label);
  expect(withArrow).toEqual(["Export Playlist", "Export a playlist to a file"]);
  for (const a of arrows) {
    if (!a.arrow) continue;
    // Drawn from borders, so the box is the triangle: 6 wide, 8 tall, its
    // point 17pt short of the hairline's inside plus the hairline itself.
    expect(a.arrow.width, a.label).toBe(6);
    expect(a.arrow.height, a.label).toBe(8);
    expect(a.arrow.right, a.label).toBe(18);
  }
});

test("the greyed entries are said to be disabled, and the live ones are not", async ({ page }) => {
  const menu = await openTreeMenu(page);
  // Greyed and announced so: the writes the mock's read-only library refuses.
  const greyed = [
    "Playlist display setting",
    "Add Artwork",
  ];
  for (const label of greyed) {
    const item = menu.getByRole("menuitem", { name: label });
    await expect(item).toHaveAttribute("aria-disabled", "true");
    await expect(item).toBeDisabled();
  }
  const live = menu.getByRole("menuitem", { name: "Export Playlist" });
  await expect(live).not.toHaveAttribute("aria-disabled");
  await expect(live).toBeEnabled();

  // The greyed ink is the capture's (97,97,97); the live ink is white.
  const colour = (label: string) =>
    menu.getByRole("menuitem", { name: label }).evaluate((el) => getComputedStyle(el).color);
  expect(await colour("Add Artwork")).toBe("rgb(97, 97, 97)");
  expect(await colour("Export Playlist")).toBe("rgb(255, 255, 255)");
});

test("Export Playlist lists the connected sticks and writes to the one chosen", async ({ page }) => {
  // #142: it opened a folder picker. rekordbox's Export Playlist submenu is
  // the connected drives [OBS rekordbox 7, Winrig 2026-10-08], and the mock
  // has two sticks.
  const menu = await openTreeMenu(page);
  await menu.getByRole("menuitem", { name: "Export Playlist" }).hover();
  const sticks = page.getByRole("menuitem", { name: "DJ STICK", exact: true });
  await expect(sticks).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "TEST", exact: true })).toBeVisible();
  await sticks.click();
  await expect(page.getByRole("contentinfo")).toContainText(/DJ STICK: Updated \d+ tracks/);
});

test("the track menu is the same panel, with rekordbox's entries less the cloud", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(2).click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await expect(menu).toBeVisible();

  const items = await rows(menu);
  // Twenty-one in the capture, less Cloud Library Sync.
  expect(items).toHaveLength(20);
  expect(items.map((i) => i.label)).toContain("Convert Memory Cues to Hot Cues");
  expect(items.map((i) => i.label)).not.toContain("Cloud Library Sync");
  for (const item of items) {
    expect(item.height, item.label).toBe(25);
    expect(item.textLeft, item.label).toBe(26);
  }
  // Twenty rows and seven separators; 1212px at 2x in the capture, one row less
  // here. The groups are unchanged: the cloud row shared one with Export Track.
  const height = await menu.evaluate((m) => m.getBoundingClientRect().height);
  expect(height).toBe(20 * 25 + 7 * 11 + 2 + 2);
  await expect(menu.getByRole("menuitem", { name: "Track information" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
});
