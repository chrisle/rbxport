/**
 * PRO DJ LINK, a pane of its own: the LINK switch, the interface it runs
 * on, and the players on the network with what each has loaded from us.
 * The interface it runs on is kept with the DJ System preferences, where
 * a stick's defaults also live.
 */
import { useEffect, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { LinkStatus } from "@/ipc/types";
import { errorMessage } from "@/lib/errorMessage";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./Preferences.module.css";
import { Button, Section } from "./controls";

/** The radio value for "no interface chosen". */
const AUTOMATIC = "";

/**
 * PRO DJ LINK: the LINK switch, the interface it runs on, and the players
 * on the network with what each has loaded from us.
 *
 * The interface is a preference, so the strip's LINK button honours it too;
 * "Automatic" leaves the choice to the app, which takes the interface the
 * players are reached through. It is changed with LINK off: a session is
 * bound to its interface for as long as it runs.
 *
 * Status arrives by event and is polled while open, including when LINK
 * is off and rekordbox starts or exits without a LINK event. The
 * session itself outlives the pane, as LINK does — a source that vanished
 * when Preferences closed would be no source at all.
 */
export function LinkPane() {
  const { preferences, update } = usePreferencesContext();
  const linkInterface = preferences.djSystem.linkInterface;
  const onChoose = (name: string | null) => update("djSystem", { linkInterface: name });
  const [link, setLink] = useState<LinkStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const revision = useRef(0);
  const actionPending = useRef(false);

  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      const requestedAt = revision.current;
      try {
        if (actionPending.current) return;
        const backend = await getBackend();
        if (!live) return;
        if (!stop) stop = backend.onLinkStatus((status) => {
          if (!live) return;
          revision.current += 1;
          setLink(status);
          setStatusError(null);
        });
        const status = await backend.linkStatus();
        // An event or Connect/Disconnect result is newer than this read.
        if (live && requestedAt === revision.current && !actionPending.current) {
          setLink(status);
          setStatusError(null);
        }
      } catch (cause) {
        if (live && requestedAt === revision.current) setStatusError(errorMessage(cause));
      } finally {
        if (live) timer = setTimeout(() => void refresh(), 2000);
      }
    };
    void refresh();
    return () => {
      live = false;
      clearTimeout(timer);
      stop?.();
    };
  }, []);

  const interfaces = link?.interfaces ?? [];
  const selectedInterface = interfaces.find((iface) => iface.name === linkInterface);
  const activeInterface = link?.on ? link.interface : null;
  const usesWifi = selectedInterface?.connection === "wireless" ||
    (linkInterface === null && (activeInterface?.connection === "wireless" ||
      interfaces.some((iface) => iface.name === activeInterface?.name && iface.connection === "wireless")));
  const canChoose = link !== null && !link.on && !busy;
  const missingInterface = linkInterface !== null && !interfaces.some((i) => i.name === linkInterface);
  const toggle = () => {
    if (actionPending.current) return;
    actionPending.current = true;
    revision.current += 1;
    setBusy(true);
    setError(null);
    void (async () => {
      try {
        const backend = await getBackend();
        const next = link?.on ? await backend.stopLinkExport() : await backend.startLinkExport(linkInterface ?? undefined, preferences.djSystem.linkKeySort);
        revision.current += 1;
        setLink(next);
        setStatusError(null);
      } catch (cause) {
        setError(errorMessage(cause));
      } finally {
        actionPending.current = false;
        setBusy(false);
      }
    })();
  };

  const status = !link ? (error || statusError ? "Unavailable" : "Checking connection…")
    : !link.on ? "Disconnected"
      : link.state === "up" ? "Connected"
        : link.state === "waiting" ? "Waiting for devices"
          : link.state === "down" ? "Connection lost" : "Connecting…";
  const problem = error ?? statusError ?? link?.problem;

  return (
    <Section title="PRO DJ LINK" label="Link">
      {problem ? <p className={styles.linkError} role="alert">{problem}</p> : null}
      <div className={styles.linkSummary}>
        <div>
          <strong className={styles.linkStatus} data-connected={link?.on && link.state === "up"} data-state={link?.state} role="status">{status}</strong>
          <p className={styles.linkHelp}>
            {link?.on && link.interface
              ? `Using ${link.interface.name} · ${link.interface.address}`
              : "Share your library with players on your network."}
          </p>
          {link?.on && link.state === "up" ? (
            <p className={styles.linkHelp}>On your player, open <b>LINK</b> and select <b>rekordbox</b>.</p>
          ) : null}
        </div>
        {link !== null && !link.on && link.problem ? null : (
          <Button onClick={toggle} disabled={busy || link === null}>
            {busy ? "Please wait…" : link?.on ? "Disconnect" : "Connect to PRO DJ LINK"}
          </Button>
        )}
      </div>
      <fieldset className={styles.linkKeySort} disabled={!canChoose}>
        <legend>Key sorting</legend>
        <label><input type="radio" name="link-key-sort" checked={preferences.djSystem.linkKeySort === "alphabetical"}
          onChange={() => update("djSystem", {linkKeySort: "alphabetical"})} /> Alphabetically — A, Ab, B, …</label>
        <label><input type="radio" name="link-key-sort" checked={(preferences.djSystem.linkKeySort ?? "musical") === "musical"}
          onChange={() => update("djSystem", {linkKeySort: "musical"})} /> Musically — Abm, B, Ebm, F#, Bbm, …</label>
        <p className={styles.linkHelp}>Applies to the key menu and tracks sorted by key on connected players. Disconnect to change.</p>
      </fieldset>
      <fieldset className={styles.networkInterfaces} disabled={busy || link === null || link.on}>
        <legend>Network interface</legend>
        <p className={styles.linkHelp}>{link?.on ? "Disconnect to change the network interface." : "Choose the network your players are connected to. Wired Ethernet is recommended."}</p>
        <table aria-label="Network interfaces">
          <thead><tr><th>Interface</th><th>Connection</th><th>Adapter</th><th>IP address</th></tr></thead>
          <tbody>
            <tr data-selected={linkInterface === null} onClick={(event) => { if (canChoose && !(event.target as HTMLElement).closest("input, label")) onChoose(null); }}>
              <td><label><input type="radio" name="link-interface" value={AUTOMATIC} checked={linkInterface === null} onChange={() => onChoose(null)} />Automatic</label></td>
              <td colSpan={3}>Let the app choose the interface</td>
            </tr>
            {interfaces.map((iface) => (
              <tr key={`${iface.name}-${iface.address}`} data-selected={linkInterface === iface.name} data-active={link?.on && link.interface?.name === iface.name && link.interface?.address === iface.address}
                onClick={(event) => { if (canChoose && !(event.target as HTMLElement).closest("input, label")) onChoose(iface.name); }}>

                <td><label><input type="radio" name="link-interface" value={iface.name} checked={linkInterface === iface.name} onChange={() => onChoose(iface.name)} />{iface.name}</label>
                  {link?.on && link.interface?.name === iface.name && link.interface?.address === iface.address
                    ? <span className={styles.linkBadge}>In use</span> : null}
                </td>
                <td>{iface.connection === "wireless" ? "Wi-Fi" : iface.connection === "wired" ? "Wired" : "Unknown"}
                  {iface.connection === "wired" ? <span className={styles.linkRecommendation}>Recommended</span> : null}
                </td>
                <td>{iface.adapter ?? "Unknown"}</td>
                <td>{iface.address}</td>
              </tr>
            ))}
            {missingInterface ? (
              <tr><td><label><input type="radio" name="link-interface" checked readOnly />{linkInterface}</label></td><td colSpan={3}>Not present</td></tr>
            ) : null}
          </tbody>
        </table>
      </fieldset>
      {usesWifi ? (
        <div className={styles.linkWarning} role="alert">
          <strong>Wi-Fi selected</strong>
          <p>PRO DJ LINK is not designed to work over Wi-Fi. Use a wired Ethernet connection for reliable playback.</p>
        </div>
      ) : null}
      <div className={styles.linkDevices}>
        <h4>Devices on your network <span>{link?.on ? link.players.length : 0}</span></h4>
        {link?.on && link.players.length ? (
          <table aria-label="Devices on the link">
            <thead><tr><th>Device</th><th>Role</th><th>IP address</th><th>Status</th></tr></thead>
            <tbody>
              {link.players.map((player) => (
                <tr key={player.number}>
                  <td>
                    <strong>{player.name}</strong>
                    {player.loaded ? <span className={styles.linkTrack}>{player.loaded.title}{player.loaded.artist ? ` · ${player.loaded.artist}` : ""}</span> : null}
                  </td>
                  <td className={styles.linkRole}>{player.kind} {player.number}</td>
                  <td className={styles.linkAddress}>{player.address}</td>
                  <td>
                    {player.loaded ? (player.playing ? "Playing" : "Loaded") : "Online"}
                    {player.master ? <span className={styles.linkBadge}>Master</span> : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <div className={styles.linkEmpty}>
            <strong>{link?.on ? "No devices found yet" : "Discover your devices"}</strong>
            <p>{link?.on
              ? "Turn on your players and mixers, then connect them to the network shown above."
              : "Choose a network interface and connect to see your players and mixers here."}</p>
          </div>
        )}
        {link?.on && link.players.some((player) => player.kind === "player") ? (
          <p className={styles.linkHelp}>Track details appear when a player loads a track from this library.</p>
        ) : null}
      </div>
    </Section>
  );
}
