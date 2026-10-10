import { describe, expect, it } from "vitest";
import { JobQueue, slices } from "./jobQueue";

const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe("JobQueue", () => {
  it("runs jobs one at a time, in the order they were added", async () => {
    const queue = new JobQueue();
    const log: string[] = [];
    let release: () => void = () => undefined;
    queue.add({ label: "a", run: async () => { log.push("a start"); await new Promise<void>((r) => { release = r; }); log.push("a end"); } });
    queue.add({ label: "b", run: () => { log.push("b"); return Promise.resolve(); } });
    await settle();
    expect(log).toEqual(["a start"]);
    expect(queue.getSnapshot().map((j) => [j.label, j.state])).toEqual([["a", "running"], ["b", "queued"]]);
    release();
    await settle();
    expect(log).toEqual(["a start", "a end", "b"]);
    expect(queue.getSnapshot()).toEqual([]);
  });

  it("stops a running job through its signal and drops a waiting one outright", async () => {
    const queue = new JobQueue();
    let sliceCount = 0;
    const first = queue.add({
      label: "a",
      run: async ({ signal }) => {
        while (!signal.aborted) { sliceCount += 1; await settle(); }
      },
    });
    let ran = false;
    const second = queue.add({ label: "b", run: () => { ran = true; return Promise.resolve(); } });
    await settle();
    queue.stop(second);
    expect(queue.getSnapshot().map((j) => j.label)).toEqual(["a"]);
    queue.stop(first);
    expect(queue.getSnapshot()[0]?.state).toBe("stopping");
    await settle();
    await settle();
    expect(sliceCount).toBeGreaterThan(0);
    expect(ran).toBe(false);
    expect(queue.getSnapshot()).toEqual([]);
  });

  it("reports progress and carries on after a job throws", async () => {
    const errors: string[] = [];
    const queue = new JobQueue((job) => errors.push(job.label));
    let release: () => void = () => undefined;
    queue.add({
      label: "a", total: 4,
      run: async ({ update }) => {
        update({ done: 2, label: "half" });
        await new Promise<void>((r) => { release = r; });
        throw new Error("no");
      },
    });
    let ran = false;
    queue.add({ label: "b", run: () => { ran = true; return Promise.resolve(); } });
    await settle();
    expect(queue.getSnapshot()[0]).toMatchObject({ label: "half", done: 2, total: 4 });
    release();
    await settle();
    expect(errors).toEqual(["half"]);
    expect(ran).toBe(true);
  });
});

describe("JobQueue.perform", () => {
  it("waits its turn, answers with the work's result, and cannot be stopped (#286)", async () => {
    const queue = new JobQueue();
    let release: () => void = () => undefined;
    queue.add({ label: "a", run: () => new Promise<void>((r) => { release = r; }) });
    let calls = 0;
    let answer: number | undefined;
    void queue.perform({ label: "Searching...", holdsAnalysis: true }, () => { calls += 1; return Promise.resolve(42); })
      .then((value) => { answer = value; });
    await settle();
    expect(queue.getSnapshot().map((j) => [j.label, j.state, j.stoppable, j.holdsAnalysis]))
      .toEqual([["a", "running", true, false], ["Searching...", "queued", false, true]]);
    const searching = queue.getSnapshot()[1]!.id;
    queue.stop(searching);
    expect(queue.getSnapshot()).toHaveLength(2);
    expect(calls).toBe(0);
    release();
    await settle();
    await settle();
    expect(calls).toBe(1);
    expect(answer).toBe(42);
    expect(queue.getSnapshot()).toEqual([]);
  });

  it("hands an error to the caller rather than to onError", async () => {
    const errors: string[] = [];
    const queue = new JobQueue((job) => errors.push(job.label));
    const failed = queue.perform({ label: "Searching..." }, () => Promise.reject(new Error("no folder")));
    await expect(failed).rejects.toThrow("no folder");
    await settle();
    expect(errors).toEqual([]);
    expect(queue.getSnapshot()).toEqual([]);
  });
});

describe("slices", () => {
  it("cuts a list into runs of at most the size", () => {
    expect(slices([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]]);
    expect(slices([], 3)).toEqual([]);
  });
});
