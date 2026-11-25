import { useEffect, useRef, useState } from "react";

import { useTranslation } from "@/i18n";
import type { MissingExportFile } from "@/ipc/types";
import styles from "./MissingFilesDialog.module.css";

/**
 * Names the selected tracks whose audio is not on disk before a sync starts,
 * and asks whether to sync the rest without them.
 */
export function MissingFilesDialog({ files, onChoose, onRepair }: {
  files: readonly MissingExportFile[];
  /** True to sync without the missing tracks, false to stop. */
  onChoose: (proceed: boolean) => void;
  onRepair: () => Promise<void>;
}) {
  const t = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const live = useRef(false);
  const choosing = useRef(onChoose);
  choosing.current = onChoose;
  const held = useRef(false);
  const [opening, setOpening] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    live.current = true;
    const element = dialog.current;
    element?.showModal();
    return () => {
      live.current = false;
      element?.close();
      // StrictMode remounts effects immediately; only a real departure stops
      // this preflight. It must never resume when a late handoff completes.
      queueMicrotask(() => { if (!live.current) choosing.current(false); });
    };
  }, []);
  const repair = async () => {
    if (held.current) return;
    held.current = true;
    setOpening(true); setFailed(false);
    try { await onRepair(); }
    catch { if (live.current) setFailed(true); }
    finally { if (live.current) { held.current = false; setOpening(false); } }
  };

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="missing-files-title"
      aria-describedby="missing-files-summary"
      onCancel={event => { event.preventDefault(); if (!held.current) onChoose(false); }}
      onKeyDown={event => event.stopPropagation()}>
      <h2 id="missing-files-title" className={styles.title}>{t("Missing audio files")}</h2>
      <p id="missing-files-summary" className={styles.summary}>
        {files.length === 1
          ? t("1 selected track has a missing audio file.")
          : t("{count} selected tracks have missing audio files.", { count: files.length })}
        {" "}{t("Usable copies already on each USB will be kept. Tracks without a usable copy will be skipped.")}
      </p>
      <ul className={styles.list} aria-label={t("Missing audio files")}>
        {files.map(file => <li key={file.id} data-track-id={file.id}>
          <strong data-i18n-ignore>{file.title}</strong>
          <span className={styles.path} data-i18n-ignore>{file.path}</span>
          <ul aria-label={t("Playlists")}>
            {file.playlists.map(playlist => <li key={playlist.id} data-playlist-id={playlist.id} data-i18n-ignore>{playlist.path}</li>)}
          </ul>
        </li>)}
      </ul>
      <p className={styles.summary}>{t("Display All Missing Files opens all missing collection files in the main window and stops this sync.")}</p>
      {failed ? <p role="alert">{t("Could not open Display All Missing Files. Try again, or use File in the main window.")}</p> : null}
      <div className={styles.buttons}>
        <button type="button" autoFocus disabled={opening} onClick={() => { if (!held.current) onChoose(false); }}>{t("Cancel")}</button>
        <button type="button" disabled={opening} onClick={() => void repair()}>{t("Display All Missing Files")}</button>
        <button type="button" disabled={opening} className={styles.primary} onClick={() => { if (!held.current) onChoose(true); }}>{t("Keep available copies and sync")}</button>
      </div>
    </dialog>
  );
}
