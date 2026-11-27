/**
 * The track list against a library that is not ready yet.
 *
 * The backend loads on its own thread, so the first `open_view` can arrive
 * before there is anything to answer with and comes back "The library has not
 * finished loading yet." `?slow` holds the mock back the same way, until
 * `window.__libraryReady()` is called.
 *
 * The tree already survived this; the table did not, and a list that opens
 * empty and stays empty is what a user sees as a blank window.
 */
import { expect, test, type Page } from "@playwright/test";

/**
 * Lets the mock's library finish loading.
 *
 * The backend module is imported lazily, so the hook does not exist until it
 * has landed — waiting for it is waiting for the same moment the real app
 * waits for.
 */
async function releaseLibrary(page: Page): Promise<void> {
  await page.waitForFunction(
    () => typeof (window as unknown as { __libraryReady?: unknown }).__libraryReady === "function",
  );
  await page.evaluate(() => (window as unknown as { __libraryReady: () => void }).__libraryReady());
}

type CreateLibraryFixture = {
  __finishLibraryCreation: () => void;
  __reportUnavailableLibrary: () => void;
};

async function startLibraryCreation(page: Page): Promise<void> {
  // Keep the real App and dialog. The native Create command returns after
  // scheduling a load, so the command's reply and the load event can arrive
  // in either order.
  await page.route("**/src/main.tsx", async (route) => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as fixtureBackend } from "/src/ipc/client.ts";
      const backend = await fixtureBackend();
      const subscribe = backend.onLibraryProblem;
      backend.onLibraryProblem = (listener) => {
        window.__reportUnavailableLibrary = () => listener({
          kind: "unavailable",
          masterDb: "/disconnected/rekordbox/master.db",
          defaultMasterDb: "/fixture/rekordbox/master.db",
        });
        return subscribe(listener);
      };
      backend.createLibrary = () => new Promise(resolve => {
        window.__finishLibraryCreation = resolve;
      });
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?nolibrary");
  await page.getByRole("button", { name: "Create", exact: true }).click();
  await expect(page.getByRole("button", { name: "Creating…" })).toBeDisabled();
}

async function finishLibraryCreation(page: Page): Promise<void> {
  await page.evaluate(async () => {
    (window as unknown as CreateLibraryFixture).__finishLibraryCreation();
    // Let the command's promise and its React render finish before checking
    // whether the dialog survived the reply.
    await new Promise<void>((resolve) => requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve());
    }));
  });
}

test("creating a library keeps the dialog until the library loads", async ({ page }) => {
  await startLibraryCreation(page);
  await finishLibraryCreation(page);
  await expect(page.getByRole("button", { name: "Creating…" })).toBeDisabled();

  await releaseLibrary(page);
  await expect(page.getByRole("dialog", { name: "No rekordbox Library" })).toHaveCount(0);
  await expect(page.locator('[role="row"] [data-col="title"]').first()).toBeVisible();
});

test("the Create reply cannot dismiss a newer library question", async ({ page }) => {
  await startLibraryCreation(page);
  await page.evaluate(() => (window as unknown as CreateLibraryFixture).__reportUnavailableLibrary());
  const unavailable = page.getByRole("dialog").filter({ hasText: "Cannot find Master Database." });
  await expect(unavailable).toBeVisible();

  await finishLibraryCreation(page);
  await expect(unavailable).toBeVisible();
});

test("a view opened before the library is ready fills in once it is", async ({ page }) => {
  await page.goto("/?slow&tracks=4000");

  // Nothing to draw yet, which is correct — the library genuinely is not up.
  await expect(page.locator('[role="row"] [data-col="title"]')).toHaveCount(0);

  await releaseLibrary(page);

  // The rows must arrive on their own. Nobody clicks anything here: the point
  // is that the view recovers without the user having to go somewhere else and
  // come back.
  await expect
    .poll(async () => page.locator('[role="row"] [data-col="title"]').count(), { timeout: 10_000 })
    .toBeGreaterThan(5);
  await expect(page.locator('[role="row"] [data-col="title"]').first()).not.toBeEmpty();
});

test("a library startup failure replaces the loading message and remains in the log", async ({ page }) => {
  const message = "database disk image is malformed";
  await page.route("**/src/main.tsx", async route => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as startupBackend } from "/src/ipc/client.ts";
      const backend = await startupBackend();
      const subscribe = backend.onLibraryProblem;
      backend.onLibraryProblem = listener => {
        window.__failLibraryStartup = () => listener({ kind: "failed", message: ${JSON.stringify(message)} });
        return subscribe(listener);
      };
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?slow");
  const footer = page.getByRole("contentinfo");
  await expect(footer).toContainText("Loading the library…");
  await page.waitForFunction(() => typeof (window as unknown as { __failLibraryStartup?: unknown }).__failLibraryStartup === "function");
  await page.evaluate(() => (window as unknown as { __failLibraryStartup: () => void }).__failLibraryStartup());

  const error = footer.getByRole("alert").getByRole("button", { name: message });
  await expect(error).toBeVisible();
  await expect(footer).not.toContainText("Loading the library…");
  await error.click();
  await expect(page.getByRole("log", { name: "Log lines" })).toContainText(message);
});

test("scrolling a recovered view still fetches the rows it lands on", async ({ page }) => {
  // The original report: pick the playlists root, drag the scrollbar halfway,
  // see nothing. A view that opened empty stays empty at every scroll position,
  // because its row count is zero and no window is ever asked for.
  await page.goto("/?slow&tracks=4000");
  await releaseLibrary(page);
  await expect
    .poll(async () => page.locator('[role="row"] [data-col="title"]').count(), { timeout: 10_000 })
    .toBeGreaterThan(5);

  await page.evaluate(() => {
    const el = document.querySelector('[data-testid="track-scroll"]');
    if (el) el.scrollTop = (el.scrollHeight - el.clientHeight) / 2;
  });

  await expect
    .poll(
      async () => {
        const t = await page.locator('[role="row"] [data-col="title"]').allTextContents();
        return t.length > 0 && t.every((x) => x.trim() !== "") ? t.length : 0;
      },
      { timeout: 10_000 },
    )
    .toBeGreaterThan(5);
});

async function holdStartupProblem(page: Page, holdReady = false) {
  await page.route("**/src/main.tsx", async route => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as startupBackend } from "/src/ipc/client.ts";
      const backend = await startupBackend();
      const subscribeProblem = backend.onLibraryProblem;
      const readTree = backend.playlistTree;
      const create = backend.createLibrary;
      let creating = false;
      backend.createLibrary = () => { creating = true; return create(); };
      backend.playlistTree = () => {
        const result = readTree();
        if (!creating || !${holdReady}) return result;
        return result.then(rows => new Promise(resolve => { window.__releaseReadyTree = () => resolve(rows); }));
      };
      let reportProblem;
      backend.onLibraryProblem = listener => {
        reportProblem = listener;
        return subscribeProblem(listener);
      };
      backend.libraryProblem = () => new Promise(resolve => {
        const missing = { kind: "missing", masterDb: "/fixture/master.db" };
        window.__releaseStartupProblem = () => resolve(missing);
        window.__newLibraryProblem = () => reportProblem({ kind: "unavailable", masterDb: "/disconnected/master.db", defaultMasterDb: "/fixture/master.db" });
        queueMicrotask(() => reportProblem(missing));
      });
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?nolibrary");
  await page.getByRole("button", { name: "Create", exact: true }).click();
  if (holdReady) {
    await page.waitForFunction(() => typeof (window as unknown as { __releaseReadyTree?: unknown }).__releaseReadyTree === "function");
    return;
  }
  await expect(page.locator('[role="row"] [data-col="title"]').first()).toBeVisible();
  await expect(page.getByRole("dialog", { name: "No rekordbox Library" })).toHaveCount(0);
}

test("an old startup problem reply cannot reopen the dialog after Create loads", async ({ page }) => {
  await holdStartupProblem(page);
  await page.evaluate(async () => {
    (window as unknown as { __releaseStartupProblem: () => void }).__releaseStartupProblem();
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  await expect(page.getByRole("dialog", { name: "No rekordbox Library" })).toHaveCount(0);
});

test("a new library problem still opens after Create loads and retires the old reply", async ({ page }) => {
  await holdStartupProblem(page);
  await page.evaluate(() => (window as unknown as { __newLibraryProblem: () => void }).__newLibraryProblem());
  const unavailable = page.getByRole("dialog").filter({ hasText: "Cannot find Master Database." });
  await expect(unavailable).toBeVisible();
  await page.evaluate(async () => {
    (window as unknown as { __releaseStartupProblem: () => void }).__releaseStartupProblem();
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  await expect(unavailable).toBeVisible();
});


test("an older ready response cannot dismiss a newer library question", async ({ page }) => {
  await holdStartupProblem(page, true);
  await page.evaluate(() => (window as unknown as { __newLibraryProblem: () => void }).__newLibraryProblem());
  const unavailable = page.getByRole("dialog").filter({ hasText: "Cannot find Master Database." });
  await expect(unavailable).toBeVisible();
  await page.evaluate(async () => {
    (window as unknown as { __releaseReadyTree: () => void }).__releaseReadyTree();
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  await expect(unavailable).toBeVisible();
});
