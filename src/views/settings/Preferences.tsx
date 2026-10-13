import type { VuDisplay } from "@/lib/vuMeter";
/**
 * The Preferences window.
 *
 * rekordbox's, drawn from the captures docs/screenshots 9.46.53 to 9.49.51 PM:
 * a sidebar of panes on the left, a strip of tabs across the top of the
 * pane, the tab's sections stacked beneath, and Reset to defaults under
 * them. Two panes are missing on purpose — PLAN and CLOUD are Pioneer's
 * subscription and its cloud sync, and there is nothing here for either.
 *
 * Every control on every pane drives something the application does. What
 * rekordbox offers that this build has nothing behind — a light theme, a
 * metronome, a streaming service — is left out rather than drawn dead: a
 * switch that changes nothing is worse than no switch.
 */
import { useEffect, useLayoutEffect, useRef, useState, type ComponentType, type SVGProps } from "react";

import {
  PrefAboutIcon, PrefAdvancedIcon, PrefAnalysisIcon, PrefAudioIcon, PrefDjSystemIcon,
  PrefKeyboardIcon, PrefViewIcon,
  LinkIcon,
} from "@/components/icons";
import { getBackend } from "@/ipc/client";
import { ReportBug } from "@/views/report/ReportBug";
import type { LibrarySummary, Limiter } from "@/ipc/types";
import { usePreferencesContext } from "@/store/usePreferences";
import type { PreferencePane } from "@/lib/preferences";
import { startWindowDrag, toggleWindowMaximise } from "@/lib/windowDrag";
import { AboutPane } from "./AboutPane";
import { AdvancedPane, ADVANCED_TABS, type AdvancedTab } from "./AdvancedPane";
import { AnalysisPane, ANALYSIS_TABS, type AnalysisTab } from "./AnalysisPane";
import { AudioPane, AUDIO_TABS, type AudioTab } from "./AudioPane";
import { DjSystemPane, DJ_SYSTEM_TABS, type DjSystemTab } from "./DjSystemPane";
import { KeyboardPane } from "./KeyboardPane";
import { USB_EXPORT_SECTIONS, UsbExportPane } from "./UsbExportPane";
import { BackupsPane } from "./BackupsPane";
import { ArchiveRestore, RefreshCw } from "lucide-react";
import { LinkPane } from "./LinkPane";
import { RekordboxPane } from "./RekordboxPane";
import styles from "./Preferences.module.css";
import { Button } from "./controls";
import { ViewPane, VIEW_TABS, type ViewTab } from "./ViewPane";

/**
 * The sidebar, in the capture's order and wording, less PLAN and CLOUD, and
 * with About at the end — ours, for the version and the update check.
 */
export type Pane = "view" | "audio" | "analysis" | "djSystem" | "link" | "keyboard" | "advanced" | "backups" | "usbExport" | "rekordbox" | "about";

/** A place in Preferences that opens a pane at one of its sections. */
type SectionTarget = "usbExportSync" | "usbExportImport";

export type PreferencesTarget = Pane | "libraryProtection" | SectionTarget;

/** The pane each section target opens, and the id of the section it scrolls to. */
const SECTION_TARGETS: Record<SectionTarget, { pane: Pane; section: string }> = {
  usbExportSync: { pane: "usbExport", section: USB_EXPORT_SECTIONS.sync },
  usbExportImport: { pane: "usbExport", section: USB_EXPORT_SECTIONS.import },
};

const isSectionTarget = (value: string | undefined): value is SectionTarget =>
  value !== undefined && Object.hasOwn(SECTION_TARGETS, value);

/** The pane a target opens. */
function paneOf(target: PreferencesTarget): Pane {
  if (target === "libraryProtection") return "advanced";
  return isSectionTarget(target) ? SECTION_TARGETS[target].pane : target;
}

function UsbStickIcon(props: SVGProps<SVGSVGElement>) {
  return <svg {...props} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true">
    <path d="M8 9V2h8v7M10 4v2m4-2v2" />
    <rect x="6" y="9" width="12" height="13" rx="3" />
    <path d="M10 18h4" />
  </svg>;
}

const PANES: readonly { id: Pane; label: string; Icon: ComponentType<SVGProps<SVGSVGElement>> }[] = [
  { id: "view", label: "View", Icon: PrefViewIcon },
  { id: "audio", label: "Audio", Icon: PrefAudioIcon },
  { id: "analysis", label: "Analysis", Icon: PrefAnalysisIcon },
  { id: "djSystem", label: "DJ System", Icon: PrefDjSystemIcon },
  { id: "keyboard", label: "Keyboard", Icon: PrefKeyboardIcon },
  { id: "advanced", label: "Advanced", Icon: PrefAdvancedIcon },
  { id: "rekordbox", label: "Rekordbox", Icon: RefreshCw },
  { id: "link", label: "PRO DJ LINK", Icon: LinkIcon },
  { id: "usbExport", label: "USB Export", Icon: UsbStickIcon },
  { id: "backups", label: "Backups", Icon: ArchiveRestore },
  { id: "about", label: "About", Icon: PrefAboutIcon },
];

/** Which stored pane a sidebar pane's Reset to defaults clears. */
const RESETS: Partial<Record<Pane, PreferencePane>> = {
  view: "view",
  audio: "audio",
  analysis: "analysis",
  djSystem: "djSystem",
  advanced: "advanced",
  usbExport: "usbExport",
  rekordbox: "rekordbox",
};

export interface PreferencesProps {
  summary: LibrarySummary | null;
  /** The master limiter as it stands, where a change goes, and how hard it is working. */
  limiter: Limiter;
  onLimiterChange: (change: Partial<Limiter>) => void;
  reduction: number;
  /** The loudest sample the device was given, per channel, for the Audio pane's meters. */
  vu?: VuDisplay | undefined;
  peakLeft?: number;
  peakRight?: number;
  onResetColumns: () => void;
  onResetLayout: () => void;
  onClose: () => void;
  /** Where to open: the missing-file manager lands on Advanced › Database. */
  initialPane?: PreferencesTarget;
  /**
   * Drawn as the whole of a window of its own — no backdrop, no title bar of
   * ours, the shell's chrome around it — rather than over the main window.
   */
  windowed?: boolean;
}

/** A pane name, or View for anything that is not one. */
export function asPane(value: string | undefined): Pane {
  return PANES.some((p) => p.id === value) ? (value as Pane) : "view";
}

/** The target a name from outside (a URL hash) stands for; an unknown one is View. */
export function asTarget(value: string | undefined): PreferencesTarget {
  return value === "libraryProtection" || isSectionTarget(value) ? value : asPane(value);
}

interface Tabs {
  view: ViewTab;
  audio: AudioTab;
  analysis: AnalysisTab;
  djSystem: DjSystemTab;
  advanced: AdvancedTab;
}

const FIRST_TABS: Tabs = {
  view: "display",
  audio: "configuration",
  analysis: "track",
  djSystem: "general",
  advanced: "database",
};

export function Preferences({
  summary, limiter, onLimiterChange, reduction, vu, peakLeft = 0, peakRight = 0, onResetColumns, onResetLayout,
  onClose, initialPane = "view", windowed = false,
}: PreferencesProps) {
  const window_ = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const [pane, setPane] = useState<Pane>(paneOf(initialPane));
  // The window is turned to another pane from outside — the File menu's
  // Missing File Manager while it is already open.
  useEffect(() => {
    setPane(paneOf(initialPane));
    if (initialPane === "libraryProtection") {
      setTabs((current) => ({ ...current, advanced: "browse" }));
      setQuery("");
    }
  }, [initialPane]);
  const [tabs, setTabs] = useState<Tabs>(FIRST_TABS);
  const [query, setQuery] = useState("");
  const { reset } = usePreferencesContext();
  const [reportOpen, setReportOpen] = useState(false);
  const openReport = () => {
    void getBackend().then((backend) => backend.openReportWindow()).then((opened) => {
      if (!opened) setReportOpen(true);
    });
  };

  useEffect(() => {
    // Focus the window so Escape reaches it and a reader lands inside.
    if (!reportOpen) window_.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (reportOpen) setReportOpen(false);
        else onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose, reportOpen]);

  // A section target lands on its section, once its pane is the one shown.
  useLayoutEffect(() => {
    if (!isSectionTarget(initialPane)) return;
    const { pane: target, section } = SECTION_TARGETS[initialPane];
    if (pane !== target) return;
    scroller.current?.querySelector(`#${section}`)?.scrollIntoView({ block: "start" });
  }, [initialPane, pane]);

  // The search box narrows the pane to the sections that mention the words
  // typed. The sections' own text is the index: every label in them is what
  // somebody would search for, so nothing has to be listed twice.
  useLayoutEffect(() => {
    const host = scroller.current;
    if (!host) return;
    const needle = query.trim().toLowerCase();
    for (const section of host.querySelectorAll<HTMLElement>("section")) {
      section.hidden = needle !== "" && !(section.textContent ?? "").toLowerCase().includes(needle);
    }
  });

  const tabStrip = <T extends string>(
    all: readonly { id: T; label: string }[],
    current: T,
    pick: (id: T) => void,
  ) => (
    <div className={styles.tabs} role="tablist">
      {all.map((t) => (
        <button
          key={t.id}
          type="button"
          role="tab"
          className={styles.tab}
          aria-selected={current === t.id}
          onClick={() => pick(t.id)}
        >
          {t.label}
        </button>
      ))}
    </div>
  );

  const resetPane = RESETS[pane];

  const body = (
      <div
        ref={window_}
        className={styles.window}
        data-windowed={windowed || undefined}
        // The backdrop closes on click; the window must not pass its own through.
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal={windowed ? undefined : "true"}
        aria-label="Preferences"
        tabIndex={-1}
      >
        {/* Drawn here rather than left to the platform, so the name sits in
            the middle of the app's own title bar grey on every OS. */}
        <header
          className={styles.titlebar}
          onMouseDown={windowed ? startWindowDrag : undefined}
          onDoubleClick={windowed ? toggleWindowMaximise : undefined}
        >
          {windowed ? null : (
            <button type="button" className={styles.close} onClick={onClose} aria-label="Close">
              ✕
            </button>
          )}
          Preferences
        </header>
        <div className={styles.body}>
          <nav className={styles.sidebar} aria-label="Preference panes">
            <div className={styles.search}>
              <svg viewBox="0 0 16 16" aria-hidden focusable="false">
                <circle cx="6.5" cy="6.5" r="4.5" fill="none" stroke="currentColor" strokeWidth="1.6" />
                <path d="M10 10l4 4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
              </svg>
              <input
                type="search"
                aria-label="Search preferences"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <ul className={styles.panes} role="tablist" aria-orientation="vertical">
              {PANES.map(({ id, label, Icon }) => (
                <li key={id}>
                  {id === "rekordbox" ? <h4 className={styles.paneGroupHeading}>RBXport</h4> : null}
                  <button
                    type="button"
                    role="tab"
                    className={styles.pane}
                    aria-selected={pane === id}
                    onClick={() => setPane(id)}
                  >
                    <Icon />
                    {label}
                  </button>
                </li>
              ))}
            </ul>
            <Button className={styles.reportBug} onClick={openReport}>Report bug</Button>
          </nav>

          <div className={styles.content}>
            {pane === "view"
              ? tabStrip(VIEW_TABS, tabs.view, (view) => setTabs((t) => ({ ...t, view })))
              : pane === "audio"
                ? tabStrip(AUDIO_TABS, tabs.audio, (audio) => setTabs((t) => ({ ...t, audio })))
                : pane === "analysis"
                  ? tabStrip(ANALYSIS_TABS, tabs.analysis, (analysis) => setTabs((t) => ({ ...t, analysis })))
                  : pane === "djSystem"
                    ? tabStrip(DJ_SYSTEM_TABS, tabs.djSystem, (djSystem) => setTabs((t) => ({ ...t, djSystem })))
                    : pane === "advanced"
                      ? tabStrip(ADVANCED_TABS, tabs.advanced, (advanced) => setTabs((t) => ({ ...t, advanced })))
                      : null}
            <div ref={scroller} className={styles.scroller} role="tabpanel">
              <div className={styles.sections}>
                {pane === "view" ? (
                  <ViewPane tab={tabs.view} onResetColumns={onResetColumns} onResetLayout={onResetLayout} />
                ) : pane === "audio" ? (
                  <AudioPane
                    tab={tabs.audio}
                    limiter={limiter}
                    onLimiterChange={onLimiterChange}
                    reduction={reduction}
                    vu={vu}
                    peakLeft={peakLeft}
                    peakRight={peakRight}
                  />
                ) : pane === "analysis" ? (
                  <AnalysisPane tab={tabs.analysis} />
                ) : pane === "djSystem" ? (
                  <DjSystemPane tab={tabs.djSystem} />
                ) : pane === "link" ? (
                  <LinkPane />
                ) : pane === "keyboard" ? (
                  <KeyboardPane />
                ) : pane === "usbExport" ? (
                  <UsbExportPane />
                ) : pane === "backups" ? (
                  <BackupsPane />
                ) : pane === "rekordbox" ? (
                  <RekordboxPane />
                ) : pane === "about" ? (
                  <AboutPane />
                ) : (
                  <AdvancedPane tab={tabs.advanced} summary={summary} />
                )}
              </div>
            </div>
            {resetPane ? (
              <Button className={styles.reset} onClick={() => reset(resetPane)}>
                Reset to defaults
              </Button>
            ) : null}
          </div>
        </div>
      </div>
  );

  return (
    <>
      {windowed ? body : (
        <div className={styles.backdrop} onMouseDown={onClose} role="presentation">
          {body}
        </div>
      )}
      {reportOpen ? <ReportBug onClose={() => setReportOpen(false)} /> : null}
    </>
  );
}
