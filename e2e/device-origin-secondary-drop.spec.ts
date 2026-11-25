import { expect, test } from "@playwright/test";

for (const targetSide of ["main", "sub"] as const) {
  for (const [formatIndex, format] of [[0, "deviceLibrary"], [1, "oneLibrary"]] as const) {
    test(`same-device ${format} tracks can be dragged to the ${targetSide} playlist`, async ({ page }) => {
      await page.addInitScript(() => {
        const evidence = { accepted: [] as string[], payloads: [] as string[] };
        Object.assign(window, { deviceOriginDropEvidence: evidence });
        document.addEventListener("dragover", (event) => {
          const row = (event.target as Element).closest('[data-kind="devicePlaylist"]');
          if (row && event.defaultPrevented) evidence.accepted.push(row.closest('[data-browser-side]')?.getAttribute("data-browser-side") ?? "");
        });
        document.addEventListener("drop", (event) => {
          if ((event.target as Element).closest('[data-kind="devicePlaylist"]')) evidence.payloads.push(event.dataTransfer?.getData("text/plain") ?? "");
        });
      });
      await page.goto("/?writable=1");
      await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
      await page.getByRole("button", { name: "Sub-Browser Window" }).click();
      const before = await page.evaluate(async () => {
        const moduleUrl = "/src/ipc/client.ts";
        const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
        const backend = await getBackend();
        const calls: string[] = [];
        Object.assign(window, { deviceOriginUnexpectedCalls: calls });
        const importPaths = backend.importPaths.bind(backend);
        backend.importPaths = (...args) => { calls.push("importPaths"); return importPaths(...args); };
        const exportTracks = backend.exportTracksToDevice.bind(backend);
        backend.exportTracksToDevice = (...args) => { calls.push("exportTracksToDevice"); return exportTracks(...args); };
        const addTracks = backend.edits.addTracksToPlaylist.bind(backend.edits);
        backend.edits.addTracksToPlaylist = (...args) => { calls.push("addTracksToPlaylist"); return addTracks(...args); };
        const path = (await backend.listDevices()).find((device) => device.name === "TEST")!.path;
        return backend.deviceLibraries(path);
      });
      const sourceSide = targetSide === "main" ? "sub" : "main";
      for (const side of [sourceSide, targetSide]) {
        const tree = page.locator(`nav[data-browser-side="${side}"]`);
        await tree.getByRole("tab", { name: "Devices" }).click();
        await tree.getByRole("button", { name: "Expand TEST" }).click();
        await expect(tree.locator('[data-kind="deviceLibrary"]')).toHaveCount(2);
      }
      const sourceTree = page.locator(`nav[data-browser-side="${sourceSide}"]`);
      await sourceTree.locator('[data-kind="deviceAllTracks"]').nth(formatIndex).click();
      const panel = page.getByRole("region", { name: "Sub-Browser" });
      const sourceGrid = sourceSide === "sub" ? panel.getByRole("grid") : page.getByRole("grid").first();
      const sourceRows = sourceGrid.locator('[role="row"]:has([role="gridcell"])');
      await expect(sourceRows).toHaveCount(6);
      const source = sourceRows.first();
      await expect(source).toBeVisible();
      const sourceTitles = await sourceRows.locator('[data-col="title"]').allTextContents();
      const targetTree = page.locator(`nav[data-browser-side="${targetSide}"]`);
      const target = targetTree.locator('[data-kind="devicePlaylist"]').filter({ hasText: /^\s*Melodic Vox/ }).nth(formatIndex);
      await expect(target).toBeVisible();
      await source.locator('[data-col="title"]').hover({ position: { x: 8, y: 8 } });
      await page.mouse.down();
      try {
        await target.hover({ position: { x: 100, y: 10 } });
        await target.hover({ position: { x: 110, y: 10 } });
        await expect(target).toHaveAttribute("data-device-track-drop", "true");
        await expect(target).toHaveAttribute("data-over", "true");
      } finally {
        await page.mouse.up();
      }
      await expect(page.getByRole("contentinfo")).toContainText("Added to Melodic Vox.");
      await target.click();
      const targetGrid = targetSide === "sub" ? panel.getByRole("grid") : page.getByRole("grid").first();
      await expect(targetGrid.locator('[role="row"]:has([role="gridcell"])')).toHaveCount(4);
      const evidence = await page.evaluate(() => (window as unknown as {
        deviceOriginDropEvidence: { accepted: string[]; payloads: string[] };
      }).deviceOriginDropEvidence);
      expect(evidence.accepted).toContain(targetSide);
      expect(evidence.payloads).toHaveLength(1);
      expect(evidence.payloads[0]).toMatch(/^file:/);
      const membership = await page.evaluate(async ({ format, payloads }) => {
        const moduleUrl = "/src/ipc/client.ts";
        const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
        const backend = await getBackend();
        const path = (await backend.listDevices()).find((device) => device.name === "TEST")!.path;
        const libraries = await backend.deviceLibraries(path);
        const target = libraries.find((library) => library.format === format)!.nodes.find((node) => node.name === "Melodic Vox")!;
        const otherFormat = format === "deviceLibrary" ? "oneLibrary" : "deviceLibrary";
        let foreignRefused = false;
        try { await backend.devicePlaylistDuplicates(path, format, target.id, ["file:/different-device/foreign.mp3"]); }
        catch { foreignRefused = true; }
        return {
          target: await backend.devicePlaylistDuplicates(path, format, target.id, payloads),
          other: await backend.devicePlaylistDuplicates(path, otherFormat, target.id, payloads),
          foreignRefused,
          libraries,
          unexpectedCalls: (window as unknown as { deviceOriginUnexpectedCalls: string[] }).deviceOriginUnexpectedCalls,
        };
      }, { format, payloads: evidence.payloads });
      expect(membership.target).toEqual(evidence.payloads);
      expect(membership.other).toEqual([]);
      expect(membership.foreignRefused).toBe(true);
      expect(membership.unexpectedCalls).toEqual([]);
      await expect(sourceRows.locator('[data-col="title"]')).toHaveText(sourceTitles);
      const otherFormat = format === "deviceLibrary" ? "oneLibrary" : "deviceLibrary";
      expect(membership.libraries.find((library) => library.format === otherFormat))
        .toEqual(before.find((library) => library.format === otherFormat));
      const edited = membership.libraries.find((library) => library.format === format)!;
      const original = before.find((library) => library.format === format)!;
      expect(edited.tracks).toBe(original.tracks);
      expect(edited.nodes.filter((node) => node.name !== "Melodic Vox"))
        .toEqual(original.nodes.filter((node) => node.name !== "Melodic Vox"));
      // A completed drag clears its eligibility; starting again still uses
      // the existing duplicate question, and Cancel leaves membership alone.
      await expect(target).not.toHaveAttribute("data-device-track-drop", "true");
      await source.locator('[data-col="title"]').dragTo(target, { targetPosition: { x: 100, y: 10 } });
      const duplicates = page.getByRole("alertdialog", { name: "Duplicate tracks" });
      await expect(duplicates).toContainText("1 of the selected tracks are already in Melodic Vox.");
      await duplicates.getByRole("button", { name: "Cancel" }).click();
      await expect(duplicates).toHaveCount(0);
      await expect(target).not.toHaveAttribute("data-device-track-drop", "true");
      await expect(targetGrid.locator('[role="row"]:has([role="gridcell"])')).toHaveCount(4);
    });
  }
}
