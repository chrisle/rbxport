import { expect, it, vi } from "vitest";
import { askToReplaceLists } from "./xmlImport";

it("asks rekordbox's OK/Cancel question under the title Import", async () => {
  const confirm = vi.fn(() => Promise.resolve(false));
  const t = (text: string) => `[${text}]`;
  await expect(askToReplaceLists({ confirm }, t)).resolves.toBe(false);
  expect(confirm).toHaveBeenCalledWith(
    "[One or several lists with the same name already exist.]\n[Do you want to replace them with the one you're importing?]",
    { yes: "[OK]", no: "[Cancel]", title: "[Import]" },
  );
});
