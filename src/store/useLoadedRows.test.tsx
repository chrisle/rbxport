/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { useLoadedRows } from "./useLoadedRows";
const host = document.createElement("div");
let root: ReturnType<typeof createRoot>;
let marked: ReadonlySet<string>;
const pending: { rows: readonly string[]; done: (ids: string[]) => void }[] = [];
function Probe({ rows, owner }: { rows: readonly string[]; owner: string }) {
  marked = useLoadedRows(rows, [[owner, owner === "old" ? 1 : 2]], owner); return null;
}
afterEach(() => { act(() => root.unmount()); __setBackend(null); pending.length = 0; });
it("chunks a dense viewport and hides answers from replaced owners", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  root = createRoot(host);
  __setBackend({ ...createMockBackend(), loadedRows: (rows) => new Promise<string[]>(done => { pending.push({ rows, done }); }) });
  const rows = Array.from({ length: 300 }, (_, at) => String(at));
  await act(async () => { await Promise.resolve(); root.render(<Probe rows={rows} owner="old" />); });
  expect(pending.map(call => call.rows.length)).toEqual([128, 128, 44]);
  await act(async () => { await Promise.resolve(); root.render(<Probe rows={rows} owner="new" />); });
  await act(async () => { await Promise.resolve(); for (const call of pending.slice(0, 3)) call.done([call.rows[0]!]); });
  expect([...marked]).toEqual([]);
  await act(async () => { await Promise.resolve(); for (const call of pending.slice(3)) call.done([call.rows.at(-1)!]); });
  expect([...marked]).toEqual(["127", "255", "299"]);
  await act(async () => { await Promise.resolve(); root.render(<Probe rows={[]} owner="new" />); });
  expect([...marked]).toEqual([]);
  await act(async () => { await Promise.resolve(); root.render(<Probe rows={rows} owner="new" />); });
  expect([...marked]).toEqual([]); // An A→empty→A transition cannot reuse the old answer.
  await act(async () => { await Promise.resolve(); for (const call of pending.slice(6)) call.done([call.rows[0]!]); });
  expect([...marked]).toEqual(["0", "128", "256"]);
  await act(async () => { await Promise.resolve(); root.render(<Probe rows={["different"]} owner="new" />); });
  expect([...marked]).toEqual([]);
});
