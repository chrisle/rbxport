import { useEffect, useRef } from "react";
import type { ImportProgress, XmlImportPreview } from "@/ipc/types";
import styles from "./XmlImportDialog.module.css";

export function XmlImportDialog({ path, preview, includeMissing, progress, onIncludeMissing, onImport, onCancel }: {
  path: string;
  preview: XmlImportPreview;
  includeMissing: boolean;
  progress: ImportProgress | null;
  onIncludeMissing: (value: boolean) => void;
  onImport: () => void;
  onCancel: () => void;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const running = progress !== null;
  useEffect(() => { panel.current?.focus(); }, []);
  const total = progress?.total ?? preview.tracks;
  const done = progress?.done ?? 0;
  const percent = total === 0 ? 100 : Math.min(100, (done / total) * 100);
  return <div className={styles.backdrop} role="presentation" onMouseDown={running ? undefined : onCancel}>
    <div ref={panel} className={styles.dialog} role="dialog" aria-modal="true" aria-label="Import rekordbox XML" tabIndex={-1} onMouseDown={e => e.stopPropagation()}>
      <header>Import rekordbox XML</header>
      <main>
        <p className={styles.path}>{path.split(/[\\/]/).at(-1)}</p>
        <dl className={styles.stats}>
          <div><dt>Tracks to import</dt><dd>{preview.available.toLocaleString()}</dd></div>
          <div><dt>Missing files</dt><dd>{preview.missing.toLocaleString()}</dd></div>
          <div><dt>Invalid locations</dt><dd>{preview.invalid.toLocaleString()}</dd></div>
          <div><dt>Playlists</dt><dd>{preview.playlists.toLocaleString()}</dd></div>
        </dl>
        {preview.missing > 0 ? <label className={styles.check}><input type="checkbox" checked={includeMissing} disabled={running} onChange={e => onIncludeMissing(e.target.checked)} /> Import missing files anyway</label> : null}
        {running ? <section className={styles.progress} aria-live="polite">
          <div className={styles.bar}><i style={{ width: `${percent}%` }} /></div>
          <p>Importing {done.toLocaleString()} of {total.toLocaleString()} tracks</p>
          <div className={styles.counters}><span>Imported <b>{progress.imported}</b></span><span>Missing <b>{progress.missing}</b></span><span>Error <b>{progress.errors}</b></span></div>
        </section> : null}
      </main>
      <footer>
        {!running ? <button type="button" onClick={onCancel}>Cancel</button> : null}
        <button type="button" className={styles.primary} disabled={running} onClick={onImport}>{running ? "Importing…" : "Import"}</button>
      </footer>
    </div>
  </div>;
}
