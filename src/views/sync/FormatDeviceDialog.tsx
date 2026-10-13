import { useEffect, useRef, useState } from "react";

import { useTranslation } from "@/i18n";
import type { Device, FormatLayout } from "@/ipc/types";
import { errorMessage } from "@/lib/errorMessage";
import styles from "./FormatDeviceDialog.module.css";

/**
 * The layout that keeps the stick's current filesystem: HFS+ for a stick that
 * is HFS+ now, under any of the names the OS reports for it, otherwise FAT32.
 * exFAT and anything else start on FAT32, as neither is offered here.
 */
export function currentLayout(fileSystem: string | undefined): FormatLayout {
  const name = (fileSystem ?? "").toUpperCase().replace(/[^A-Z0-9+]/g, "");
  return name === "HFS" || name === "HFS+" || name === "HFSPLUS" || name.startsWith("MACOSEXTENDED") ? "hfsPlus" : "fat32";
}

/**
 * Asks how a stick should be formatted, and says plainly that all of it is
 * erased: every partition, not only the volume that shows up in the list.
 */
export function FormatDeviceDialog({ device, onFormat, onClose }: {
  device: Device;
  /** Erases and formats the stick; rejects with what went wrong. */
  onFormat: (layout: FormatLayout) => Promise<void>;
  onClose: () => void;
}) {
  const t = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [layout, setLayout] = useState<FormatLayout>(() => currentLayout(device.fileSystem));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  const format = () => {
    setBusy(true);
    setError("");
    onFormat(layout).then(onClose, (e: unknown) => {
      setBusy(false);
      setError(errorMessage(e));
    });
  };

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="format-device-title"
      aria-describedby="format-device-warning"
      onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => { event.preventDefault(); if (!busy) format(); }}>
        <h2 id="format-device-title" className={styles.title}>{t("Format USB")}</h2>
        <p className={styles.device} title={device.path}>{device.name}</p>
        <fieldset className={styles.options} disabled={busy}>
          <legend className={styles.legend}>{t("Format")}</legend>
          <label className={styles.option}>
            <input type="radio" name="format-layout" checked={layout === "fat32"} onChange={() => setLayout("fat32")} />
            <span>
              <strong>{t("FAT32")}</strong>
              <span className={styles.hint}>{t("Compatible with CDJ, Windows, Mac.")}</span>
            </span>
          </label>
          <label className={styles.option}>
            <input type="radio" name="format-layout" checked={layout === "hfsPlus"} onChange={() => setLayout("hfsPlus")} />
            <span>
              <strong>{t("HFS+")}</strong>
              <span className={styles.hint}>{t("Compatible with CDJ, Mac.")}</span>
            </span>
          </label>
        </fieldset>
        <p id="format-device-warning" className={styles.warning} role="alert">
          {t("ALL DATA WILL BE ERASED")}
        </p>
        {error ? <p className={styles.error} role="alert">{error}</p> : null}
        <div className={styles.buttons}>
          <button type="button" onClick={onClose} disabled={busy}>{t("Cancel")}</button>
          <button type="submit" className={styles.danger} disabled={busy}>
            {busy ? t("Formatting…") : t("Erase and Format")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
