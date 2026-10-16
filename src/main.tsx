import { StrictMode, Suspense, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./styles/base.css";
import { Localization } from "./i18n";
import { syncRekordboxBrowseAtStartup } from "./lib/rekordboxBrowse";
import { initializeSentry } from "./lib/sentry";
import * as Sentry from "@sentry/react";

// Suppress the webview's Reload/Inspect menu in every app window. Leave
// propagation intact so the app's context-menu handlers still receive it.
window.addEventListener("contextmenu", (event) => event.preventDefault());

const el = document.getElementById("root");
if (!el) throw new Error("#root missing from index.html");
initializeSentry();
const root = createRoot(el, {
  onCaughtError: (error, errorInfo) => {
    Sentry.captureReactException(error, { componentStack: errorInfo.componentStack ?? null });
  },
  onUncaughtError: (error, errorInfo) => {
    Sentry.captureReactException(error, { componentStack: errorInfo.componentStack ?? null });
  },
});

// The same bundle serves the Preferences, Sync Manager, Report bug and Log
// windows: the shell opens them at `#preferences/<pane>`, `#sync`, `#report`
// and `#log`, and that is all each draws.
const report = window.location.hash.startsWith("#report");
const preferences = window.location.hash.startsWith("#preferences");
const sync = window.location.hash.startsWith("#sync");
const log = window.location.hash.startsWith("#log");

function mount(view: ReactNode) {
  root.render(
    <StrictMode>
      <Localization>
        <Suspense fallback={null}>{view}</Suspense>
      </Localization>
    </StrictMode>,
  );
}

// A secondary window's code is its own chunk, loaded here before the first
// render rather than through `lazy`. React 19 holds a Suspense boundary's
// content back until 300 ms after its fallback was shown, so a lazy root
// kept every window blank for that long although its chunk had arrived in a
// few milliseconds.
function secondaryWindow(): Promise<ReactNode> | null {
  if (preferences) return import("./views/settings/PreferencesWindow").then(({ PreferencesWindow }) => <PreferencesWindow />);
  if (sync) return import("./views/sync/SyncWindow").then(({ SyncWindow }) => <SyncWindow />);
  if (report) return import("./views/report/ReportBug").then(({ ReportWindow }) => <ReportWindow />);
  if (log) return import("./views/log/LogView").then(({ LogWindow }) => <LogWindow />);
  return null;
}

// Finish evaluating this entry module before loading the mock backend. The
// production bundle can share code back into this module, so a top-level await
// on that dynamic import would leave both sides waiting and the window blank.
const view = secondaryWindow();
const browse = preferences || sync || log ? Promise.resolve() : syncRekordboxBrowseAtStartup().catch(() => {});
void Promise.all([view, browse]).then(([secondary]) => mount(secondary ?? <App />));
