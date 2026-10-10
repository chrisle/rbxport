/**
 * Long library operations (adding hundreds of tracks to a playlist, removing
 * them from the collection) run one after another through this queue, so the
 * status bar can show what is running and what is waiting, and stop either.
 *
 * Kept apart from React so the ordering and stopping are testable without a
 * component. A job does its work in slices and calls `update` between them;
 * it stops early by checking `signal`.
 */

export interface JobView {
  id: number;
  label: string;
  /** Slices of work finished, out of `total`; `total` 0 means unknown. */
  done: number;
  total: number;
  state: "queued" | "running" | "stopping";
  /** The playlist the job is filling, so its list can show rows on the way. */
  target: string | null;
  /** Rows still to arrive in `target`. */
  pendingRows: number;
  /** Whether Stop can end it; false for one backend call that cannot be cut short. */
  stoppable: boolean;
  /**
   * Whether analysis waits for it: a job that moves tracks' files, so a track
   * analysed meanwhile would be read from where it no longer is.
   */
  holdsAnalysis: boolean;
}

export interface JobUpdate {
  label?: string;
  done?: number;
  total?: number;
  pendingRows?: number;
}

export interface JobContext {
  signal: AbortSignal;
  update: (change: JobUpdate) => void;
}

export interface JobSpec {
  label: string;
  total?: number;
  target?: string;
  pendingRows?: number;
  /** Defaults to true. */
  stoppable?: boolean;
  /** Defaults to false. */
  holdsAnalysis?: boolean;
  run: (context: JobContext) => Promise<void>;
}

interface Entry {
  view: JobView;
  run: JobSpec["run"];
  abort: AbortController;
}

export class JobQueue {
  private entries: Entry[] = [];
  private snapshot: readonly JobView[] = [];
  private listeners = new Set<() => void>();
  private next = 1;
  private working = false;

  /** `onError` hears a job that threw; the queue goes on to the next one. */
  constructor(private readonly onError: (job: JobView, error: unknown) => void = () => undefined) {}

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  getSnapshot = (): readonly JobView[] => this.snapshot;

  add(spec: JobSpec): number {
    const id = this.next++;
    this.entries.push({
      view: {
        id, label: spec.label, done: 0, total: spec.total ?? 0, state: "queued",
        target: spec.target ?? null, pendingRows: spec.pendingRows ?? 0,
        stoppable: spec.stoppable ?? true, holdsAnalysis: spec.holdsAnalysis ?? false,
      },
      run: spec.run,
      abort: new AbortController(),
    });
    this.publish();
    void this.drain();
    return id;
  }

  /**
   * Waits its turn in the queue, then runs `work` as a job that cannot be
   * stopped, and settles as `work` does: for one backend call the caller
   * needs the answer of. An error goes to the caller, not to `onError`.
   */
  perform<T>(spec: Omit<JobSpec, "run" | "stoppable">, work: (context: JobContext) => Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      this.add({
        ...spec,
        stoppable: false,
        run: async (context) => {
          try {
            resolve(await work(context));
          } catch (error) {
            reject(error instanceof Error ? error : new Error(String(error)));
          }
        },
      });
    });
  }

  /**
   * Stops a running job after its current slice, or drops a waiting one. A
   * job that cannot be stopped is left alone, waiting or running.
   */
  stop(id: number): void {
    const entry = this.entries.find((e) => e.view.id === id);
    if (!entry || !entry.view.stoppable) return;
    if (entry.view.state === "queued") {
      this.entries = this.entries.filter((e) => e !== entry);
    } else {
      entry.view = { ...entry.view, state: "stopping" };
      entry.abort.abort();
    }
    this.publish();
  }

  private publish(): void {
    this.snapshot = this.entries.map((e) => e.view);
    this.listeners.forEach((listener) => listener());
  }

  private async drain(): Promise<void> {
    if (this.working) return;
    this.working = true;
    try {
      for (;;) {
        const entry = this.entries.find((e) => e.view.state === "queued");
        if (!entry) return;
        entry.view = { ...entry.view, state: "running" };
        this.publish();
        const context: JobContext = {
          signal: entry.abort.signal,
          update: (change) => {
            entry.view = { ...entry.view, ...change };
            this.publish();
          },
        };
        try {
          await entry.run(context);
        } catch (error) {
          this.onError(entry.view, error);
        }
        this.entries = this.entries.filter((e) => e !== entry);
        this.publish();
      }
    } finally {
      this.working = false;
    }
  }
}

/** `items` in slices of `size`, so a job can report between them. */
export function slices<T>(items: readonly T[], size: number): T[][] {
  const out: T[][] = [];
  for (let at = 0; at < items.length; at += size) out.push(items.slice(at, at + size));
  return out;
}
