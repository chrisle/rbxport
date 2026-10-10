/**
 * LRU page cache for track rows.
 *
 * Rust owns the ordering; the frontend only ever holds a bounded window. This
 * lives outside React state on purpose — a page arriving must not re-render the
 * whole table, only the rows whose slots it fills.
 */
export const PAGE_SIZE = 64;
const MAX_PAGES = 100; // ~6400 rows, well under the memory budget

/**
 * Identity of the data a page belongs to. It must combine the view AND the
 * library generation: `gen` alone is shared across views, so pages fetched for
 * a previous view would look valid for the next one and the table would lag a
 * sort behind.
 */
export type CacheToken = string;

export interface CachedPage<T> {
  token: CacheToken;
  rows: readonly T[];
}

export class RowCache<T> {
  readonly pageSize: number;
  #pages = new Map<number, CachedPage<T>>();
  #maxPages: number;

  constructor(pageSize = PAGE_SIZE, maxPages = MAX_PAGES) {
    this.pageSize = pageSize;
    this.#maxPages = maxPages;
  }

  static pageOf(index: number, pageSize = PAGE_SIZE): number {
    return Math.floor(index / pageSize);
  }

  get(index: number, token: CacheToken): T | undefined {
    const page = Math.floor(index / this.pageSize);
    const entry = this.#pages.get(page);
    if (!entry || entry.token !== token) return undefined;
    // Touch for LRU ordering (Map preserves insertion order).
    this.#pages.delete(page);
    this.#pages.set(page, entry);
    return entry.rows[index % this.pageSize];
  }

  /**
   * The row at an index under whatever token its page was fetched with.
   *
   * A stale picture, for a view that is being fetched again after an edit
   * or a re-sort: the page it had is drawn until the replacement lands,
   * rather than every row going blank in between. `missingPages` still
   * counts such a page as missing, so it is fetched again as soon as it is
   * on screen, and `get` is the read for a caller that must not see it.
   */
  peek(index: number): T | undefined {
    const page = Math.floor(index / this.pageSize);
    const entry = this.#pages.get(page);
    if (!entry) return undefined;
    this.#pages.delete(page);
    this.#pages.set(page, entry);
    return entry.rows[index % this.pageSize];
  }

  setPage(page: number, token: CacheToken, rows: readonly T[]): void {
    this.#pages.delete(page);
    this.#pages.set(page, { token, rows });
    while (this.#pages.size > this.#maxPages) {
      const oldest = this.#pages.keys().next();
      if (oldest.done) break;
      this.#pages.delete(oldest.value);
    }
  }

  hasPage(page: number, token: CacheToken): boolean {
    return this.#pages.get(page)?.token === token;
  }

  /** Pages covering [start, end) that are missing or stale. */
  missingPages(start: number, end: number, token: CacheToken): number[] {
    const first = Math.floor(Math.max(0, start) / this.pageSize);
    const last = Math.floor(Math.max(0, end - 1) / this.pageSize);
    const out: number[] = [];
    for (let p = first; p <= last; p++) if (!this.hasPage(p, token)) out.push(p);
    return out;
  }

  /** Whether any cached row satisfies a predicate. The same scan as `patch`. */
  holds(pick: (row: T) => boolean): boolean {
    for (const entry of this.#pages.values()) {
      if (entry.rows.some(pick)) return true;
    }
    return false;
  }

  /**
   * Rewrites the cached rows a predicate picks, in place, and says whether
   * any were. For a change the backend made to one row without a reload —
   * a cue edit — where dropping every page to pick up one field would
   * blank the table. A scan of at most `maxPages` pages, once per edit.
   */
  patch(pick: (row: T) => boolean, change: (row: T) => T): boolean {
    let touched = false;
    for (const [page, entry] of this.#pages) {
      let rows: T[] | null = null;
      entry.rows.forEach((row, i) => {
        if (!pick(row)) return;
        rows ??= [...entry.rows];
        rows[i] = change(row);
      });
      if (rows) {
        this.#pages.set(page, { token: entry.token, rows });
        touched = true;
      }
    }
    return touched;
  }

  clear(): void {
    this.#pages.clear();
  }

  get size(): number {
    return this.#pages.size;
  }
}
