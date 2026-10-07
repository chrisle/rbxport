import { errorMessage } from "@/lib/errorMessage";
import { useEffect, useRef, useState } from "react";
import { getBackend } from "@/ipc/client";
import type { Backend, Backup } from "@/ipc/types";
import { formatBytes } from "@/lib/format";
import { useTranslation } from "@/i18n";
import { useBackupProgress } from "@/store/useBackupProgress";
import { Button, Section } from "./controls";
import styles from "./BackupsPane.module.css";
import { BackupSizeChart } from "./BackupSizeChart";
import layout from "./PaneLayout.module.css";

export function BackupsPane() {
  const t = useTranslation();
  const job = useBackupProgress();
  const [backups, setBackups] = useState<Backup[]>([]);
  const [directory, setDirectory] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const running = useRef(false);
  useEffect(() => {
    let live = true;
    void getBackend().then(b => Promise.all([b.listBackups(), b.backupDirectory()])).then(([entries, path]) => {
      if (live) { setBackups(entries); setDirectory(path); }
    }).catch(e => { if (live) setError(errorMessage(e)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, []);
  useEffect(() => {
    if (!job.progress.path) return;
    let live = true;
    void getBackend().then(b => b.listBackups()).then(entries => {
      if (live) setBackups(entries);
    }).catch(e => { if (live) setError(errorMessage(e)); });
    return () => { live = false; };
  }, [job.progress.path]);
  const run = async (label: string, action: (backend: Backend) => Promise<string>) => {
    if (running.current) return;
    running.current = true;
    setBusy(label); setError(""); setMessage("");
    try {
      const backend = await getBackend();
      const message = await action(backend);
      setMessage(message);
      setBackups(await backend.listBackups());
    } catch (e) { setError(errorMessage(e)); }
    finally { running.current = false; setBusy(""); }
  };
  const unavailable = loading || busy !== "" || job.progress.running;
  const copying = job.progress.phase === "copying" && job.progress.totalBytes > 0;
  const percent = Math.min(100, Math.max(0, Math.floor(job.progress.copiedBytes / (job.progress.totalBytes || 1) * 100)));
  const stopping = job.progress.phase === "stopping";
  return <Section title={t("Backups")}>
    {error || job.error ? <p role="alert" className={styles.error}>{error || job.error}</p> : null}

    <BackupSizeChart />
    <section className={`${layout.summary} ${job.progress.running ? styles.activeBackup : ""}`} aria-label={t("Backup your Library")}>
      {job.progress.running ? <div className={styles.backupProgress}>
        <div className={styles.progressHeading} role="status" aria-live="polite">
          <strong>{job.progress.phase === "copying" ? t("Backing up your library") : job.text}</strong>
          {copying ? <span className={styles.percent}>{percent}%</span> : null}
        </div>
        <progress className={styles.progressBar} aria-label={t("Backup progress")} max={100}
          value={copying ? percent : undefined} />
        <div className={styles.progressDetails}>
          {copying ? t("{copied} of {total}", { copied: formatBytes(job.progress.copiedBytes), total: formatBytes(job.progress.totalBytes) })
            : stopping ? t("Removing the unfinished backup…")
              : job.progress.phase === "compressing" ? t("Finishing your compressed ZIP backup.")
              : job.progress.phase === "validating" ? t("Checking the saved files before finishing.")
                : t("Getting your library files ready.")}
        </div>
        {!stopping && job.progress.currentItem ? <div className={styles.currentItem} title={job.progress.currentItem} aria-label={t("Current backup item")}>
          {job.progress.currentItem}
        </div> : null}
        <div className={styles.progressFooter}>
          <p>{t("You can keep using RBXport while this runs.")}</p>
          <Button className={styles.backupButton} disabled={stopping} onClick={() => void job.stop()}>{stopping ? t("Stopping…") : t("Stop backup")}</Button>
        </div>
      </div> : <>
      <div>
        <strong>{t("Backup your Library")}</strong>
        <p className={layout.help}>{t("Save your library in a compressed ZIP. Music files are not backed up.")}</p>
        <p className={styles.status} role="status" aria-live="polite">{busy || message || (job.error ? "" : job.text)}</p>
      </div>
      <Button className={styles.backupButton} disabled={busy !== "" || job.progress.running} onClick={() => {
        setError(""); setMessage(""); void job.start();
      }}>{t("Create backup")}</Button>
      </>}
    </section>
    <section className={styles.destination} aria-label={t("Default backup folder")}>
      <div>
        <strong>{t("Default backup folder")}</strong>
        {directory ? <button type="button" role="link" className={styles.directoryLink} onClick={() => {
          setError("");
          void getBackend().then(b => b.openBackupDirectory()).catch(e => setError(errorMessage(e)));
        }}>{directory}</button> : <p className={layout.help}>{t("Loading folder…")}</p>}
        <p className={layout.help}>{t("New backups are saved here. Existing backups stay in their current folder.")}</p>
      </div>
      <Button className={styles.backupButton} disabled={unavailable} onClick={() => void run(t("Choosing a backup folder…"), async b => {
        const destination = await b.pickFolder(t("Choose default backup folder"));
        if (!destination) return "";
        setBusy(t("Saving backup folder…"));
        setDirectory(await b.setBackupDirectory(destination));
        return t("Default backup folder updated.");
      })}>{t("Change folder…")}</Button>
    </section>
    <h4 className={`${styles.sectionHeading} ${styles.savedHeading}`}>{t("Saved backups")}</h4>
    {loading ? <p className={styles.status} role="status">{t("Loading backups…")}</p> : null}
    {!loading && backups.length === 0 ? <div className={layout.empty}>
      <strong>{error ? t("Backups unavailable") : t("No backups yet.")}</strong>
      <p>{error ? t("Reopen this page to try again.") : t("Create a backup to see it here.")}</p>
    </div> : null}
    {backups.length > 0 ? <div className={styles.tableScroll}><table className={styles.table} aria-label={t("Library backups")}>
      <thead><tr><th>{t("Date")}</th><th>{t("Time")}</th><th>{t("Size")}</th><th>{t("Actions")}</th></tr></thead>
      <tbody>{backups.map(backup => {
        const created = new Date(backup.createdAt);
        const date = created.toLocaleString();
        return <tr key={backup.path}>
          <td><time dateTime={created.toISOString()}>{created.toLocaleDateString()}</time></td>
          <td><time dateTime={created.toISOString()}>{created.toLocaleTimeString()}</time></td>
          <td className={styles.size}>{formatBytes(backup.bytes)}</td>
          <td><div className={styles.actions}>
            <Button className={`${styles.backupButton} ${styles.deleteButton}`} disabled={unavailable} onClick={() => void run(t("Deleting backup…"), async b => {
              if (!await b.confirm(t("Delete the backup from {date}? This cannot be undone.", { date }))) return "";
              await b.deleteBackup(backup.path); return t("Backup deleted.");
            })}>{t("Delete")}</Button>
          </div></td>
        </tr>;
      })}</tbody>
    </table></div> : null}
    <section className={`${styles.destination} ${styles.restoreFile}`} aria-label={t("Restore a backup")}>
      <div>
        <h4 className={styles.sectionHeading}>{t("Restore a backup")}</h4>
        <p className={layout.help}>{t("To restore, quit RBXport and open RBXport Restore. It shows what each backup holds and lets you restore all of it or only some parts.")}</p>
      </div>
    </section>
  </Section>;
}
