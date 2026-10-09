/**
 * About: what this is, which version, who made it, and under what terms.
 * Not a pane rekordbox has — its version is under the application menu.
 *
 * A check runs here rather than asking the main window for one, because
 * this pane may be a window of its own with no main window's state to read.
 * Its result, and any download's progress — this pane's own or one an
 * automatic check is running silently elsewhere — are shown in place.
 */
import { useEffect, useState, type SVGProps } from "react";
import { Github, Globe, Heart, Instagram, Twitch } from "lucide-react";

import { getBackend } from "@/ipc/client";
import type { UpdateProgress } from "@/ipc/types";
import { formatBytes } from "@/lib/changelog";
import { usePreferencesContext } from "@/store/usePreferences";
import layout from "./PaneLayout.module.css";
import styles from "./Preferences.module.css";
import { Button, Select, Toggle } from "./controls";
import { useTranslation } from "@/i18n";

/** Where a check this pane made itself has got to. */
type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate"; version: string }
  | { kind: "store" }
  | { kind: "available"; version: string }
  | { kind: "ready"; version: string };

// Discord brand mark from Simple Icons (CC0).
function DiscordIcon({ size = 19, ...props }: SVGProps<SVGSVGElement> & { size?: number }) {
  return <svg {...props} width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
    <path d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z" />
  </svg>;
}

// TikTok brand mark from Simple Icons (CC0).
function TikTokIcon({ size = 19, ...props }: SVGProps<SVGSVGElement> & { size?: number }) {
  return <svg {...props} width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
    <path d="M12.525.02c1.31-.02 2.61-.01 3.91-.02.08 1.53.63 3.09 1.75 4.17 1.12 1.11 2.7 1.62 4.24 1.79v4.03c-1.44-.05-2.89-.35-4.2-.97-.57-.26-1.1-.59-1.62-.93-.01 2.92.01 5.84-.02 8.75-.08 1.4-.54 2.79-1.35 3.94-1.31 1.92-3.58 3.17-5.91 3.21-1.43.08-2.86-.31-4.08-1.03-2.02-1.19-3.44-3.37-3.65-5.71-.02-.5-.03-1-.01-1.49.18-1.9 1.12-3.72 2.58-4.96 1.66-1.44 3.98-2.13 6.15-1.72.02 1.48-.04 2.96-.04 4.44-.99-.32-2.15-.23-3.02.37-.63.41-1.11 1.04-1.36 1.75-.21.51-.15 1.07-.14 1.61.24 1.64 1.82 3.02 3.5 2.87 1.12-.01 2.19-.66 2.77-1.61.19-.33.4-.67.41-1.06.1-1.79.06-3.57.07-5.36.01-4.03-.01-8.05.02-12.07z" />
  </svg>;
}

/** Where the links go. */
export const ABOUT_LINKS = [
  { label: "Instagram", Icon: Instagram, url: "https://instagram.com/triodeofficial" },
  { label: "TikTok", Icon: TikTokIcon, url: "https://www.tiktok.com/@triodeofficial" },
  { label: "Twitch", Icon: Twitch, url: "https://twitch.tv/triodeofficial" },
  { label: "Discord", Icon: DiscordIcon, url: "https://discord.gg/vUkcYZhwuR" },
  { label: "GitHub", Icon: Github, url: "https://github.com/chrisle" },
  { label: "Web", Icon: Globe, url: "https://triodeofficial.com" },
];

const SUPPORT_URL = "https://www.paypal.com/donate/?hosted_button_id=H6GGU8PHP8CJE";

export function AboutPane() {
  const t = useTranslation();
  const { preferences, update } = usePreferencesContext();
  const { checkUpdates, updateFrequency } = preferences.advanced;
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ kind: "idle" });
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  // A download's progress, whoever started it: an automatic check running
  // silently in the main window counts just as much as one from here.
  const [progress, setProgress] = useState<UpdateProgress | null>(null);

  useEffect(() => {
    let live = true;
    // The automatic updater may have finished before About was opened. Its
    // completed download is held by the backend, including in a separate
    // Preferences window, so show it without another network check.
    void getBackend()
      .then((backend) => backend.readyUpdate())
      .then((ready) => {
        if (live && ready) setUpdateStatus((current) => current.kind === "idle"
          ? { kind: "ready", version: ready.version }
          : current);
      })
      .catch(() => {});
    return () => { live = false; };
  }, []);

  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    void getBackend().then((backend) => {
      if (!live) return;
      stop = backend.onUpdateProgress((next) => {
        if (live) setProgress(next);
      });
    });
    return () => {
      live = false;
      stop?.();
    };
  }, []);

  useEffect(() => {
    let live = true;
    void getBackend()
      .then((backend) => backend.appVersion())
      .then((found) => {
        if (live) setVersion(found);
      })
      .catch(() => {
        // A build with no shell behind it has no version to give; the pane
        // shows a dash rather than an error nobody asked about.
      });
    return () => {
      live = false;
    };
  }, []);

  const open = (url: string) => {
    void getBackend().then((backend) => backend.openUrl(url)).catch(() => {});
  };

  const checkForUpdates = () => {
    setUpdateStatus({ kind: "checking" });
    setUpdateError(null);
    void (async () => {
      try {
        const backend = await getBackend();
        const found = await backend.checkForUpdate();
        if (found.storeInstall) {
          setUpdateStatus({ kind: "store" });
        } else if (found.version === null) {
          setUpdateStatus({ kind: "upToDate", version: found.currentVersion });
        } else if (found.ready) {
          setUpdateStatus({ kind: "ready", version: found.version });
        } else {
          setUpdateStatus({ kind: "available", version: found.version });
          // An update found is taken without asking, as any check's is; this
          // pane shows the download happening rather than starting it unseen.
          void backend.downloadUpdate()
            .then((ready) => setUpdateStatus({ kind: "ready", version: ready.version }))
            .catch(() => setUpdateError("Couldn’t download the update. Please try again."));
        }
      } catch {
        setUpdateStatus({ kind: "idle" });
        setUpdateError("Couldn’t check for updates. Please try again.");
      }
    })();
  };

  const statusText = updateStatus.kind === "checking"
    ? t("Checking for updates…")
    : updateStatus.kind === "store"
    ? t("This copy of rbxport is from the Microsoft Store. Get updates from the Microsoft Store.")
    : updateStatus.kind === "upToDate"
    ? t("rbxport v{version} is up to date.", { version: updateStatus.version })
    : updateStatus.kind === "available"
    ? t("Update available v{version}.", { version: updateStatus.version })
    : updateStatus.kind === "ready"
    ? t("Update v{version} downloaded — restart rbxport to use it.", { version: updateStatus.version })
    : null;

  // Done when the last event's downloaded byte count reached the total; the
  // final event of a download always carries that, even with no total known
  // until then.
  const downloading = progress !== null && !(progress.total !== null && progress.downloaded >= progress.total);

  return (
    <>
      <section className={styles.section} aria-label="About">
        <div className={styles.aboutHeader}>
          <div>
            <h1 className={styles.aboutName}>rbxport</h1>
            <p className={styles.aboutVersion} data-testid="about-version">v{version ?? "—"}</p>
          </div>
          <Button disabled={updateStatus.kind === "checking"} onClick={checkForUpdates}>
            {updateStatus.kind === "checking" ? "Checking…" : "Check for updates"}
          </Button>
        </div>
        <div className={styles.aboutUpdates}>
          <div className={styles.updateControls}>
            <Toggle
              label="Download updates"
              checked={checkUpdates}
              onChange={(checkUpdates) => update("advanced", { checkUpdates })}
            />
            <Select
              label="Update frequency"
              disabled={!checkUpdates}
              value={updateFrequency}
              choices={[
                { value: "start", label: "After startup" },
                { value: "daily", label: "Daily" },
                { value: "weekly", label: "Weekly" },
              ]}
              onChange={(updateFrequency) => update("advanced", { updateFrequency })}
            />
          </div>
          <p className={layout.help}>Updates download in the background and install after you quit.</p>
          {statusText ? <p className={styles.updateHint} aria-live="polite">{statusText}</p> : null}
          {downloading ? (
            <div className={styles.updateProgressRow}>
              <div
                className={styles.updateProgress}
                role="progressbar"
                aria-label="Downloading update"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={
                  progress?.total != null ? Math.round((progress.downloaded / progress.total) * 100) : undefined
                }
                data-indeterminate={progress?.total == null || undefined}
              >
                <span
                  className={styles.updateProgressFill}
                  style={
                    progress?.total != null
                      ? { width: `${Math.min((progress.downloaded / progress.total) * 100, 100)}%` }
                      : undefined
                  }
                />
              </div>
              <span className={styles.updateProgressText}>
                {progress?.total != null
                  ? `${formatBytes(progress.downloaded)} of ${formatBytes(progress.total)}`
                  : formatBytes(progress?.downloaded ?? 0)}
              </span>
            </div>
          ) : null}
          {updateError ? <p className={styles.updateError} role="alert">{updateError}</p> : null}
        </div>
        <div className={styles.aboutSupport}>
          <h2>Support rbxport</h2>
          <p>rbxport is free to use and independently developed by TRIODE.</p>
          <p>If it makes your DJ workflow easier, consider supporting continued development and future features.</p>
          <Button className={styles.supportButton} onClick={() => open(SUPPORT_URL)}><Heart className={styles.supportHeart} size={16} aria-hidden="true" /> Support rbxport</Button>
        </div>
      </section>
      <div className={styles.aboutLegal}>
        <details>
          <summary>Licence</summary>
          <p className={styles.aboutLine}>
            rbxport is free software, licensed under the GNU General Public License version 2 or, at your
            option, any later version. It comes with no warranty. It includes the Rubber Band Library
            by Particular Programs Ltd., under the same licence.
          </p>
        </details>
        <details>
          <summary>Disclaimer</summary>
          <p className={styles.aboutLine}>
            rbxport is an independent project and is not affiliated with, endorsed by, or sponsored by
            AlphaTheta Corporation, Pioneer DJ, or any other third party. rekordbox, CDJ, XDJ and
            PRO DJ LINK are trademarks of their respective owners. Back up your library before using
            any feature that writes to it.
          </p>
        </details>
      </div>
      <footer className={styles.aboutFoot}>
        <div className={styles.aboutLinks} role="group" aria-label="Author links">
          {ABOUT_LINKS.map(({ label, url, Icon }) => (
            <button key={label} type="button" className={styles.aboutSocial} aria-label={label} title={label} onClick={() => open(url)}>
              <Icon size={19} aria-hidden="true" />
            </button>
          ))}
        </div>
        <p className={styles.aboutMade}><span>Made by TRIODE with</span> <Heart className={styles.supportHeart} size="1em" aria-hidden="true" /> <span>in California</span></p>
      </footer>
    </>
  );
}
