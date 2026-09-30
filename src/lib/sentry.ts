import * as Sentry from "@sentry/react";

const configuredDsn: unknown = import.meta.env.VITE_SENTRY_DSN;
const dsn = typeof configuredDsn === "string" ? configuredDsn : undefined;

/**
 * Starts error reporting for packaged builds when a DSN was supplied at build
 * time. Development, web mock, and builds without a DSN remain entirely
 * offline.
 */
export function initializeSentry() {
  if (!dsn || import.meta.env.DEV) return;

  Sentry.init({
    dsn,
    release: `rbxport@${__APP_VERSION__}`,
    environment: import.meta.env.MODE,
    // Error/crash reporting only: no tracing, replay, user identity, request
    // data, or interaction history. Avoid the default browser session,
    // console, breadcrumb, HTTP-context and culture integrations too.
    defaultIntegrations: false,
    integrations: [
      Sentry.eventFiltersIntegration(),
      Sentry.browserApiErrorsIntegration(),
      Sentry.globalHandlersIntegration(),
      Sentry.linkedErrorsIntegration(),
      Sentry.dedupeIntegration(),
    ],
    maxBreadcrumbs: 0,
    dataCollection: {
      userInfo: false,
      cookies: false,
      httpHeaders: false,
      httpBodies: [],
      urlQueryParams: false,
      stackFrameVariables: false,
      frameContextLines: 0,
    },
    beforeSend(event) {
      delete event.user;
      delete event.request;
      delete event.breadcrumbs;
      return event;
    },
  });
}
