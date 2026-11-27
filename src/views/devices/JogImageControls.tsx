import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { getBackend } from "@/ipc/client";
import type { Device, JogImageAction, JogImageState } from "@/ipc/types";
import { errorMessage } from "@/lib/errorMessage";
import styles from "./DevicePanel.module.css";

interface Props {
  device?: Device;
  busy?: boolean;
  onError?: ((message: string) => void) | undefined;
}

/** Each explicit action belongs to the committed pane/medium that admitted it. */
export function JogImageControls({ device, busy = false, onError }: Props) {
  const t = useTranslation();
  const path = device?.path ?? null;
  const volumeId = device?.volumeId ?? null;
  const stamp = device?.export?.written ?? "";
  const owner = useMemo(() => ({ path, volumeId, stamp, version: 0, busy: false }), [path, volumeId, stamp]);
  const active = useRef<typeof owner | null>(null);
  const reportError = useRef(onError);
  useLayoutEffect(() => { reportError.current = onError; }, [onError]);
  const [error, setError] = useState<{ owner: typeof owner; message: string } | null>(null);
  useLayoutEffect(() => { active.current = owner; return () => { if (active.current === owner) active.current = null; }; }, [owner]);
  const [read, setRead] = useState<{ owner: typeof owner; state: JogImageState } | null>(null);
  const [working, setWorking] = useState<typeof owner | null>(null);
  const state = read?.owner === owner ? read.state : null;
  useEffect(() => {
    const version = ++owner.version;
    void getBackend().then(backend => backend.jogImage(path, volumeId, "read")).then(state => {
      if (active.current === owner && version === owner.version) setRead({ owner, state });
    }).catch((error: unknown) => {
      if (active.current === owner && version === owner.version) {
        const message = errorMessage(error); setError({ owner, message }); reportError.current?.(message);
      }
    });
  }, [owner, path, volumeId]);

  const change = async (action: Exclude<JogImageAction, "read">) => {
    if (active.current !== owner || state === null || owner.busy || busy) return;
    const version = ++owner.version;
    setError(null);
    owner.busy = true;
    setWorking(owner);
    try {
      const backend = await getBackend();
      const source = action === "import" ? await backend.pickImage(t("Import image file")) : undefined;
      if (active.current !== owner || source === null) return;
      const saved = await backend.jogImage(path, volumeId, action, source);
      if (active.current !== owner || version !== owner.version) return;
      setRead({ owner, state: saved });
    } catch (error: unknown) {
      if (active.current === owner && version === owner.version) {
        const message = errorMessage(error); setError({ owner, message }); reportError.current?.(message);
      }
    } finally {
      owner.busy = false;
      if (active.current === owner && version === owner.version) setWorking(null);
    }
  };
  const selected = state?.mode;
  return (
    <fieldset className={styles.group} disabled={state === null || working === owner || busy}>
      <legend className={styles.caption}>{t("Image On-Jog Display")}</legend>
      {device && <div className={styles.radios}>
        {([ ["artwork", "ARTWORK"], ["custom", "CUSTOMIMAGE"] ] as const).map(([value, label]) => (
          <label key={value} className={styles.radio}>
            <input type="radio" name={device ? "device-jog-image" : "preference-jog-image"} checked={selected === value} onChange={() => { void change(value); }} />
            {t(label)}
          </label>
        ))}
      </div>}
      {error?.owner === owner && <p role="alert">{error.message}</p>}
      {state?.imageError && <p role="status">{state.imageError}</p>}
      <div className={styles.jog}>
        <div className={styles.disc}>
          {state?.image ? <img className={styles.jogPreview} src={state.image} alt={t("CUSTOMIMAGE")} /> : <span className={styles.discText}>{t("No image")}</span>}
        </div>
        <div className={styles.imageActions}>
          {device && <button type="button" className={styles.imageButton} onClick={() => { void change("copy"); }}>{t("Copy from [Preferences]")}</button>}
          <button type="button" className={styles.imageButton} onClick={() => { void change("import"); }}>{t("Import image file")}</button>
          <button type="button" className={styles.imageButton} disabled={!state?.image && !state?.imagePresent} onClick={() => { void change("delete"); }}>{t("Delete image")}</button>
        </div>
      </div>
    </fieldset>
  );
}
