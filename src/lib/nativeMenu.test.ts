import { expect, it } from "vitest";

import { nativeMenuLabels } from "./nativeMenu";

it("sends every native menu label through the active translator", () => {
  const labels = nativeMenuLabels((source) => `translated:${source}`);

  expect(labels.File).toBe("translated:File");
  expect(labels.Edit).toBe("translated:Edit");
  expect(labels.Track).toBe("translated:Track");
  expect(labels["Locate track loaded on deck"]).toBe("translated:Locate track loaded on deck");
  expect(labels.View).toBe("translated:View");
  expect(labels.Help).toBe("translated:Help");
  expect(labels["Report bug…"]).toBe("translated:Report bug…");
  expect(Object.keys(labels)).toHaveLength(36);
});
